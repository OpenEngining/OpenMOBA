use asset::data::Spell;
use protocol::{DamageKind, Slot, Target, UnitId, UnitKind};

use super::{
    Cast, Kit, approach, begin, calculate, cast_range, glow, named_spell, rank, set_cooldown,
    spell_of, spend, target_in_range,
};
use crate::Game;
use crate::unit::{Order, Unit};

const QUIET_TIME: f32 = 8.0;
const PERSEVERANCE: &str = "perseverance";
const NEAREST: &str = "judgment_nearest";
const COURAGE_GUARD: &str = "courage_guard";
const COURAGE_GLOW: &str = "courage_glow";
const REGEN_PERIOD: f32 = 5.0;
const SPIN_RADIUS: f32 = 325.0;
const SPIN_TICKS: f32 = 7.0;
const EXECUTE_RANGE: f32 = 400.0;
const SPIN_RECAST_LOCK: f32 = 1.0;
const SPIN_CRIT: f32 = 1.3;
const SPIN_CRIT_BONUS_RATIO: f32 = 0.3;

pub const KIT: Kit = Kit {
    died,
    recast,
    recasting,
    cast,
    fire,
    range,
    attack_clip,
    attack_sound,
    modify_attack,
    on_kill,
    tick,
    ..Kit::NONE
};

fn range(_: &Game, unit: &Unit, slot: Slot, base: f32) -> f32 {
    match slot {
        Slot::E => SPIN_RADIUS,
        Slot::R => base.max(EXECUTE_RANGE) + unit.radius(),
        _ => base,
    }
}

fn recasting(unit: &Unit, slot: Slot) -> bool {
    slot == Slot::E && unit.has_buff("judgment")
}

fn recast(game: &mut Game, id: UnitId, slot: Slot, _target: Target) -> bool {
    let Some(unit) = game.unit(id).filter(|unit| unit.has_buff("judgment")) else {
        return false;
    };
    if slot != Slot::E {
        return false;
    }
    if !unit.has_buff("judgment_lock") {
        stop_spinning(game, id);
    }
    true
}

fn cast(game: &mut Game, id: UnitId, slot: Slot, target: Target, spell: &Spell, rank: u8) {
    match slot {
        Slot::Q => {
            spend(game, id, slot, spell.cooldown(rank), spell.mana(rank));
            if let Some(unit) = game.unit_mut(id) {
                unit.buffs
                    .retain(|buff| !matches!(buff.name, "slow" | "wither"));
                for buff in &mut unit.buffs {
                    buff.bonus.slow = 0.0;
                }
                unit.attack_ready_at = 0.0;
            }
            let mut haste = game.buff(
                "decisive_strike_haste",
                spell.value("MovementSpeedDuration", rank),
            );
            haste.bonus.move_speed_percent = spell.value("MovementSpeedAmount", rank);
            game.add_buff(id, haste);
            let strike = game.buff("decisive_strike", spell.value("AttackWindow", rank));
            game.add_buff(id, strike);
            let position = position_of(game, id);
            game.effect("GarenQ", position, Some(id));
            game.effect("GarenQSword", position, Some(id));
            game.sound(id, "q_cast");
        }
        Slot::W => {
            spend(game, id, slot, spell.cooldown(rank), spell.mana(rank));
            let shield = calculate(game, id, spell, rank, "TotalShield");
            game.shield(id, shield, spell.value("UpfrontDuration", rank));
            let mut courage = game.buff(COURAGE_GUARD, spell.value("DRDuration", rank));
            courage.bonus.damage_reduction = spell.value("DRPercent", rank);
            game.add_buff(id, courage);
            let position = position_of(game, id);
            game.effect("GarenW", position, Some(id));
            game.sound(id, "w");
        }
        Slot::E => {
            let Some(unit) = game.unit(id) else {
                return;
            };
            let bonus_speed =
                (unit.stats.attack_speed / unit.stats.base_attack_speed.max(0.1) - 1.0).max(0.0);
            let ticks =
                SPIN_TICKS + (bonus_speed / spell.value("ASPerTick", rank).max(0.01)).floor();
            let duration = spell.value("Duration", rank).max(3.0);
            spend(game, id, slot, 0.0, spell.mana(rank));
            let mut judgment = game.buff("judgment", f32::MAX);
            judgment.stacks = ticks as u32;
            judgment.value = duration / ticks;
            game.add_buff(id, judgment);
            let next = game.buff("judgment_tick", duration / ticks);
            game.add_buff(id, next);
            let disarm = game.buff("disarm", duration);
            game.add_buff(id, disarm);
            let lock = game.buff("judgment_lock", SPIN_RECAST_LOCK);
            game.add_buff(id, lock);
            if let Some(unit) = game.unit_mut(id)
                && let Order::Attack(_) = unit.order
            {
                unit.order = Order::Idle;
            }
            game.hold(id, "Spell3");
            let position = position_of(game, id);
            game.effect("GarenE", position, Some(id));
            game.sound(id, "e_spin");
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
            if game
                .unit(other)
                .is_some_and(|other| other.champion().is_none())
            {
                return;
            }
            if game.unit(id).is_some_and(|unit| unit.has_buff("judgment")) {
                stop_spinning(game, id);
            }
            spend(game, id, slot, spell.cooldown(rank), spell.mana(rank));
            begin(game, id, slot, target, 0.45, 0.2, "Spell4");
            game.sound(id, "r");
        }
        _ => {}
    }
}

