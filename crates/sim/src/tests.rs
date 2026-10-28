use std::sync::Arc;

use asset::data::{
    Attack, ChampionData, GameData, Item, ItemStats, Limit, QuickBuy, UnitData, UnitStats,
};
use glam::Vec2;
use protocol::{Command, Slot, Summoner, TICK, Team, UnitId, UnitKind};

use crate::items;
use crate::layout::Layout;
use crate::{Cheat, Entrant, FIRST_WAVE, Game, MINION_INTERVAL, START_GOLD, START_LEVEL};

fn stats(health: f32, damage: f32, range: f32) -> UnitStats {
    UnitStats {
        health,
        health_per_level: 100.0,
        mana: 300.0,
        attack_damage: damage,
        armor: 20.0,
        magic_resist: 30.0,
        move_speed: 330.0,
        attack_range: range,
        attack_speed: 0.7,
        attack_speed_ratio: 0.7,
        crit_damage: 1.75,
        gameplay_radius: 65.0,
        pathing_radius: 35.0,
        acquisition_range: 800.0,
        gold_given: 20.0,
        xp_given: 60.0,
        attack: Attack {
            total_time: 1.4,
            cast_time: 0.3,
            missile_speed: 1500.0,
            ..Attack::default()
        },
        ..UnitStats::default()
    }
}

fn data() -> Arc<GameData> {
    let champion = |name: &str| ChampionData {
        name: name.into(),
        stats: stats(600.0, 60.0, 125.0),
        ..ChampionData::default()
    };
    let unit = |name: &str, health: f32, damage: f32, range: f32| UnitData {
        name: name.into(),
        stats: stats(health, damage, range),
    };
    Arc::new(GameData {
        summoners: Vec::new(),
        quick_buy: QuickBuy::default(),
        champions: ["Emperor", "Cardmaster", "Colossus", "Wanderer", "Guardian"]
            .into_iter()
            .map(champion)
            .collect(),
        units: vec![
            unit("HA_OrderMinionMelee", 450.0, 12.0, 110.0),
            unit("HA_ChaosMinionMelee", 450.0, 12.0, 110.0),
            unit("HA_OrderMinionRanged", 300.0, 20.0, 550.0),
            unit("HA_ChaosMinionRanged", 300.0, 20.0, 550.0),
            unit("Turret", 4000.0, 180.0, 775.0),
            unit("Inhibitor", 3000.0, 0.0, 0.0),
            unit("Nexus", 5000.0, 0.0, 0.0),
        ],
        items: vec![
            Item {
                id: 1036,
                name: "Long Sword".into(),
                price: 350,
                total_price: 350,
                stats: ItemStats {
                    attack_damage: 10.0,
                    ..ItemStats::default()
                },
                sell_back: 0.7,
                ..Item::default()
            },
            recipe(1028, 400, 400, &[], &[]),
            recipe(3044, 450, 1200, &[1036, 1028], &[]),
            recipe(items::SPELLBLADE, 900, 900, &[], &["sheen"]),
            recipe(
                items::TRI_FORCE,
                1233,
                3333,
                &[3044, items::SPELLBLADE],
                &["sheen"],
            ),
            recipe(
                items::BANE_BLADE,
                1900,
                2800,
                &[items::SPELLBLADE],
                &["sheen"],
            ),
            Item {
                stats: ItemStats {
                    ability_power: 130.0,
                    ..ItemStats::default()
                },
                ..recipe(items::ARCHMAGES_CROWN, 1100, 3500, &[], &[])
            },
            Item {
                stats: ItemStats {
                    health: 1000.0,
                    ..ItemStats::default()
                },
                ..recipe(items::TITANS_ARMOR, 1100, 3100, &[], &[])
            },
            Item {
                max_stack: 5,
                sell_back: 0.4,
                ..recipe(items::HEALTH_POTION, 50, 50, &[], &["potion"])
            },
        ],
    })
}

fn recipe(id: u32, price: u32, total_price: u32, from: &[u32], limits: &[&str]) -> Item {
    Item {
        id,
        price,
        total_price,
        from: from.to_vec(),
        limits: limits
            .iter()
            .map(|group| Limit {
                group: (*group).into(),
                max: 1,
            })
            .collect(),
        sell_back: 0.7,
        ..Item::default()
    }
}

fn game(players: &[(&str, Team)]) -> Game {
    let players: Vec<Entrant> = players
        .iter()
        .map(|(name, team)| Entrant::new(name, *team, None))
        .collect();
    Game::new(data(), Layout::placeholder(), &players, 7)
}

pub fn assets() -> std::path::PathBuf {
    std::env::var_os("LLL_ASSETS").map_or_else(
        || std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web/assets"),
        std::path::PathBuf::from,
    )
}

fn run(game: &mut Game, seconds: f32) {
    for _ in 0..(seconds / TICK).round() as u32 {
        game.step();
    }
}

#[test]
fn champions_start_in_their_fountain_with_aram_gold_and_level() {
    let game = game(&[("a", Team::Blue), ("b", Team::Red)]);
    for seat in &game.seats {
        let unit = game.unit(seat.unit).unwrap();
        assert_eq!(unit.level, START_LEVEL);
        assert_eq!(unit.gold, START_GOLD);
        assert_eq!(unit.skill_points, START_LEVEL);
        assert!(unit.position.distance(game.layout.fountain(seat.team)) < 300.0);
        assert!(unit.health > 0.0 && unit.health == unit.stats.max_health);
    }
}

#[test]
fn picked_champions_are_kept() {
    let players = [
        Entrant::new("a", Team::Blue, Some(3)),
        Entrant::new("b", Team::Red, Some(3)),
    ];
    let game = Game::new(data(), Layout::placeholder(), &players, 7);
    assert!(game.seats.iter().all(|seat| seat.champion == 3));
}

#[test]
fn champions_on_one_team_get_different_champions() {
    let game = game(&[("a", Team::Blue), ("b", Team::Blue), ("c", Team::Blue)]);
    let mut champions: Vec<u8> = game.seats.iter().map(|seat| seat.champion).collect();
    champions.sort();
    champions.dedup();
    assert_eq!(champions.len(), 3);
}

#[test]
fn move_order_walks_at_move_speed() {
    let mut game = game(&[("a", Team::Blue)]);
    let id = game.seats[0].unit;
    let start = game.unit(id).unwrap().position;
    let goal = start + Vec2::new(2000.0, 0.0);
    game.command(id, Command::Move(goal.to_array()));
    run(&mut game, 1.0);
    let moved = game.unit(id).unwrap().position.distance(start);
    let speed = game.unit(id).unwrap().stats.move_speed;
    assert!((moved - speed).abs() < 20.0, "moved {moved}, speed {speed}");
}

