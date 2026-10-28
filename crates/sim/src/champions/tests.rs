use std::sync::Arc;

use asset::data::GameData;
use glam::Vec2;
use protocol::{Command, DamageKind, Slot, TICK, Target, Team, UnitId};

use crate::layout::Layout;
use crate::unit::SLOTS;
use crate::{Entrant, Game};

const MIDDLE: Vec2 = Vec2::new(6500.0, 6500.0);

fn real_data() -> Option<Arc<GameData>> {
    let bytes = std::fs::read(crate::tests::assets().join("data.bin")).ok()?;
    GameData::from_bytes(&bytes).ok().map(Arc::new)
}

struct Duel {
    game: Game,
    hero: UnitId,
    enemy: UnitId,
}

fn duel(hero: &str, enemy: &str, distance: f32) -> Option<Duel> {
    let data = real_data()?;
    let index = |name: &str| {
        data.champions
            .iter()
            .position(|champion| champion.name == name)
            .map(|index| index as u8)
    };
    let players = [
        Entrant::new("hero", Team::Blue, Some(index(hero)?)),
        Entrant::new("enemy", Team::Red, Some(index(enemy)?)),
    ];
    let mut game = Game::new(data.clone(), Layout::placeholder(), &players, 7);
    let (hero, enemy) = (game.seats[0].unit, game.seats[1].unit);
    for (id, position) in [(hero, MIDDLE), (enemy, MIDDLE + Vec2::new(distance, 0.0))] {
        let unit = game.unit_mut(id)?;
        unit.position = position;
        unit.last_position = position;
        for slot in SLOTS {
            unit.ability_mut(slot).rank = 3;
        }
        unit.mana = unit.stats.max_mana;
    }
    Some(Duel { game, hero, enemy })
}

fn dummy(game: &mut Game, team: Team, position: Vec2) -> UnitId {
    let name = if team == Team::Blue {
        "HA_OrderMinionMelee"
    } else {
        "HA_ChaosMinionMelee"
    };
    let base = game.unit_stats(name);
    let id = game.allocate();
    let mut unit = crate::unit::Unit::new(
        id,
        protocol::UnitKind::Minion(protocol::MinionKind::Melee),
        team,
        name,
        base,
        position,
    );
    unit.stats = crate::stats::compute(&unit.base, 1, &crate::stats::Bonus::default());
    unit.health = unit.stats.max_health;
    game.units.push(unit);
    game.stun(id, 1000.0, None);
    id
}

fn run(game: &mut Game, seconds: f32) {
    for _ in 0..(seconds / TICK).round() as u32 {
        game.step();
    }
}

fn cast(game: &mut Game, id: UnitId, slot: Slot, at: Vec2, unit: Option<UnitId>) {
    game.command(
        id,
        Command::Cast(
            slot,
            Target {
                point: at.to_array(),
                unit,
            },
        ),
    );
}

fn health(game: &Game, id: UnitId) -> f32 {
    game.unit(id).map_or(0.0, |unit| unit.health)
}

#[test]
fn swordsman_spins_for_three_seconds_and_cannot_attack_meanwhile() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Swordsman", "Wanderer", 200.0)
    else {
        return;
    };
    let before = health(&game, enemy);
    cast(&mut game, hero, Slot::E, MIDDLE, None);
    assert!(game.unit(hero).unwrap().is_disarmed());
    run(&mut game, 1.0);
    let after_one = health(&game, enemy);
    assert!(after_one < before);
    run(&mut game, 2.5);
    let hero_unit = game.unit(hero).unwrap();
    assert!(!hero_unit.is_disarmed());
    assert!(!hero_unit.animation.is_some_and(|animation| animation.hold));
    assert!(hero_unit.ability(Slot::E).cooldown > 0.0);
    assert!(health(&game, enemy) < after_one);
}

#[test]
fn swordsman_judgment_marks_only_the_nearest_enemy_while_spinning() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Swordsman", "Wanderer", 250.0)
    else {
        return;
    };
    let marked = |game: &Game, id: UnitId| game.unit(id).unwrap().has_buff("judgment_nearest");
    cast(&mut game, hero, Slot::E, MIDDLE, None);
    run(&mut game, 0.5);
    assert!(marked(&game, enemy));
    let (team, position) = game
        .unit(hero)
        .map(|unit| (unit.team.enemy(), unit.position))
        .unwrap();
    let closer = dummy(&mut game, team, position + Vec2::new(50.0, 0.0));
    run(&mut game, 0.5);
    assert!(marked(&game, closer));
    assert!(!marked(&game, enemy));
    run(&mut game, 2.5);
    assert!(!marked(&game, closer));
}

#[test]
fn swordsman_judgment_spins_can_crit() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Swordsman", "Wanderer", 200.0)
    else {
        return;
    };
    game.shield(enemy, 100_000.0, 60.0);
    let mut edge = game.buff("edge", f32::MAX);
    edge.bonus.crit_chance = 1.0;
    game.add_buff(hero, edge);
    game.events.clear();
    cast(&mut game, hero, Slot::E, MIDDLE, None);
    run(&mut game, 1.0);
    let crits: Vec<bool> = game
        .events
        .iter()
        .filter_map(|event| match event {
            protocol::Event::Damage {
                source, critical, ..
            } if *source == Some(hero) => Some(*critical),
            _ => None,
        })
        .collect();
    assert!(!crits.is_empty() && crits.iter().all(|critical| *critical));
}

#[test]
fn swordsman_justice_interrupts_judgment() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Swordsman", "Wanderer", 300.0)
    else {
        return;
    };
    cast(&mut game, hero, Slot::E, MIDDLE, None);
    run(&mut game, 0.5);
    let at = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::R, at, Some(enemy));
    let hero_unit = game.unit(hero).unwrap();
    assert!(!hero_unit.has_buff("judgment"));
    assert!(hero_unit.ability(Slot::R).cooldown > 0.0);
}

#[test]
fn swordsman_judgment_shred_lands_on_six_and_refreshes_every_sixth_after() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Swordsman", "Wanderer", 200.0)
    else {
        return;
    };
    game.shield(enemy, 100_000.0, 60.0);
    let spell = super::spell_of(&game, hero, Slot::E).unwrap();
    let mut refreshed = Vec::new();
    for hit in 1..=13 {
        super::swordsman::shred(&mut game, hero, enemy, &spell, 3);
        if let Some(shred) = game.unit(enemy).unwrap().buff("judgment_shred")
            && shred.expires > game.time + 5.99
        {
            refreshed.push(hit);
        }
        run(&mut game, 0.1);
    }
    assert_eq!(refreshed, [6, 7, 13]);
}

#[test]
fn swordsman_decisive_strike_removes_every_slow() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Swordsman", "Guardian", 400.0)
    else {
        return;
    };
    let at = game.unit(hero).unwrap().position;
    cast(&mut game, enemy, Slot::W, at, Some(hero));
    game.slow(hero, 0.3, 5.0, Some((enemy, Slot::E)));
    run(&mut game, 1.0);
    assert!(game.unit(hero).unwrap().has_buff("wither"));
    cast(&mut game, hero, Slot::Q, at, None);
    run(&mut game, TICK);
    let unit = game.unit(hero).unwrap();
    assert!(unit.buffs.iter().all(|buff| buff.bonus.slow == 0.0));
    assert!(!unit.has_buff("wither"));
}

#[test]
fn swordsman_recasting_e_after_a_second_stops_the_spin_early() {
    let Some(Duel { mut game, hero, .. }) = duel("Swordsman", "Wanderer", 200.0) else {
        return;
    };
    cast(&mut game, hero, Slot::E, MIDDLE, None);
    run(&mut game, 0.5);
    cast(&mut game, hero, Slot::E, MIDDLE, None);
    assert!(game.unit(hero).unwrap().has_buff("judgment"));
    run(&mut game, 0.6);
    cast(&mut game, hero, Slot::E, MIDDLE, None);
    let hero_unit = game.unit(hero).unwrap();
    assert!(!hero_unit.has_buff("judgment"));
    assert!(hero_unit.ability(Slot::E).cooldown > 0.0);
}

#[test]
fn swordsman_decisive_strike_silences_and_judgment_executes() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Swordsman", "Wanderer", 150.0)
    else {
        return;
    };
    cast(&mut game, hero, Slot::Q, MIDDLE, None);
    game.command(hero, Command::Attack(enemy));
    run(&mut game, 1.0);
    assert!(game.unit(enemy).unwrap().is_silenced());
    let full = game.unit(enemy).unwrap().stats.max_health;
    game.unit_mut(enemy).unwrap().health = full * 0.3;
    let low = health(&game, enemy);
    let position = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::R, position, Some(enemy));
    run(&mut game, 1.0);
    let missing = full - low;
    assert!(low - health(&game, enemy) >= missing * 0.25);
}

fn distance(game: &Game, a: UnitId, b: UnitId) -> f32 {
    game.unit(a)
        .zip(game.unit(b))
        .map_or(f32::MAX, |(a, b)| a.position.distance(b.position))
}

fn stacks(game: &Game, id: UnitId, name: &str) -> u32 {
    game.unit(id)
        .and_then(|unit| unit.buff(name))
        .map_or(0, |buff| buff.stacks)
}

#[test]
fn warlord_decimate_blade_bleeds_and_heals() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Warlord", "Wanderer", 330.0)
    else {
        return;
    };
    let wounded = game.unit(hero).unwrap().stats.max_health * 0.5;
    game.unit_mut(hero).unwrap().health = wounded;
    let before = health(&game, enemy);
    cast(&mut game, hero, Slot::Q, MIDDLE, None);
    run(&mut game, 1.0);
    assert!(health(&game, enemy) < before);
    assert_eq!(stacks(&game, enemy, "hemorrhage"), 1);
    assert!(health(&game, hero) > wounded);
}

#[test]
fn warlord_walks_while_hefting_decimate_and_swings_where_he_ends_up() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Warlord", "Wanderer", 700.0)
    else {
        return;
    };
    let before = health(&game, enemy);
    cast(&mut game, hero, Slot::Q, MIDDLE, None);
    game.command(
        hero,
        Command::Move((MIDDLE + Vec2::new(700.0, 0.0)).to_array()),
    );
    run(&mut game, 0.3);
    let unit = game.unit(hero).unwrap();
    assert!(unit.position.x > MIDDLE.x);
    assert!(unit.ignores_collision());
    run(&mut game, 0.6);
    assert!(health(&game, enemy) < before);
    assert_eq!(stacks(&game, enemy, "hemorrhage"), 1);
}

#[test]
fn warlord_crippling_strike_kill_refunds_half_its_cooldown_and_its_mana() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Warlord", "Wanderer", 150.0)
    else {
        return;
    };
    let range = game.unit(hero).unwrap().stats.attack_range;
    game.unit_mut(enemy).unwrap().health = 1.0;
    let mana = game.unit(hero).unwrap().mana;
    cast(&mut game, hero, Slot::W, MIDDLE, None);
    assert_eq!(game.unit(hero).unwrap().stats.attack_range, range + 25.0);
    game.command(hero, Command::Attack(enemy));
    run(&mut game, 1.0);
    assert!(game.unit(enemy).unwrap().dead);
    let unit = game.unit(hero).unwrap();
    let strike = unit.ability(Slot::W);
    assert!(strike.cooldown < strike.cooldown_total * 0.5);
    assert!(strike.cooldown > 0.0);
    assert!((unit.mana - mana).abs() < 1.0, "{} vs {mana}", unit.mana);
}

#[test]
fn warlord_killing_blows_leave_no_bleed_on_the_corpse() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Warlord", "Wanderer", 150.0)
    else {
        return;
    };
    game.unit_mut(enemy).unwrap().health = 1.0;
    game.command(hero, Command::Attack(enemy));
    run(&mut game, 1.0);
    let corpse = game.unit(enemy).unwrap();
    assert!(corpse.dead);
    assert!(!corpse.has_buff("hemorrhage"));
}

#[test]
fn warlord_five_bleeds_grant_warlords_might() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Warlord", "Wanderer", 150.0)
    else {
        return;
    };
    game.shield(enemy, 100_000.0, 60.0);
    let damage = game.unit(hero).unwrap().stats.attack_damage;
    game.command(hero, Command::Attack(enemy));
    run(&mut game, 9.0);
    assert_eq!(stacks(&game, enemy, "hemorrhage"), 5);
    assert!(game.unit(hero).unwrap().has_buff("warlords_might"));
    assert!(game.unit(hero).unwrap().stats.attack_damage > damage);
}

#[test]
fn warlord_apprehend_pulls_enemies_in_front() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Warlord", "Wanderer", 480.0)
    else {
        return;
    };
    let toward = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::E, toward, None);
    run(&mut game, 0.8);
    assert!(distance(&game, hero, enemy) < 250.0);
}

#[test]
fn warlord_stays_planted_briefly_after_apprehend() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Warlord", "Wanderer", 480.0)
    else {
        return;
    };
    let toward = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::E, toward, None);
    game.command(
        hero,
        Command::Move((MIDDLE - Vec2::new(500.0, 0.0)).to_array()),
    );
    run(&mut game, 0.6);
    assert_eq!(game.unit(hero).unwrap().position, MIDDLE);
    run(&mut game, 0.3);
    assert!(game.unit(hero).unwrap().position.x < MIDDLE.x);
}

#[test]
fn warlord_guillotine_resets_on_kill() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Warlord", "Wanderer", 300.0)
    else {
        return;
    };
    game.unit_mut(enemy).unwrap().health = 50.0;
    let at = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::R, at, Some(enemy));
    run(&mut game, 1.0);
    assert!(game.unit(enemy).unwrap().dead);
    assert_eq!(game.unit(hero).unwrap().ability(Slot::R).cooldown, 0.0);
    assert!(game.unit(hero).unwrap().has_buff("warlords_might"));
    run(&mut game, 21.0);
    assert_eq!(game.unit(hero).unwrap().ability(Slot::R).cooldown, 0.0);
}

#[test]
fn warlord_guillotine_reset_at_low_rank_only_lasts_twenty_seconds() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Warlord", "Wanderer", 300.0)
    else {
        return;
    };
    game.unit_mut(hero).unwrap().ability_mut(Slot::R).rank = 1;
    game.unit_mut(enemy).unwrap().health = 50.0;
    let at = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::R, at, Some(enemy));
    run(&mut game, 1.0);
    assert!(game.unit(enemy).unwrap().dead);
    assert_eq!(game.unit(hero).unwrap().ability(Slot::R).cooldown, 0.0);
    run(&mut game, 20.0);
    assert!(game.unit(hero).unwrap().ability(Slot::R).cooldown > 0.0);
}

#[test]
fn warlord_guillotine_bleeds_survivors() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Warlord", "Wanderer", 300.0)
    else {
        return;
    };
    let at = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::R, at, Some(enemy));
    run(&mut game, 0.45);
    assert!(!game.unit(enemy).unwrap().dead);
    assert_eq!(stacks(&game, enemy, "hemorrhage"), 1);
}

