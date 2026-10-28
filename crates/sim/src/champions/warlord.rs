use asset::data::Spell;
use glam::Vec2;
use protocol::{DamageKind, Slot, TICK, Target, UnitId};

use super::{
    Cast, Kit, approach, begin, begin_mobile, calculate, cast_range, dash, enemies_in_cone,
    named_spell, rank, spell_of, spend, target_in_range,
};
use crate::Game;
use crate::unit::{DashKind, Unit};

const BLEED_STACKS: u32 = 5;
const BLEED_DURATION: f32 = 5.0;
const BLEED_TICKS: f32 = 4.0;
const MIGHT_DURATION: f32 = 5.0;
const BLADE_RADIUS: f32 = 460.0;
const HANDLE_RADIUS: f32 = 240.0;
const DECIMATE_WINDUP: f32 = 0.75;
const DECIMATE_GHOSTED: f32 = 1.0;
const HEALED_CHAMPIONS: usize = 3;
const PULL_RANGE: f32 = 535.0;
const CONE_HALF_ANGLE: f32 = 25.0;
const APPREHEND_LOCK: f32 = 0.4;
const LEAP_TIME: f32 = 0.35;
const STRIKE_WINDOW: f32 = 4.0;
const RESET_WINDOW: f32 = 20.0;
const MAX_GUILLOTINE_RANK: u8 = 3;
const STRIKE_RANGE: f32 = 25.0;
const STRIKE_REFUND: f32 = 0.5;
pub const KIT: Kit = Kit {
    died,
    cast,
    fire,
    range,
    dashed,
    attack_clip,
    attack_sound,
    modify_attack,
    on_attack_hit,
    on_kill,
    afflicted,
    tick,
    ..Kit::NONE
};

fn died(game: &mut Game, id: UnitId) {
    let time = game.time;
    if let Some(unit) = game.unit_mut(id)
        && let Some((expires, remaining)) = unit
            .buff("guillotine_reset")
            .map(|window| (window.expires, window.value))
    {
        let elapsed = RESET_WINDOW - (expires - time);
        unit.ability_mut(Slot::R).cooldown = (remaining - elapsed).max(0.0);
    }
}

fn tick(game: &mut Game, id: UnitId) {
    let time = game.time;
    if let Some(unit) = game.unit_mut(id)
        && let Some(remaining) = unit
            .buff("guillotine_reset")
            .filter(|window| window.expires <= time + TICK)
            .map(|window| window.value)
    {
        unit.ability_mut(Slot::R).cooldown = (remaining - RESET_WINDOW).max(0.0);
    }
    let Some(unit) = game.unit(id) else {
        return;
    };
    let rank = unit.ability(Slot::E).rank;
    if rank == 0 {
        return;
    }
    let spell = spell_of(game, id, Slot::E).unwrap_or_default();
    let penetration = spell.value("PassivePercentArmorPen", rank) / 100.0;
    if unit
        .buff("apprehend")
        .map(|buff| buff.bonus.armor_penetration)
        != Some(penetration)
    {
        let mut buff = game.buff("apprehend", f32::MAX);
        buff.bonus.armor_penetration = penetration;
        game.add_buff(id, buff);
    }
}

fn range(_: &Game, unit: &Unit, slot: Slot, base: f32) -> f32 {
    match slot {
        Slot::Q => BLADE_RADIUS,
        Slot::R => base + unit.radius(),
        _ => base,
    }
}