#[test]
fn champions_on_the_same_spot_are_pushed_apart() {
    let mut game = game(&[("a", Team::Blue), ("b", Team::Blue)]);
    let (first, second) = (game.seats[0].unit, game.seats[1].unit);
    let spot = Vec2::new(6500.0, 6500.0);
    for id in [first, second] {
        game.unit_mut(id).unwrap().position = spot;
    }
    run(&mut game, 1.0);
    let distance = game
        .unit(first)
        .unwrap()
        .position
        .distance(game.unit(second).unwrap().position);
    assert!(distance > 40.0, "still {distance} apart");
}

#[test]
fn champions_standing_in_a_turret_are_pushed_out() {
    let mut game = game(&[("a", Team::Blue)]);
    let id = game.seats[0].unit;
    let (turret, reach) = game
        .units
        .iter()
        .find(|unit| unit.kind == UnitKind::Turret && unit.team == Team::Red)
        .map(|unit| (unit.position, unit.base.pathing_radius))
        .unwrap();
    let unit = game.unit_mut(id).unwrap();
    unit.position = turret + Vec2::new(40.0, 0.0);
    let reach = reach + unit.base.pathing_radius;
    run(&mut game, 1.0);
    let distance = game.unit(id).unwrap().position.distance(turret);
    assert!(distance >= reach - 1.0, "only {distance} from the turret");
}

#[test]
fn minions_leave_the_barracks_one_at_a_time_and_march_down_the_lane() {
    let mut game = game(&[]);
    let blue = |game: &Game| -> Vec<Vec2> {
        game.units
            .iter()
            .filter(|unit| unit.is_minion() && unit.team == Team::Blue)
            .map(|unit| unit.position)
            .collect()
    };
    run(&mut game, FIRST_WAVE + 0.1);
    let first = blue(&game);
    assert_eq!(first.len(), 1);
    assert!(first[0].distance(game.layout.barracks[0]) < 50.0);
    run(&mut game, MINION_INTERVAL * 6.0);
    assert_eq!(blue(&game).len(), 6);
    let start: f32 = blue(&game).iter().map(|position| position.x).sum();
    run(&mut game, 3.0);
    let later: f32 = blue(&game).iter().map(|position| position.x).sum();
    assert!(later > start);
}

#[test]
fn minions_fight_and_die_in_the_middle() {
    let mut game = game(&[]);
    run(&mut game, FIRST_WAVE + 60.0);
    let deaths = game
        .events
        .iter()
        .filter(|event| matches!(event, protocol::Event::Kill { .. }))
        .count();
    let alive = game.units.iter().filter(|unit| unit.is_minion()).count();
    assert!(
        deaths > 0 || alive < 12 * 2,
        "deaths {deaths} alive {alive}"
    );
}

#[test]
fn attacking_a_champion_deals_mitigated_damage() {
    let mut game = game(&[("a", Team::Blue), ("b", Team::Red)]);
    let (a, b) = (game.seats[0].unit, game.seats[1].unit);
    let middle = Vec2::new(6000.0, 6000.0);
    game.unit_mut(a).unwrap().position = middle;
    game.unit_mut(b).unwrap().position = middle + Vec2::new(100.0, 0.0);
    let total = |game: &Game| {
        let unit = game.unit(b).unwrap();
        unit.health + unit.shield()
    };
    game.step();
    let before = total(&game);
    game.command(a, Command::Attack(b));
    run(&mut game, 1.0);
    assert!(total(&game) < before);
}

#[test]
fn killing_a_champion_pays_the_killer() {
    let mut game = game(&[("a", Team::Blue), ("b", Team::Red)]);
    let (a, b) = (game.seats[0].unit, game.seats[1].unit);
    let gold = game.unit(a).unwrap().gold;
    game.damage(Some(a), b, 100000.0, protocol::DamageKind::True, false);
    assert!(game.unit(b).unwrap().dead);
    assert!(game.unit(a).unwrap().gold >= gold + crate::KILL_GOLD);
    assert_eq!(game.seats[0].score.kills, 1);
    assert_eq!(game.seats[1].score.deaths, 1);
    let wait = crate::respawn_time(game.unit(b).unwrap().level) + 0.5;
    run(&mut game, wait);
    let unit = game.unit(b).unwrap();
    assert!(!unit.dead);
    assert!(unit.position.distance(game.layout.fountain(Team::Red)) < 1.0);
}

#[test]
fn champions_executed_after_champion_damage_credit_the_last_champion() {
    let mut game = game(&[("a", Team::Blue), ("b", Team::Red), ("c", Team::Blue)]);
    let (a, b, c) = (game.seats[0].unit, game.seats[1].unit, game.seats[2].unit);
    game.damage(Some(c), b, 1.0, protocol::DamageKind::True, false);
    game.damage(Some(a), b, 1.0, protocol::DamageKind::True, false);
    game.events.clear();
    game.damage(None, b, 100000.0, protocol::DamageKind::True, false);
    assert!(game.events.iter().any(|event| matches!(
        event,
        protocol::Event::Kill { killer: Some(killer), victim, assists, .. }
            if *killer == a && *victim == b && *assists == [c]
    )));
    assert_eq!(game.seats[0].score.kills, 1);
    assert_eq!(game.seats[2].score.assists, 1);
}

#[test]
fn kills_tell_everyone_how_many_of_the_victims_team_still_stand() {
    let mut game = game(&[("a", Team::Blue), ("b", Team::Red), ("c", Team::Red)]);
    let (a, b, c) = (game.seats[0].unit, game.seats[1].unit, game.seats[2].unit);
    let survivors = |game: &Game| {
        game.events.iter().find_map(|event| match event {
            protocol::Event::Kill { survivors, .. } => Some(*survivors),
            _ => None,
        })
    };
    game.damage(Some(a), b, 100000.0, protocol::DamageKind::True, false);
    assert_eq!(survivors(&game), Some(1));
    game.events.clear();
    game.damage(Some(a), c, 100000.0, protocol::DamageKind::True, false);
    assert_eq!(survivors(&game), Some(0));
}

#[test]
fn scores_carry_each_champions_current_kill_streak() {
    let mut game = game(&[("a", Team::Blue), ("b", Team::Red), ("c", Team::Red)]);
    let (a, b, c) = (game.seats[0].unit, game.seats[1].unit, game.seats[2].unit);
    game.damage(Some(a), b, 100000.0, protocol::DamageKind::True, false);
    assert_eq!(game.seats[0].score.spree, 1);
    game.damage(Some(c), a, 100000.0, protocol::DamageKind::True, false);
    assert_eq!(game.seats[0].score.spree, 0);
    assert_eq!(game.seats[2].score.spree, 1);
}