#[test]
fn void_walker_force_pulse_quickens_when_nearby_champions_cast() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("VoidWalker", "Swordsman", 1500.0)
    else {
        return;
    };
    let toward = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::E, toward, None);
    let pulse = |game: &Game| *game.unit(hero).unwrap().ability(Slot::E);
    assert_eq!(pulse(&game).cooldown, pulse(&game).cooldown_total);
    let before = pulse(&game).cooldown;
    cast(&mut game, enemy, Slot::W, toward, None);
    assert!((before - pulse(&game).cooldown - 0.75).abs() < 1e-4);
    game.unit_mut(enemy).unwrap().position = MIDDLE + Vec2::new(2500.0, 0.0);
    let before = pulse(&game).cooldown;
    cast(&mut game, enemy, Slot::Q, toward, None);
    assert_eq!(pulse(&game).cooldown, before);
}

#[test]
fn void_walker_nether_blade_costs_its_mana() {
    let Some(Duel { mut game, hero, .. }) = duel("VoidWalker", "Swordsman", 1000.0) else {
        return;
    };
    let mana = game.unit(hero).unwrap().mana;
    cast(&mut game, hero, Slot::W, MIDDLE, None);
    assert_eq!(game.unit(hero).unwrap().mana, mana - 1.0);
}

#[test]
fn wanderer_abilities_count_as_casts_for_force_pulse() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("VoidWalker", "Wanderer", 1500.0)
    else {
        return;
    };
    let toward = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::E, toward, None);
    let pulse = |game: &Game| game.unit(hero).unwrap().ability(Slot::E).cooldown;
    let before = pulse(&game);
    cast(&mut game, enemy, Slot::Q, MIDDLE, None);
    assert!((before - pulse(&game) - 0.75).abs() < 1e-4);
}

#[test]
fn void_walker_null_sphere_breaks_channels_but_not_windups() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("VoidWalker", "Cardmaster", 500.0)
    else {
        return;
    };
    let gate = MIDDLE + Vec2::new(0.0, 2000.0);
    cast(&mut game, enemy, Slot::R, gate, None);
    cast(&mut game, enemy, Slot::R, gate, None);
    let at = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::Q, at, Some(enemy));
    run(&mut game, 2.0);
    assert!(game.unit(enemy).unwrap().position.distance(at) < 1.0);
    run(&mut game, 8.0);
    game.unit_mut(hero).unwrap().ability_mut(Slot::Q).cooldown = 0.0;
    cast(&mut game, enemy, Slot::Q, MIDDLE, None);
    game.interrupt(enemy, true);
    assert!(matches!(
        game.unit(enemy).unwrap().action,
        crate::unit::Action::Cast { .. }
    ));
}

#[test]
fn void_walker_void_stone_ghosts_him() {
    let Some(Duel { mut game, hero, .. }) = duel("VoidWalker", "Swordsman", 400.0) else {
        return;
    };
    run(&mut game, TICK);
    assert!(game.unit(hero).unwrap().ignores_collision());
}

#[test]
fn void_walker_riftwalk_scales_with_max_mana() {
    let Some(Duel { game, hero, .. }) = duel("VoidWalker", "Swordsman", 400.0) else {
        return;
    };
    let spell = super::spell_of(&game, hero, Slot::R).unwrap();
    let mana = game.unit(hero).unwrap().stats.max_mana;
    let base = super::calculate(&game, hero, &spell, 3, "BaseDamage");
    let stack = super::calculate(&game, hero, &spell, 3, "BonusDamage");
    assert!((base - spell.value("RBaseDamage", 3) - 0.02 * mana).abs() < 0.01);
    assert!((stack - spell.value("StackDamage", 3) - 0.01 * mana).abs() < 0.01);
}

#[test]
fn wanderer_steel_tempest_shortens_linearly_with_bonus_attack_speed() {
    let Some(Duel { mut game, hero, .. }) = duel("Wanderer", "Swordsman", 2000.0) else {
        return;
    };
    let windup = |game: &Game| match game.unit(hero).unwrap().action {
        crate::unit::Action::Cast { windup, .. } => windup,
        _ => 0.0,
    };
    let bonus = |game: &Game| {
        let stats = game.unit(hero).unwrap().stats;
        stats.attack_speed / stats.base_attack_speed - 1.0
    };
    for extra in [0.0, 0.6, 2.0] {
        let mut speed = game.buff("speed", f32::MAX);
        speed.bonus.attack_speed = extra;
        game.add_buff(hero, speed);
        let bonus = bonus(&game);
        cast(&mut game, hero, Slot::Q, MIDDLE + Vec2::X, None);
        let expected = (0.35 - 0.035 * bonus / 0.24).max(0.175);
        assert!((windup(&game) - expected).abs() < 1e-4, "{bonus}");
        let cooldown = game.unit(hero).unwrap().ability(Slot::Q).cooldown;
        let expected = (4.0 * (1.0 - bonus / 1.67)).max(1.33);
        assert!((cooldown - expected).abs() < 1e-3, "{cooldown} {bonus}");
        run(&mut game, 5.0);
    }
}

#[test]
fn wanderer_last_breath_lands_behind_the_target_and_resets_the_storm() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Wanderer", "Swordsman", 900.0)
    else {
        return;
    };
    let mut storm = game.buff("gathering_storm", 6.0);
    storm.stacks = 1;
    game.add_buff(hero, storm);
    game.knock_up(enemy, 1.0, None);
    let at = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::R, at + Vec2::new(0.0, 1200.0), None);
    assert_eq!(game.unit(hero).unwrap().position, MIDDLE);
    cast(&mut game, hero, Slot::R, at + Vec2::new(0.0, 300.0), None);
    let unit = game.unit(hero).unwrap();
    assert!(unit.position.x > at.x);
    assert!(!unit.has_buff("gathering_storm"));
    assert!(unit.ability(Slot::R).cooldown > 0.0);
}

#[test]
fn wanderer_last_breath_crits_ignore_sixty_percent_of_bonus_armor() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Wanderer", "Swordsman", 900.0)
    else {
        return;
    };
    game.shield(enemy, 100_000.0, 60.0);
    let mut plate = game.buff("plate", f32::MAX);
    plate.bonus.armor = 100.0;
    game.add_buff(enemy, plate);
    game.knock_up(enemy, 1.0, None);
    let at = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::R, at, None);
    assert!(game.unit(hero).unwrap().has_buff("last_breath"));
    let normal = game.damage(Some(hero), enemy, 100.0, DamageKind::Physical, false);
    let critical = game.damage(Some(hero), enemy, 100.0, DamageKind::Physical, true);
    let armor = game.unit(enemy).unwrap().stats.armor;
    let ratio =
        crate::stats::resist_multiplier(armor - 60.0) / crate::stats::resist_multiplier(armor);
    assert!(
        (critical / normal - ratio).abs() < 1e-4,
        "{critical} {normal} {ratio}"
    );
}

#[test]
fn wanderer_doubles_crit_and_turns_the_overflow_into_attack_damage() {
    let Some(Duel { mut game, hero, .. }) = duel("Wanderer", "Swordsman", 1000.0) else {
        return;
    };
    run(&mut game, TICK);
    let base = game.unit(hero).unwrap().clone();
    let mut edge = game.buff("edge", f32::MAX);
    edge.bonus.crit_chance = 0.3;
    game.add_buff(hero, edge);
    run(&mut game, TICK);
    assert!((game.unit(hero).unwrap().stats.crit_chance - 0.6).abs() < 1e-5);
    let mut more = game.buff("more", f32::MAX);
    more.bonus.crit_chance = 0.3;
    game.add_buff(hero, more);
    for _ in 0..3 {
        run(&mut game, TICK);
        let stats = game.unit(hero).unwrap().stats;
        assert_eq!(stats.crit_chance, 1.0);
        assert!((stats.attack_damage - base.stats.attack_damage - 10.0).abs() < 1e-3);
        assert!((stats.crit_damage - base.stats.crit_damage).abs() < 1e-5);
    }
    assert!((base.stats.crit_damage - base.base.crit_damage * 0.95).abs() < 1e-5);
}

#[test]
fn snapshots_carry_ally_ultimates_and_respawn_timers() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Swordsman", "Warlord", 500.0)
    else {
        return;
    };
    let state = |game: &Game, viewer, id| {
        game.snapshot(Some(viewer))
            .units
            .into_iter()
            .find(|unit| unit.id == id)
    };
    let ultimate = |game: &Game, viewer| state(game, viewer, hero).and_then(|unit| unit.ultimate);
    assert_eq!(ultimate(&game, hero), Some(0.0));
    assert_eq!(ultimate(&game, enemy), None);
    game.damage(None, hero, 100_000.0, DamageKind::True, false);
    run(&mut game, TICK);
    let respawn = state(&game, hero, hero).and_then(|unit| unit.respawn);
    assert!(respawn.is_some_and(|left| left > 0.0));
}

#[test]
fn snapshots_list_every_relic_with_its_side_and_timer() {
    let Some(Duel { game, .. }) = duel("Swordsman", "Warlord", 500.0) else {
        return;
    };
    let relics = game.snapshot(None).relics;
    assert_eq!(relics.len(), 2);
    assert!(relics.iter().any(|relic| relic.team == Team::Blue));
    assert!(relics.iter().any(|relic| relic.team == Team::Red));
    assert!(
        relics
            .iter()
            .all(|relic| relic.inner && relic.ready_in > 80.0)
    );
}

#[test]
fn health_relics_restore_missing_health_then_beam_allies() {
    let Some(Duel { mut game, hero, .. }) = duel("Swordsman", "Warlord", 3000.0) else {
        return;
    };
    let index = game
        .relics
        .iter()
        .position(|relic| relic.team == Team::Blue)
        .unwrap();
    game.relics[index].ready_at = game.time;
    let pad = game.relics[index].position;
    let max = game.unit(hero).unwrap().stats.max_health;
    let unit = game.unit_mut(hero).unwrap();
    unit.position = pad;
    unit.last_position = pad;
    unit.health = max * 0.5;
    run(&mut game, TICK);
    let after_pickup = health(&game, hero);
    let missing = max * 0.5;
    assert!((after_pickup - (max * 0.5 + missing * 0.08)).abs() < 5.0);
    run(&mut game, 2.6);
    let missing = max - after_pickup;
    assert!(health(&game, hero) - after_pickup > missing * 0.15);
    let left = game.snapshot(None).relics[index].ready_in;
    assert!((left - 89.9).abs() < 0.5, "{left}");
}

#[test]
fn shown_ranges_match_expected() {
    let expected = [
        ("Emperor", [740.0, 525.0, 1100.0, 250.0]),
        ("Cardmaster", [1450.0, 0.0, 0.0, 5500.0]),
        ("Wanderer", [450.0, 250.0, 475.0, 1400.0]),
        ("Swordsman", [0.0, 0.0, 325.0, 400.0]),
        ("Warlord", [460.0, 0.0, 535.0, 460.0]),
        ("Colossus", [625.0, 0.0, 400.0, 1000.0]),
        ("Guardian", [0.0, 700.0, 650.0, 0.0]),
        ("VoidWalker", [650.0, 0.0, 600.0, 500.0]),
        ("Sorcerer", [1000.0, 950.0, 725.0, 650.0]),
    ];
    for (name, ranges) in expected {
        let Some(Duel { game, hero, .. }) = duel(name, "Swordsman", 1000.0) else {
            return;
        };
        let unit = game.unit(hero).unwrap();
        for (slot, range) in SLOTS.into_iter().zip(ranges) {
            let shown = super::cast_range(&game, unit, slot);
            let melee = if range > 0.0 { unit.radius() } else { 0.0 };
            assert!(
                (shown - range).abs() < 1.0 || (shown - range - melee).abs() < 1.0,
                "{name} {slot:?} shows {shown}, expected {range}"
            );
        }
    }
}

#[test]
fn void_walker_null_sphere_shields_him_and_hurts_the_target() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("VoidWalker", "Swordsman", 500.0)
    else {
        return;
    };
    let before = health(&game, enemy);
    let toward = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::Q, toward, Some(enemy));
    run(&mut game, 0.3);
    assert!(!game.unit(hero).unwrap().shields.is_empty());
    run(&mut game, 0.7);
    assert!(health(&game, enemy) < before);
}

#[test]
fn void_walker_nether_blade_restores_mana_on_a_champion() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("VoidWalker", "Swordsman", 150.0)
    else {
        return;
    };
    game.unit_mut(hero).unwrap().mana = 10.0;
    cast(&mut game, hero, Slot::W, MIDDLE, None);
    game.command(hero, Command::Attack(enemy));
    run(&mut game, 1.0);
    let hero_unit = game.unit(hero).unwrap();
    assert!(!hero_unit.has_buff("nether_blade"));
    assert!(hero_unit.mana > hero_unit.stats.max_mana * 0.15);
}

#[test]
fn void_walker_force_pulse_slows_enemies_in_front() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("VoidWalker", "Swordsman", 450.0)
    else {
        return;
    };
    let speed = game.unit(enemy).unwrap().stats.move_speed;
    let toward = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::E, toward, None);
    run(&mut game, 0.4);
    assert!(game.unit(enemy).unwrap().stats.move_speed < speed * 0.6);
}

#[test]
fn void_walker_glows_only_while_force_pulse_is_ready() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("VoidWalker", "Swordsman", 450.0)
    else {
        return;
    };
    run(&mut game, 0.1);
    assert!(game.unit(hero).unwrap().has_buff("force_pulse_ready"));
    let toward = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::E, toward, None);
    run(&mut game, 0.4);
    assert!(!game.unit(hero).unwrap().has_buff("force_pulse_ready"));
}

#[test]
fn void_walker_riftwalk_blinks_and_doubles_its_cost() {
    let Some(Duel { mut game, hero, .. }) = duel("VoidWalker", "Swordsman", 2000.0) else {
        return;
    };
    let start = game.unit(hero).unwrap().position;
    cast(
        &mut game,
        hero,
        Slot::R,
        start + Vec2::new(0.0, 2000.0),
        None,
    );
    run(&mut game, 0.5);
    let landed = game.unit(hero).unwrap().position;
    assert!((landed.distance(start) - 500.0).abs() < 1.0);
    game.unit_mut(hero).unwrap().ability_mut(Slot::R).cooldown = 0.0;
    let mana = game.unit(hero).unwrap().mana;
    cast(
        &mut game,
        hero,
        Slot::R,
        landed + Vec2::new(0.0, 300.0),
        None,
    );
    let spent = mana - game.unit(hero).unwrap().mana;
    let base = super::spell_of(&game, hero, Slot::R).unwrap().mana(3);
    assert!((spent - base * 2.0).abs() < 0.01);
}

#[test]
fn sorcerer_baleful_strike_stops_after_two_targets() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Sorcerer", "Swordsman", 300.0)
    else {
        return;
    };
    let players = [
        Entrant::new("second", Team::Red, Some(0)),
        Entrant::new("third", Team::Red, Some(1)),
    ];
    let extra = Game::new(game.data.clone(), Layout::placeholder(), &players, 7);
    let mut others = Vec::new();
    let champions = extra
        .units
        .into_iter()
        .filter(|unit| unit.champion().is_some());
    for (offset, mut unit) in [500.0, 700.0].into_iter().zip(champions) {
        unit.id = game.allocate();
        unit.position = MIDDLE + Vec2::new(offset, 0.0);
        others.push(unit.id);
        game.units.push(unit);
    }
    let healths: Vec<f32> = [enemy, others[0], others[1]]
        .iter()
        .map(|id| health(&game, *id))
        .collect();
    cast(
        &mut game,
        hero,
        Slot::Q,
        MIDDLE + Vec2::new(900.0, 0.0),
        None,
    );
    run(&mut game, 1.0);
    assert!(health(&game, enemy) < healths[0]);
    assert!(health(&game, others[0]) < healths[1]);
    assert!((health(&game, others[1]) - healths[2]).abs() < 1.0);
    assert!(game.unit(hero).unwrap().counter >= 2.0);
}

