use asset::clip_id;
use asset::data::Spell;
use glam::Vec2;
use protocol::{DamageKind, Slot, TICK, Target, UnitId};

use super::{
    Cast, Kit, Zone, begin, begin_channel, calculate, cast_range, clamp_point, enemies_on_segment,
    named_spell, rank, spell_of, spend,
};
use crate::stats;
use crate::unit::{Action, Unit, slot_index};
use crate::{Game, Hit, Missile, Motion};

const PASSIVE: &str = "XerathAscended2";
const MANA_SURGE: &str = "mana_surge_cooldown";
const MANA_SURGE_COOLDOWN: f32 = 16.0;
const ARCANOPULSE: &str = "arcanopulse";
const ARCANOPULSE_MIN_RANGE: f32 = 700.0;
const ARCANOPULSE_MAX_RANGE: f32 = 1450.0;
const ARCANOPULSE_GROWTH: f32 = 1.5;
const ARCANOPULSE_HOLD: f32 = 3.0;
const ARCANOPULSE_MAX_SLOW: f32 = 0.4;
const ARCANOPULSE_LOCKOUT: f32 = 0.528;
const CHARGE_WINDUP: f32 = 1.0;
const EYE_OF_DESTRUCTION: &str = "eye_of_destruction";
const EYE_RADIUS: f32 = 275.0;
const EYE_CENTER: f32 = 125.0;
const SHOCKING_ORB_SPEED: f32 = 1400.0;
const SHOCKING_ORB_WIDTH: f32 = 60.0;
const RITE: &str = "rite_of_the_arcane";
const RITE_SHOT: &str = "arcane_barrage";
const ARCANE_PERFECTION: &str = "arcane_perfection";
const RITE_UNLOCK: f32 = 0.5;
const RITE_ROOTED: f32 = 1.25;
const RITE_INTERVAL: f32 = 0.627;
const RITE_IMPACT: f32 = 0.627;
const RITE_START: f32 = 0.767;
const RELEASE_CLIP: &str = "{f980ad57}";
const RITE_END_CLIP: &str = "{eeddca89}";

pub const KIT: Kit = Kit {
    died,
    recast,
    recasting,
    cast,
    fire,
    range,
    area,
    missile_hit,
    on_attack_hit,
    on_kill,
    tick,
    zone,
    passive_cooldown: Some(MANA_SURGE),
    silent_hits: &[Slot::E],
    ..Kit::NONE
};

fn cast(game: &mut Game, id: UnitId, slot: Slot, target: Target, spell: &Spell, rank: u8) {
    match slot {
        Slot::Q => charge(game, id, spell, rank),
        Slot::W => {
            spend(game, id, slot, spell.cooldown(rank), spell.mana(rank));
            begin(
                game,
                id,
                slot,
                target,
                spell.cast_time.max(0.25),
                0.1,
                "Spell4",
            );
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
        }
        Slot::R => rite(game, id, target, spell, rank),
        _ => {}
    }
}

fn range(game: &Game, unit: &Unit, slot: Slot, base: f32) -> f32 {
    match charge_elapsed(game, unit) {
        Some(elapsed) if slot == Slot::Q => charged_range(elapsed),
        _ => base,
    }
}

fn area(game: &Game, unit: &Unit, slot: Slot) -> f32 {
    match spell_of(game, unit.id, slot) {
        Some(spell) if slot == Slot::R && unit.has_buff(RITE) => {
            spell.value("AoESize", rank(game, unit.id, slot))
        }
        _ => 0.0,
    }
}

fn recasting(unit: &Unit, slot: Slot) -> bool {
    match slot {
        Slot::Q => unit.has_buff(ARCANOPULSE),
        Slot::R => unit.has_buff(RITE),
        _ => false,
    }
}

fn recast(game: &mut Game, id: UnitId, slot: Slot, target: Target) -> bool {
    let Some(unit) = game.unit(id) else {
        return false;
    };
    if unit.has_buff(ARCANOPULSE) {
        if slot == Slot::Q {
            release(game, id, target);
        }
        return true;
    }
    if unit.has_buff(RITE) {
        if slot == Slot::R {
            barrage(game, id, target);
        }
        return true;
    }
    false
}

fn charge(game: &mut Game, id: UnitId, spell: &Spell, rank: u8) {
    spend(game, id, Slot::Q, 0.0, spell.mana(rank));
    let mut buff = game.buff(ARCANOPULSE, f32::MAX);
    buff.value = game.time;
    game.add_buff(id, buff);
    let disarm = game.buff("disarm", f32::MAX);
    game.add_buff(id, disarm);
    game.hold(id, "Channel_Wndup");
    let position = position_of(game, id);
    game.effect("XerathQCharge", position, Some(id));
    game.sound(id, "q");
}