fn cast(game: &mut Game, id: UnitId, slot: Slot, target: Target, spell: &Spell, rank: u8) {
    match slot {
        Slot::Q => {
            spend(game, id, slot, spell.cooldown(rank), spell.mana(rank));
            begin_mobile(game, id, slot, target, DECIMATE_WINDUP, 0.1, "Spell1");
            game.sound(id, "q");
            let ghosted = game.buff("ghosted", DECIMATE_GHOSTED);
            game.add_buff(id, ghosted);
            let position = position(game, id);
            game.effect("DariusQ", position, Some(id));
        }
        Slot::W => {
            spend(game, id, slot, spell.cooldown(rank), spell.mana(rank));
            let mut strike = game.buff("crippling_strike", STRIKE_WINDOW);
            strike.bonus.attack_range = STRIKE_RANGE;
            game.add_buff(id, strike);
            if let Some(unit) = game.unit_mut(id) {
                unit.attack_ready_at = 0.0;
            }
            let position = position(game, id);
            game.effect("DariusW", position, Some(id));
            game.sound(id, "w_cast");
        }
        Slot::E => {
            spend(game, id, slot, spell.cooldown(rank), spell.mana(rank));
            begin(
                game,
                id,
                slot,
                target,
                spell.cast_time,
                APPREHEND_LOCK,
                "Spell3",
            );
            game.sound(id, "e");
        }
        Slot::R => {
            let Some(unit) = game.unit(id) else {
                return;
            };
            let range = cast_range(game, unit, slot);
            let Some(other) = target_in_range(game, id, &target, range, true) else {
                if target.unit.is_some() {
                    approach(game, id, slot, target);
                }
                return;
            };
            let Some((from, to)) = game
                .unit(other)
                .filter(|other| other.champion().is_some())
                .map(|other| (unit.position, other.position))
            else {
                return;
            };
            let landing = to - (to - from).normalize_or_zero() * 100.0;
            let speed = (from.distance(landing) / LEAP_TIME).max(1000.0);
            let free = unit.has_buff("guillotine_reset");
            let mana = if free { 0.0 } else { spell.mana(rank) };
            spend(game, id, slot, spell.cooldown(rank), mana);
            if let Some(unit) = game.unit_mut(id) {
                unit.buffs.retain(|buff| buff.name != "guillotine_reset");
            }
            dash(
                game,
                id,
                landing,
                speed,
                DashKind::Spell(Slot::R, Some(other)),
            );
            game.play(id, "Spell4", 1.0);
            game.sound(id, "r");
        }
        _ => {}
    }
}

fn fire(game: &mut Game, cast: &Cast) {
    match cast.slot {
        Slot::Q => decimate(game, cast),
        Slot::E => apprehend(game, cast),
        _ => {}
    }
}

fn decimate(game: &mut Game, cast: &Cast) {
    let Cast {
        id,
        spell,
        rank,
        position,
        team,
        ..
    } = *cast;
    let blade = calculate(game, id, spell, rank, "BladeDamage");
    let handle = calculate(game, id, spell, rank, "HandleDamage");
    let victims: Vec<(UnitId, bool)> = game
        .enemies_near(team, position, BLADE_RADIUS)
        .into_iter()
        .filter_map(|victim| {
            let unit = game.unit(victim).filter(|unit| !unit.is_structure())?;
            Some((victim, unit.position.distance(position) > HANDLE_RADIUS))
        })
        .collect();
    let mut healed = 0;
    for (victim, outer) in victims {
        if outer {
            game.damage(Some(id), victim, blade, DamageKind::Physical, false);
            bleed(game, id, victim);
            let at = game.unit(victim).map_or(position, |unit| unit.position);
            game.effect("DariusQHit", at, Some(victim));
            if game
                .unit(victim)
                .is_some_and(|unit| unit.champion().is_some())
            {
                healed += 1;
            }
        } else {
            game.damage(Some(id), victim, handle, DamageKind::Physical, false);
            let at = game.unit(victim).map_or(position, |unit| unit.position);
            game.effect("DariusQInner", at, Some(victim));
        }
    }
    let Some(unit) = game.unit(id) else {
        return;
    };
    let missing = unit.stats.max_health - unit.health;
    let heal = missing * spell.value("MissingHealthHeal", rank) / 100.0
        * healed.min(HEALED_CHAMPIONS) as f32;
    if heal > 0.0 {
        game.heal(id, heal);
        game.effect("DariusQHeal", position, Some(id));
    }
}