#[test]
fn sorcerer_dark_matter_lands_after_a_delay() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Sorcerer", "Swordsman", 600.0)
    else {
        return;
    };
    let before = health(&game, enemy);
    let at = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::W, at, None);
    run(&mut game, 1.0);
    assert!((health(&game, enemy) - before).abs() < 1.0);
    run(&mut game, 0.8);
    assert!(health(&game, enemy) < before);
}

#[test]
fn sorcerer_event_horizon_stuns_enemies_on_its_edge() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Sorcerer", "Swordsman", 500.0)
    else {
        return;
    };
    let at = game.unit(enemy).unwrap().position - Vec2::new(380.0, 0.0);
    cast(&mut game, hero, Slot::E, at, None);
    run(&mut game, 1.0);
    assert!(game.unit(enemy).unwrap().has_buff("stun"));
}

#[test]
fn sorcerer_primordial_burst_hits_harder_on_wounded_targets() {
    let burst = |fraction: f32| {
        let Duel {
            mut game,
            hero,
            enemy,
        } = duel("Sorcerer", "Swordsman", 500.0)?;
        let full = game.unit(enemy)?.stats.max_health;
        game.unit_mut(enemy)?.health = full * fraction;
        game.shield(enemy, 100_000.0, 60.0);
        let at = game.unit(enemy)?.position;
        cast(&mut game, hero, Slot::R, at, Some(enemy));
        run(&mut game, 2.0);
        game.events
            .iter()
            .filter_map(|event| match event {
                protocol::Event::Damage { target, amount, .. } if *target == enemy => Some(*amount),
                _ => None,
            })
            .reduce(f32::max)
    };
    let (Some(healthy), Some(wounded)) = (burst(1.0), burst(0.3)) else {
        return;
    };
    assert!(healthy > 0.0);
    assert!(wounded > healthy * 1.9);
}

#[test]
fn emperor_soldiers_stab_350_past_the_target_edge() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Emperor", "Swordsman", 800.0)
    else {
        return;
    };
    cast(
        &mut game,
        hero,
        Slot::W,
        MIDDLE + Vec2::new(400.0, 0.0),
        None,
    );
    run(&mut game, 0.6);
    let before = health(&game, enemy);
    game.command(hero, Command::Attack(enemy));
    run(&mut game, 1.5);
    assert!(health(&game, enemy) < before);
    assert!(game.unit(hero).unwrap().position.distance(MIDDLE) < 1.0);
}

#[test]
fn emperor_points_his_soldiers_at_the_target_instead_of_swinging_his_staff() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Emperor", "Swordsman", 800.0)
    else {
        return;
    };
    cast(
        &mut game,
        hero,
        Slot::W,
        MIDDLE + Vec2::new(400.0, 0.0),
        None,
    );
    run(&mut game, 0.6);
    game.command(hero, Command::Attack(enemy));
    run(&mut game, 0.1);
    let animation = game.unit(hero).unwrap().animation.unwrap();
    assert_eq!(animation.clip, asset::clip_id("Attack_Command"));
}

#[test]
fn emperor_spears_pierce_fifty_units_past_the_target() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Emperor", "Swordsman", 800.0)
    else {
        return;
    };
    let near = dummy(&mut game, Team::Red, MIDDLE + Vec2::new(840.0, 0.0));
    let far = dummy(&mut game, Team::Red, MIDDLE + Vec2::new(940.0, 0.0));
    cast(
        &mut game,
        hero,
        Slot::W,
        MIDDLE + Vec2::new(520.0, 0.0),
        None,
    );
    run(&mut game, 0.6);
    let (near_health, far_health) = (health(&game, near), health(&game, far));
    game.command(hero, Command::Attack(enemy));
    run(&mut game, 1.0);
    assert!(health(&game, near) < near_health);
    assert_eq!(health(&game, far), far_health);
}

#[test]
fn emperor_emperors_divide_shoves_enemies_and_walls_them_off() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Emperor", "Swordsman", 200.0)
    else {
        return;
    };
    let before = health(&game, enemy);
    cast(
        &mut game,
        hero,
        Slot::R,
        MIDDLE + Vec2::new(500.0, 0.0),
        None,
    );
    run(&mut game, 1.5);
    assert!(health(&game, enemy) < before);
    let shoved = game.unit(enemy).unwrap().position;
    assert!((shoved.x - MIDDLE.x - 650.0).abs() < 20.0, "{shoved:?}");
    let guards = game
        .units
        .iter()
        .filter(|unit| unit.kind == protocol::UnitKind::Soldier && unit.alive())
        .count();
    assert_eq!(guards, 8);
    assert!(super::emperor::soldiers(&game, hero).is_empty());
    game.command(enemy, Command::Move(MIDDLE.to_array()));
    run(&mut game, 2.0);
    assert!(game.unit(enemy).unwrap().position.x > MIDDLE.x + 400.0);
    game.command(
        hero,
        Command::Move((MIDDLE + Vec2::new(800.0, 0.0)).to_array()),
    );
    run(&mut game, 2.0);
    assert!(game.unit(hero).unwrap().position.x > MIDDLE.x + 450.0);
    run(&mut game, 3.0);
    game.command(enemy, Command::Move(MIDDLE.to_array()));
    run(&mut game, 2.0);
    assert!(game.unit(enemy).unwrap().position.x < MIDDLE.x + 300.0);
}

#[test]
fn emperor_keeps_every_soldier_he_raises() {
    let Some(Duel { mut game, hero, .. }) = duel("Emperor", "Swordsman", 2000.0) else {
        return;
    };
    for step in 0..5 {
        game.unit_mut(hero).unwrap().ability_mut(Slot::W).charges = 1;
        game.unit_mut(hero).unwrap().ability_mut(Slot::W).cooldown = 0.0;
        cast(
            &mut game,
            hero,
            Slot::W,
            MIDDLE + Vec2::new(300.0, step as f32 * 80.0),
            None,
        );
        run(&mut game, 0.5);
    }
    let soldiers = game
        .units
        .iter()
        .filter(|unit| unit.kind == protocol::UnitKind::Soldier && unit.alive())
        .count();
    assert_eq!(soldiers, 5);
}

#[test]
fn emperor_conquering_sands_hits_once_and_slows() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Emperor", "Swordsman", 600.0)
    else {
        return;
    };
    let toward = game.unit(enemy).unwrap().position;
    for offset in [-60.0, 60.0] {
        cast(
            &mut game,
            hero,
            Slot::W,
            MIDDLE + Vec2::new(150.0, offset),
            None,
        );
        run(&mut game, 0.6);
    }
    let speed = game.unit(enemy).unwrap().stats.move_speed;
    let before = health(&game, enemy);
    game.events.clear();
    cast(&mut game, hero, Slot::Q, toward, None);
    run(&mut game, 0.3);
    let hits = game
        .events
        .iter()
        .filter(|event| {
            matches!(event, protocol::Event::Damage { source: Some(source), target, .. }
                if *source == hero && *target == enemy)
        })
        .count();
    assert_eq!(hits, 1);
    assert!(health(&game, enemy) < before);
    assert!(game.unit(enemy).unwrap().stats.move_speed < speed * 0.8);
}

#[test]
fn swordsman_decisive_strike_hits_for_its_total_damage() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Swordsman", "Wanderer", 150.0)
    else {
        return;
    };
    let spell = super::spell_of(&game, hero, Slot::Q).unwrap();
    let attack_damage = game.unit(hero).unwrap().stats.attack_damage;
    let raw = spell.value("BaseDamage", 3) + spell.value("tADRatio", 3) * attack_damage;
    let expected = raw * crate::stats::resist_multiplier(game.unit(enemy).unwrap().stats.armor);
    cast(&mut game, hero, Slot::Q, MIDDLE, None);
    game.events.clear();
    game.command(hero, Command::Attack(enemy));
    run(&mut game, 1.0);
    let dealt = game
        .events
        .iter()
        .find_map(|event| match event {
            protocol::Event::Damage {
                source: Some(source),
                target,
                amount,
                ..
            } if *source == hero && *target == enemy => Some(*amount),
            _ => None,
        })
        .unwrap();
    assert!(
        (dealt - expected).abs() < 1.0,
        "dealt {dealt} expected {expected}"
    );
}

#[test]
fn cardmaster_unused_pick_a_card_and_destiny_go_on_cooldown() {
    let Some(Duel { mut game, hero, .. }) = duel("Cardmaster", "Swordsman", 2000.0) else {
        return;
    };
    cast(&mut game, hero, Slot::W, MIDDLE, None);
    cast(&mut game, hero, Slot::R, MIDDLE, None);
    run(&mut game, 1.0);
    let ability = |game: &Game, slot| game.unit(hero).unwrap().ability(slot).cooldown;
    assert_eq!(ability(&game, Slot::W), 0.0);
    assert_eq!(ability(&game, Slot::R), 0.0);
    run(&mut game, 10.0);
    let unit = game.unit(hero).unwrap();
    assert!(!unit.has_buff("pick_a_card") && !unit.has_buff("destiny"));
    assert!(
        ["card_blue", "card_red", "card_gold"]
            .iter()
            .all(|card| !unit.has_buff(card))
    );
    assert!(ability(&game, Slot::W) > 0.0);
    assert!(ability(&game, Slot::R) > 0.0);
}

#[test]
fn cardmaster_stuns_break_the_gate_channel() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Cardmaster", "Swordsman", 4000.0)
    else {
        return;
    };
    let target = MIDDLE + Vec2::new(0.0, 2000.0);
    cast(&mut game, hero, Slot::R, target, None);
    cast(&mut game, hero, Slot::R, target, None);
    run(&mut game, 0.5);
    let held = game.unit(hero).unwrap().animation;
    assert!(held.is_some_and(|animation| animation.hold));
    game.stun(hero, 0.5, Some((enemy, Slot::Q)));
    run(&mut game, 1.5);
    let unit = game.unit(hero).unwrap();
    assert_eq!(unit.position, MIDDLE);
    assert!(unit.animation.is_none_or(|animation| !animation.hold));
}

#[test]
fn cardmaster_destiny_reveals_enemy_champions() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Cardmaster", "Swordsman", 4000.0)
    else {
        return;
    };
    cast(&mut game, hero, Slot::R, MIDDLE, None);
    run(&mut game, 0.5);
    let time = game.time;
    assert!(game.unit(enemy).unwrap().revealed_until > time + 9.0);
    assert!(game.unit(hero).unwrap().revealed_until <= time);
}

#[test]
fn cardmaster_wild_cards_fly_at_card_speed() {
    let Some(Duel { mut game, hero, .. }) = duel("Cardmaster", "Swordsman", 2000.0) else {
        return;
    };
    cast(
        &mut game,
        hero,
        Slot::Q,
        MIDDLE + Vec2::new(500.0, 0.0),
        None,
    );
    run(&mut game, 0.5);
    let speeds: Vec<f32> = game
        .missiles
        .iter()
        .filter(|missile| missile.owner == hero)
        .map(|missile| missile.speed)
        .collect();
    assert_eq!(speeds.len(), 3, "{speeds:?}");
    assert!(speeds.iter().all(|speed| (speed - 1000.0).abs() < 1.0));
}

#[test]
fn guardian_soul_eater_life_steal_grows_at_seven_and_thirteen() {
    let Some(Duel { mut game, hero, .. }) = duel("Guardian", "Swordsman", 4000.0) else {
        return;
    };
    for (level, expected) in [(1, 0.1), (7, 0.15), (13, 0.2)] {
        game.unit_mut(hero).unwrap().level = level;
        run(&mut game, 0.1);
        let unit = game.unit(hero).unwrap();
        let life_steal = unit.buff("soul_eater").unwrap().bonus.life_steal;
        assert!(
            (life_steal - expected).abs() < 1e-6,
            "{level}: {life_steal}"
        );
    }
}

#[test]
fn guardian_wither_ramps_each_second_from_when_it_lands() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Guardian", "Swordsman", 400.0)
    else {
        return;
    };
    let at = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::W, at, Some(enemy));
    let mut slows = Vec::new();
    for _ in 0..(6.0 / TICK) as u32 {
        run(&mut game, TICK);
        if let Some(wither) = game.unit(enemy).unwrap().buff("wither") {
            let elapsed = wither.duration - (wither.expires - game.time);
            slows.push((elapsed, wither.bonus.slow, wither.bonus.cripple));
        }
    }
    let at = |time: f32| {
        slows
            .iter()
            .find(|(elapsed, ..)| *elapsed >= time)
            .map(|&(_, slow, cripple)| (slow, cripple))
            .unwrap()
    };
    let close = |a: f32, b: f32| (a - b).abs() < 1e-4;
    assert!(close(at(0.5).0, 0.35));
    assert!(close(at(1.5).0, 0.44));
    assert!(close(at(4.5).0, 0.71));
    assert!(close(at(4.5).1, 0.71 * 0.75));
}

#[test]
fn guardian_fury_burns_a_share_of_max_health_scaling_with_ability_power() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Guardian", "Swordsman", 420.0)
    else {
        return;
    };
    let mut power = game.buff("power", f32::MAX);
    power.bonus.ability_power = 100.0;
    game.add_buff(hero, power);
    let range = game.unit(hero).unwrap().stats.attack_range;
    cast(&mut game, hero, Slot::R, MIDDLE, None);
    run(&mut game, TICK);
    assert!(!game.unit(hero).unwrap().has_buff("fury"));
    while !game.unit(hero).unwrap().has_buff("fury") {
        run(&mut game, TICK);
    }
    assert_eq!(game.unit(hero).unwrap().stats.attack_range, range + 50.0);
    let snapshot = game.snapshot(None);
    let shown = snapshot.units.iter().find(|unit| unit.id == hero).unwrap();
    assert!((shown.size - 1.4).abs() < 1e-5);
    let before = game.unit(enemy).unwrap().health;
    run(&mut game, 0.5);
    let hero_unit = game.unit(enemy).unwrap();
    let balance = game.balance(hero).damage_dealt * game.balance(enemy).damage_taken;
    let expected = hero_unit.stats.max_health
        * 0.03
        * balance
        * crate::stats::resist_multiplier(hero_unit.stats.magic_resist);
    let dealt = before - hero_unit.health;
    assert!((dealt - expected).abs() < 2.0, "{dealt} vs {expected}");
}

#[test]
fn guardian_siphoning_strike_reaches_fifty_further() {
    let Some(Duel { mut game, hero, .. }) = duel("Guardian", "Swordsman", 4000.0) else {
        return;
    };
    let range = game.unit(hero).unwrap().stats.attack_range;
    cast(&mut game, hero, Slot::Q, MIDDLE, None);
    run(&mut game, TICK);
    assert_eq!(game.unit(hero).unwrap().stats.attack_range, range + 50.0);
}