fn charge_elapsed(game: &Game, unit: &Unit) -> Option<f32> {
    unit.buff(ARCANOPULSE).map(|buff| game.time - buff.value)
}

fn charged_range(elapsed: f32) -> f32 {
    let grown = (elapsed / ARCANOPULSE_GROWTH).clamp(0.0, 1.0);
    ARCANOPULSE_MIN_RANGE + (ARCANOPULSE_MAX_RANGE - ARCANOPULSE_MIN_RANGE) * grown
}

fn release(game: &mut Game, id: UnitId, target: Target) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    if !unit.can_act() {
        return;
    }
    let range = charge_elapsed(game, unit).map_or(ARCANOPULSE_MIN_RANGE, charged_range);
    let position = unit.position;
    let direction = (Vec2::from_array(target.point) - position)
        .try_normalize()
        .unwrap_or(Vec2::new(unit.facing.sin(), unit.facing.cos()));
    end_charge(game, id);
    start_cooldown(game, id, Slot::Q, 1.0);
    let end = Target {
        point: (position + direction * range).to_array(),
        unit: None,
    };
    begin(
        game,
        id,
        Slot::Q,
        end,
        ARCANOPULSE_LOCKOUT,
        0.0,
        RELEASE_CLIP,
    );
    game.effect("XerathQ", position, Some(id));
}

fn end_charge(game: &mut Game, id: UnitId) {
    if let Some(unit) = game.unit_mut(id) {
        unit.buffs
            .retain(|buff| buff.name != ARCANOPULSE && buff.name != "disarm");
    }
    game.release(id, "Channel_Wndup");
    game.release(id, "Channel");
    game.refresh_stats(id);
    let position = position_of(game, id);
    game.effect("XerathQChargeEnd", position, Some(id));
}

fn charging(game: &mut Game, id: UnitId) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    let Some(elapsed) = charge_elapsed(game, unit) else {
        return;
    };
    if unit.dead || unit.is_stunned() || unit.is_silenced() {
        end_charge(game, id);
        start_cooldown(game, id, Slot::Q, 1.0);
        return;
    }
    if elapsed + TICK >= ARCANOPULSE_HOLD {
        let spell = spell_of(game, id, Slot::Q).unwrap_or_default();
        let rank = rank(game, id, Slot::Q);
        let refund = spell.mana(rank) * spell.value("ManaRefundFail", rank);
        end_charge(game, id);
        start_cooldown(game, id, Slot::Q, 1.0);
        if let Some(unit) = game.unit_mut(id) {
            unit.mana = (unit.mana + refund).min(unit.stats.max_mana);
        }
        return;
    }
    let winding = unit
        .animation
        .is_some_and(|animation| animation.clip == clip_id("Channel_Wndup"));
    if winding && elapsed >= CHARGE_WINDUP {
        game.hold(id, "Channel");
    }
    let slow = ARCANOPULSE_MAX_SLOW * (elapsed / ARCANOPULSE_GROWTH).min(1.0);
    if let Some(buff) = game
        .unit_mut(id)
        .and_then(|unit| unit.buff_mut(ARCANOPULSE))
    {
        buff.bonus.slow = slow;
    }
    game.refresh_stats(id);
}

fn rite(game: &mut Game, id: UnitId, target: Target, spell: &Spell, rank: u8) {
    spend(game, id, Slot::R, 0.0, spell.mana(rank));
    let duration = spell.value("Duration", rank);
    let mut buff = game.buff(RITE, f32::MAX);
    buff.stacks = spell.value("NumberOfShots", rank) as u32;
    buff.value = f32::MIN;
    game.add_buff(id, buff);
    begin_channel(game, id, Slot::R, target, duration);
    game.play(id, "Spell5start", 1.0);
    let position = position_of(game, id);
    game.effect("XerathR", position, Some(id));
    game.sound(id, "r");
}

fn rite_elapsed(unit: &Unit) -> Option<f32> {
    match unit.action {
        Action::Cast {
            slot: Slot::R,
            channel: true,
            elapsed,
            ..
        } => Some(elapsed),
        _ => None,
    }
}

