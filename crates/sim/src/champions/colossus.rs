use asset::clip_id;
use asset::data::Spell;
use glam::Vec2;
use protocol::{DamageKind, Slot, TICK, Target, UnitId};

use super::{
    Cast, Kit, approach, begin, calculate, cast_range, clamp_point, dash, direction,
    enemies_in_cone, named_spell, rank, spell_of, spend, target_in_range,
};
use crate::unit::{DashKind, Shield, Unit};
use crate::{Game, Hit, Missile, Motion};

const UNSTOPPABLE_SPEED: f32 = 1500.0;
const SHARD_RETURN_SPEED: f32 = 2000.0;
const GRANITE_SHIELD: &str = "granite_shield";
const EMPOWER_WINDOW: f32 = 6.0;
const EMPOWER_RANGE: f32 = 50.0;
const CLEAVE_HALF_ANGLE: f32 = 45.0;

pub const KIT: Kit = Kit {
    cast,
    fire,
    range,
    dashed,
    missile_hit,
    attack_clip,
    on_attack_hit,
    tick,
    movement: &[Slot::R],
    ..Kit::NONE
};

fn range(_: &Game, unit: &Unit, slot: Slot, base: f32) -> f32 {
    match slot {
        Slot::Q => base + unit.radius(),
        _ => base,
    }
}

fn cast(game: &mut Game, id: UnitId, slot: Slot, target: Target, spell: &Spell, rank: u8) {
    match slot {
        Slot::Q => {
            let Some(unit) = game.unit(id) else {
                return;
            };
            let range = cast_range(game, unit, slot);
            if target_in_range(game, id, &target, range, true).is_none() {
                if target.unit.is_some() {
                    approach(game, id, slot, target);
                }
                return;
            }
            spend(game, id, slot, spell.cooldown(rank), spell.mana(rank));
            begin(game, id, slot, target, 0.25, 0.1, "Spell2");
            game.sound(id, "q");
        }
        Slot::W => {
            spend(game, id, slot, spell.cooldown(rank), spell.mana(rank));
            let mut buff = game.buff("thunderclap_ready", EMPOWER_WINDOW);
            buff.bonus.attack_range = EMPOWER_RANGE;
            game.add_buff(id, buff);
            let position = game.unit(id).map_or(Vec2::ZERO, |unit| unit.position);
            game.effect("ThunderclapReady", position, Some(id));
            if let Some(unit) = game.unit_mut(id) {
                unit.attack_ready_at = 0.0;
            }
            game.play(id, "Spell1", 1.0);
            game.sound(id, "w");
        }
        Slot::E => {
            spend(game, id, slot, spell.cooldown(rank), spell.mana(rank));
            begin(
                game,
                id,
                slot,
                target,
                spell.cast_time.max(0.24),
                0.1,
                "Spell3",
            );
            game.sound(id, "e");
        }
        Slot::R => {
            let Some(unit) = game.unit(id) else {
                return;
            };
            let destination = clamp_point(
                unit.position,
                Vec2::from_array(target.point),
                spell.range(rank).max(1000.0),
            );
            let speed = UNSTOPPABLE_SPEED + unit.stats.move_speed;
            let duration = unit.position.distance(destination) / speed + TICK;
            spend(game, id, slot, spell.cooldown(rank), spell.mana(rank));
            let buff = game.buff("unstoppable", duration);
            game.add_buff(id, buff);
            dash(game, id, destination, speed, DashKind::Spell(Slot::R, None));
            game.hold(id, "Spell4");
            game.sound(id, "r");
        }
        _ => {}
    }
}