#[test]
fn guardian_spirit_fire_bursts_after_a_delay_and_its_shred_lingers() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Guardian", "Swordsman", 500.0)
    else {
        return;
    };
    let at = game.unit(enemy).unwrap().position;
    let health = game.unit(enemy).unwrap().health;
    cast(&mut game, hero, Slot::E, at, None);
    run(&mut game, 0.45);
    assert_eq!(game.unit(enemy).unwrap().health, health);
    run(&mut game, 0.1);
    let hero_unit = game.unit(enemy).unwrap();
    assert!(hero_unit.health < health);
    assert!(hero_unit.has_buff("spirit_fire"));
    game.unit_mut(enemy).unwrap().position = at + Vec2::new(2000.0, 0.0);
    run(&mut game, 0.9);
    assert!(game.unit(enemy).unwrap().has_buff("spirit_fire"));
    run(&mut game, 0.2);
    assert!(!game.unit(enemy).unwrap().has_buff("spirit_fire"));
}

#[test]
fn guardian_keeps_his_current_health_when_fury_ends() {
    let Some(Duel { mut game, hero, .. }) = duel("Guardian", "Swordsman", 4000.0) else {
        return;
    };
    cast(&mut game, hero, Slot::R, MIDDLE, None);
    run(&mut game, 1.0);
    let unit = game.unit_mut(hero).unwrap();
    unit.health = 500.0;
    run(&mut game, 15.0);
    let unit = game.unit(hero).unwrap();
    assert!(!unit.has_buff("fury"));
    assert!(unit.health >= 500.0, "{}", unit.health);
}

#[test]
fn guardian_siphoning_strike_stacks_only_on_its_own_kills() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Guardian", "Swordsman", 150.0)
    else {
        return;
    };
    game.unit_mut(enemy).unwrap().health = 1.0;
    cast(&mut game, hero, Slot::Q, MIDDLE, None);
    game.command(hero, Command::Attack(enemy));
    run(&mut game, 1.5);
    assert!(game.unit(enemy).unwrap().dead);
    let stacks = game.unit(hero).unwrap().counter;
    assert_eq!(stacks, 24.0);
    run(&mut game, 60.0);
    let position = game.unit(hero).unwrap().position + Vec2::new(150.0, 0.0);
    let unit = game.unit_mut(enemy).unwrap();
    assert!(!unit.dead);
    unit.position = position;
    unit.health = 1.0;
    game.command(hero, Command::Attack(enemy));
    run(&mut game, 1.5);
    assert!(game.unit(enemy).unwrap().dead);
    assert_eq!(game.unit(hero).unwrap().counter, stacks);
}

#[test]
fn swordsman_courage_grows_by_the_aram_amount_per_kill() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Swordsman", "Wanderer", 150.0)
    else {
        return;
    };
    let armor = game.unit(hero).unwrap().stats.armor;
    game.damage(
        Some(hero),
        enemy,
        100000.0,
        protocol::DamageKind::True,
        false,
    );
    run(&mut game, 0.1);
    let gained = game.unit(hero).unwrap().stats.armor - armor;
    assert!((gained - 0.4).abs() < 0.01, "gained {gained}");
}

#[test]
fn void_walker_null_sphere_shield_only_blocks_magic_damage() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("VoidWalker", "Swordsman", 500.0)
    else {
        return;
    };
    let toward = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::Q, toward, Some(enemy));
    run(&mut game, 0.3);
    let before = health(&game, hero);
    game.damage(
        Some(enemy),
        hero,
        50.0,
        protocol::DamageKind::Physical,
        false,
    );
    assert!(health(&game, hero) < before);
    let before = health(&game, hero);
    game.damage(Some(enemy), hero, 50.0, protocol::DamageKind::Magic, false);
    assert_eq!(health(&game, hero), before);
}

#[test]
fn warlord_apprehend_passively_grants_armor_penetration() {
    let Some(Duel { mut game, hero, .. }) = duel("Warlord", "Swordsman", 2000.0) else {
        return;
    };
    run(&mut game, 0.1);
    let penetration = game.unit(hero).unwrap().stats.armor_penetration;
    assert!((penetration - 0.3).abs() < 1e-4, "{penetration}");
}

#[test]
fn wanderer_flow_fills_with_distance_actually_walked() {
    let Some(Duel { mut game, hero, .. }) = duel("Wanderer", "Swordsman", 3000.0) else {
        return;
    };
    game.command(
        hero,
        Command::Move((MIDDLE - Vec2::new(0.0, 400.0)).to_array()),
    );
    run(&mut game, 3.0);
    let unit = game.unit(hero).unwrap();
    let walked = unit.position.distance(MIDDLE);
    assert!(walked > 300.0);
    assert!(
        (unit.counter - walked / 59.0).abs() < 0.5,
        "{} {walked}",
        unit.counter
    );
    run(&mut game, 2.0);
    let still = game.unit(hero).unwrap().counter;
    run(&mut game, 1.0);
    assert_eq!(game.unit(hero).unwrap().counter, still);
}

#[test]
fn wanderer_sweeping_blade_ghosts_him_for_two_seconds() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Wanderer", "Swordsman", 300.0)
    else {
        return;
    };
    let at = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::E, at, Some(enemy));
    run(&mut game, 1.9);
    assert!(game.unit(hero).unwrap().ignores_collision());
    run(&mut game, 0.2);
    assert!(!game.unit(hero).unwrap().ignores_collision());
}

#[test]
fn wanderer_can_cast_q_during_sweeping_blade() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Wanderer", "Swordsman", 300.0)
    else {
        return;
    };
    let toward = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::E, toward, Some(enemy));
    run(&mut game, 0.05);
    assert!(matches!(
        game.unit(hero).unwrap().action,
        crate::unit::Action::Dash { .. }
    ));
    cast(&mut game, hero, Slot::Q, toward, None);
    assert!(game.unit(hero).unwrap().ability(Slot::Q).cooldown > 0.0);
    assert!(game.unit(hero).unwrap().has_buff("sweeping_blade_q"));
}

#[test]
fn emperor_follows_a_soldier_commanded_during_shifting_sands() {
    let Some(Duel { mut game, hero, .. }) = duel("Emperor", "Swordsman", 3000.0) else {
        return;
    };
    cast(
        &mut game,
        hero,
        Slot::W,
        MIDDLE + Vec2::new(500.0, 0.0),
        None,
    );
    run(&mut game, 0.6);
    cast(
        &mut game,
        hero,
        Slot::E,
        MIDDLE + Vec2::new(500.0, 0.0),
        None,
    );
    run(&mut game, 0.05);
    cast(
        &mut game,
        hero,
        Slot::Q,
        MIDDLE + Vec2::new(500.0, 600.0),
        None,
    );
    let unit = game.unit(hero).unwrap();
    assert!(unit.ability(Slot::Q).cooldown > 0.0);
    assert!(matches!(unit.action, crate::unit::Action::Dash { .. }));
    run(&mut game, 1.0);
    let landing = game.unit(hero).unwrap().position;
    assert!(landing.distance(MIDDLE + Vec2::new(500.0, 600.0)) < 10.0);
}

#[test]
fn emperor_shifting_sands_stops_on_the_first_champion() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Emperor", "Swordsman", 300.0)
    else {
        return;
    };
    let soldier = MIDDLE + Vec2::new(500.0, 0.0);
    cast(&mut game, hero, Slot::W, soldier, None);
    run(&mut game, 0.6);
    let charges = game.unit(hero).unwrap().ability(Slot::W).charges;
    let before = health(&game, enemy);
    cast(&mut game, hero, Slot::E, soldier, None);
    run(&mut game, 0.6);
    let unit = game.unit(hero).unwrap();
    assert!(health(&game, enemy) < before);
    assert!(unit.position.distance(soldier) > 150.0);
    assert_eq!(unit.ability(Slot::W).charges, charges + 1);
}

#[test]
fn colossus_unstoppable_force_ignores_crowd_control() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Colossus", "Wanderer", 2000.0)
    else {
        return;
    };
    let goal = MIDDLE + Vec2::new(900.0, 0.0);
    cast(&mut game, hero, Slot::R, goal, None);
    run(&mut game, 0.1);
    game.knock_up(hero, 1.0, Some((enemy, Slot::Q)));
    game.stun(hero, 1.0, Some((enemy, Slot::Q)));
    run(&mut game, 0.5);
    let unit = game.unit(hero).unwrap();
    assert!(unit.position.distance(goal) < 5.0, "{}", unit.position);
    assert!(!unit.is_stunned());
}

#[test]
fn colossus_thunderclap_empowers_one_attack_then_cleaves() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Colossus", "Swordsman", 150.0)
    else {
        return;
    };
    cast(&mut game, hero, Slot::W, MIDDLE, None);
    assert_eq!(game.unit(hero).unwrap().stats.attack_range, {
        let unit = game.unit(hero).unwrap();
        unit.base.attack_range + 50.0
    });
    game.command(hero, Command::Attack(enemy));
    run(&mut game, 0.6);
    let unit = game.unit(hero).unwrap();
    assert!(!unit.has_buff("thunderclap_ready"));
    assert!(unit.has_buff("thunderclap"));
    let hits = game
        .events
        .iter()
        .filter(|event| {
            matches!(event, protocol::Event::Damage { source: Some(source), target, .. }
                if *source == hero && *target == enemy)
        })
        .count();
    assert_eq!(hits, 2);
}

#[test]
fn colossus_seismic_shard_steals_the_speed_the_target_lost() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Colossus", "Swordsman", 500.0)
    else {
        return;
    };
    let speed = |game: &Game| game.unit(enemy).unwrap().stats.move_speed;
    let before = speed(&game);
    let at = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::Q, at, Some(enemy));
    run(&mut game, 1.0);
    assert!(game.unit(enemy).unwrap().has_buff("slow"));
    let lost = before - speed(&game);
    let unit = game.unit(hero).unwrap();
    let gained = unit.buff("seismic_haste").unwrap().bonus.move_speed;
    assert!(
        lost > 0.0 && (gained - lost).abs() < 1.0,
        "{gained} vs {lost}"
    );
}

#[test]
fn colossus_seismic_shard_returns_to_him_without_hurting() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Colossus", "Swordsman", 500.0)
    else {
        return;
    };
    let returning = |game: &Game| {
        game.missiles
            .iter()
            .any(|missile| missile.kind == asset::clip_id("SeismicShardReturn"))
    };
    let at = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::Q, at, Some(enemy));
    let mut seen = false;
    for _ in 0..60 {
        run(&mut game, TICK);
        seen |= returning(&game);
        if seen {
            break;
        }
    }
    assert!(seen);
    let health = game.unit(hero).unwrap().health;
    run(&mut game, 1.0);
    assert!(!returning(&game));
    assert!(game.unit(hero).unwrap().health >= health);
}

#[test]
fn colossus_granite_shield_returns_after_its_cooldown() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Colossus", "Swordsman", 2000.0)
    else {
        return;
    };
    run(&mut game, 0.1);
    let granite = |game: &Game| game.unit(hero).unwrap().shield();
    let full = game.unit(hero).unwrap().stats.max_health * 0.1;
    assert!((granite(&game) - full).abs() < 1.0);
    game.damage(
        Some(enemy),
        hero,
        full * 2.0,
        protocol::DamageKind::True,
        false,
    );
    run(&mut game, 7.0);
    assert_eq!(granite(&game), 0.0);
    assert!(!game.unit(hero).unwrap().has_buff("granite_shield"));
    run(&mut game, 1.1);
    assert!((granite(&game) - full).abs() < 1.0);
}

#[test]
fn cardmaster_stacked_deck_fires_on_the_fourth_attack_and_refills_on_respawn() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Cardmaster", "Swordsman", 400.0)
    else {
        return;
    };
    game.unit_mut(enemy).unwrap().health = 1.0e6;
    game.command(hero, Command::Attack(enemy));
    let magic = |game: &Game| {
        game.events
            .iter()
            .filter(|event| {
                matches!(event, protocol::Event::Damage { source: Some(source), kind: protocol::DamageKind::Magic, .. }
                    if *source == hero)
            })
            .count()
    };
    while game.unit(hero).unwrap().counter < 3.0 {
        run(&mut game, TICK);
    }
    assert_eq!(magic(&game), 0);
    while game.unit(hero).unwrap().counter > 0.0 {
        run(&mut game, TICK);
    }
    assert_eq!(magic(&game), 1);
    game.damage(Some(enemy), hero, 1.0e6, protocol::DamageKind::True, false);
    run(&mut game, 60.0);
    assert_eq!(game.unit(hero).unwrap().counter, 3.0);
}

#[test]
fn swordsman_perseverance_ignores_minion_hits_but_not_champions() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Swordsman", "Wanderer", 3000.0)
    else {
        return;
    };
    while !game.units.iter().any(|unit| unit.is_minion()) {
        run(&mut game, 1.0);
    }
    let team = game.unit(hero).unwrap().team;
    let minion = game
        .units
        .iter()
        .find(|unit| unit.is_minion() && unit.team != team)
        .map(|unit| unit.id)
        .unwrap();
    let full = game.unit(hero).unwrap().stats.max_health;
    let regen = game.unit(hero).unwrap().stats.health_regen;
    let gain = |game: &mut Game, from: UnitId| {
        game.unit_mut(hero).unwrap().health = full * 0.5;
        game.damage(Some(from), hero, 1.0, protocol::DamageKind::True, false);
        let before = health(game, hero);
        run(game, 1.0);
        health(game, hero) - before - regen
    };
    assert!(gain(&mut game, minion) > 1.0);
    assert!(gain(&mut game, enemy) < 0.5);
}

#[test]
fn swordsman_sparkles_while_perseverance_is_active() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Swordsman", "Wanderer", 3000.0)
    else {
        return;
    };
    run(&mut game, 0.1);
    assert!(game.unit(hero).unwrap().has_buff("perseverance"));
    game.damage(Some(enemy), hero, 1.0, protocol::DamageKind::True, false);
    run(&mut game, 7.5);
    assert!(!game.unit(hero).unwrap().has_buff("perseverance"));
    run(&mut game, 1.0);
    assert!(game.unit(hero).unwrap().has_buff("perseverance"));
}

#[test]
fn swordsman_courage_glows_as_long_as_its_damage_reduction() {
    let Some(Duel { mut game, hero, .. }) = duel("Swordsman", "Wanderer", 3000.0) else {
        return;
    };
    let glowing = |game: &Game| game.unit(hero).unwrap().has_buff("courage_glow");
    cast(&mut game, hero, Slot::W, MIDDLE, None);
    run(&mut game, 0.1);
    let guard = game
        .unit(hero)
        .unwrap()
        .buff("courage_guard")
        .unwrap()
        .expires
        - game.time;
    assert!(glowing(&game));
    run(&mut game, guard - 0.1);
    assert!(glowing(&game));
    run(&mut game, 0.2);
    assert!(!glowing(&game));
}

#[test]
fn spellblade_hands_glow_until_the_empowered_attack_lands() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Swordsman", "Wanderer", 150.0)
    else {
        return;
    };
    game.unit_mut(hero).unwrap().items[0] = Some(crate::items::BANE_BLADE);
    let glowing = |game: &Game| game.unit(hero).unwrap().has_buff("spellblade_glow");
    run(&mut game, 0.1);
    assert!(!glowing(&game));
    cast(&mut game, hero, Slot::W, MIDDLE, None);
    run(&mut game, 0.1);
    assert!(glowing(&game));
    game.command(hero, Command::Attack(enemy));
    run(&mut game, 3.0);
    assert!(!glowing(&game));
}