fn apprehend(game: &mut Game, cast: &Cast) {
    let Cast {
        id,
        spell,
        rank,
        position,
        team,
        facing,
        ..
    } = *cast;
    let radius = game.unit(id).map_or(0.0, Unit::radius);
    let victims = enemies_in_cone(game, team, position, facing, PULL_RANGE, CONE_HALF_ANGLE);
    let slow = spell.value("SlowPercent", rank) / 100.0;
    let duration = spell.value("SlowDuration", rank);
    for victim in &victims {
        let Some((at, reach)) = game
            .unit(*victim)
            .map(|other| (other.position, radius + other.radius()))
        else {
            continue;
        };
        let landing = position + (at - position).normalize_or(facing) * reach;
        game.slow(*victim, slow, duration, Some((id, Slot::E)));
        game.effect("DariusEHit", at, Some(*victim));
        game.knock_back(*victim, landing, 1500.0);
    }
    if !victims.is_empty() {
        game.sound(id, "e_hit");
    }
}

fn dashed(game: &mut Game, id: UnitId, slot: Slot, target: Option<UnitId>) {
    let (Slot::R, Some(target)) = (slot, target) else {
        return;
    };
    let Some(victim) = game.unit(target).filter(|victim| victim.alive()) else {
        return;
    };
    let spell = spell_of(game, id, Slot::R).unwrap_or_default();
    let rank = rank(game, id, Slot::R);
    let stacks = victim
        .buffs
        .iter()
        .find(|buff| buff.name == "hemorrhage" && buff.source == Some(id))
        .map_or(0, |buff| buff.stacks);
    let at = victim.position;
    let bonus = 1.0 + spell.value("RDamagePercentPerHemoStack", rank) * stacks as f32;
    let damage = calculate(game, id, &spell, rank, "Damage") * bonus.min(2.0);
    game.effect("DariusR", at, Some(target));
    game.sound(id, "r_hit");
    game.damage(Some(id), target, damage, DamageKind::True, false);
    if !game.unit(target).is_some_and(|victim| victim.dead) {
        bleed(game, id, target);
        return;
    }
    warlords_might(game, id);
    if rank < MAX_GUILLOTINE_RANK {
        let remaining = game
            .unit(id)
            .map_or(0.0, |unit| unit.ability(Slot::R).cooldown);
        let mut window = game.buff("guillotine_reset", RESET_WINDOW);
        window.value = remaining;
        game.add_buff(id, window);
    }
    if let Some(unit) = game.unit_mut(id) {
        unit.ability_mut(Slot::R).cooldown = 0.0;
    }
    game.effect("DariusRReset", at, Some(id));
}

fn attack_clip(unit: &Unit) -> Option<&'static str> {
    unit.has_buff("crippling_strike").then_some("Spell2")
}

fn attack_sound(unit: &Unit) -> Option<&'static str> {
    unit.has_buff("crippling_strike").then_some("w")
}

fn modify_attack(game: &mut Game, attacker: UnitId, target: UnitId, amount: f32) -> f32 {
    if !game
        .unit(attacker)
        .is_some_and(|unit| unit.has_buff("crippling_strike"))
    {
        return amount;
    }
    let spell = spell_of(game, attacker, Slot::W).unwrap_or_default();
    let rank = rank(game, attacker, Slot::W);
    let time = game.time;
    if let Some(unit) = game.unit_mut(attacker) {
        unit.buffs.retain(|buff| buff.name != "crippling_strike");
    }
    let mut struck = game.buff("crippling_strike_hit", 0.0);
    struck.expires = time;
    struck.source = Some(target);
    game.add_buff(attacker, struck);
    game.slow(
        target,
        spell.value("SlowPercent", rank) / 100.0,
        spell.value("SlowDuration", rank),
        Some((attacker, Slot::W)),
    );
    let at = game.unit(target).map_or(Vec2::ZERO, |unit| unit.position);
    game.effect("DariusWHit", at, Some(target));
    amount * spell.value("ADRatio", rank).max(1.0)
}