fn fire(game: &mut Game, cast: &Cast) {
    if cast.slot != Slot::R {
        return;
    }
    let Some(target) = cast.target.unit else {
        return;
    };
    let Some(victim) = game.unit(target).filter(|victim| victim.alive()) else {
        return;
    };
    let missing = victim.stats.max_health - victim.health;
    let position = victim.position;
    let damage = cast.spell.value("BaseDamage", cast.rank)
        + cast.spell.value("ExecuteDamage", cast.rank) * missing;
    game.effect("GarenR", position, Some(target));
    game.sound(cast.id, "r_hit");
    game.damage(Some(cast.id), target, damage, DamageKind::True, false);
    if game.unit(target).is_some_and(|victim| victim.dead) {
        game.effect("GarenRKill", position, Some(target));
    }
}

fn attack_clip(unit: &Unit) -> Option<&'static str> {
    unit.has_buff("decisive_strike").then_some("Spell1")
}

fn attack_sound(unit: &Unit) -> Option<&'static str> {
    unit.has_buff("decisive_strike").then_some("q")
}

fn modify_attack(game: &mut Game, attacker: UnitId, target: UnitId, amount: f32) -> f32 {
    if !game
        .unit(attacker)
        .is_some_and(|unit| unit.has_buff("decisive_strike"))
    {
        return amount;
    }
    let spell = spell_of(game, attacker, Slot::Q).unwrap_or_default();
    let rank = rank(game, attacker, Slot::Q);
    let bonus = calculate(game, attacker, &spell, rank, "TotalDamage");
    if let Some(unit) = game.unit_mut(attacker) {
        unit.buffs.retain(|buff| buff.name != "decisive_strike");
    }
    let silence = game.crowd_control(
        "silence",
        spell.value("SilenceDuration", rank),
        Some((attacker, Slot::Q)),
    );
    game.add_buff(target, silence);
    let position = position_of(game, target);
    game.effect("GarenQHit", position, Some(target));
    bonus
}