#[test]
fn spellblade_fades_ten_seconds_after_the_last_cast() {
    let Some(Duel { mut game, hero, .. }) = duel("Swordsman", "Wanderer", 3000.0) else {
        return;
    };
    game.unit_mut(hero).unwrap().items[0] = Some(crate::items::SPELLBLADE);
    let charged = |game: &Game| game.unit(hero).unwrap().has_buff("spellblade");
    cast(&mut game, hero, Slot::W, MIDDLE, None);
    run(&mut game, 9.5);
    assert!(charged(&game));
    run(&mut game, 1.0);
    assert!(!charged(&game));
}

#[test]
fn thorn_armor_hurts_and_wounds_champions_that_attack_it() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Swordsman", "Wanderer", 150.0)
    else {
        return;
    };
    game.unit_mut(enemy).unwrap().items[0] = Some(crate::items::THORN_ARMOR);
    let before = health(&game, hero);
    game.command(hero, Command::Attack(enemy));
    run(&mut game, 3.0);
    assert!(before - health(&game, hero) >= 15.0);
    assert!(game.unit(hero).unwrap().has_buff("grievous_wounds"));
}

#[test]
fn leviathan_slayer_brings_down_every_third_hit_harder_on_wounded_targets() {
    let Some(Duel { mut game, hero, .. }) = duel("Swordsman", "Wanderer", 3000.0) else {
        return;
    };
    game.unit_mut(hero).unwrap().items[0] = Some(crate::items::LEVIATHAN_SLAYER);
    let (team, position) = game
        .unit(hero)
        .map(|unit| (unit.team.enemy(), unit.position))
        .unwrap();
    let target = dummy(&mut game, team, position + Vec2::new(100.0, 0.0));
    let full = game.unit(target).unwrap().stats.max_health;
    game.unit_mut(target).unwrap().health = full * 0.5;
    let stacks = |game: &Game| {
        game.unit(hero)
            .unwrap()
            .buff("bring_it_down")
            .map_or(0, |buff| buff.stacks)
    };
    crate::items::on_attack_hit(&mut game, hero, target);
    crate::items::on_attack_hit(&mut game, hero, target);
    assert_eq!(stacks(&game), 2);
    assert_eq!(health(&game, target), full * 0.5);
    crate::items::on_attack_hit(&mut game, hero, target);
    assert_eq!(stacks(&game), 0);
    assert!(full * 0.5 - health(&game, target) > 150.0);
}

#[test]
fn leviathan_slayer_glows_while_the_third_hit_is_ready() {
    let Some(Duel { mut game, hero, .. }) = duel("Swordsman", "Wanderer", 3000.0) else {
        return;
    };
    game.unit_mut(hero).unwrap().items[0] = Some(crate::items::LEVIATHAN_SLAYER);
    let (team, position) = game
        .unit(hero)
        .map(|unit| (unit.team.enemy(), unit.position))
        .unwrap();
    let target = dummy(&mut game, team, position + Vec2::new(100.0, 0.0));
    let glowing = |game: &Game| game.unit(hero).unwrap().has_buff("bring_it_down_glow");
    crate::items::on_attack_hit(&mut game, hero, target);
    run(&mut game, 0.1);
    assert!(!glowing(&game));
    crate::items::on_attack_hit(&mut game, hero, target);
    run(&mut game, 0.1);
    assert!(glowing(&game));
    crate::items::on_attack_hit(&mut game, hero, target);
    run(&mut game, 0.1);
    assert!(!glowing(&game));
}

fn ruin(game: &mut Game, team: Team, tier: crate::unit::Tier) -> (UnitId, Vec2) {
    let (turret, position) = game
        .units
        .iter()
        .find(|unit| unit.team == team && unit.tier == Some(tier))
        .map(|unit| (unit.id, unit.position))
        .unwrap();
    game.unit_mut(turret).unwrap().invulnerable = false;
    game.damage(None, turret, 1.0e6, DamageKind::True, false);
    (turret, position)
}

fn sun_disc(game: &Game) -> Option<&crate::unit::Unit> {
    game.units
        .iter()
        .find(|unit| unit.kind == protocol::UnitKind::SunDisc && unit.alive())
}

#[test]
fn emperor_raises_a_sun_disc_from_turret_ruins() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Emperor", "Swordsman", 3000.0)
    else {
        return;
    };
    let (turret, position) = ruin(&mut game, Team::Red, crate::unit::Tier::Outer);
    assert_eq!(game.snapshot(Some(hero)).interactables, vec![turret]);
    assert!(game.snapshot(Some(enemy)).interactables.is_empty());
    game.unit_mut(hero).unwrap().position = position - Vec2::new(1200.0, 0.0);
    game.command(hero, Command::Attack(turret));
    run(&mut game, 4.0);
    let disc = sun_disc(&game).unwrap();
    assert_eq!(disc.position, position);
    assert_eq!(disc.team, Team::Blue);
    assert_eq!(disc.owner, Some(hero));
    let snapshot = game.snapshot(Some(hero));
    assert!(snapshot.interactables.is_empty());
    let (remaining, total) = snapshot.you.unwrap().passive_cooldown.unwrap();
    assert_eq!(total, 90.0);
    assert!(remaining > 85.0);
}

#[test]
fn sun_discs_assemble_before_shooting_magic_damage() {
    let Some(Duel { mut game, hero, .. }) = duel("Emperor", "Swordsman", 3000.0) else {
        return;
    };
    let (turret, position) = ruin(&mut game, Team::Red, crate::unit::Tier::Outer);
    game.unit_mut(hero).unwrap().position = position - Vec2::new(500.0, 0.0);
    game.command(hero, Command::Attack(turret));
    run(&mut game, 0.6);
    let target = dummy(&mut game, Team::Red, position + Vec2::new(400.0, 0.0));
    let full = health(&game, target);
    run(&mut game, 4.0);
    assert_eq!(health(&game, target), full);
    run(&mut game, 2.0);
    assert!(health(&game, target) < full);
}

#[test]
fn sun_discs_decay_and_emperor_waits_for_the_cooldown() {
    let Some(Duel { mut game, hero, .. }) = duel("Emperor", "Swordsman", 3000.0) else {
        return;
    };
    let (turret, position) = ruin(&mut game, Team::Red, crate::unit::Tier::Outer);
    game.unit_mut(hero).unwrap().position = position - Vec2::new(500.0, 0.0);
    game.command(hero, Command::Attack(turret));
    run(&mut game, 0.6);
    assert!(sun_disc(&game).is_some());
    run(&mut game, 51.0);
    assert!(sun_disc(&game).is_none());
    game.command(hero, Command::Attack(turret));
    run(&mut game, 0.6);
    assert!(sun_disc(&game).is_none());
    run(&mut game, 40.0);
    game.command(hero, Command::Attack(turret));
    run(&mut game, 0.6);
    assert!(sun_disc(&game).is_some());
}

#[test]
fn sun_discs_crumble_over_forty_five_seconds() {
    let Some(Duel { mut game, hero, .. }) = duel("Emperor", "Swordsman", 3000.0) else {
        return;
    };
    let (turret, position) = ruin(&mut game, Team::Red, crate::unit::Tier::Outer);
    game.unit_mut(hero).unwrap().position = position - Vec2::new(500.0, 0.0);
    game.command(hero, Command::Attack(turret));
    run(&mut game, 0.5 + 5.0 + 9.0);
    let disc = sun_disc(&game).unwrap();
    assert_eq!(disc.stats.max_health, 1750.0);
    assert!((disc.health / disc.stats.max_health - 0.8).abs() < 0.02);
    assert!(disc.stats.attack_damage >= 230.0);
    assert!(disc.stats.armor >= 30.0);
}

#[test]
fn turrets_leave_sun_discs_alone() {
    let Some(Duel { mut game, hero, .. }) = duel("Emperor", "Swordsman", 3000.0) else {
        return;
    };
    let (turret, position) = ruin(&mut game, Team::Red, crate::unit::Tier::Outer);
    let inner = game
        .units
        .iter()
        .find(|unit| unit.team == Team::Red && unit.tier == Some(crate::unit::Tier::Inner))
        .map(|unit| unit.id)
        .unwrap();
    game.unit_mut(inner).unwrap().position = position + Vec2::new(600.0, 0.0);
    game.unit_mut(hero).unwrap().position = position - Vec2::new(500.0, 0.0);
    game.command(hero, Command::Attack(turret));
    run(&mut game, 0.5 + 5.0 + 5.0);
    let disc = sun_disc(&game).unwrap();
    assert!(disc.health / disc.stats.max_health > 0.85);
}

#[test]
fn only_lane_turret_ruins_can_hold_a_sun_disc() {
    let Some(Duel { mut game, hero, .. }) = duel("Emperor", "Swordsman", 3000.0) else {
        return;
    };
    let (turret, position) = ruin(&mut game, Team::Red, crate::unit::Tier::Nexus);
    game.unit_mut(hero).unwrap().position = position - Vec2::new(500.0, 0.0);
    game.command(hero, Command::Attack(turret));
    run(&mut game, 0.6);
    assert!(sun_disc(&game).is_none());
}

#[test]
fn sorcerer_gains_aram_takedown_stacks() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Sorcerer", "Swordsman", 2000.0)
    else {
        return;
    };
    game.damage(Some(hero), enemy, 1.0e6, protocol::DamageKind::True, false);
    assert_eq!(game.unit(hero).unwrap().counter, 3.0);
}

#[test]
fn sorcerer_primordial_burst_accelerates_up_to_its_max_speed() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Sorcerer", "Swordsman", 600.0)
    else {
        return;
    };
    let toward = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::R, toward, Some(enemy));
    let mut speeds = Vec::new();
    for _ in 0..40 {
        run(&mut game, TICK);
        speeds.extend(
            game.missiles
                .iter()
                .filter(|missile| missile.kind == asset::clip_id("VeigarR"))
                .map(|missile| missile.speed),
        );
    }
    assert!(speeds.len() > 2, "{speeds:?}");
    assert!(speeds.windows(2).all(|pair| pair[1] >= pair[0]));
    assert!(speeds[0] < 700.0);
}

#[test]
fn emperor_attacks_turrets_himself_even_with_soldiers_nearby() {
    let Some(Duel { mut game, hero, .. }) = duel("Emperor", "Swordsman", 3000.0) else {
        return;
    };
    let team = game.unit(hero).unwrap().team;
    let turret = game
        .units
        .iter()
        .find(|unit| unit.kind == protocol::UnitKind::Turret && unit.team != team)
        .map(|unit| (unit.id, unit.position));
    let Some((turret, at)) = turret else {
        return;
    };
    let unit = game.unit_mut(hero).unwrap();
    unit.position = at + Vec2::new(0.0, 600.0);
    cast(&mut game, hero, Slot::W, at + Vec2::new(0.0, 250.0), None);
    run(&mut game, 0.6);
    let before = health(&game, turret);
    game.command(hero, Command::Attack(turret));
    run(&mut game, 3.0);
    assert!(health(&game, turret) < before);
}

#[test]
fn emperor_soldiers_go_dormant_when_he_strays_from_them() {
    let Some(Duel { mut game, hero, .. }) = duel("Emperor", "Swordsman", 3000.0) else {
        return;
    };
    cast(
        &mut game,
        hero,
        Slot::W,
        MIDDLE + Vec2::new(300.0, 0.0),
        None,
    );
    run(&mut game, 0.5);
    let soldier = super::emperor::soldiers(&game, hero)[0];
    let clip = |game: &Game| {
        game.unit(soldier)
            .unwrap()
            .animation
            .map(|animation| animation.clip)
    };
    game.unit_mut(hero).unwrap().position = MIDDLE - Vec2::new(500.0, 0.0);
    run(&mut game, TICK);
    assert_eq!(clip(&game), Some(asset::clip_id("Inactive_Loop")));
    game.unit_mut(hero).unwrap().position = MIDDLE;
    run(&mut game, TICK);
    assert_eq!(clip(&game), Some(asset::clip_id("Reactivate")));
}

#[test]
fn emperor_soldiers_fade_twice_as_fast_near_enemy_turrets() {
    let Some(Duel { mut game, hero, .. }) = duel("Emperor", "Swordsman", 3000.0) else {
        return;
    };
    let team = game.unit(hero).unwrap().team;
    let Some(at) = game
        .units
        .iter()
        .find(|unit| unit.kind == protocol::UnitKind::Turret && unit.team != team)
        .map(|unit| unit.position)
    else {
        return;
    };
    game.unit_mut(hero).unwrap().position = at + Vec2::new(0.0, 900.0);
    cast(&mut game, hero, Slot::W, at + Vec2::new(0.0, 500.0), None);
    run(&mut game, 0.5);
    let soldier = super::emperor::soldiers(&game, hero)[0];
    let left = |game: &Game| game.unit(soldier).unwrap().expires.unwrap() - game.time;
    let before = left(&game);
    run(&mut game, 1.0);
    assert!((before - left(&game) - 2.0).abs() < 0.05);
}

#[test]
fn wanderer_wind_wall_blocks_targeted_attacks() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Wanderer", "Cardmaster", 500.0)
    else {
        return;
    };
    let toward = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::W, toward, None);
    run(&mut game, 0.1);
    let before = health(&game, hero);
    game.command(enemy, Command::Attack(hero));
    run(&mut game, 2.0);
    assert_eq!(health(&game, hero), before);
}

#[test]
fn ascendant_arcanopulse_reaches_further_the_longer_it_charges() {
    for (held, reaches) in [(0.3, false), (1.6, true)] {
        let Some(Duel {
            mut game,
            hero,
            enemy,
        }) = duel("Ascendant", "Swordsman", 1300.0)
        else {
            return;
        };
        let before = health(&game, enemy);
        let at = game.unit(enemy).unwrap().position;
        cast(&mut game, hero, Slot::Q, at, None);
        run(&mut game, held);
        cast(&mut game, hero, Slot::Q, at, None);
        run(&mut game, 0.7);
        assert_eq!(health(&game, enemy) < before, reaches, "held {held}");
        assert!(game.unit(hero).unwrap().ability(Slot::Q).cooldown > 0.0);
    }
}

#[test]
fn spells_waiting_for_a_second_press_say_so_until_it_comes() {
    for (champion, slot) in [("Ascendant", Slot::Q), ("Cardmaster", Slot::W)] {
        let Some(Duel { mut game, hero, .. }) = duel(champion, "Swordsman", 1300.0) else {
            return;
        };
        let index = SLOTS
            .iter()
            .position(|candidate| *candidate == slot)
            .unwrap();
        let recast = |game: &Game| game.snapshot(Some(hero)).you.unwrap().abilities[index].recast;
        let at = game.unit(hero).unwrap().position + Vec2::new(500.0, 0.0);
        assert!(!recast(&game), "{champion}");
        cast(&mut game, hero, slot, at, None);
        run(&mut game, 0.3);
        assert!(recast(&game), "{champion}");
        cast(&mut game, hero, slot, at, None);
        run(&mut game, 0.7);
        assert!(!recast(&game), "{champion}");
    }
}