fn fire(game: &mut Game, cast: &Cast) {
    let Cast {
        id,
        spell,
        rank,
        position,
        team,
        ..
    } = *cast;
    match cast.slot {
        Slot::Q => {
            let Some(target_id) = cast.target.unit else {
                return;
            };
            let damage = calculate(game, id, spell, rank, "QDamageCalc");
            let missile = game.allocate();
            game.missiles.push(Missile {
                id: missile,
                owner: id,
                team,
                kind: clip_id("SeismicShard"),
                position,
                height: 150.0,
                speed: spell.missile_speed.max(1200.0),
                width: 0.0,
                motion: Motion::Homing(target_id),
                hit: Hit::Spell(Slot::Q, damage),
                hit_units: Vec::new(),
                acceleration: None,
            });
        }
        Slot::E => {
            let damage = calculate(game, id, spell, rank, "EDamageCalc");
            let radius = Spell::by_rank(&spell.cast_radius, rank).max(400.0);
            let reduction = spell.value("ASReduction", rank) / 100.0;
            for victim in game.enemies_near(team, position, radius) {
                if game.unit(victim).is_some_and(Unit::is_structure) {
                    continue;
                }
                game.damage(Some(id), victim, damage, DamageKind::Magic, false);
                let mut buff = game.buff("ground_slam", spell.value("Duration", rank).max(3.0));
                buff.source = Some(id);
                buff.bonus.cripple = reduction;
                game.add_buff(victim, buff);
                let at = game.unit(victim).map_or(position, |unit| unit.position);
                game.effect("LandslideHit", at, Some(victim));
                game.effect("LandslideSlow", at, Some(victim));
            }
            game.effect("Landslide", position, Some(id));
        }
        _ => {}
    }
}

fn dashed(game: &mut Game, id: UnitId, slot: Slot, _target: Option<UnitId>) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    if slot != Slot::R {
        return;
    }
    let (position, team, rank) = (unit.position, unit.team, unit.ability(Slot::R).rank);
    game.release(id, "Spell4");
    let spell = spell_of(game, id, Slot::R).unwrap_or_default();
    let damage = calculate(game, id, &spell, rank, "TotalDamage");
    let duration = spell.value("KnockupDuration", rank).max(1.5);
    for victim in game.enemies_near(team, position, 325.0) {
        if game.unit(victim).is_some_and(Unit::is_structure) {
            continue;
        }
        game.damage(Some(id), victim, damage, DamageKind::Magic, false);
        game.knock_up(victim, duration, Some((id, Slot::R)));
        let at = game.unit(victim).map_or(position, |unit| unit.position);
        game.effect("UnstoppableForceHit", at, Some(victim));
    }
    game.effect("UnstoppableForce", position, Some(id));
}

fn missile_hit(game: &mut Game, missile: &Missile, target: UnitId, _slot: Slot, damage: f32) {
    let owner = missile.owner;
    let spell = spell_of(game, owner, Slot::Q).unwrap_or_default();
    let rank = rank(game, owner, Slot::Q);
    game.damage(Some(owner), target, damage, DamageKind::Magic, false);
    let steal = spell.value("SpeedSteal", rank) / 100.0;
    let duration = spell.value("SlowDuration", rank).max(3.0);
    let speed = |game: &Game| game.unit(target).map_or(0.0, |unit| unit.stats.move_speed);
    let before = speed(game);
    game.slow(target, steal, duration, Some((owner, Slot::Q)));
    let mut buff = game.buff("seismic_haste", duration);
    buff.bonus.move_speed = (before - speed(game)).max(0.0);
    game.add_buff(owner, buff);
    if let Some(position) = game.unit(owner).map(|unit| unit.position) {
        game.effect("SeismicHaste", position, Some(owner));
    }
    let from = game
        .unit(target)
        .map_or(missile.position, |unit| unit.position);
    let id = game.allocate();
    game.missiles.push(Missile {
        id,
        owner,
        team: missile.team,
        kind: clip_id("SeismicShardReturn"),
        position: from,
        height: missile.height,
        speed: SHARD_RETURN_SPEED,
        width: 0.0,
        motion: Motion::Homing(owner),
        hit: Hit::Visual,
        hit_units: Vec::new(),
        acceleration: None,
    });
}

fn attack_clip(unit: &Unit) -> Option<&'static str> {
    unit.has_buff("thunderclap_ready")
        .then_some("Spell1_Attack")
}