fn barrage(game: &mut Game, id: UnitId, target: Target) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    let (Some(elapsed), Some(buff)) = (rite_elapsed(unit), unit.buff(RITE)) else {
        return;
    };
    let (shots, last) = (buff.stacks, buff.value);
    if elapsed < RITE_UNLOCK || game.time - last < RITE_INTERVAL || shots == 0 {
        return;
    }
    let (position, team) = (unit.position, unit.team);
    let range = cast_range(game, unit, Slot::R);
    let spell = spell_of(game, id, Slot::R).unwrap_or_default();
    let rank = rank(game, id, Slot::R);
    let center = clamp_point(position, Vec2::from_array(target.point), range);
    let time = game.time;
    game.zones.push(Zone {
        owner: id,
        team,
        name: RITE_SHOT,
        center,
        radius: spell.value("AoESize", rank),
        armed: time + RITE_IMPACT,
        expires: time + RITE_IMPACT + 1.0,
        next_tick: time + RITE_IMPACT,
        damage: 0.0,
        value: 0.0,
    });
    let missile = game.allocate();
    game.missiles.push(Missile {
        id: missile,
        owner: id,
        team,
        kind: clip_id("XerathLocusPulse"),
        position,
        height: 100.0,
        speed: position.distance(center) / RITE_IMPACT,
        width: 0.0,
        motion: Motion::Point(center),
        hit: Hit::Visual,
        hit_units: Vec::new(),
        acceleration: None,
    });
    game.owned_effect("XerathRTarget", id, center, None);
    let left = shots - 1;
    if let Some(buff) = game.unit_mut(id).and_then(|unit| unit.buff_mut(RITE)) {
        buff.stacks = left;
        buff.value = time;
    }
    if left == 0 {
        if let Some(unit) = game.unit_mut(id) {
            unit.action = Action::None;
        }
        end_rite(game, id);
    }
}

fn rite_tick(game: &mut Game, id: UnitId) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    if !unit.has_buff(RITE) {
        return;
    }
    let Some(elapsed) = rite_elapsed(unit) else {
        end_rite(game, id);
        return;
    };
    if elapsed >= RITE_ROOTED && !unit.path.is_empty() {
        if let Some(unit) = game.unit_mut(id) {
            unit.action = Action::None;
        }
        end_rite(game, id);
        return;
    }
    let looping = unit
        .animation
        .is_some_and(|animation| animation.hold && animation.clip == clip_id("Spell5Loop"));
    if !looping && elapsed >= RITE_START {
        game.hold(id, "Spell5Loop");
    }
}

fn died(game: &mut Game, id: UnitId) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    let (charging, rite) = (unit.has_buff(ARCANOPULSE), unit.has_buff(RITE));
    if charging {
        end_charge(game, id);
        start_cooldown(game, id, Slot::Q, 1.0);
    }
    if rite {
        end_rite(game, id);
    }
}

fn end_rite(game: &mut Game, id: UnitId) {
    let spell = spell_of(game, id, Slot::R).unwrap_or_default();
    let rank = rank(game, id, Slot::R);
    let total = spell.value("NumberOfShots", rank) as u32;
    let landed = game.time + RITE_IMPACT + TICK;
    let Some(unit) = game.unit_mut(id) else {
        return;
    };
    let unused = unit.buff(RITE).is_some_and(|buff| buff.stacks == total);
    unit.buffs.retain(|buff| buff.name != RITE);
    if let Some(buff) = unit.buff_mut(ARCANE_PERFECTION) {
        buff.expires = buff.expires.min(landed);
    }
    let share = if unused {
        1.0 - spell.value("FailCastRefund", rank)
    } else {
        1.0
    };
    start_cooldown(game, id, Slot::R, share);
    game.release(id, "Spell5Loop");
    game.play(id, RITE_END_CLIP, 1.0);
    let position = position_of(game, id);
    game.effect("XerathREnd", position, Some(id));
}