#[test]
fn waves_facing_a_fallen_inhibitor_are_led_by_a_super_minion() {
    let mut game = game(&[("a", Team::Blue), ("b", Team::Red)]);
    for unit in game
        .units
        .iter_mut()
        .filter(|unit| unit.kind == UnitKind::Inhibitor && unit.team == Team::Red)
    {
        unit.dead = true;
    }
    game.time = game.next_wave;
    game.step();
    let supers: Vec<Team> = game
        .units
        .iter()
        .filter(|unit| unit.kind == UnitKind::Minion(protocol::MinionKind::Super))
        .map(|unit| unit.team)
        .collect();
    assert!(supers.contains(&Team::Blue));
}

#[test]
fn champions_dying_without_champion_damage_are_executed() {
    let mut game = game(&[("a", Team::Blue), ("b", Team::Red)]);
    let b = game.seats[1].unit;
    game.damage(None, b, 100000.0, protocol::DamageKind::True, false);
    assert!(game.events.iter().any(|event| matches!(
        event,
        protocol::Event::Kill { killer: None, victim, assists, .. }
            if *victim == b && assists.is_empty()
    )));
    assert_eq!(game.seats[0].score.kills, 0);
}

#[test]
fn turrets_shoot_enemy_minions_in_range() {
    let mut game = game(&[]);
    let turret = game
        .units
        .iter()
        .find(|unit| {
            unit.kind == UnitKind::Turret
                && unit.team == Team::Red
                && unit.tier == Some(crate::unit::Tier::Outer)
        })
        .unwrap()
        .position;
    run(&mut game, FIRST_WAVE + 0.1);
    let minion = game
        .units
        .iter()
        .find(|unit| unit.is_minion() && unit.team == Team::Blue)
        .unwrap()
        .id;
    game.unit_mut(minion).unwrap().position = turret + Vec2::new(300.0, 0.0);
    let before = game.unit(minion).unwrap().health;
    let facing = |game: &Game| {
        game.units
            .iter()
            .find(|unit| unit.kind == UnitKind::Turret && unit.position == turret)
            .unwrap()
            .facing
    };
    let standing = facing(&game);
    run(&mut game, 2.5);
    assert!(game.unit(minion).is_none_or(|unit| unit.health < before));
    assert_eq!(facing(&game), standing);
}

#[test]
fn turrets_only_defend_champions_hit_under_them() {
    let mut game = game(&[("a", Team::Blue), ("b", Team::Red)]);
    let (ally, enemy) = (game.seats[0].unit, game.seats[1].unit);
    let (turret, position) = game
        .units
        .iter()
        .find(|unit| {
            unit.kind == UnitKind::Turret
                && unit.team == Team::Blue
                && unit.tier == Some(crate::unit::Tier::Outer)
        })
        .map(|unit| (unit.id, unit.position))
        .unwrap();
    run(&mut game, FIRST_WAVE + 0.1);
    let minion = game
        .units
        .iter()
        .find(|unit| unit.is_minion() && unit.team == Team::Red)
        .unwrap()
        .id;
    let away = Vec2::new(-1.0, 1.0).normalize();
    for (id, offset) in [(minion, 200.0), (enemy, 500.0), (ally, 1500.0)] {
        let unit = game.unit_mut(id).unwrap();
        unit.position = position + away * offset;
        unit.last_position = unit.position;
    }
    let target = |game: &Game| match game.unit(turret).unwrap().order {
        crate::unit::Order::Attack(target) => Some(target),
        _ => None,
    };
    game.damage(Some(enemy), ally, 1.0, protocol::DamageKind::True, false);
    game.step();
    assert_eq!(target(&game), Some(minion));
    let unit = game.unit_mut(ally).unwrap();
    unit.position = position + away * 300.0;
    game.damage(Some(enemy), ally, 1.0, protocol::DamageKind::True, false);
    game.step();
    assert_eq!(target(&game), Some(enemy));
}

#[test]
fn inner_structures_are_protected_until_the_outer_turret_falls() {
    let mut game = game(&[]);
    let inner = game
        .units
        .iter()
        .find(|unit| unit.team == Team::Red && unit.tier == Some(crate::unit::Tier::Inner))
        .unwrap()
        .id;
    assert!(game.unit(inner).unwrap().invulnerable);
    let outer = game
        .units
        .iter()
        .find(|unit| unit.team == Team::Red && unit.tier == Some(crate::unit::Tier::Outer))
        .unwrap()
        .id;
    game.damage(None, outer, 1e9, protocol::DamageKind::True, false);
    run(&mut game, TICK);
    assert!(!game.unit(inner).unwrap().invulnerable);
}

#[test]
fn destroying_the_nexus_ends_the_game() {
    let mut game = game(&[]);
    for unit in game.units.iter_mut().filter(|unit| unit.team == Team::Red) {
        unit.invulnerable = false;
    }
    let nexus = game
        .units
        .iter()
        .find(|unit| unit.team == Team::Red && unit.kind == UnitKind::Nexus)
        .unwrap()
        .id;
    game.update_structure_protection();
    for unit in game
        .units
        .iter_mut()
        .filter(|unit| unit.team == Team::Red && unit.is_structure())
    {
        if unit.kind != UnitKind::Nexus {
            unit.dead = true;
        }
    }
    game.update_structure_protection();
    game.damage(None, nexus, 1e9, protocol::DamageKind::True, false);
    assert_eq!(game.winner, Some(Team::Blue));
}

#[test]
fn skill_points_follow_standard_rules() {
    let mut game = game(&[("a", Team::Blue)]);
    let id = game.seats[0].unit;
    game.command(id, Command::LevelUp(Slot::R));
    assert_eq!(game.unit(id).unwrap().ability(Slot::R).rank, 0);
    game.command(id, Command::LevelUp(Slot::Q));
    game.command(id, Command::LevelUp(Slot::Q));
    assert_eq!(game.unit(id).unwrap().ability(Slot::Q).rank, 2);
    game.command(id, Command::LevelUp(Slot::Q));
    assert_eq!(game.unit(id).unwrap().ability(Slot::Q).rank, 2);
    game.command(id, Command::LevelUp(Slot::W));
    assert_eq!(game.unit(id).unwrap().skill_points, 0);
}

#[test]
fn undo_reverts_trades_until_combat() {
    let mut game = game(&[("a", Team::Blue)]);
    let id = game.seats[0].unit;
    game.unit_mut(id).unwrap().gold = 10_000.0;
    game.command(id, Command::Buy(1036));
    game.command(id, Command::Buy(1028));
    game.command(id, Command::Buy(3044));
    game.command(id, Command::Sell(0));
    assert!(game.snapshot(Some(id)).you.unwrap().can_undo);
    game.command(id, Command::Undo);
    assert_eq!(game.unit(id).unwrap().items[0], Some(3044));
    game.command(id, Command::Undo);
    let unit = game.unit(id).unwrap();
    assert_eq!(&unit.items[..2], [Some(1036), Some(1028)]);
    assert_eq!(unit.gold, 10_000.0 - 750.0);
    game.damage(None, id, 10.0, protocol::DamageKind::True, false);
    game.command(id, Command::Undo);
    assert_eq!(game.unit(id).unwrap().gold, 10_000.0 - 750.0);
    assert!(!game.snapshot(Some(id)).you.unwrap().can_undo);
}

