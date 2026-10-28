use asset::clip_id;
use asset::data::Spell;
use glam::Vec2;
use protocol::{DamageKind, Slot, Target, UnitId};

use super::{
    Cast, Kit, approach, begin, calculate, cast_range, clamp_point, enemies_in_cone, glow, rank,
    spell_of, spend, target_in_range,
};
use crate::unit::{Unit, slot_index};
use crate::{Game, Hit, Missile, Motion, facing_to};

const VOID_STONE: f32 = 0.1;
const PULSE_READY: &str = "force_pulse_ready";
const NETHER_BLADE_WINDOW: f32 = 5.0;
const NETHER_BLADE_RANGE: f32 = 50.0;
const PULSE_RANGE: f32 = 600.0;
const PULSE_HALF_ANGLE: f32 = 39.0;
const PULSE_PASSIVE_RANGE: f32 = 1800.0;
const RIFT_RADIUS: f32 = 270.0;
const MAX_RIFT_STACKS: u32 = 4;

pub const KIT: Kit = Kit {
    cast,
    fire,
    range,
    missile_hit,
    attack_clip,
    attack_sound,
    on_attack_hit,
    tick,
    movement: &[Slot::R],
    ..Kit::NONE
};

fn tick(game: &mut Game, id: UnitId) {
    pulse_ready(game, id);
    if game
        .unit(id)
        .is_none_or(|unit| unit.dead || unit.has_buff("void_stone"))
    {
        return;
    }
    let mut buff = game.buff("void_stone", f32::MAX);
    buff.bonus.magic_damage_reduction = VOID_STONE;
    game.add_buff(id, buff);
    game.refresh_stats(id);
}

fn pulse_ready(game: &mut Game, id: UnitId) {
    let ready = game.unit(id).is_some_and(|unit| {
        let pulse = unit.ability(Slot::E);
        pulse.rank > 0 && pulse.cooldown <= 0.0
    });
    glow(
        game,
        id,
        PULSE_READY,
        ready,
        ["ForcePulseReady", "ForcePulseSpent"],
    );
}

fn range(_: &Game, unit: &Unit, slot: Slot, base: f32) -> f32 {
    match slot {
        Slot::Q => base + unit.radius(),
        Slot::E => PULSE_RANGE,
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
            begin(
                game,
                id,
                slot,
                target,
                spell.cast_time.max(0.25),
                0.1,
                "Spell1",
            );
            game.sound(id, "q");
            let shield = calculate(game, id, spell, rank, "TotalShield");
            game.magic_shield(id, shield, spell.value("ShieldDuration", rank));
            let position = position(game, id);
            game.effect("KassadinQShield", position, Some(id));
        }
        Slot::W => {
            spend(game, id, slot, spell.cooldown(rank), spell.mana(rank));
            let mut buff = game.buff("nether_blade", NETHER_BLADE_WINDOW);
            buff.bonus.attack_range = NETHER_BLADE_RANGE;
            game.add_buff(id, buff);
            game.refresh_stats(id);
            if let Some(unit) = game.unit_mut(id) {
                unit.attack_ready_at = 0.0;
            }
            let position = position(game, id);
            game.effect("KassadinW", position, Some(id));
            game.sound(id, "w_cast");
        }
        Slot::E => {
            spend(game, id, slot, spell.cooldown(rank), spell.mana(rank));
            begin(
                game,
                id,
                slot,
                target,
                spell.cast_time.max(0.25),
                0.1,
                "Spell3",
            );
            game.sound(id, "e");
        }
        Slot::R => {
            let stacks = rift_stacks(game, id);
            let cost = spell.mana(rank) * 2_f32.powi(stacks as i32);
            if game.unit(id).is_none_or(|unit| unit.mana < cost) {
                return;
            }
            spend(game, id, slot, spell.cooldown(rank), cost);
            begin(
                game,
                id,
                slot,
                target,
                spell.cast_time.max(0.25),
                0.1,
                "Spell4",
            );
            game.sound(id, "r");
        }
        _ => {}
    }
}

fn fire(game: &mut Game, cast: &Cast) {
    match cast.slot {
        Slot::Q => null_sphere(game, cast),
        Slot::E => force_pulse(game, cast),
        Slot::R => riftwalk(game, cast),
        _ => {}
    }
}

fn null_sphere(game: &mut Game, cast: &Cast) {
    let Some(target) = cast.target.unit else {
        return;
    };
    let damage = calculate(game, cast.id, cast.spell, cast.rank, "TotalDamage");
    let missile = game.allocate();
    game.missiles.push(Missile {
        id: missile,
        owner: cast.id,
        team: cast.team,
        kind: clip_id("NullLance"),
        position: cast.position,
        height: 100.0,
        speed: cast.spell.missile_speed.max(1400.0),
        width: 0.0,
        motion: Motion::Homing(target),
        hit: Hit::Spell(Slot::Q, damage),
        hit_units: Vec::new(),
        acceleration: None,
    });
}

fn force_pulse(game: &mut Game, cast: &Cast) {
    let Cast {
        id,
        spell,
        rank,
        position,
        team,
        facing,
        ..
    } = *cast;
    let damage = calculate(game, id, spell, rank, "TotalDamage");
    let slow = spell.value("SlowAmount", rank) / 100.0;
    let duration = spell.value("SlowDuration", rank);
    let victims = enemies_in_cone(game, team, position, facing, PULSE_RANGE, PULSE_HALF_ANGLE);
    for victim in &victims {
        game.damage(Some(id), *victim, damage, DamageKind::Magic, false);
        game.slow(*victim, slow, duration, Some((id, Slot::E)));
        let at = game.unit(*victim).map_or(position, |unit| unit.position);
        game.effect("KassadinEHit", at, Some(*victim));
    }
    game.effect("KassadinE", position, Some(id));
    if !victims.is_empty() {
        game.sound(id, "e_hit");
    }
}

