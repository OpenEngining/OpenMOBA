use asset::data::Spell;
use glam::Vec2;
use protocol::{DamageKind, Slot, TICK, Target, UnitId};

use super::{
    Cast, Kit, Zone, approach, approach_point, begin, calculate, cast_range, clamp_point, rank,
    spell_of, spend, target_in_range,
};
use crate::Game;
use crate::stats::{self, Bonus};
use crate::unit::{Buff, Unit, slot_index};

const SPIRIT_FIRE_DELAY: f32 = 0.264;
const SPIRIT_FIRE_LINGER: f32 = 1.0;

pub const KIT: Kit = Kit {
    cast,
    fire,
    range,
    attack_clip,
    attack_sound,
    modify_attack,
    on_kill,
    tick,
    afflicted,
    zone,
    ..Kit::NONE
};

fn range(_: &Game, unit: &Unit, slot: Slot, base: f32) -> f32 {
    match slot {
        Slot::W => base + unit.radius(),
        _ => base,
    }
}

fn cast(game: &mut Game, id: UnitId, slot: Slot, target: Target, spell: &Spell, rank: u8) {
    match slot {
        Slot::Q => {
            let haste = game.unit(id).map_or(0.0, |unit| unit.stats.ability_haste);
            let fury = game.unit(id).is_some_and(|unit| unit.has_buff("fury"));
            spend(game, id, slot, 0.0, spell.mana(rank));
            game.sound(id, "q");
            let mut cooldown = stats::cooldown(spell.cooldown(rank), haste);
            if fury {
                cooldown *= 0.5;
            }
            if let Some(unit) = game.unit_mut(id) {
                let q = &mut unit.abilities[slot_index(Slot::Q)];
                q.cooldown = cooldown;
                q.cooldown_total = cooldown;
                unit.attack_ready_at = 0.0;
            }
            let mut buff = game.buff("siphoning_strike", spell.value("BuffDuration", rank));
            buff.bonus.attack_range = spell.value("BonusRange", rank);
            game.add_buff(id, buff);
            let position = game.unit(id).map_or(Vec2::ZERO, |unit| unit.position);
            game.effect("SiphoningStrike", position, Some(id));
        }
        Slot::W => {
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
            if game
                .unit(other)
                .is_some_and(|other| other.champion().is_none())
            {
                return;
            }
            spend(game, id, slot, spell.cooldown(rank), spell.mana(rank));
            begin(game, id, slot, target, 0.25, 0.1, "Spell2");
            game.sound(id, "w");
        }
        Slot::E => {
            let Some(unit) = game.unit(id) else {
                return;
            };
            if unit.position.distance(Vec2::from_array(target.point)) > spell.range(rank) + 100.0 {
                approach_point(game, id, target);
                return;
            }
            spend(game, id, slot, spell.cooldown(rank), spell.mana(rank));
            begin(game, id, slot, target, 0.25, 0.1, "Spell3");
            game.sound(id, "e");
        }
        Slot::R => {
            spend(game, id, slot, spell.cooldown(rank), spell.mana(rank));
            let Some(position) = game.unit(id).map(|unit| unit.position) else {
                return;
            };
            let target = Target {
                point: position.to_array(),
                unit: None,
            };
            begin(game, id, slot, target, spell.cast_time, 0.0, "Spell4");
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
        point,
        ..
    } = *cast;
    match cast.slot {
        Slot::W => {
            let Some(target_id) = cast.target.unit else {
                return;
            };
            let duration = spell.value("Duration", rank).max(5.0);
            let mut buff = game.buff("wither", duration);
            buff.source = Some(id);
            buff.value = spell.value("SlowBase", rank) / 100.0;
            buff.stacks = u32::from(rank);
            buff.bonus.slow = buff.value;
            buff.bonus.cripple = buff.value * spell.value("AttackSpeedSlowMult", rank);
            game.add_buff(target_id, buff);
            game.effect("Wither", point, Some(target_id));
        }
        Slot::E => {
            let center = clamp_point(position, point, spell.range(rank));
            let radius = Spell::by_rank(&spell.cast_radius, rank).max(400.0);
            let duration = spell.value("Duration", rank);
            let per_second = calculate(game, id, spell, rank, "TotalDotDamage") / duration;
            let armed = game.time + SPIRIT_FIRE_DELAY;
            game.zones.push(Zone {
                owner: id,
                team,
                name: "spirit_fire",
                center,
                radius,
                armed,
                expires: armed + duration + TICK,
                next_tick: armed,
                damage: per_second,
                value: -spell.value("ArmorShredPercent", rank),
            });
            game.lasting_effect("SpiritFire", id, center, armed + duration);
        }
        Slot::R => {
            let duration = spell.value("Duration", rank);
            let mut buff = game.buff("fury", duration);
            buff.bonus.health = spell.value("BonusHealth", rank);
            buff.bonus.armor = spell.value("InitialResistGain", rank);
            buff.bonus.magic_resist = spell.value("InitialResistGain", rank);
            buff.bonus.attack_range = spell.value("AttackRangeIncrease", rank);
            buff.bonus.size = spell.value("SizeIncreasePercent", rank);
            buff.stacks = u32::from(rank);
            game.add_buff(id, buff);
            game.effect("NasusR", position, Some(id));
            game.effect("FuryHands", position, Some(id));
        }
        _ => {}
    }
}

fn attack_clip(unit: &Unit) -> Option<&'static str> {
    unit.has_buff("siphoning_strike").then_some("Spell1")
}