#[test]
fn potions_stack_to_five_in_one_slot() {
    let mut game = game(&[("a", Team::Blue)]);
    let id = game.seats[0].unit;
    for _ in 0..6 {
        game.command(id, Command::Buy(items::HEALTH_POTION));
    }
    let unit = game.unit(id).unwrap();
    assert_eq!(
        unit.items,
        [Some(items::HEALTH_POTION), None, None, None, None, None]
    );
    assert_eq!(unit.stacks[0], 5);
    assert_eq!(unit.gold, START_GOLD - 250.0);
    game.command(id, Command::Sell(0));
    assert_eq!(game.unit(id).unwrap().stacks[0], 4);
    assert_eq!(game.unit(id).unwrap().gold, START_GOLD - 250.0 + 20.0);
    game.command(id, Command::Undo);
    assert_eq!(game.unit(id).unwrap().stacks[0], 5);
    game.command(
        id,
        Command::UseItem(
            0,
            protocol::Target {
                point: [0.0, 0.0],
                unit: None,
            },
        ),
    );
    assert_eq!(game.unit(id).unwrap().stacks[0], 4);
    assert!(!game.snapshot(Some(id)).you.unwrap().can_undo);
}

#[test]
fn shopping_only_works_in_the_fountain() {
    let mut game = game(&[("a", Team::Blue)]);
    let id = game.seats[0].unit;
    let damage = game.unit(id).unwrap().stats.attack_damage;
    game.command(id, Command::Buy(1036));
    let unit = game.unit(id).unwrap();
    assert_eq!(unit.gold, START_GOLD - 350.0);
    assert_eq!(unit.stats.attack_damage, damage + 10.0);
    game.unit_mut(id).unwrap().position = Vec2::new(6000.0, 6000.0);
    game.command(id, Command::Buy(1036));
    assert_eq!(game.unit(id).unwrap().gold, START_GOLD - 350.0);
}

#[test]
fn buying_uses_components_from_the_whole_recipe_and_respects_limits() {
    let mut game = game(&[("a", Team::Blue)]);
    let id = game.seats[0].unit;
    game.unit_mut(id).unwrap().gold = 10_000.0;
    game.command(id, Command::Buy(1036));
    game.command(id, Command::Buy(items::SPELLBLADE));
    let gold = game.unit(id).unwrap().gold;
    let full = game.data.item(items::TRI_FORCE).unwrap().total_price as f32;
    game.command(id, Command::Buy(items::TRI_FORCE));
    let unit = game.unit(id).unwrap();
    assert_eq!(unit.items[0], Some(items::TRI_FORCE));
    assert_eq!(unit.items.iter().flatten().count(), 1);
    assert_eq!(gold - unit.gold, full - 350.0 - 900.0);
    game.command(id, Command::Buy(items::BANE_BLADE));
    assert!(!items::has(game.unit(id).unwrap(), items::BANE_BLADE));
    game.command(id, Command::Sell(0));
    assert_eq!(
        game.unit(id).unwrap().gold,
        gold - (full - 1250.0) + (full * 0.7).floor()
    );
}

#[test]
fn flash_blinks_up_to_its_range() {
    let mut game = game(&[("a", Team::Blue)]);
    let id = game.seats[0].unit;
    let start = game.unit(id).unwrap().position;
    let far = start + Vec2::new(1000.0, 0.0);
    game.command(
        id,
        Command::Cast(
            Slot::D,
            protocol::Target {
                point: far.to_array(),
                unit: None,
            },
        ),
    );
    let after = game.unit(id).unwrap().position;
    assert!((after.distance(start) - crate::champions::FLASH_RANGE).abs() < 1.0);
    assert!(game.unit(id).unwrap().ability(Slot::D).cooldown > 0.0);
}

#[test]
fn snapshots_report_your_state() {
    let game = game(&[("a", Team::Blue)]);
    let id = game.seats[0].unit;
    let snapshot = game.snapshot(Some(id));
    let you = snapshot.you.unwrap();
    assert_eq!(you.gold, START_GOLD);
    assert!(you.can_shop);
    assert!(snapshot.units.iter().any(|unit| unit.id == id));
}

#[test]
fn map_is_walkable_from_base_to_base() {
    let assets = assets();
    let Ok(layout) = crate::map::layout(&assets) else {
        return;
    };
    let nav = layout.nav.as_ref().expect("nav grid");
    let structures = |team| {
        layout
            .structures
            .iter()
            .filter(|placement| placement.team == team)
            .count()
    };
    assert_eq!(structures(Team::Blue), 7);
    assert_eq!(structures(Team::Red), 7);
    let route = crate::nav::route(nav, layout.fountain(Team::Blue), layout.fountain(Team::Red));
    let mut anchor = layout.fountain(Team::Blue);
    for point in &route {
        assert!(
            crate::nav::clear_line(nav, anchor, *point),
            "{anchor} -> {point}"
        );
        anchor = *point;
    }
    assert!(anchor.distance(layout.fountain(Team::Red)) < 100.0);
}

#[test]
fn champions_walk_around_turrets_instead_of_through_them() {
    let Ok(layout) = crate::map::layout(&assets()) else {
        return;
    };
    let mut game = Game::new(data(), layout, &[Entrant::new("a", Team::Blue, None)], 1);
    let id = game.seats[0].unit;
    let turret = game
        .units
        .iter()
        .find(|unit| unit.kind == UnitKind::Turret)
        .map(|unit| (unit.position, unit.base.pathing_radius))
        .unwrap();
    let start = game.walkable_point(turret.0 - Vec2::new(450.0, 0.0));
    let goal = game.walkable_point(turret.0 + Vec2::new(450.0, 0.0));
    let unit = game.unit_mut(id).unwrap();
    unit.position = start;
    unit.last_position = start;
    game.command(id, Command::Move(goal.to_array()));
    let mut closest = f32::MAX;
    for _ in 0..(4.0 / TICK) as usize {
        game.step();
        closest = closest.min(game.unit(id).unwrap().position.distance(turret.0));
    }
    assert!(closest >= turret.1, "came within {closest} of the turret");
    assert!(game.unit(id).unwrap().position.distance(goal) < 50.0);
}