fn fire(game: &mut Game, cast: &Cast) {
    let Cast {
        id,
        spell,
        rank,
        position,
        team,
        point,
        facing,
        ..
    } = *cast;
    let time = game.time;
    match cast.slot {
        Slot::Q => {
            let damage = calculate(game, id, spell, rank, "TooltipTotalDamage");
            let half_width = spell.value("RectangleWidth", rank) / 2.0;
            for victim in enemies_on_segment(game, team, position, point, half_width) {
                game.damage(Some(id), victim, damage, DamageKind::Magic, false);
                let at = game.unit(victim).map_or(point, |unit| unit.position);
                game.effect("XerathQHit", at, Some(victim));
                game.sound_on(id, victim, "q_hit");
            }
            game.effect("XerathQBeam", point, Some(id));
            game.sound(id, "q_cast");
        }
        Slot::W => {
            let center = clamp_point(position, point, spell.range(rank));
            let delay = spell.value("DamageDelay", rank);
            game.zones.push(Zone {
                owner: id,
                team,
                name: EYE_OF_DESTRUCTION,
                center,
                radius: EYE_RADIUS,
                armed: time + delay,
                expires: time + delay + 1.0,
                next_tick: time + delay,
                damage: calculate(game, id, spell, rank, "TotalDamage"),
                value: calculate(game, id, spell, rank, "SweetSpotTotalDamage"),
            });
            game.lasting_effect("XerathW", id, center, time + delay);
            game.sound(id, "w");
        }
        Slot::E => {
            let damage = calculate(game, id, spell, rank, "TooltipTotalDamage");
            let missile = game.allocate();
            game.missiles.push(Missile {
                id: missile,
                owner: id,
                team,
                kind: clip_id("XerathMageSpearMissile"),
                position,
                height: 100.0,
                speed: SHOCKING_ORB_SPEED,
                width: SHOCKING_ORB_WIDTH,
                motion: Motion::Line {
                    direction: facing,
                    remaining: spell.value("Range", rank),
                    limit: Some(1),
                },
                hit: Hit::Spell(Slot::E, damage),
                hit_units: Vec::new(),
                acceleration: None,
            });
            game.sound(id, "e");
        }
        Slot::R => {
            end_rite(game, id);
        }
        _ => {}
    }
}

fn zone(game: &mut Game, index: usize) {
    let zone = game.zones[index];
    if game.time < zone.armed {
        return;
    }
    game.zones[index].expires = game.time;
    match zone.name {
        EYE_OF_DESTRUCTION => eye_of_destruction(game, &zone),
        RITE_SHOT => arcane_barrage(game, &zone),
        _ => {}
    }
}

fn eye_of_destruction(game: &mut Game, zone: &Zone) {
    let spell = spell_of(game, zone.owner, Slot::W).unwrap_or_default();
    let rank = rank(game, zone.owner, Slot::W);
    let duration = spell.value("SlowDuration", rank);
    for victim in game.enemies_near(zone.team, zone.center, zone.radius) {
        let Some(unit) = game.unit(victim).filter(|unit| !unit.is_structure()) else {
            continue;
        };
        let centered = unit.position.distance(zone.center) <= EYE_CENTER + unit.radius();
        let (damage, slow) = if centered {
            (zone.value, spell.value("SweetSpotSlowAmount", rank))
        } else {
            (zone.damage, spell.value("SlowAmount", rank))
        };
        game.damage(Some(zone.owner), victim, damage, DamageKind::Magic, false);
        game.slow(victim, slow, duration, Some((zone.owner, Slot::W)));
        if let Some(buff) = game
            .unit_mut(victim)
            .and_then(|unit| unit.buff_mut("slow"))
            .filter(|buff| buff.source == Some(zone.owner) && buff.slot == Some(Slot::W))
        {
            buff.value = slow;
        }
        let at = game.unit(victim).map_or(zone.center, |unit| unit.position);
        game.effect("XerathWHit", at, Some(victim));
    }
    game.effect("XerathWBlast", zone.center, Some(zone.owner));
    game.sound(zone.owner, "w_hit");
}

fn decay_slows(game: &mut Game, id: UnitId) {
    let floor = spell_of(game, id, Slot::W)
        .map(|spell| spell.value("SlowAmount", rank(game, id, Slot::W)))
        .unwrap_or_default();
    let time = game.time;
    let slowed: Vec<UnitId> = game
        .units
        .iter()
        .filter(|unit| {
            unit.buffs.iter().any(|buff| {
                buff.name == "slow"
                    && buff.source == Some(id)
                    && buff.slot == Some(Slot::W)
                    && buff.value > floor
            })
        })
        .map(|unit| unit.id)
        .collect();
    for victim in slowed {
        if let Some(buff) = game
            .unit_mut(victim)
            .and_then(|unit| unit.buff_mut("slow"))
            .filter(|buff| buff.source == Some(id) && buff.slot == Some(Slot::W))
        {
            let left = ((buff.expires - time) / buff.duration.max(f32::EPSILON)).clamp(0.0, 1.0);
            buff.bonus.slow = floor + (buff.value - floor) * left;
        }
        game.refresh_stats(victim);
    }
}