fn on_kill(game: &mut Game, killer: UnitId, victim: UnitId) {
    let counts = game.unit(victim).is_some_and(|victim| {
        matches!(
            victim.kind,
            UnitKind::Champion(_) | UnitKind::Minion(_) | UnitKind::Soldier
        )
    });
    let Some(unit) = game.unit(killer) else {
        return;
    };
    if !counts || unit.ability(Slot::W).rank == 0 {
        return;
    }
    let stacks = unit.buff("courage").map_or(0, |buff| buff.stacks);
    let spell = spell_of(game, killer, Slot::W).unwrap_or_default();
    let per_kill = spell.value("ResistGainOnKillTooltip", 1);
    if per_kill <= 0.0 || per_kill * stacks as f32 >= spell.value("ResistMax", 1) {
        return;
    }
    let mut buff = game.buff("courage", f32::MAX);
    buff.stacks = stacks + 1;
    buff.bonus.armor = per_kill * buff.stacks as f32;
    buff.bonus.magic_resist = buff.bonus.armor;
    game.add_buff(killer, buff);
}

fn tick(game: &mut Game, id: UnitId) {
    regenerate(game, id);
    spin(game, id);
    courage(game, id);
}

fn courage(game: &mut Game, id: UnitId) {
    let guarded = game
        .unit(id)
        .is_some_and(|unit| unit.has_buff(COURAGE_GUARD));
    glow(game, id, COURAGE_GLOW, guarded, ["Courage", "CourageEnd"]);
}

fn regenerate(game: &mut Game, id: UnitId) {
    let time = game.time;
    let Some(unit) = game.unit(id) else {
        return;
    };
    let interrupts = |by: UnitId| {
        game.unit(by)
            .is_some_and(|by| by.champion().is_some() || by.kind == UnitKind::Turret)
    };
    let quiet = !unit
        .last_hit_by
        .iter()
        .any(|(by, at)| time - at < QUIET_TIME && interrupts(*by));
    let full = unit.health >= unit.stats.max_health;
    glow(
        game,
        id,
        PERSEVERANCE,
        quiet,
        ["Perseverance", "PerseveranceEnd"],
    );
    if !quiet || full {
        return;
    }
    let Some(unit) = game.unit(id) else {
        return;
    };
    let max_health = unit.stats.max_health;
    let level = f32::from(unit.level);
    let percent = named_spell(game, id, "GarenPassive")
        .map(|spell| calculate(game, id, &spell, 1, "RegenCalc"))
        .filter(|percent| *percent > 0.0)
        .unwrap_or((1.5 + (10.1 - 1.5) * (level - 1.0) / 17.0) / 100.0);
    if let Some(unit) = game.unit_mut(id) {
        unit.health =
            (unit.health + max_health * percent / REGEN_PERIOD * protocol::TICK).min(max_health);
    }
}

fn spin(game: &mut Game, id: UnitId) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    let Some(judgment) = unit.buff("judgment").cloned() else {
        return;
    };
    if unit.has_buff("judgment_tick") {
        return;
    }
    let (position, team) = (unit.position, unit.team);
    let crit_chance = unit.stats.crit_chance;
    let bonus_crit = unit.stats.crit_damage - unit.base.crit_damage;
    let spell = spell_of(game, id, Slot::E).unwrap_or_default();
    let rank = rank(game, id, Slot::E);
    let critical = crit_chance > 0.0 && game.next_random() < crit_chance;
    let damage = calculate(game, id, &spell, rank, "TotalDamage")
        * if critical {
            SPIN_CRIT + SPIN_CRIT_BONUS_RATIO * bonus_crit
        } else {
            1.0
        };
    let victims: Vec<UnitId> = game
        .enemies_near(team, position, SPIN_RADIUS)
        .into_iter()
        .filter(|victim| !game.unit(*victim).is_some_and(Unit::is_structure))
        .collect();
    let distance = |victim: &UnitId| {
        game.unit(*victim)
            .map_or(f32::MAX, |unit| unit.position.distance(position))
    };
    let nearest = victims
        .iter()
        .copied()
        .min_by(|a, b| distance(a).total_cmp(&distance(b)));
    mark_nearest(game, id, nearest, judgment.value * judgment.stacks as f32);
    for victim in victims {
        let bonus = if Some(victim) == nearest {
            1.0 + spell.value("NearestEnemyBonus", rank)
        } else {
            1.0
        };
        game.damage(
            Some(id),
            victim,
            damage * bonus,
            DamageKind::Physical,
            critical,
        );
        let at = position_of(game, victim);
        game.effect("GarenEHit", at, Some(victim));
        shred(game, id, victim, &spell, rank);
    }
    let remaining = judgment.stacks.saturating_sub(1);
    if remaining == 0 {
        stop_spinning(game, id);
        return;
    }
    if let Some(buff) = game.unit_mut(id).and_then(|unit| unit.buff_mut("judgment")) {
        buff.stacks = remaining;
    }
    let next = game.buff("judgment_tick", judgment.value);
    game.add_buff(id, next);
}