#[test]
fn units_find_a_new_way_when_their_old_one_runs_into_a_turret() {
    let Ok(layout) = crate::map::layout(&assets()) else {
        return;
    };
    let mut game = Game::new(data(), layout, &[Entrant::new("a", Team::Blue, None)], 1);
    let id = game.seats[0].unit;
    let turret = game
        .units
        .iter()
        .find(|unit| unit.kind == UnitKind::Turret)
        .map(|unit| unit.position)
        .unwrap();
    let start = game.walkable_point(turret - Vec2::new(450.0, 0.0));
    let goal = game.walkable_point(turret + Vec2::new(450.0, 0.0));
    let unit = game.unit_mut(id).unwrap();
    unit.position = start;
    unit.last_position = start;
    unit.order = crate::unit::Order::Move(goal);
    unit.path = vec![goal];
    unit.route = vec![goal];
    unit.route_goal = Some(goal);
    for _ in 0..(4.0 / TICK) as usize {
        game.step();
        let nav = game.layout.nav.as_ref().unwrap();
        assert!(crate::nav::walkable(nav, game.unit(id).unwrap().position));
    }
    assert!(game.unit(id).unwrap().position.distance(goal) < 50.0);
}

#[test]
fn minion_waves_spawn_clear_of_structures() {
    let Ok(layout) = crate::map::layout(&assets()) else {
        return;
    };
    let mut game = Game::new(data(), layout, &[Entrant::new("a", Team::Blue, None)], 1);
    while !game.units.iter().any(|unit| unit.is_minion()) {
        game.step();
    }
    let nav = game.layout.nav.as_ref().unwrap();
    assert!(
        game.units
            .iter()
            .filter(|unit| unit.is_minion())
            .all(|unit| crate::nav::walkable(nav, unit.position))
    );
}

#[test]
fn every_extracted_item_costs_its_listed_total_from_scratch() {
    let Ok(bytes) = std::fs::read(assets().join("data.bin")) else {
        return;
    };
    let data = GameData::from_bytes(&bytes).expect("data");
    for item in &data.items {
        let cost = data
            .quote(item.id, &[None; 6], &[0; 6])
            .map(|quote| quote.cost);
        assert_eq!(cost, Some(item.total_price), "{}", item.name);
    }
}

#[test]
fn every_extracted_champion_has_a_kit() {
    let assets = assets();
    let Ok(bytes) = std::fs::read(assets.join("data.bin")) else {
        return;
    };
    let data = GameData::from_bytes(&bytes).expect("data");
    for champion in &data.champions {
        assert!(
            crate::champions::kit(&champion.name).is_some(),
            "{} has no kit",
            champion.name
        );
    }
}

#[test]
fn turrets_hold_the_lane_on_the_real_map_without_players() {
    let assets = assets();
    let (Ok(layout), Ok(bytes)) = (
        crate::map::layout(&assets),
        std::fs::read(assets.join("data.bin")),
    ) else {
        return;
    };
    let data = Arc::new(GameData::from_bytes(&bytes).expect("data"));
    let mut game = Game::new(data, layout, &[], 7);
    for _ in 0..(protocol::TICK_RATE * 300) {
        game.step();
        game.clear_events();
    }
    let turrets = |team| {
        game.units
            .iter()
            .filter(|unit| unit.kind == UnitKind::Turret && unit.team == team && unit.alive())
            .count()
    };
    assert_eq!(turrets(Team::Blue), 4);
    assert_eq!(turrets(Team::Red), 4);
    assert!(game.units.iter().any(|unit| unit.is_minion()));
}

#[test]
fn enemies_out_of_sight_are_not_sent() {
    let mut game = game(&[("a", Team::Blue), ("b", Team::Red)]);
    let blue = game.seats[0].unit;
    let red = game.seats[1].unit;
    let snapshot = game.snapshot(Some(blue));
    assert!(!snapshot.units.iter().any(|unit| unit.id == red));
    let near = game.unit(blue).unwrap().position + Vec2::new(300.0, 0.0);
    game.units
        .iter_mut()
        .find(|unit| unit.id == red)
        .unwrap()
        .position = near;
    let snapshot = game.snapshot(Some(blue));
    assert!(snapshot.units.iter().any(|unit| unit.id == red));
    assert!(
        snapshot
            .units
            .iter()
            .any(|unit| unit.kind == UnitKind::Nexus && unit.team == Team::Red)
    );
}

#[test]
fn stasis_makes_you_untouchable_and_potions_heal() {
    let mut game = game(&[("a", Team::Blue)]);
    let id = game.seats[0].unit;
    let unit = game.units.iter_mut().find(|unit| unit.id == id).unwrap();
    unit.items[0] = Some(crate::items::STASIS_HOURGLASS);
    unit.items[1] = Some(crate::items::HEALTH_POTION);
    unit.health = 100.0;
    game.command(
        id,
        Command::UseItem(
            0,
            protocol::Target {
                point: [0.0, 0.0],
                unit: None,
            },
        ),
    );
    assert_eq!(
        game.damage(None, id, 50.0, protocol::DamageKind::True, false),
        0.0
    );
    run(&mut game, 3.0);
    assert!(game.damage(None, id, 50.0, protocol::DamageKind::True, false) > 0.0);
    let before = game.unit(id).unwrap().health;
    game.command(
        id,
        Command::UseItem(
            1,
            protocol::Target {
                point: [0.0, 0.0],
                unit: None,
            },
        ),
    );
    assert_eq!(game.unit(id).unwrap().items[1], None);
    run(&mut game, 5.0);
    assert!(game.unit(id).unwrap().health > before + 30.0);
}

#[test]
fn ruined_blade_slows_every_third_hit_and_spares_structures() {
    let (mut game, hero, enemy) = summoner_duel([Summoner::Flash, Summoner::Heal]);
    game.unit_mut(hero).unwrap().items[0] = Some(items::RUINED_BLADE);
    let speed = |game: &Game| game.unit(enemy).unwrap().stats.move_speed;
    let normal = speed(&game);
    game.on_hit(hero, enemy);
    game.on_hit(hero, enemy);
    assert_eq!(speed(&game), normal);
    game.on_hit(hero, enemy);
    assert!(speed(&game) < normal);
    let turret = game
        .units
        .iter()
        .find(|unit| unit.kind == UnitKind::Turret && unit.team == Team::Red)
        .map(|unit| unit.id)
        .unwrap();
    let health = game.unit(turret).unwrap().health;
    game.on_hit(hero, turret);
    assert_eq!(game.unit(turret).unwrap().health, health);
}

#[test]
fn archmages_crown_raises_ability_power_by_thirty_percent() {
    let mut game = game(&[("a", Team::Blue)]);
    let id = game.seats[0].unit;
    game.unit_mut(id).unwrap().items[0] = Some(items::ARCHMAGES_CROWN);
    game.refresh_stats(id);
    let power = game.unit(id).unwrap().stats.ability_power;
    assert!((power - 130.0 * 1.3).abs() < 0.01, "{power}");
}

#[test]
fn sunfire_burns_only_for_a_while_after_a_fight() {
    let (mut game, hero, enemy) = summoner_duel([Summoner::Flash, Summoner::Heal]);
    game.unit_mut(hero).unwrap().items[0] = Some(items::SUNFIRE_SHIELD);
    let health = |game: &Game| game.unit(enemy).unwrap().health;
    let calm = health(&game);
    run(&mut game, 2.0);
    assert_eq!(health(&game), calm);
    game.damage(None, hero, 10.0, protocol::DamageKind::True, false);
    run(&mut game, 1.1);
    let burnt = health(&game);
    assert!(burnt < calm);
    run(&mut game, 3.0);
    let settled = health(&game);
    run(&mut game, 3.0);
    assert_eq!(health(&game), settled);
}