#[test]
fn ascendant_arcanopulse_aim_grows_while_it_charges() {
    let Some(Duel { mut game, hero, .. }) = duel("Ascendant", "Swordsman", 1300.0) else {
        return;
    };
    let range = |game: &Game| game.snapshot(Some(hero)).you.unwrap().abilities[0].range;
    let at = game.unit(hero).unwrap().position + Vec2::new(500.0, 0.0);
    cast(&mut game, hero, Slot::Q, at, None);
    run(&mut game, 0.1);
    let early = range(&game);
    run(&mut game, 1.5);
    let late = range(&game);
    assert!(early < 800.0, "{early}");
    assert!(late > 1400.0, "{late}");
}

#[test]
fn ascendant_rite_aims_with_the_size_of_its_blast() {
    let Some(Duel { mut game, hero, .. }) = duel("Ascendant", "Swordsman", 1300.0) else {
        return;
    };
    let area = |game: &Game| game.snapshot(Some(hero)).you.unwrap().abilities[3].area;
    let at = game.unit(hero).unwrap().position + Vec2::new(500.0, 0.0);
    assert_eq!(area(&game), 0.0);
    cast(&mut game, hero, Slot::R, at, None);
    run(&mut game, 0.3);
    assert!(area(&game) > 100.0, "{}", area(&game));
}

#[test]
fn ascendant_charging_slows_him_and_locks_his_attacks_and_other_spells() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Ascendant", "Swordsman", 400.0)
    else {
        return;
    };
    let speed = game.unit(hero).unwrap().stats.move_speed;
    let at = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::Q, at, None);
    run(&mut game, 1.6);
    let unit = game.unit(hero).unwrap();
    assert!(unit.is_disarmed());
    assert!(unit.stats.move_speed < speed * 0.7);
    cast(&mut game, hero, Slot::E, at, None);
    run(&mut game, 0.1);
    assert_eq!(game.unit(hero).unwrap().ability(Slot::E).cooldown, 0.0);
}

#[test]
fn ascendant_arcanopulse_refunds_half_its_mana_when_never_released() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Ascendant", "Swordsman", 3000.0)
    else {
        return;
    };
    let mana = |game: &Game| game.unit(hero).unwrap().mana;
    let full = mana(&game);
    let at = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::Q, at, None);
    run(&mut game, TICK);
    let spent = full - mana(&game);
    assert!(spent > 0.0);
    run(&mut game, 3.1);
    let unit = game.unit(hero).unwrap();
    assert!(!unit.has_buff("arcanopulse") && !unit.is_disarmed());
    assert!(unit.ability(Slot::Q).cooldown > 0.0);
    let lost = full - mana(&game);
    assert!((lost - spent / 2.0).abs() < 10.0, "{lost} of {spent}");
}

fn shocking_orb_stun(
    Duel {
        mut game,
        hero,
        enemy,
    }: Duel,
) -> f32 {
    let at = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::E, at, None);
    for _ in 0..120 {
        run(&mut game, TICK);
        if let Some(stun) = game.unit(enemy).unwrap().buff("stun") {
            return stun.duration;
        }
    }
    panic!("the orb never landed");
}

#[test]
fn ascendant_shocking_orb_stuns_longer_the_further_it_flies() {
    let (Some(near), Some(far)) = (
        duel("Ascendant", "Swordsman", 200.0),
        duel("Ascendant", "Swordsman", 1000.0),
    ) else {
        return;
    };
    let (near, far) = (shocking_orb_stun(near), shocking_orb_stun(far));
    assert!(near < 1.1 && far > 1.8, "{near} {far}");
}

fn eye_of_destruction(
    Duel {
        mut game,
        hero,
        enemy,
    }: Duel,
    offset: f32,
) -> (f32, f32) {
    let before = health(&game, enemy);
    let at = game.unit(enemy).unwrap().position + Vec2::new(0.0, offset);
    cast(&mut game, hero, Slot::W, at, None);
    run(&mut game, 1.0);
    let unit = game.unit(enemy).unwrap();
    let slow = unit.buff("slow").map_or(0.0, |buff| buff.bonus.slow);
    (before - unit.health, slow)
}

#[test]
fn ascendant_eye_of_destruction_hits_harder_and_slows_more_at_its_center() {
    let (Some(center), Some(edge)) = (
        duel("Ascendant", "Swordsman", 800.0),
        duel("Ascendant", "Swordsman", 800.0),
    ) else {
        return;
    };
    let (center, edge) = (
        eye_of_destruction(center, 0.0),
        eye_of_destruction(edge, 220.0),
    );
    assert!(
        edge.0 > 0.0 && center.0 > edge.0 * 1.5,
        "{center:?} {edge:?}"
    );
    assert!(
        center.1 > 0.5 && (edge.1 - 0.25).abs() < 0.01,
        "{center:?} {edge:?}"
    );
}

#[test]
fn ascendant_rite_of_the_arcane_ramps_on_champions_and_ends_after_the_last_shot() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Ascendant", "Swordsman", 3000.0)
    else {
        return;
    };
    let at = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::R, at, None);
    run(&mut game, 0.6);
    let mut hits = Vec::new();
    for _ in 0..6 {
        let unit = game.unit_mut(enemy).unwrap();
        unit.health = unit.stats.max_health;
        let before = unit.health;
        cast(&mut game, hero, Slot::R, at, None);
        run(&mut game, 0.7);
        hits.push(before - health(&game, enemy));
    }
    assert!(hits.windows(2).all(|pair| pair[1] > pair[0]), "{hits:?}");
    let unit = game.unit(hero).unwrap();
    assert!(!unit.has_buff("rite_of_the_arcane"));
    let ability = unit.ability(Slot::R);
    assert!(ability.cooldown > 90.0, "{}", ability.cooldown);
}

#[test]
fn ascendant_rite_of_the_arcane_refunds_half_its_cooldown_when_unused() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Ascendant", "Swordsman", 3000.0)
    else {
        return;
    };
    let at = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::R, at, None);
    run(&mut game, 10.2);
    let unit = game.unit(hero).unwrap();
    assert!(!unit.has_buff("rite_of_the_arcane"));
    let cooldown = unit.ability(Slot::R).cooldown;
    assert!((40.0..55.0).contains(&cooldown), "{cooldown}");
}

#[test]
fn ascendant_moving_breaks_the_rite_only_after_it_settles() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Ascendant", "Swordsman", 3000.0)
    else {
        return;
    };
    let at = game.unit(enemy).unwrap().position;
    let start = game.unit(hero).unwrap().position;
    cast(&mut game, hero, Slot::R, at, None);
    run(&mut game, 0.3);
    game.command(
        hero,
        Command::Move((start + Vec2::new(-500.0, 0.0)).to_array()),
    );
    run(&mut game, 0.5);
    let unit = game.unit(hero).unwrap();
    assert!(unit.has_buff("rite_of_the_arcane"));
    assert_eq!(unit.position, start);
    run(&mut game, 0.6);
    assert!(!game.unit(hero).unwrap().has_buff("rite_of_the_arcane"));
}

#[test]
fn ascendant_mana_surge_restores_mana_once_per_cooldown() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Ascendant", "Swordsman", 400.0)
    else {
        return;
    };
    game.unit_mut(hero).unwrap().mana = 100.0;
    game.command(hero, Command::Attack(enemy));
    run(&mut game, 2.0);
    let unit = game.unit(hero).unwrap();
    assert!(unit.has_buff("mana_surge_cooldown"));
    assert!(unit.mana >= 160.0, "{}", unit.mana);
    let mana = unit.mana;
    run(&mut game, 4.0);
    assert!(game.unit(hero).unwrap().mana - mana < 20.0);
}

fn ammo(game: &Game, id: UnitId) -> u32 {
    stacks(game, id, "whisper_ammo")
}

#[test]
fn sniper_fires_four_shots_then_reloads_and_the_last_one_executes() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Sniper", "Swordsman", 400.0)
    else {
        return;
    };
    game.stun(enemy, 1000.0, None);
    assert_eq!(ammo(&game, hero), 4);
    game.command(hero, Command::Attack(enemy));
    let mut hits = Vec::new();
    let mut last = health(&game, enemy);
    for _ in 0..(8.0 / TICK) as u32 {
        run(&mut game, TICK);
        let now = health(&game, enemy);
        if now < last {
            hits.push(last - now);
        }
        last = now;
        if hits.len() == 4 {
            break;
        }
    }
    assert_eq!(hits.len(), 4, "{hits:?}");
    assert!(hits[3] > hits[0] * 1.4, "{hits:?}");
    let unit = game.unit(hero).unwrap();
    assert_eq!(ammo(&game, hero), 0);
    assert!(unit.is_disarmed());
    game.command(hero, Command::Stop);
    run(&mut game, 2.6);
    assert_eq!(ammo(&game, hero), 4);
    assert!(!game.unit(hero).unwrap().is_disarmed());
}

#[test]
fn sniper_tops_up_his_ammo_only_after_leaving_combat() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Sniper", "Swordsman", 400.0)
    else {
        return;
    };
    game.stun(enemy, 1000.0, None);
    game.command(hero, Command::Attack(enemy));
    while ammo(&game, hero) == 4 {
        run(&mut game, TICK);
    }
    game.command(hero, Command::Stop);
    let unit = game.unit_mut(enemy).unwrap();
    unit.position = MIDDLE + Vec2::new(3000.0, 0.0);
    unit.last_position = unit.position;
    run(&mut game, 7.0);
    assert_eq!(ammo(&game, hero), 3);
    run(&mut game, 4.0);
    assert_eq!(ammo(&game, hero), 4);
}

#[test]
fn sniper_turns_bonus_attack_speed_into_attack_damage() {
    let Some(Duel { mut game, hero, .. }) = duel("Sniper", "Swordsman", 400.0) else {
        return;
    };
    let before = game.unit(hero).unwrap().stats;
    let mut buff = game.buff("frenzy", 100.0);
    buff.bonus.attack_speed = 0.5;
    game.add_buff(hero, buff);
    let after = game.unit(hero).unwrap().stats;
    assert_eq!(after.attack_speed, before.attack_speed);
    assert!(
        after.attack_damage > before.attack_damage * 1.1,
        "{} {}",
        before.attack_damage,
        after.attack_damage
    );
}

#[test]
fn sniper_dancing_grenade_bounces_and_hits_harder_after_a_kill() {
    let mut hits = Vec::new();
    for kill in [false, true] {
        let Some(Duel {
            mut game,
            hero,
            enemy,
        }) = duel("Sniper", "Swordsman", 500.0)
        else {
            return;
        };
        game.stun(enemy, 1000.0, None);
        let minion = dummy(&mut game, Team::Red, MIDDLE + Vec2::new(400.0, 150.0));
        if kill {
            game.unit_mut(minion).unwrap().health = 1.0;
        }
        let before = (health(&game, minion), health(&game, enemy));
        let at = game.unit(minion).unwrap().position;
        cast(&mut game, hero, Slot::Q, at, Some(minion));
        run(&mut game, 1.5);
        assert!(health(&game, minion) < before.0);
        hits.push(before.1 - health(&game, enemy));
    }
    assert!(hits[0] > 0.0 && hits[1] > hits[0] * 1.3, "{hits:?}");
}

fn deadly_flourish(spotted: bool) -> Option<(bool, f32, f32)> {
    let Duel {
        mut game,
        hero,
        enemy,
    } = duel("Sniper", "Swordsman", 1200.0)?;
    game.stun(enemy, 1000.0, None);
    let behind = dummy(&mut game, Team::Red, MIDDLE + Vec2::new(1500.0, 0.0));
    if spotted {
        let mark = game.buff("caught_out", 4.0);
        game.add_buff(enemy, mark);
    }
    let (enemy_before, behind_before) = (health(&game, enemy), health(&game, behind));
    let at = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::W, at, None);
    run(&mut game, 0.9);
    let rooted = game.unit(enemy).unwrap().has_buff("root");
    Some((
        rooted,
        enemy_before - health(&game, enemy),
        behind_before - health(&game, behind),
    ))
}

#[test]
fn sniper_deadly_flourish_roots_only_spotted_champions_and_stops_on_them() {
    let (Some(plain), Some(spotted)) = (deadly_flourish(false), deadly_flourish(true)) else {
        return;
    };
    assert!(!plain.0 && plain.1 > 0.0 && plain.2 == 0.0, "{plain:?}");
    assert!(
        spotted.0 && spotted.1 > 0.0 && spotted.2 == 0.0,
        "{spotted:?}"
    );
}

#[test]
fn sniper_lotus_trap_hides_from_enemies_then_blooms_slows_and_bursts() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Sniper", "Swordsman", 1500.0)
    else {
        return;
    };
    game.unit_mut(hero).unwrap().ability_mut(Slot::E).charges = 2;
    let trap = MIDDLE + Vec2::new(600.0, 0.0);
    cast(&mut game, hero, Slot::E, trap, None);
    run(&mut game, 0.5);
    game.stun(hero, 1000.0, None);
    run(&mut game, 1.0);
    assert_eq!(game.props.len(), 1);
    assert_eq!(game.snapshot(Some(hero)).props.len(), 1);
    assert!(game.snapshot(Some(enemy)).props.is_empty());
    let before = health(&game, enemy);
    let unit = game.unit_mut(enemy).unwrap();
    unit.position = trap;
    unit.last_position = trap;
    game.stun(enemy, 1000.0, None);
    run(&mut game, 0.5);
    assert!(game.props.is_empty());
    let unit = game.unit(enemy).unwrap();
    assert!(unit.has_buff("slow") && unit.has_buff("caught_out"));
    assert_eq!(unit.health, before);
    run(&mut game, 2.0);
    assert!(health(&game, enemy) < before);
}

#[test]
fn sniper_curtain_call_stays_in_its_cone_stops_on_champions_and_ends_after_four_shots() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Sniper", "Swordsman", 1500.0)
    else {
        return;
    };
    game.stun(enemy, 1000.0, None);
    let behind = dummy(&mut game, Team::Red, MIDDLE + Vec2::new(2000.0, 0.0));
    let at = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::R, at, None);
    run(&mut game, 0.3);
    cast(
        &mut game,
        hero,
        Slot::R,
        MIDDLE + Vec2::new(0.0, 1500.0),
        None,
    );
    run(&mut game, TICK);
    let shot = game.missiles.last().unwrap();
    let crate::Motion::Line { direction, .. } = shot.motion else {
        panic!("the shot should fly in a line");
    };
    let angle = direction.angle_to(Vec2::X).abs().to_degrees();
    assert!((angle - 30.0).abs() < 1.0, "{angle}");
    let before = (health(&game, enemy), health(&game, behind));
    for _ in 0..3 {
        run(&mut game, 1.1);
        cast(&mut game, hero, Slot::R, at, None);
    }
    run(&mut game, 1.0);
    assert!(health(&game, enemy) < before.0);
    assert_eq!(health(&game, behind), before.1);
    let unit = game.unit(hero).unwrap();
    assert!(!unit.has_buff("curtain_call"));
    assert!(unit.ability(Slot::R).cooldown > 60.0);
}

#[test]
fn colossus_granite_shield_waits_its_cooldown_after_soaking_a_hit() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Colossus", "Swordsman", 3000.0)
    else {
        return;
    };
    run(&mut game, 0.2);
    let shield = |game: &Game| game.unit(hero).unwrap().shield();
    let full = shield(&game);
    assert!(full > 0.0);
    game.damage(Some(enemy), hero, 30.0, DamageKind::True, false);
    let soaked = shield(&game);
    assert!(soaked < full);
    run(&mut game, 2.0);
    assert_eq!(shield(&game), soaked);
}