fn arcane_barrage(game: &mut Game, zone: &Zone) {
    let owner = zone.owner;
    let spell = spell_of(game, owner, Slot::R).unwrap_or_default();
    let rank = rank(game, owner, Slot::R);
    let stacks = game
        .unit(owner)
        .and_then(|unit| unit.buff(ARCANE_PERFECTION))
        .map_or(0, |buff| buff.stacks);
    let damage = calculate(game, owner, &spell, rank, "TooltipTotalDamage")
        + stacks as f32 * calculate(game, owner, &spell, rank, "RampDamageCalc");
    let mut champion_hit = false;
    for victim in game.enemies_near(zone.team, zone.center, zone.radius) {
        let Some(unit) = game.unit(victim).filter(|unit| !unit.is_structure()) else {
            continue;
        };
        champion_hit |= unit.champion().is_some();
        game.damage(Some(owner), victim, damage, DamageKind::Magic, false);
    }
    game.effect("XerathRBlast", zone.center, Some(owner));
    game.sound(owner, "r_hit");
    let most = (spell.value("NumberOfShots", rank) as u32).saturating_sub(1);
    let channeling = game.unit(owner).is_some_and(|unit| unit.has_buff(RITE));
    if champion_hit && channeling && stacks < most {
        let mut buff = game.buff(ARCANE_PERFECTION, spell.value("Duration", rank));
        buff.stacks = stacks + 1;
        game.add_buff(owner, buff);
    }
}

fn missile_hit(game: &mut Game, missile: &Missile, target: UnitId, slot: Slot, damage: f32) {
    if slot != Slot::E {
        return;
    }
    let owner = missile.owner;
    let spell = spell_of(game, owner, Slot::E).unwrap_or_default();
    let rank = rank(game, owner, Slot::E);
    let range = spell.value("Range", rank).max(1.0);
    let remaining = match missile.motion {
        Motion::Line { remaining, .. } => remaining,
        _ => range,
    };
    let traveled = ((range - remaining) / range).clamp(0.0, 1.0);
    let shortest = spell.value("MinStunDuration", rank);
    let longest = spell.value("MaxStunDuration", rank);
    game.damage(Some(owner), target, damage, DamageKind::Magic, false);
    game.stun(
        target,
        shortest + (longest - shortest) * traveled,
        Some((owner, Slot::E)),
    );
}

fn on_attack_hit(game: &mut Game, attacker: UnitId, target: UnitId) {
    let Some(unit) = game.unit(attacker) else {
        return;
    };
    if unit.has_buff(MANA_SURGE) {
        return;
    }
    let (mana, max_mana) = (unit.mana, unit.stats.max_mana);
    let champion = game
        .unit(target)
        .is_some_and(|unit| unit.champion().is_some());
    let passive = named_spell(game, attacker, PASSIVE).unwrap_or_default();
    let restore = calculate(
        game,
        attacker,
        &passive,
        1,
        if champion {
            "ChampionManaRestoreTT"
        } else {
            "MinionManaRestoreTT"
        },
    );
    if mana + restore > max_mana {
        return;
    }
    if let Some(unit) = game.unit_mut(attacker) {
        unit.mana += restore;
    }
    let cooldown = game.buff(MANA_SURGE, MANA_SURGE_COOLDOWN);
    game.add_buff(attacker, cooldown);
    let position = position_of(game, attacker);
    game.effect("ManaSurge", position, Some(attacker));
}

fn on_kill(game: &mut Game, killer: UnitId, _victim: UnitId) {
    let refund = named_spell(game, killer, PASSIVE)
        .unwrap_or_default()
        .value("CooldownKillRefund", 1);
    if let Some(buff) = game
        .unit_mut(killer)
        .and_then(|unit| unit.buff_mut(MANA_SURGE))
    {
        buff.expires -= refund;
    }
}

fn tick(game: &mut Game, id: UnitId) {
    charging(game, id);
    rite_tick(game, id);
    decay_slows(game, id);
}

fn start_cooldown(game: &mut Game, id: UnitId, slot: Slot, share: f32) {
    let spell = spell_of(game, id, slot).unwrap_or_default();
    let rank = rank(game, id, slot);
    if let Some(unit) = game.unit_mut(id) {
        let cooldown = stats::cooldown(spell.cooldown(rank), unit.stats.ability_haste) * share;
        let ability = &mut unit.abilities[slot_index(slot)];
        ability.cooldown = cooldown;
        ability.cooldown_total = cooldown;
    }
}

fn position_of(game: &Game, id: UnitId) -> Vec2 {
    game.unit(id).map_or(Vec2::ZERO, |unit| unit.position)
}