fn attack_sound(unit: &Unit) -> Option<&'static str> {
    unit.has_buff("siphoning_strike").then_some("q_attack")
}

fn modify_attack(game: &mut Game, attacker: UnitId, target: UnitId, amount: f32) -> f32 {
    let Some(unit) = game.unit(attacker) else {
        return amount;
    };
    if !unit.has_buff("siphoning_strike") {
        return amount;
    }
    let spell = spell_of(game, attacker, Slot::Q).unwrap_or_default();
    let rank = unit.ability(Slot::Q).rank;
    let bonus = spell.value("BonusDamage", rank) + unit.counter;
    let time = game.time;
    if let Some(unit) = game.unit_mut(attacker) {
        unit.buffs.retain(|buff| buff.name != "siphoning_strike");
        unit.buffs.push(Buff {
            name: "siphoning_strike_hit",
            expires: time,
            duration: 0.0,
            stacks: 1,
            bonus: Bonus::default(),
            source: Some(target),
            slot: None,
            value: 0.0,
        });
    }
    let source = game.unit(attacker).map_or(Vec2::ZERO, |unit| unit.position);
    let at = game.unit(target).map_or(source, |unit| unit.position);
    game.effect("SiphoningStrikeHit", at, Some(target));
    game.effect("SiphoningStrikeSpent", source, Some(attacker));
    amount + bonus
}

fn on_kill(game: &mut Game, killer: UnitId, victim: UnitId) {
    let siphoned = game.unit(killer).is_some_and(|unit| {
        unit.buffs
            .iter()
            .any(|buff| buff.name == "siphoning_strike_hit" && buff.source == Some(victim))
    });
    if !siphoned {
        return;
    }
    let large = game
        .unit(victim)
        .is_some_and(|unit| unit.champion().is_some() || unit.is_large_minion());
    let spell = spell_of(game, killer, Slot::Q).unwrap_or_default();
    let rank = rank(game, killer, Slot::Q);
    let stacks = spell.value(if large { "BigStacks" } else { "BasicStacks" }, rank);
    if let Some(unit) = game.unit_mut(killer) {
        unit.counter += stacks;
    }
}