#[test]
fn titans_armor_adds_item_health_and_heals_after_eight_calm_seconds() {
    let mut game = game(&[("a", Team::Blue)]);
    let id = game.seats[0].unit;
    let base = game.unit(id).unwrap().stats.max_health;
    let unit = game.unit_mut(id).unwrap();
    unit.items[0] = Some(items::TITANS_ARMOR);
    unit.items[1] = Some(items::TITANS_ARMOR);
    unit.position = Vec2::new(6500.0, 6500.0);
    game.refresh_stats(id);
    let max = game.unit(id).unwrap().stats.max_health;
    assert!((max - base - 2240.0).abs() < 0.01, "{max}");
    game.unit_mut(id).unwrap().health = 1000.0;
    game.damage(None, id, 10.0, protocol::DamageKind::True, false);
    let hurt = game.unit(id).unwrap().health;
    run(&mut game, 7.5);
    let calm = game.unit(id).unwrap().health;
    run(&mut game, 3.0);
    let healed = game.unit(id).unwrap().health;
    assert!(calm - hurt < 100.0, "{hurt} {calm}");
    assert!(healed - calm > 100.0, "{calm} {healed}");
}

#[test]
fn tri_force_quickens_after_an_attack() {
    let (mut game, hero, enemy) = summoner_duel([Summoner::Flash, Summoner::Heal]);
    game.unit_mut(hero).unwrap().items[0] = Some(items::TRI_FORCE);
    game.refresh_stats(hero);
    let speed = |game: &Game| game.unit(hero).unwrap().stats.move_speed;
    let normal = speed(&game);
    game.on_hit(hero, enemy);
    assert!((speed(&game) - normal - 20.0).abs() < 0.01);
    run(&mut game, 2.1);
    assert_eq!(speed(&game), normal);
}

#[test]
fn overlapping_potions_heal_their_full_amount_and_no_more() {
    let mut game = game(&[("a", Team::Blue)]);
    let id = game.seats[0].unit;
    let unit = game.unit_mut(id).unwrap();
    unit.position = Vec2::new(6500.0, 6500.0);
    unit.items[0] = Some(items::HEALTH_POTION);
    unit.stacks[0] = 2;
    unit.health = 100.0;
    let drink = |game: &mut Game| {
        game.command(
            id,
            Command::UseItem(
                0,
                protocol::Target {
                    point: [0.0, 0.0],
                    unit: None,
                },
            ),
        );
    };
    drink(&mut game);
    run(&mut game, 10.0);
    drink(&mut game);
    run(&mut game, 20.0);
    let healed = game.unit(id).unwrap().health - 100.0;
    assert!((healed - 240.0).abs() < 10.0, "{healed}");
}

#[test]
fn stasis_cannot_be_targeted_or_crowd_controlled() {
    let (mut game, hero, enemy) = summoner_duel([Summoner::Flash, Summoner::Heal]);
    game.unit_mut(hero).unwrap().items[0] = Some(items::STASIS_HOURGLASS);
    game.command(
        hero,
        Command::UseItem(
            0,
            protocol::Target {
                point: [0.0, 0.0],
                unit: None,
            },
        ),
    );
    let speed = game.unit(hero).unwrap().stats.move_speed;
    game.stun(hero, 1.0, None);
    game.slow(hero, 0.5, 1.0, None);
    let unit = game.unit(hero).unwrap();
    assert!(!unit.has_buff("stun"));
    assert_eq!(unit.stats.move_speed, speed);
    game.command(enemy, Command::Attack(hero));
    run(&mut game, 1.0);
    assert!(
        !game.events.iter().any(
            |event| matches!(event, protocol::Event::Damage { target, .. } if *target == hero)
        )
    );
    let snapshot = game.snapshot(Some(enemy));
    let seen = snapshot.units.iter().find(|unit| unit.id == hero).unwrap();
    assert!(!seen.targetable);
}

#[test]
fn stasis_stays_on_cooldown_through_death() {
    let (mut game, hero, enemy) = summoner_duel([Summoner::Flash, Summoner::Heal]);
    game.unit_mut(hero).unwrap().items[0] = Some(items::STASIS_HOURGLASS);
    game.command(
        hero,
        Command::UseItem(
            0,
            protocol::Target {
                point: [0.0, 0.0],
                unit: None,
            },
        ),
    );
    run(&mut game, 3.0);
    game.damage(
        Some(enemy),
        hero,
        100_000.0,
        protocol::DamageKind::True,
        false,
    );
    assert!(game.unit(hero).unwrap().dead);
    let unit = game.unit(hero).unwrap();
    assert!(items::cooldown(&game, unit, 0) > 100.0);
}

#[test]
fn the_dead_take_no_orders() {
    let (mut game, hero, enemy) = summoner_duel([Summoner::Flash, Summoner::Heal]);
    game.damage(
        Some(enemy),
        hero,
        100_000.0,
        protocol::DamageKind::True,
        false,
    );
    for command in [
        Command::Move([0.0, 0.0]),
        Command::AttackMove([0.0, 0.0]),
        Command::Attack(enemy),
    ] {
        game.command(hero, command);
        assert!(matches!(
            game.unit(hero).unwrap().order,
            crate::unit::Order::Idle
        ));
    }
}

#[test]
fn the_dead_are_not_knocked_around() {
    let (mut game, hero, enemy) = summoner_duel([Summoner::Flash, Summoner::Heal]);
    game.damage(
        Some(enemy),
        hero,
        100_000.0,
        protocol::DamageKind::True,
        false,
    );
    game.knock_back(hero, Vec2::new(9000.0, 9000.0), 300.0);
    game.knock_up(hero, 1.0, None);
    assert!(matches!(
        game.unit(hero).unwrap().action,
        crate::unit::Action::None
    ));
}

#[test]
fn champions_respawn_without_effects_from_their_last_fight() {
    let (mut game, hero, enemy) = summoner_duel([Summoner::Flash, Summoner::Heal]);
    game.damage(
        Some(enemy),
        hero,
        100_000.0,
        protocol::DamageKind::True,
        false,
    );
    game.slow(hero, 0.5, 60.0, None);
    let stun = game.buff("stun", 60.0);
    game.add_buff(hero, stun);
    let bleed = game.buff("hemorrhage", 60.0);
    game.unit_mut(hero).unwrap().buffs.push(bleed);
    let wait = crate::respawn_time(game.unit(hero).unwrap().level) + 0.5;
    run(&mut game, wait);
    let unit = game.unit(hero).unwrap();
    assert!(unit.alive());
    assert!(unit.buffs.is_empty(), "{:?}", unit.buffs);
}