fn on_attack_hit(game: &mut Game, attacker: UnitId, target: UnitId) {
    let Some(unit) = game.unit(attacker) else {
        return;
    };
    let (team, position, facing) = (unit.team, unit.position, unit.facing);
    let empowered = unit.has_buff("thunderclap_ready");
    let rank = unit.ability(Slot::W).rank;
    let spell = spell_of(game, attacker, Slot::W).unwrap_or_default();
    if empowered {
        if let Some(unit) = game.unit_mut(attacker) {
            unit.buffs.retain(|buff| buff.name != "thunderclap_ready");
        }
        let bonus = calculate(game, attacker, &spell, rank, "TotalBonusDamage");
        game.damage(Some(attacker), target, bonus, DamageKind::Physical, false);
        let buff = game.buff("thunderclap", spell.value("ThunderclapBuffDuration", rank));
        game.add_buff(attacker, buff);
        game.effect("Thunderclap", position, Some(attacker));
        game.sound(attacker, "w_hit");
    }
    if !game
        .unit(attacker)
        .is_some_and(|unit| unit.has_buff("thunderclap"))
    {
        return;
    }
    let toward = game.unit(target).map_or(position, |unit| unit.position);
    let splash = calculate(game, attacker, &spell, rank, "ThunderclapSplash");
    let range = spell.value("ThunderClapSplashRange", rank);
    let direction = direction(position, toward, facing);
    game.effect("ThunderclapCleave", position, Some(attacker));
    for victim in enemies_in_cone(game, team, position, direction, range, CLEAVE_HALF_ANGLE) {
        if victim != target {
            game.damage(Some(attacker), victim, splash, DamageKind::Physical, false);
            let at = game.unit(victim).map_or(position, |unit| unit.position);
            game.effect("ThunderclapHit", at, Some(victim));
        }
    }
}

fn granite_shield(game: &mut Game, id: UnitId) {
    let Some(spell) = named_spell(game, id, "MalphiteShield") else {
        return;
    };
    let cooldown = calculate(game, id, &spell, 1, "PassiveCooldown");
    let amount = calculate(game, id, &spell, 1, "TotalShield");
    let time = game.time;
    let Some(unit) = game.unit_mut(id) else {
        return;
    };
    if unit.alive() && time - unit.hurt_at >= cooldown {
        unit.shields.retain(|shield| shield.name != GRANITE_SHIELD);
        unit.shields.push(Shield {
            name: GRANITE_SHIELD,
            amount,
            expires: f32::MAX,
            magic_only: false,
        });
    }
    let shielded = unit
        .shields
        .iter()
        .any(|shield| shield.name == GRANITE_SHIELD && shield.amount > 0.0);
    if shielded == unit.has_buff(GRANITE_SHIELD) {
        return;
    }
    let position = unit.position;
    if shielded {
        let buff = game.buff(GRANITE_SHIELD, f32::MAX);
        game.add_buff(id, buff);
        game.effect("GraniteShieldUp", position, Some(id));
        game.sound(id, "shield");
    } else {
        if let Some(unit) = game.unit_mut(id) {
            unit.buffs.retain(|buff| buff.name != GRANITE_SHIELD);
        }
        game.refresh_stats(id);
        game.effect("GraniteShieldBreak", position, Some(id));
    }
}

fn tick(game: &mut Game, id: UnitId) {
    granite_shield(game, id);
    let rank = rank(game, id, Slot::W);
    if rank > 0 {
        let spell = spell_of(game, id, Slot::W).unwrap_or_default();
        let multiplier = if game
            .unit(id)
            .is_some_and(|unit| unit.has_buff("granite_shield"))
        {
            spell.value("BonusArmorPassiveMultiplier", rank).max(1.0)
        } else {
            1.0
        };
        let percent = spell.value("BonusArmorPassive", rank) * multiplier;
        let current = game
            .unit(id)
            .and_then(|unit| unit.buff("obduracy"))
            .map(|buff| buff.bonus.armor_percent);
        if current != Some(percent) {
            let mut buff = game.buff("obduracy", f32::MAX);
            buff.bonus.armor_percent = percent;
            game.add_buff(id, buff);
        }
    }
}