fn tick(game: &mut Game, id: UnitId) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    let life_steal = match unit.level {
        0..=6 => 0.1,
        7..=12 => 0.15,
        _ => 0.2,
    };
    let current = unit.buff("soul_eater").map(|buff| buff.bonus.life_steal);
    if current != Some(life_steal) {
        let mut buff = game.buff("soul_eater", f32::MAX);
        buff.bonus.life_steal = life_steal;
        game.add_buff(id, buff);
    }
    let Some(unit) = game.unit(id) else {
        return;
    };
    let stacks = unit.counter as u32;
    if stacks > 0 && unit.buff("siphoning_strike_stacks").map(|buff| buff.stacks) != Some(stacks) {
        let mut buff = game.buff("siphoning_strike_stacks", f32::MAX);
        buff.stacks = stacks;
        game.add_buff(id, buff);
    }
    let Some(unit) = game.unit(id) else {
        return;
    };
    if unit
        .buff("fury")
        .is_some_and(|fury| fury.expires <= game.time + TICK)
    {
        let position = unit.position;
        game.effect("NasusREnd", position, Some(id));
        game.sound(id, "r_end");
    }
    let Some(unit) = game.unit(id) else {
        return;
    };
    let Some(fury) = unit.buff("fury") else {
        return;
    };
    let Some(spell) = spell_of(game, id, Slot::R) else {
        return;
    };
    let rank = fury.stacks as u8;
    let rate = spell.value("TickRate", rank);
    let elapsed = fury.duration - (fury.expires - game.time);
    let ticks = (elapsed / TICK).round() as u32;
    let period = (rate / TICK).round().max(1.0) as u32;
    if ticks == 0 || !ticks.is_multiple_of(period) {
        return;
    }
    let percent = calculate(game, id, &spell, rank, "DamageCalc") * rate;
    let radius =
        spell.value("MinAoERadius", rank) + 80.0 * spell.value("SizeIncreasePercent", rank);
    let (team, position) = (unit.team, unit.position);
    for enemy in game.enemies_near(team, position, radius) {
        let Some(other) = game.unit(enemy) else {
            continue;
        };
        if other.is_structure() {
            continue;
        }
        let damage = other.stats.max_health * percent;
        game.damage(Some(id), enemy, damage, DamageKind::Magic, false);
    }
}

fn zone(game: &mut Game, index: usize) {
    let zone = game.zones[index];
    if game.time < zone.armed {
        return;
    }
    let victims: Vec<UnitId> = game
        .enemies_near(zone.team, zone.center, zone.radius)
        .into_iter()
        .filter(|victim| game.unit(*victim).is_some_and(|unit| !unit.is_structure()))
        .collect();
    for &victim in &victims {
        let mut buff = game.buff("spirit_fire", SPIRIT_FIRE_LINGER);
        buff.source = Some(zone.owner);
        buff.bonus.armor_percent = -zone.value;
        game.add_buff(victim, buff);
    }
    if zone.next_tick > game.time {
        return;
    }
    game.zones[index].next_tick = zone.next_tick + 1.0;
    let first = zone.next_tick == zone.armed;
    let damage = if first {
        let rank = rank(game, zone.owner, Slot::E);
        spell_of(game, zone.owner, Slot::E).map_or(0.0, |spell| {
            calculate(game, zone.owner, &spell, rank, "InitialDamage")
        })
    } else {
        zone.damage
    };
    for victim in victims {
        game.damage(Some(zone.owner), victim, damage, DamageKind::Magic, false);
    }
}

fn afflicted(game: &mut Game, id: UnitId, name: &'static str) {
    if name != "wither" {
        return;
    }
    let Some(wither) = game.unit(id).and_then(|unit| unit.buff("wither")).cloned() else {
        return;
    };
    let Some(spell) = wither
        .source
        .and_then(|source| spell_of(game, source, Slot::W))
    else {
        return;
    };
    let rank = wither.stacks as u8;
    let elapsed = wither.duration - (wither.expires - game.time);
    let steps = (elapsed + TICK / 2.0).floor();
    let slow = (wither.value + steps * spell.value("SlowPerTick", rank) / 100.0)
        .min(spell.value("MaxSlowTooltipOnly", rank) / 100.0);
    if (slow - wither.bonus.slow).abs() < 1e-6 {
        return;
    }
    let mut buff = wither;
    buff.bonus.slow = slow;
    buff.bonus.cripple = slow * spell.value("AttackSpeedSlowMult", rank);
    game.add_buff(id, buff);
}