fn summoner_duel(spells: [Summoner; 2]) -> (Game, UnitId, UnitId) {
    let mut game = game(&[("a", Team::Blue), ("b", Team::Red)]);
    let (hero, enemy) = (game.seats[0].unit, game.seats[1].unit);
    let middle = Vec2::new(6500.0, 6500.0);
    for (id, offset) in [(hero, 0.0), (enemy, 300.0)] {
        let unit = game.unit_mut(id).unwrap();
        unit.position = middle + Vec2::new(offset, 0.0);
        unit.summoners = spells;
    }
    (game, hero, enemy)
}

fn cast_at(game: &mut Game, id: UnitId, slot: Slot, unit: Option<UnitId>) {
    let point = unit
        .and_then(|unit| game.unit(unit))
        .map_or(Vec2::ZERO, |unit| unit.position);
    game.command(
        id,
        Command::Cast(
            slot,
            protocol::Target {
                point: point.to_array(),
                unit,
            },
        ),
    );
}

#[test]
fn roots_stop_flash_and_the_mark_dash() {
    let (mut game, hero, enemy) = summoner_duel([Summoner::Flash, Summoner::Mark]);
    let start = game.unit(hero).unwrap().position;
    cast_at(&mut game, hero, Slot::F, Some(enemy));
    run(&mut game, 0.5);
    assert!(game.unit(enemy).unwrap().has_buff("mark"));
    game.root(hero, 2.0, None);
    cast_at(&mut game, hero, Slot::D, Some(enemy));
    cast_at(&mut game, hero, Slot::F, Some(enemy));
    let unit = game.unit(hero).unwrap();
    assert_eq!(unit.position, start);
    assert_eq!(unit.ability(Slot::D).cooldown, 0.0);
    assert!(!matches!(unit.action, crate::unit::Action::Dash { .. }));
}

#[test]
fn snapshots_tell_you_when_your_mark_can_dash() {
    let (mut game, hero, enemy) = summoner_duel([Summoner::Flash, Summoner::Mark]);
    let recast = |game: &Game| game.snapshot(Some(hero)).you.unwrap().abilities[5].recast;
    assert!(!recast(&game));
    cast_at(&mut game, hero, Slot::F, Some(enemy));
    run(&mut game, 0.5);
    assert!(recast(&game));
    assert!(!game.snapshot(Some(enemy)).you.unwrap().abilities[5].recast);
    cast_at(&mut game, hero, Slot::F, Some(enemy));
    assert!(!recast(&game));
}

#[test]
fn heal_ghost_and_barrier_help_yourself() {
    let (mut game, hero, _) = summoner_duel([Summoner::Heal, Summoner::Ghost]);
    let speed = game.unit(hero).unwrap().stats.move_speed;
    game.unit_mut(hero).unwrap().health = 100.0;
    cast_at(&mut game, hero, Slot::D, None);
    cast_at(&mut game, hero, Slot::F, None);
    run(&mut game, 0.1);
    let unit = game.unit(hero).unwrap();
    assert!(unit.health > 150.0);
    assert!(unit.stats.move_speed > speed * 1.2);
    assert!(unit.ability(Slot::D).cooldown > 0.0 && unit.ability(Slot::F).cooldown > 0.0);
    game.unit_mut(hero).unwrap().summoners = [Summoner::Barrier, Summoner::Ghost];
    cast_at(&mut game, hero, Slot::D, None);
    assert_eq!(game.unit(hero).unwrap().shield(), 0.0);
    game.unit_mut(hero).unwrap().ability_mut(Slot::D).cooldown = 0.0;
    cast_at(&mut game, hero, Slot::D, None);
    assert!(game.unit(hero).unwrap().shield() > 100.0);
}

#[test]
fn exhaust_weakens_and_ignite_burns_through_healing() {
    let (mut game, hero, enemy) = summoner_duel([Summoner::Exhaust, Summoner::Ignite]);
    cast_at(&mut game, hero, Slot::D, Some(enemy));
    assert!(game.unit(enemy).unwrap().has_buff("exhaust"));
    let before = game.unit(hero).unwrap().health;
    let dealt = game.damage(Some(enemy), hero, 100.0, protocol::DamageKind::True, false);
    assert!((dealt - 65.0).abs() < 0.01);
    assert!(game.unit(hero).unwrap().health < before);
    game.clear_events();
    cast_at(&mut game, hero, Slot::F, Some(enemy));
    run(&mut game, 5.5);
    let burned: f32 = game
        .events
        .iter()
        .filter_map(|event| match event {
            protocol::Event::Damage {
                source: Some(source),
                target,
                amount,
                ..
            } if *source == hero && *target == enemy => Some(*amount),
            _ => None,
        })
        .sum();
    assert!(burned >= 69.0, "ignite dealt {burned}");
    game.unit_mut(hero).unwrap().ability_mut(Slot::F).cooldown = 0.0;
    cast_at(&mut game, hero, Slot::F, Some(enemy));
    game.unit_mut(enemy).unwrap().health = 200.0;
    game.heal(enemy, 100.0);
    assert!((game.unit(enemy).unwrap().health - 260.0).abs() < 0.01);
}

#[test]
fn cleanse_clears_disables_and_clarity_restores_mana() {
    let (mut game, hero, _) = summoner_duel([Summoner::Cleanse, Summoner::Clarity]);
    game.stun(hero, 2.0, None);
    game.slow(hero, 0.5, 2.0, None);
    cast_at(&mut game, hero, Slot::D, None);
    let unit = game.unit(hero).unwrap();
    assert!(!unit.is_stunned() && !unit.has_buff("slow"));
    game.unit_mut(hero).unwrap().mana = 0.0;
    cast_at(&mut game, hero, Slot::F, None);
    let unit = game.unit(hero).unwrap();
    assert!((unit.mana - unit.stats.max_mana * 0.5).abs() < 0.01);
}

#[test]
fn practice_cheats_parse_and_apply() {
    assert_eq!(Cheat::parse("/level 18"), Some(Cheat::Level(18)));
    assert_eq!(Cheat::parse("/level 99"), Some(Cheat::Level(18)));
    assert_eq!(Cheat::parse("/nocd"), Some(Cheat::NoCooldowns));
    assert_eq!(Cheat::parse("level 6"), None);
    assert_eq!(Cheat::parse("/gold"), None);
    assert_eq!(Cheat::parse("/level nan"), None);
    assert_eq!(Cheat::parse("/gold inf"), None);
    let mut game = game(&[("a", Team::Blue)]);
    let id = game.seats[0].unit;
    game.cheat(id, Cheat::Level(6));
    assert_eq!(game.unit(id).unwrap().level, 6);
    game.cheat(id, Cheat::NoCooldowns);
    game.unit_mut(id).unwrap().ability_mut(Slot::Q).cooldown = 10.0;
    run(&mut game, 0.1);
    assert!(game.unit(id).unwrap().ability(Slot::Q).cooldown <= TICK);
}