fn die_mid_cast(hero: &str, slot: Slot) -> Option<f32> {
    let Duel {
        mut game,
        hero,
        enemy,
    } = duel(hero, "Swordsman", 1500.0)?;
    let at = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, slot, at, None);
    run(&mut game, 0.3);
    game.unit_mut(hero).unwrap().health = 1.0;
    game.damage(Some(enemy), hero, 100.0, DamageKind::True, false);
    assert!(game.unit(hero).unwrap().dead);
    Some(game.unit(hero).unwrap().ability(slot).cooldown)
}

#[test]
fn dying_mid_cast_still_puts_the_ability_on_cooldown() {
    for (hero, slot) in [
        ("Sniper", Slot::R),
        ("Ascendant", Slot::R),
        ("Ascendant", Slot::Q),
        ("Cardmaster", Slot::W),
        ("Cardmaster", Slot::R),
        ("Swordsman", Slot::E),
    ] {
        let Some(cooldown) = die_mid_cast(hero, slot) else {
            return;
        };
        assert!(cooldown > 1.0, "{hero} {slot:?} {cooldown}");
    }
}

#[test]
fn warlord_dying_in_his_reset_window_resumes_the_guillotine_cooldown() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Warlord", "Swordsman", 300.0)
    else {
        return;
    };
    game.unit_mut(hero).unwrap().ability_mut(Slot::R).rank = 1;
    game.unit_mut(enemy).unwrap().health = 50.0;
    cast(&mut game, hero, Slot::R, MIDDLE, Some(enemy));
    run(&mut game, 1.0);
    assert!(game.unit(enemy).unwrap().dead);
    assert!(game.unit(hero).unwrap().has_buff("guillotine_reset"));
    game.unit_mut(hero).unwrap().health = 1.0;
    game.damage(Some(enemy), hero, 100.0, DamageKind::True, false);
    assert!(game.unit(hero).unwrap().ability(Slot::R).cooldown > 50.0);
}

#[test]
fn cardmaster_wild_cards_hit_each_enemy_only_once_per_cast() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Cardmaster", "Swordsman", 60.0)
    else {
        return;
    };
    game.stun(enemy, 1000.0, None);
    let before = health(&game, enemy);
    let at = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::Q, at, None);
    run(&mut game, 0.6);
    let lost = before - health(&game, enemy);
    let spell = super::spell_of(&game, hero, Slot::Q).unwrap();
    let one = super::calculate(&game, hero, &spell, 3, "TotalDamage");
    assert!(lost > 0.0 && lost < one * 1.2, "{lost} {one}");
}

#[test]
fn sniper_cannot_throw_a_trap_over_his_deadly_flourish_windup() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Sniper", "Swordsman", 1200.0)
    else {
        return;
    };
    game.unit_mut(hero).unwrap().ability_mut(Slot::E).charges = 2;
    game.stun(enemy, 1000.0, None);
    let before = health(&game, enemy);
    let at = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::W, at, None);
    run(&mut game, 0.2);
    cast(
        &mut game,
        hero,
        Slot::E,
        MIDDLE + Vec2::new(300.0, 0.0),
        None,
    );
    run(&mut game, 0.8);
    assert_eq!(game.unit(hero).unwrap().ability(Slot::E).charges, 2);
    assert!(health(&game, enemy) < before);
}

#[test]
fn sniper_traps_wait_their_short_cooldown_between_charges() {
    let Some(Duel { mut game, hero, .. }) = duel("Sniper", "Swordsman", 1200.0) else {
        return;
    };
    game.unit_mut(hero).unwrap().ability_mut(Slot::E).charges = 2;
    cast(
        &mut game,
        hero,
        Slot::E,
        MIDDLE + Vec2::new(300.0, 0.0),
        None,
    );
    run(&mut game, 0.5);
    cast(
        &mut game,
        hero,
        Slot::E,
        MIDDLE + Vec2::new(0.0, 300.0),
        None,
    );
    run(&mut game, 0.5);
    assert_eq!(game.unit(hero).unwrap().ability(Slot::E).charges, 1);
}

#[test]
fn sniper_traps_show_their_short_cooldown_then_the_recharge_once_out_of_charges() {
    let Some(Duel { mut game, hero, .. }) = duel("Sniper", "Swordsman", 1200.0) else {
        return;
    };
    game.unit_mut(hero).unwrap().ability_mut(Slot::E).charges = 1;
    let shown = |game: &Game| {
        let ability = game.snapshot(Some(hero)).you.unwrap().abilities[2];
        (ability.cooldown, ability.cooldown_total)
    };
    cast(
        &mut game,
        hero,
        Slot::E,
        MIDDLE + Vec2::new(300.0, 0.0),
        None,
    );
    run(&mut game, 0.1);
    let unit = game.unit(hero).unwrap();
    let ability = *unit.ability(Slot::E);
    assert_eq!(ability.charges, 0);
    let (cooldown, total) = shown(&game);
    assert!((cooldown - ability.recharge).abs() < 0.01, "{cooldown}");
    assert!((total - ability.recharge_total).abs() < 0.01, "{total}");
    assert!(cooldown > ability.cooldown);
    game.unit_mut(hero).unwrap().ability_mut(Slot::E).charges = 1;
    let (cooldown, total) = shown(&game);
    assert_eq!(cooldown, ability.cooldown);
    assert_eq!(total, ability.cooldown_total);
}

#[test]
fn sorcerer_primordial_burst_only_targets_champions() {
    let Some(Duel { mut game, hero, .. }) = duel("Sorcerer", "Swordsman", 3000.0) else {
        return;
    };
    let minion = dummy(&mut game, Team::Red, MIDDLE + Vec2::new(300.0, 0.0));
    cast(&mut game, hero, Slot::R, MIDDLE, Some(minion));
    let mut burst = false;
    for _ in 0..(1.5 / TICK) as u32 {
        game.step();
        burst |= game.events.iter().any(|event| {
            matches!(event, protocol::Event::Damage { target, kind: DamageKind::Magic, .. } if *target == minion)
        });
    }
    assert!(!burst);
    assert_eq!(game.unit(hero).unwrap().ability(Slot::R).cooldown, 0.0);
}

#[test]
fn wanderer_last_breath_catches_enemies_knocked_back_by_allies() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Wanderer", "Swordsman", 500.0)
    else {
        return;
    };
    let at = game.unit(enemy).unwrap().position;
    game.knock_back(enemy, at + Vec2::new(300.0, 0.0), 1000.0);
    cast(&mut game, hero, Slot::R, at, None);
    run(&mut game, TICK);
    assert!(game.unit(hero).unwrap().ability(Slot::R).cooldown > 0.0);
}

#[test]
fn cardmaster_card_attacks_count_toward_stacked_deck() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Cardmaster", "Swordsman", 400.0)
    else {
        return;
    };
    game.stun(enemy, 1000.0, None);
    let card = game.buff("card_blue", 6.0);
    game.add_buff(hero, card);
    game.command(hero, Command::Attack(enemy));
    run(&mut game, 0.8);
    let unit = game.unit(hero).unwrap();
    assert!(!unit.has_buff("card_blue"));
    assert_eq!(unit.counter, 1.0);
}

#[test]
fn every_sound_champions_play_was_extracted() {
    let Ok(bytes) = std::fs::read(crate::tests::assets().join("audio/index.bin")) else {
        return;
    };
    let index = asset::audio::AudioIndex::from_bytes(&bytes).unwrap();
    let Some(data) = real_data() else {
        return;
    };
    let mut missing = std::collections::BTreeSet::new();
    for champion in &data.champions {
        let Some(Duel {
            mut game,
            hero,
            enemy,
        }) = duel(&champion.name, "Swordsman", 450.0)
        else {
            continue;
        };
        game.unit_mut(hero).unwrap().ability_mut(Slot::W).charges = 2;
        game.unit_mut(hero).unwrap().ability_mut(Slot::E).charges = 2;
        game.stun(enemy, 1000.0, None);
        let at = game.unit(enemy).unwrap().position;
        for slot in [
            Slot::W,
            Slot::Q,
            Slot::E,
            Slot::R,
            Slot::Q,
            Slot::W,
            Slot::R,
        ] {
            for _ in 0..(1.2 / TICK) as u32 {
                cast(&mut game, hero, slot, at, Some(enemy));
                game.step();
                for event in &game.events {
                    if let protocol::Event::Sound { key, .. } = event
                        && !index.sounds.contains_key(key)
                    {
                        missing.insert(key.clone());
                    }
                }
                if game.unit(hero).unwrap().dead {
                    break;
                }
            }
            game.command(hero, Command::Attack(enemy));
            run(&mut game, 1.5);
            let unit = game.unit_mut(enemy).unwrap();
            unit.health = unit.stats.max_health;
        }
    }
    assert!(missing.is_empty(), "{missing:?}");
}

#[test]
fn every_cast_plays_its_sound_once() {
    let Some(data) = real_data() else {
        return;
    };
    let mut wrong = Vec::new();
    for champion in &data.champions {
        for (slot, key) in [
            (Slot::Q, "q"),
            (Slot::W, "w"),
            (Slot::E, "e"),
            (Slot::R, "r"),
        ] {
            let Some(Duel {
                mut game,
                hero,
                enemy,
            }) = duel(&champion.name, "Swordsman", 450.0)
            else {
                return;
            };
            let unit = game.unit_mut(hero).unwrap();
            unit.ability_mut(Slot::W).charges = 2;
            unit.ability_mut(Slot::E).charges = 2;
            game.stun(enemy, 1000.0, None);
            if champion.name == "Emperor" && slot != Slot::W {
                let at = game.unit(enemy).unwrap().position;
                cast(&mut game, hero, Slot::W, at, None);
                run(&mut game, 0.5);
            }
            let at = game.unit(enemy).unwrap().position;
            let key = match (champion.name.as_str(), slot) {
                ("Cardmaster", Slot::E) | ("Wanderer", Slot::R) => continue,
                ("Swordsman", Slot::Q) => "q_cast",
                ("Warlord" | "VoidWalker", Slot::W) => "w_cast",
                _ => key,
            };
            let wanted = format!("{}.{key}", champion.key());
            let mut count = 0;
            cast(&mut game, hero, slot, at, Some(enemy));
            for _ in 0..(1.0 / TICK) as u32 {
                game.step();
                count += game
                    .events
                    .iter()
                    .filter(|event| matches!(event, protocol::Event::Sound { key, .. } if *key == wanted))
                    .count();
                game.clear_events();
            }
            if count != 1 {
                wrong.push(format!("{wanted} x{count}"));
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:?}");
}

#[test]
fn wanderer_wind_wall_cooldown_shrinks_with_ability_haste() {
    let Some(Duel { mut game, hero, .. }) = duel("Wanderer", "Swordsman", 3000.0) else {
        return;
    };
    let mut haste = game.buff("haste", 100.0);
    haste.bonus.ability_haste = 100.0;
    game.add_buff(hero, haste);
    let spell = super::spell_of(&game, hero, Slot::W).unwrap();
    cast(&mut game, hero, Slot::W, MIDDLE + Vec2::X * 100.0, None);
    run(&mut game, TICK);
    let cooldown = game.unit(hero).unwrap().ability(Slot::W).cooldown;
    assert!(
        (cooldown - spell.cooldown(3) / 2.0).abs() < 0.2,
        "{cooldown}"
    );
}

#[test]
fn colossus_stopped_by_a_wall_still_lands_unstoppable_force() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Colossus", "Swordsman", 900.0)
    else {
        return;
    };
    game.barriers.push(super::Wall {
        team: Team::Red,
        from: MIDDLE + Vec2::new(500.0, -400.0),
        to: MIDDLE + Vec2::new(500.0, 400.0),
        armed: 0.0,
        expires: 100.0,
    });
    let at = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::R, at, None);
    run(&mut game, 1.0);
    let unit = game.unit(hero).unwrap();
    assert!(unit.position.x < MIDDLE.x + 500.0);
    assert!(
        game.events
            .iter()
            .any(|event| matches!(event, protocol::Event::Effect { name, .. } if *name == asset::clip_id("UnstoppableForce")))
    );
}

#[test]
fn cardmaster_cannot_gate_while_silenced_or_rooted() {
    for effect in ["silence", "root"] {
        let Some(Duel {
            mut game,
            hero,
            enemy,
        }) = duel("Cardmaster", "Swordsman", 3000.0)
        else {
            return;
        };
        cast(&mut game, hero, Slot::R, MIDDLE, None);
        run(&mut game, 0.2);
        let lock = game.crowd_control(effect, 2.0, Some((enemy, Slot::E)));
        game.add_buff(hero, lock);
        cast(
            &mut game,
            hero,
            Slot::R,
            MIDDLE + Vec2::new(0.0, 2000.0),
            None,
        );
        run(&mut game, 2.0);
        assert_eq!(game.unit(hero).unwrap().position, MIDDLE, "{effect}");
    }
}

#[test]
fn cardmaster_cannot_lock_a_card_while_silenced_or_stunned() {
    for effect in ["silence", "stun"] {
        let Some(Duel {
            mut game,
            hero,
            enemy,
        }) = duel("Cardmaster", "Swordsman", 3000.0)
        else {
            return;
        };
        cast(&mut game, hero, Slot::W, MIDDLE, None);
        run(&mut game, 0.2);
        let lock = game.crowd_control(effect, 1.0, Some((enemy, Slot::E)));
        game.add_buff(hero, lock);
        cast(&mut game, hero, Slot::W, MIDDLE, None);
        run(&mut game, 0.1);
        assert!(game.unit(hero).unwrap().has_buff("pick_a_card"), "{effect}");
        run(&mut game, 1.0);
        cast(&mut game, hero, Slot::W, MIDDLE, None);
        run(&mut game, 0.1);
        assert!(
            !game.unit(hero).unwrap().has_buff("pick_a_card"),
            "{effect}"
        );
    }
}

#[test]
fn missiles_report_where_they_land_to_whoever_can_see_it() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Ascendant", "Swordsman", 600.0)
    else {
        return;
    };
    let at = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::E, at, None);
    let mut landed = None;
    for _ in 0..(2.0 / TICK) as usize {
        game.step();
        landed = landed.or_else(|| {
            game.events.iter().find_map(|event| match event {
                protocol::Event::MissileLanded { id, .. } => Some(*id),
                _ => None,
            })
        });
        if landed.is_some() {
            break;
        }
        game.clear_events();
    }
    let landed = landed.expect("the orb lands");
    let told = |viewer: UnitId| {
        game.snapshot(Some(viewer)).events.iter().any(
            |event| matches!(event, protocol::Event::MissileLanded { id, .. } if *id == landed),
        )
    };
    assert!(told(hero));
    assert!(told(enemy));
}

#[test]
fn colossus_lets_go_of_his_charge_animation_when_he_lands() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Colossus", "Swordsman", 600.0)
    else {
        return;
    };
    let at = game.unit(enemy).unwrap().position;
    cast(&mut game, hero, Slot::R, at, None);
    run(&mut game, 0.1);
    let charging = game.unit(hero).unwrap().animation.unwrap();
    assert!(charging.hold && charging.clip == asset::clip_id("Spell4"));
    run(&mut game, 1.0);
    assert!(
        game.unit(hero)
            .unwrap()
            .animation
            .is_none_or(|animation| !animation.hold)
    );
}