pub(super) fn shred(game: &mut Game, id: UnitId, victim: UnitId, spell: &Spell, rank: u8) {
    if !game
        .unit(victim)
        .is_some_and(|unit| unit.champion().is_some())
    {
        return;
    }
    let stacks = game
        .unit(victim)
        .and_then(|unit| unit.buff("judgment_stacks"))
        .filter(|buff| buff.source == Some(id))
        .map_or(0, |buff| buff.stacks)
        + 1;
    let duration = spell.value("ShredDuration", rank).max(6.0);
    let mut buff = game.buff("judgment_stacks", duration);
    buff.source = Some(id);
    buff.stacks = stacks;
    game.add_buff(victim, buff);
    let needed = spell.value("StacksToShred", rank).max(6.0) as u32;
    if stacks == needed || stacks > needed && (stacks - needed - 1).is_multiple_of(needed) {
        let mut shred = game.buff("judgment_shred", duration);
        shred.source = Some(id);
        shred.bonus.armor_percent = -spell.value("ShredAmount", rank);
        game.add_buff(victim, shred);
        let position = position_of(game, victim);
        game.effect("GarenEShred", position, Some(victim));
    }
}

fn mark_nearest(game: &mut Game, id: UnitId, nearest: Option<UnitId>, remaining: f32) {
    let marked: Vec<UnitId> = game
        .units
        .iter()
        .filter(|unit| {
            unit.buff(NEAREST)
                .is_some_and(|buff| buff.source == Some(id))
        })
        .map(|unit| unit.id)
        .collect();
    for unit in marked.iter().copied().filter(|unit| Some(*unit) != nearest) {
        if let Some(unit) = game.unit_mut(unit) {
            unit.buffs.retain(|buff| buff.name != NEAREST);
        }
        let position = position_of(game, unit);
        game.effect("GarenENearestEnd", position, Some(unit));
    }
    let Some(nearest) = nearest.filter(|nearest| !marked.contains(nearest)) else {
        return;
    };
    let mut buff = game.buff(NEAREST, remaining);
    buff.source = Some(id);
    game.add_buff(nearest, buff);
    let position = position_of(game, nearest);
    game.effect("GarenENearest", position, Some(nearest));
}

fn died(game: &mut Game, id: UnitId) {
    if game.unit(id).is_some_and(|unit| unit.has_buff("judgment")) {
        stop_spinning(game, id);
    }
}

fn stop_spinning(game: &mut Game, id: UnitId) {
    mark_nearest(game, id, None, 0.0);
    let spell = spell_of(game, id, Slot::E).unwrap_or_default();
    let rank = rank(game, id, Slot::E);
    if let Some(unit) = game.unit_mut(id) {
        unit.buffs.retain(|buff| {
            !matches!(
                buff.name,
                "judgment" | "judgment_tick" | "judgment_lock" | "disarm"
            )
        });
    }
    set_cooldown(game, id, Slot::E, spell.cooldown(rank));
    game.release(id, "Spell3");
    let position = position_of(game, id);
    game.effect("GarenEEnd", position, Some(id));
}

fn position_of(game: &Game, id: UnitId) -> glam::Vec2 {
    game.unit(id).map_or(glam::Vec2::ZERO, |unit| unit.position)
}