fn on_kill(game: &mut Game, killer: UnitId, victim: UnitId) {
    let struck = game.unit(killer).is_some_and(|unit| {
        unit.buffs
            .iter()
            .any(|buff| buff.name == "crippling_strike_hit" && buff.source == Some(victim))
    });
    if !struck {
        return;
    }
    let mana = spell_of(game, killer, Slot::W)
        .map_or(0.0, |spell| spell.mana(rank(game, killer, Slot::W)));
    if let Some(unit) = game.unit_mut(killer) {
        let strike = unit.ability_mut(Slot::W);
        strike.cooldown = (strike.cooldown - strike.cooldown_total * STRIKE_REFUND).max(0.0);
        unit.mana = (unit.mana + mana).min(unit.stats.max_mana);
    }
}

fn on_attack_hit(game: &mut Game, attacker: UnitId, target: UnitId) {
    bleed(game, attacker, target);
}

fn bleed(game: &mut Game, id: UnitId, target: UnitId) {
    let Some(victim) = game.unit(target).filter(|unit| !unit.is_structure()) else {
        return;
    };
    let champion = victim.champion().is_some();
    let existing = victim
        .buffs
        .iter()
        .find(|buff| buff.name == "hemorrhage" && buff.source == Some(id))
        .cloned();
    let at = victim.position;
    let mighty = game
        .unit(id)
        .is_some_and(|unit| unit.has_buff("warlords_might"));
    let stacks = if mighty {
        BLEED_STACKS
    } else {
        existing
            .as_ref()
            .map_or(1, |buff| (buff.stacks + 1).min(BLEED_STACKS))
    };
    let mut buff = game.buff("hemorrhage", BLEED_DURATION);
    buff.source = Some(id);
    buff.stacks = stacks;
    buff.value = existing.map_or(game.time + BLEED_DURATION / BLEED_TICKS, |buff| buff.value);
    game.add_sourced_buff(target, buff);
    let kind = if champion { "" } else { "Minion" };
    game.effect(&format!("DariusHemo{stacks}{kind}"), at, Some(target));
    if stacks == BLEED_STACKS && champion && !mighty {
        warlords_might(game, id);
    }
}

fn warlords_might(game: &mut Game, id: UnitId) {
    let marker = named_spell(game, id, "DariusHemoMarker").unwrap_or_default();
    let mut might = game.buff("warlords_might", MIGHT_DURATION);
    might.bonus.attack_damage = calculate(game, id, &marker, 1, "NoxianMightBonusAD");
    game.add_buff(id, might);
    let position = position(game, id);
    game.effect("DariusMight", position, Some(id));
    game.effect("DariusMightParts", position, Some(id));
    game.sound(id, "might");
}

fn afflicted(game: &mut Game, id: UnitId, name: &'static str) {
    if name != "hemorrhage" {
        return;
    }
    let time = game.time;
    let Some(bleed) = game
        .unit(id)
        .and_then(|unit| unit.buff("hemorrhage"))
        .filter(|buff| time >= buff.value)
        .cloned()
    else {
        return;
    };
    let Some(source) = bleed.source else {
        return;
    };
    let marker = named_spell(game, source, "DariusHemoMarker").unwrap_or_default();
    let per_stack = calculate(game, source, &marker, 1, "BleedDamagePerStack");
    if let Some(buff) = game
        .unit_mut(id)
        .and_then(|unit| unit.buff_mut("hemorrhage"))
    {
        buff.value = time + BLEED_DURATION / BLEED_TICKS;
    }
    let damage = per_stack * bleed.stacks as f32 / BLEED_TICKS;
    game.damage(Some(source), id, damage, DamageKind::Physical, false);
}

fn position(game: &Game, id: UnitId) -> Vec2 {
    game.unit(id).map_or(Vec2::ZERO, |unit| unit.position)
}