#[test]
fn attack_animations_reach_their_firing_frame_as_the_attack_fires() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Cardmaster", "Swordsman", 400.0)
    else {
        return;
    };
    game.stun(enemy, 1000.0, None);
    game.command(hero, Command::Attack(enemy));
    run(&mut game, TICK * 2.0);
    let unit = game.unit(hero).unwrap();
    let crate::unit::Action::Attack { windup, .. } = unit.action else {
        panic!("twisted fate should be winding up");
    };
    let spell = game.data.champions[usize::from(unit.champion().unwrap())]
        .spell("CardmasterBasicAttack")
        .unwrap();
    let speed = unit.animation.unwrap().speed;
    assert!(
        (windup * speed - spell.cast_frame / 30.0).abs() < 0.01,
        "{speed}"
    );
}

#[test]
fn emperor_and_his_soldier_stop_outside_a_turret() {
    let (Some(data), Ok(layout)) = (real_data(), crate::map::layout(&crate::tests::assets()))
    else {
        return;
    };
    let emperor_idx = data
        .champions
        .iter()
        .position(|champion| champion.name == "Emperor")
        .unwrap() as u8;
    let players = [crate::Entrant::new("hero", Team::Blue, Some(emperor_idx))];
    let mut game = Game::new(data, layout, &players, 7);
    let hero = game.seats[0].unit;
    for slot in crate::unit::SLOTS {
        game.unit_mut(hero).unwrap().ability_mut(slot).rank = 3;
    }
    let turret = game
        .units
        .iter()
        .filter(|unit| unit.kind == protocol::UnitKind::Turret && unit.team == Team::Blue)
        .map(|unit| unit.position)
        .max_by(|a, b| a.x.total_cmp(&b.x))
        .unwrap();
    let start = game.walkable_point(turret + Vec2::new(-450.0, 0.0));
    let unit = game.unit_mut(hero).unwrap();
    unit.position = start;
    unit.last_position = start;
    unit.mana = unit.stats.max_mana;
    cast(
        &mut game,
        hero,
        Slot::W,
        turret + Vec2::new(-300.0, 0.0),
        None,
    );
    run(&mut game, 0.6);
    cast(&mut game, hero, Slot::Q, turret, None);
    run(&mut game, 1.0);
    let soldier = super::emperor::soldiers(&game, hero)[0];
    let clear = |game: &Game, id| game.unit(id).unwrap().position.distance(turret) >= 125.0;
    assert!(clear(&game, soldier));
    cast(&mut game, hero, Slot::E, turret, None);
    run(&mut game, 1.0);
    assert!(!matches!(
        game.unit(hero).unwrap().action,
        crate::unit::Action::Dash { .. }
    ));
    assert!(clear(&game, hero));
}

#[test]
fn roots_stop_dashes_and_blinks() {
    for (champion, slot) in [
        ("Wanderer", Slot::E),
        ("Colossus", Slot::R),
        ("VoidWalker", Slot::R),
        ("Emperor", Slot::E),
    ] {
        let Some(Duel {
            mut game,
            hero,
            enemy,
        }) = duel(champion, "Swordsman", 400.0)
        else {
            return;
        };
        let target = MIDDLE + Vec2::new(400.0, 0.0);
        if champion == "Emperor" {
            cast(&mut game, hero, Slot::W, target, None);
            run(&mut game, 0.6);
        }
        game.root(hero, 2.0, None);
        cast(&mut game, hero, slot, target, Some(enemy));
        game.step();
        let unit = game.unit(hero).unwrap();
        assert!(
            !matches!(unit.action, crate::unit::Action::Dash { .. }),
            "{champion}"
        );
        assert_eq!(unit.ability(slot).cooldown, 0.0, "{champion}");
    }
}

#[test]
fn targeted_spells_ignore_structures() {
    for (champion, slot) in [
        ("Wanderer", Slot::E),
        ("Colossus", Slot::Q),
        ("VoidWalker", Slot::Q),
        ("Sniper", Slot::Q),
    ] {
        let Some(Duel { mut game, hero, .. }) = duel(champion, "Swordsman", 3000.0) else {
            return;
        };
        let (turret, position) = game
            .units
            .iter()
            .find(|unit| unit.kind == protocol::UnitKind::Turret && unit.team == Team::Red)
            .map(|unit| (unit.id, unit.position))
            .unwrap();
        let unit = game.unit_mut(hero).unwrap();
        unit.position = position + Vec2::new(-300.0, 0.0);
        unit.last_position = unit.position;
        cast(&mut game, hero, slot, position, Some(turret));
        assert_eq!(
            game.unit(hero).unwrap().ability(slot).cooldown,
            0.0,
            "{champion}"
        );
    }
}

#[test]
fn colossus_ground_slam_spares_structures() {
    let Some(Duel { mut game, hero, .. }) = duel("Colossus", "Swordsman", 3000.0) else {
        return;
    };
    let (turret, position) = game
        .units
        .iter()
        .find(|unit| unit.kind == protocol::UnitKind::Turret && unit.team == Team::Red)
        .map(|unit| (unit.id, unit.position))
        .unwrap();
    let unit = game.unit_mut(hero).unwrap();
    unit.position = position + Vec2::new(-200.0, 0.0);
    unit.last_position = unit.position;
    let before = health(&game, turret);
    cast(&mut game, hero, Slot::E, position, None);
    run(&mut game, 0.5);
    assert_eq!(health(&game, turret), before);
    assert!(!game.unit(turret).unwrap().has_buff("ground_slam"));
}

#[test]
fn sorcerer_dark_matter_is_cancelled_by_a_stun_during_the_windup() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Sorcerer", "Swordsman", 800.0)
    else {
        return;
    };
    let before = health(&game, enemy);
    cast(
        &mut game,
        hero,
        Slot::W,
        MIDDLE + Vec2::new(800.0, 0.0),
        None,
    );
    game.step();
    game.stun(hero, 1.0, None);
    run(&mut game, 2.0);
    assert!(game.zones.is_empty());
    assert_eq!(health(&game, enemy), before);
}

#[test]
fn swordsman_judgment_counts_as_one_cast() {
    let Some(Duel { mut game, hero, .. }) = duel("Swordsman", "Warlord", 3000.0) else {
        return;
    };
    game.unit_mut(hero).unwrap().items[0] = Some(crate::items::SPELLBLADE);
    cast(&mut game, hero, Slot::E, MIDDLE, None);
    game.step();
    assert!(game.unit(hero).unwrap().has_buff("spellblade"));
    game.unit_mut(hero)
        .unwrap()
        .buffs
        .retain(|buff| !matches!(buff.name, "spellblade" | "spellblade_cooldown"));
    run(&mut game, 4.0);
    let unit = game.unit(hero).unwrap();
    assert!(!unit.has_buff("judgment"));
    assert!(unit.ability(Slot::E).cooldown > 0.0);
    assert!(!unit.has_buff("spellblade"));
}

#[test]
fn guardian_fury_has_no_damage_cap_on_champions() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Guardian", "Warlord", 420.0)
    else {
        return;
    };
    let mut bulk = game.buff("bulk", f32::MAX);
    bulk.bonus.health = 20_000.0;
    game.add_buff(enemy, bulk);
    let mut power = game.buff("power", f32::MAX);
    power.bonus.ability_power = 100.0;
    game.add_buff(hero, power);
    cast(&mut game, hero, Slot::R, MIDDLE, None);
    while !game.unit(hero).unwrap().has_buff("fury") {
        run(&mut game, TICK);
    }
    let before = health(&game, enemy);
    run(&mut game, 3.0);
    let enemy_unit = game.unit(enemy).unwrap();
    let balance = game.balance(hero).damage_dealt * game.balance(enemy).damage_taken;
    let expected = enemy_unit.stats.max_health
        * 0.03
        * 6.0
        * balance
        * crate::stats::resist_multiplier(enemy_unit.stats.magic_resist);
    let dealt = before - enemy_unit.health;
    assert!(
        (dealt / expected - 1.0).abs() < 0.05,
        "{dealt} vs {expected}"
    );
}

#[test]
fn emperor_cannot_raise_a_soldier_inside_a_structure() {
    let (Some(data), Ok(layout)) = (real_data(), crate::map::layout(&crate::tests::assets()))
    else {
        return;
    };
    let emperor_idx = data
        .champions
        .iter()
        .position(|champion| champion.name == "Emperor")
        .unwrap() as u8;
    let players = [crate::Entrant::new("hero", Team::Blue, Some(emperor_idx))];
    let mut game = Game::new(data, layout, &players, 7);
    let hero = game.seats[0].unit;
    game.unit_mut(hero).unwrap().ability_mut(Slot::W).rank = 1;
    let nexus = game
        .units
        .iter()
        .find(|unit| unit.kind == protocol::UnitKind::Nexus && unit.team == Team::Blue)
        .map(|unit| unit.position)
        .unwrap();
    let start = game.walkable_point(nexus + Vec2::new(0.0, 400.0));
    let unit = game.unit_mut(hero).unwrap();
    unit.position = start;
    unit.last_position = start;
    cast(&mut game, hero, Slot::W, nexus, None);
    run(&mut game, 0.6);
    let soldier = super::emperor::soldiers(&game, hero)[0];
    let nav = game.layout.nav.as_ref().unwrap();
    assert!(crate::nav::walkable(
        nav,
        game.unit(soldier).unwrap().position
    ));
}

struct Chaos(u64);

impl Chaos {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn pick<T: Copy>(&mut self, options: &[T]) -> Option<T> {
        let count = options.len() as u64;
        (count > 0).then(|| options[(self.next() % count) as usize])
    }

    fn command(&mut self, game: &Game, id: UnitId) -> Command {
        let unit = game.unit(id).unwrap();
        let goal = game.layout.fountain(match unit.team {
            Team::Blue => Team::Red,
            Team::Red => Team::Blue,
        });
        let nearby: Vec<(UnitId, Vec2)> = game
            .units
            .iter()
            .filter(|other| {
                other.team != unit.team
                    && other.alive()
                    && other.position.distance(unit.position) < 1200.0
            })
            .map(|other| (other.id, other.position))
            .collect();
        match self.next() % 10 {
            0..=3 => Command::AttackMove(goal.to_array()),
            4..=8 => {
                let slot = self
                    .pick(&[Slot::Q, Slot::W, Slot::E, Slot::R, Slot::D, Slot::F])
                    .unwrap();
                let (target, point) = self
                    .pick(&nearby)
                    .map_or((None, goal), |(other, at)| (Some(other), at));
                Command::Cast(
                    slot,
                    Target {
                        point: point.to_array(),
                        unit: target,
                    },
                )
            }
            _ => Command::LevelUp(self.pick(&[Slot::Q, Slot::W, Slot::E, Slot::R]).unwrap()),
        }
    }
}

#[test]
fn a_chaotic_five_versus_five_keeps_every_unit_sound() {
    let (Some(data), Ok(layout)) = (real_data(), crate::map::layout(&crate::tests::assets()))
    else {
        return;
    };
    let count = data.champions.len() as u8;
    let players: Vec<Entrant> = (0..10u8)
        .map(|index| {
            let team = if index < 5 { Team::Blue } else { Team::Red };
            Entrant::new(&format!("p{index}"), team, Some(index % count))
        })
        .collect();
    let mut game = Game::new(data, layout, &players, 3);
    let champions: Vec<UnitId> = game.seats.iter().map(|seat| seat.unit).collect();
    let mut chaos = Chaos(12345);
    let mut displaced: std::collections::HashMap<UnitId, f32> = std::collections::HashMap::new();
    for _ in 0..(300.0 / TICK) as usize {
        for &id in &champions {
            if chaos.next().is_multiple_of(20) {
                let command = chaos.command(&game, id);
                game.command(id, command);
            }
        }
        game.step();
        game.clear_events();
        let nav = game.layout.nav.as_ref().unwrap();
        for unit in game.units.iter().filter(|unit| unit.alive()) {
            assert!(unit.position.is_finite(), "{}", unit.data);
            assert!(unit.health <= unit.stats.max_health + 0.01, "{}", unit.data);
            assert!(
                (0.0..=unit.stats.max_mana + 0.01).contains(&unit.mana),
                "{}",
                unit.data
            );
            assert!(unit.buffs.len() < 30, "{}", unit.data);
            let moving = matches!(unit.action, crate::unit::Action::Dash { .. });
            let airborne = matches!(unit.action, crate::unit::Action::Airborne { .. });
            let time = displaced.entry(unit.id).or_default();
            *time = if moving || airborne {
                *time + TICK
            } else {
                0.0
            };
            assert!(*time < 3.0, "{} is stuck in {:?}", unit.data, unit.action);
            assert!(
                unit.is_structure() || moving || crate::nav::walkable(nav, unit.position),
                "{} stands in a wall at {}",
                unit.data,
                unit.position
            );
        }
        assert!(game.missiles.len() < 200);
        assert!(game.zones.len() < 50);
    }
}

#[test]
fn spells_cast_at_a_turret_do_not_leave_the_order_hanging() {
    let Some(Duel { mut game, hero, .. }) = duel("VoidWalker", "Swordsman", 3000.0) else {
        return;
    };
    let (turret, position) = game
        .units
        .iter()
        .find(|unit| unit.kind == protocol::UnitKind::Turret && unit.team == Team::Red)
        .map(|unit| (unit.id, unit.position))
        .unwrap();
    let unit = game.unit_mut(hero).unwrap();
    unit.position = position + Vec2::new(-500.0, 0.0);
    unit.last_position = unit.position;
    cast(&mut game, hero, Slot::Q, position, Some(turret));
    run(&mut game, 0.5);
    assert!(matches!(
        game.unit(hero).unwrap().order,
        crate::unit::Order::Idle
    ));
}

#[test]
fn enemies_see_snipers_curtain_call_cone_even_when_sniper_is_hidden() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Sniper", "Swordsman", 3000.0)
    else {
        return;
    };
    let at = game.unit(enemy).unwrap().position;
    game.clear_events();
    cast(&mut game, hero, Slot::R, at, None);
    run(&mut game, 0.3);
    let snapshot = game.snapshot(Some(enemy));
    assert!(!snapshot.units.iter().any(|unit| unit.id == hero));
    let cone = snapshot.events.iter().find_map(|event| match event {
        protocol::Event::Effect { name, facing, .. } if *name == asset::clip_id("JhinR") => {
            Some(*facing)
        }
        _ => None,
    });
    let facing = cone.expect("the enemy should get the cone");
    assert!((facing - game.unit(hero).unwrap().facing).abs() < 1e-3);
}

#[test]
fn enemies_who_walk_up_after_the_cast_still_see_sorcerers_cage_until_it_closes() {
    let Some(Duel {
        mut game,
        hero,
        enemy,
    }) = duel("Sorcerer", "Swordsman", 4000.0)
    else {
        return;
    };
    let center = MIDDLE + Vec2::new(600.0, 0.0);
    cast(&mut game, hero, Slot::E, center, None);
    run(&mut game, 0.5);
    let cage = |game: &Game| {
        game.snapshot(Some(enemy))
            .props
            .iter()
            .any(|prop| prop.effect == asset::clip_id("VeigarE"))
    };
    assert!(!cage(&game));
    game.unit_mut(enemy).unwrap().position = center + Vec2::new(500.0, 0.0);
    assert!(cage(&game));
    run(&mut game, 4.0);
    assert!(!cage(&game));
}