fn riftwalk(game: &mut Game, cast: &Cast) {
    let Cast {
        id,
        spell,
        rank,
        position,
        team,
        point,
        ..
    } = *cast;
    let reach = game
        .unit(id)
        .map_or(0.0, |unit| cast_range(game, unit, Slot::R));
    let destination = game.walkable_point(clamp_point(position, point, reach));
    if let Some(unit) = game.unit_mut(id) {
        unit.position = destination;
        unit.facing = facing_to(position, destination, unit.facing);
        unit.path.clear();
    }
    game.effect("KassadinR", position, Some(id));
    game.effect("KassadinAppear", destination, Some(id));
    let stacks = rift_stacks(game, id);
    let damage = calculate(game, id, spell, rank, "BaseDamage")
        + calculate(game, id, spell, rank, "BonusDamage") * stacks as f32;
    for victim in game.enemies_near(team, destination, RIFT_RADIUS) {
        if !game.unit(victim).is_some_and(Unit::is_structure) {
            game.damage(Some(id), victim, damage, DamageKind::Magic, false);
        }
    }
    let mut buff = game.buff("riftwalk", spell.value("RStackDuration", rank).max(15.0));
    buff.stacks = (stacks + 1).min(MAX_RIFT_STACKS);
    game.add_buff(id, buff);
}

fn rift_stacks(game: &Game, id: UnitId) -> u32 {
    game.unit(id)
        .and_then(|unit| unit.buff("riftwalk"))
        .map_or(0, |buff| buff.stacks)
}

pub fn ability_cast(game: &mut Game, caster: UnitId, slot: Slot) {
    if matches!(slot, Slot::D | Slot::F) {
        return;
    }
    let Some(at) = game
        .unit(caster)
        .filter(|unit| unit.champion().is_some() && !unit.untargetable())
        .map(|unit| unit.position)
    else {
        return;
    };
    let listeners: Vec<UnitId> = game
        .units
        .iter()
        .filter(|unit| {
            unit.alive()
                && unit.ability(Slot::E).rank > 0
                && unit.position.distance(at) <= PULSE_PASSIVE_RANGE
                && !(unit.id == caster && slot == Slot::E)
                && unit
                    .champion()
                    .and_then(|index| game.data.champions.get(usize::from(index)))
                    .is_some_and(|champion| champion.name == "VoidWalker")
        })
        .map(|unit| unit.id)
        .collect();
    for id in listeners {
        let spell = spell_of(game, id, Slot::E).unwrap_or_default();
        let reduction = spell.value("ReductionPerSpellCast", rank(game, id, Slot::E));
        if let Some(unit) = game.unit_mut(id) {
            let pulse = &mut unit.abilities[slot_index(Slot::E)];
            pulse.cooldown = (pulse.cooldown - reduction).max(0.0);
        }
    }
}

fn missile_hit(game: &mut Game, missile: &Missile, target: UnitId, _slot: Slot, damage: f32) {
    game.interrupt(target, true);
    game.damage(
        Some(missile.owner),
        target,
        damage,
        DamageKind::Magic,
        false,
    );
}

fn attack_clip(unit: &Unit) -> Option<&'static str> {
    unit.has_buff("nether_blade").then_some("Spell2")
}

fn attack_sound(unit: &Unit) -> Option<&'static str> {
    unit.has_buff("nether_blade").then_some("w")
}

fn on_attack_hit(game: &mut Game, attacker: UnitId, target: UnitId) {
    let rank = rank(game, attacker, Slot::W);
    if rank == 0 {
        return;
    }
    let spell = spell_of(game, attacker, Slot::W).unwrap_or_default();
    let empowered = game
        .unit(attacker)
        .is_some_and(|unit| unit.has_buff("nether_blade"));
    if !empowered {
        let damage = calculate(game, attacker, &spell, rank, "OnHitDamage");
        game.damage(Some(attacker), target, damage, DamageKind::Magic, false);
        return;
    }
    if let Some(unit) = game.unit_mut(attacker) {
        unit.buffs.retain(|buff| buff.name != "nether_blade");
    }
    game.refresh_stats(attacker);
    let own = position(game, attacker);
    game.effect("KassadinWEnd", own, Some(attacker));
    let damage = calculate(game, attacker, &spell, rank, "ActiveDamage");
    game.damage(Some(attacker), target, damage, DamageKind::Magic, false);
    let champion = game
        .unit(target)
        .is_some_and(|unit| unit.champion().is_some());
    let ratio = if champion {
        spell.value("ChampionMissingManaRatio", rank)
    } else {
        spell.value("MissingManaRatio", rank)
    } / 100.0;
    if let Some(unit) = game.unit_mut(attacker) {
        let missing = unit.stats.max_mana - unit.mana;
        unit.mana = (unit.mana + missing * ratio).min(unit.stats.max_mana);
    }
    let at = position(game, target);
    game.effect("KassadinWHit", at, Some(target));
    game.sound(attacker, "w_hit");
}

fn position(game: &Game, id: UnitId) -> Vec2 {
    game.unit(id).map_or(Vec2::ZERO, |unit| unit.position)
}