#[test]
fn minion_spawning_can_be_switched_off_and_on_like_the_practice_tool() {
    assert_eq!(Cheat::parse("/minions"), Some(Cheat::Minions));
    let mut game = game(&[("a", Team::Blue)]);
    let id = game.seats[0].unit;
    let minions = |game: &Game| game.units.iter().filter(|unit| unit.is_minion()).count();
    let first_wave = game.layout.first_wave;
    game.cheat(id, Cheat::Minions);
    run(&mut game, first_wave + 1.0);
    assert_eq!(minions(&game), 0);
    game.cheat(id, Cheat::Minions);
    run(&mut game, 1.0);
    assert!(minions(&game) > 0);
}

#[test]
fn no_cooldowns_only_affects_whoever_typed_it() {
    let mut game = game(&[("a", Team::Blue), ("b", Team::Red)]);
    let (a, b) = (game.seats[0].unit, game.seats[1].unit);
    game.cheat(a, Cheat::NoCooldowns);
    game.unit_mut(b).unwrap().ability_mut(Slot::Q).cooldown = 10.0;
    run(&mut game, 0.1);
    assert!(game.unit(b).unwrap().ability(Slot::Q).cooldown > 9.0);
}

#[test]
fn the_strongest_of_several_slows_applies_until_it_ends() {
    let mut game = game(&[("a", Team::Blue), ("b", Team::Red), ("c", Team::Blue)]);
    let (a, b, c) = (game.seats[0].unit, game.seats[1].unit, game.seats[2].unit);
    let speed = |game: &Game| game.unit(b).unwrap().stats.move_speed;
    let full = speed(&game);
    game.slow(b, 0.5, 1.0, Some((a, protocol::Slot::Q)));
    game.slow(b, 0.2, 3.0, Some((c, protocol::Slot::Q)));
    let strong = speed(&game);
    run(&mut game, 1.5);
    let weak = speed(&game);
    run(&mut game, 2.0);
    assert!(strong < weak && weak < full, "{strong} {weak} {full}");
    assert_eq!(speed(&game), full);
}

#[test]
fn aram_balance_scales_damage_haste_and_attack_speed() {
    let mut data = (*data()).clone();
    data.champions[0].balance = asset::data::Balance {
        damage_dealt: 0.9,
        damage_taken: 1.1,
        attack_speed: 1.025,
        ability_haste: 20.0,
    };
    let players = [
        Entrant::new("a", Team::Blue, Some(0)),
        Entrant::new("b", Team::Red, Some(1)),
    ];
    let mut game = Game::new(Arc::new(data), Layout::placeholder(), &players, 7);
    let (a, b) = (game.seats[0].unit, game.seats[1].unit);
    game.step();
    let health = |game: &Game, id| game.unit(id).unwrap().health;
    let before = health(&game, b);
    game.damage(Some(a), b, 100.0, protocol::DamageKind::True, false);
    assert!((before - health(&game, b) - 90.0).abs() < 0.01);
    let before = health(&game, a);
    game.damage(Some(b), a, 100.0, protocol::DamageKind::True, false);
    assert!((before - health(&game, a) - 110.0).abs() < 0.01);
    let (balanced, plain) = (game.unit(a).unwrap().stats, game.unit(b).unwrap().stats);
    assert_eq!(balanced.ability_haste, 20.0);
    assert!((balanced.attack_speed - plain.attack_speed * 1.025).abs() < 1e-4);
}

fn camouflage(game: &mut Game, id: UnitId) {
    let mut buff = game.buff(crate::unit::CAMOUFLAGE, 100.0);
    buff.value = 500.0;
    game.add_buff(id, buff);
}

#[test]
fn camouflaged_champions_are_hidden_until_an_enemy_champion_comes_close() {
    let mut game = game(&[("a", Team::Blue), ("b", Team::Red)]);
    let (a, b) = (game.seats[0].unit, game.seats[1].unit);
    let middle = Vec2::new(6000.0, 6000.0);
    game.unit_mut(a).unwrap().position = middle;
    game.unit_mut(b).unwrap().position = middle + Vec2::new(700.0, 0.0);
    camouflage(&mut game, a);
    let shows = |game: &Game, viewer: UnitId| {
        game.snapshot(Some(viewer))
            .units
            .iter()
            .any(|unit| unit.id == a)
    };
    assert!(shows(&game, a));
    assert!(!shows(&game, b));
    game.unit_mut(b).unwrap().position = middle + Vec2::new(400.0, 0.0);
    assert!(shows(&game, b));
}

#[test]
fn enemies_lose_and_cannot_pick_camouflaged_targets() {
    let mut game = game(&[("a", Team::Blue), ("b", Team::Red)]);
    let (a, b) = (game.seats[0].unit, game.seats[1].unit);
    let middle = Vec2::new(6000.0, 6000.0);
    game.unit_mut(a).unwrap().position = middle;
    game.unit_mut(b).unwrap().position = middle + Vec2::new(900.0, 0.0);
    game.command(b, Command::Attack(a));
    run(&mut game, 0.1);
    camouflage(&mut game, a);
    run(&mut game, 0.1);
    assert!(matches!(
        game.unit(b).unwrap().order,
        crate::unit::Order::Idle
    ));
    game.command(b, Command::Attack(a));
    assert!(matches!(
        game.unit(b).unwrap().order,
        crate::unit::Order::Idle
    ));
}

#[test]
fn allies_hidden_in_brush_are_marked_until_an_enemy_can_see_them() {
    let mut game = game(&[("a", Team::Blue), ("b", Team::Red)]);
    let (a, b) = (game.seats[0].unit, game.seats[1].unit);
    game.layout.nav = Some(asset::map::NavGrid {
        min: [0.0, 0.0],
        cell_size: 100.0,
        width: 100,
        height: 100,
        flags: (0..100 * 100)
            .map(|index| {
                if index % 100 < 20 {
                    asset::map::NavGrid::BRUSH
                } else {
                    0
                }
            })
            .collect(),
        heights: asset::map::HeightField::default(),
    });
    game.unit_mut(a).unwrap().position = Vec2::new(1000.0, 5000.0);
    game.unit_mut(b).unwrap().position = Vec2::new(2600.0, 5000.0);
    let marked = |game: &Game| {
        game.snapshot(Some(a))
            .units
            .iter()
            .find(|unit| unit.id == a)
            .is_some_and(|unit| unit.buffs.contains(&asset::clip_id("brush")))
    };
    assert!(marked(&game));
    assert!(!game.snapshot(Some(b)).units.iter().any(|unit| unit.id == a));
    game.unit_mut(b).unwrap().position = Vec2::new(1300.0, 5000.0);
    assert!(!marked(&game));
}
