use asset::clip_id;
use glam::Vec2;
use protocol::{DamageKind, Slot, Summoner, Target, UnitId};

use super::{approach, clamp_point, dash, direction, target_in_range};
use crate::unit::{DashKind, Order, Unit, slot_index};
use crate::{Game, Hit, Missile, Motion, facing_to};

pub const FLASH_RANGE: f32 = 400.0;
const MARK_RANGE: f32 = 1600.0;
const EXHAUST_RANGE: f32 = 650.0;
const IGNITE_RANGE: f32 = 600.0;
const HEAL_RANGE: f32 = 850.0;
const CLARITY_RANGE: f32 = 600.0;
const GHOST_DURATION: f32 = 10.0;
const BARRIER_DURATION: f32 = 2.5;
const EXHAUST_DURATION: f32 = 3.0;
const EXHAUST_SLOW: f32 = 0.3;
const IGNITE_DURATION: f32 = 5.0;
const HEAL_HASTE: f32 = 0.3;
const CLEANSED: [&str; 8] = [
    "stun",
    "root",
    "silence",
    "disarm",
    "slow",
    "exhaust",
    "ignite",
    "grievous_wounds",
];

pub fn summoner(unit: &Unit, slot: Slot) -> Option<Summoner> {
    match slot {
        Slot::D => Some(unit.summoners[0]),
        Slot::F => Some(unit.summoners[1]),
        _ => None,
    }
}

pub fn range(unit: &Unit, slot: Slot) -> f32 {
    match summoner(unit, slot) {
        Some(Summoner::Exhaust) => EXHAUST_RANGE + unit.radius(),
        Some(Summoner::Ignite) => IGNITE_RANGE + unit.radius(),
        Some(Summoner::Heal) => HEAL_RANGE,
        Some(Summoner::Mark) => MARK_RANGE,
        Some(Summoner::Clarity) => CLARITY_RANGE,
        Some(Summoner::Flash) => FLASH_RANGE,
        _ => 0.0,
    }
}

fn by_level(unit: &Unit, first: f32, last: f32) -> f32 {
    first + (last - first) * f32::from(unit.level.max(1) - 1) / 17.0
}

fn cooldown(game: &Game, summoner: Summoner) -> f32 {
    game.data
        .summoner(summoner.id())
        .map_or(180.0, |data| data.cooldown)
}

fn start_cooldown(game: &mut Game, id: UnitId, slot: Slot, summoner: Summoner) {
    let cooldown = cooldown(game, summoner);
    if let Some(unit) = game.unit_mut(id) {
        let ability = &mut unit.abilities[slot_index(slot)];
        ability.cooldown = cooldown;
        ability.cooldown_total = cooldown;
    }
}

pub fn cast(game: &mut Game, id: UnitId, slot: Slot, target: Target) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    let Some(summoner) = summoner(unit, slot) else {
        return;
    };
    if summoner == Summoner::Mark {
        mark(game, id, slot, target);
        return;
    }
    if !unit.alive() || unit.ability(slot).cooldown > 0.0 {
        return;
    }
    let usable = match summoner {
        Summoner::Flash => !unit.is_stunned() && !unit.has_buff("root"),
        Summoner::Cleanse => true,
        _ => unit.can_act(),
    };
    if !usable {
        return;
    }
    let cast = match summoner {
        Summoner::Flash => flash(game, id, target),
        Summoner::Heal => heal(game, id, target),
        Summoner::Ghost => ghost(game, id),
        Summoner::Barrier => barrier(game, id),
        Summoner::Exhaust => exhaust(game, id, slot, target),
        Summoner::Clarity => clarity(game, id),
        Summoner::Cleanse => cleanse(game, id),
        Summoner::Ignite => ignite(game, id, slot, target),
        Summoner::Mark => false,
    };
    if cast {
        start_cooldown(game, id, slot, summoner);
    }
}

fn position(game: &Game, id: UnitId) -> Vec2 {
    game.unit(id).map_or(Vec2::ZERO, |unit| unit.position)
}

fn flash(game: &mut Game, id: UnitId, target: Target) -> bool {
    let from = position(game, id);
    let destination = game.walkable_point(clamp_point(
        from,
        Vec2::from_array(target.point),
        FLASH_RANGE,
    ));
    if let Some(unit) = game.unit_mut(id) {
        unit.position = destination;
        unit.facing = facing_to(from, destination, unit.facing);
        unit.path.clear();
        if matches!(unit.order, Order::Move(_)) {
            unit.order = Order::Idle;
        }
    }
    game.effect("Flash", from, Some(id));
    game.effect("FlashArrive", destination, Some(id));
    true
}

fn heal(game: &mut Game, id: UnitId, target: Target) -> bool {
    let Some(unit) = game.unit(id).cloned() else {
        return false;
    };
    let amount = by_level(&unit, 90.0, 345.0);
    let ally = |other: &&Unit| {
        other.id != id
            && other.team == unit.team
            && other.alive()
            && other.champion().is_some()
            && other.position.distance(unit.position) <= HEAL_RANGE
    };
    let partner = target
        .unit
        .and_then(|target| game.unit(target))
        .filter(ally)
        .or_else(|| {
            game.units.iter().filter(ally).min_by(|a, b| {
                (a.health / a.stats.max_health).total_cmp(&(b.health / b.stats.max_health))
            })
        })
        .map(|other| (other.id, other.position));
    for (healed, at, effect) in [(id, unit.position, "SummonerHeal")]
        .into_iter()
        .chain(partner.map(|(other, at)| (other, at, "SummonerHealTarget")))
    {
        game.heal(healed, amount);
        let mut haste = game.buff("summoner_heal", 1.0);
        haste.bonus.move_speed_percent = HEAL_HASTE;
        game.add_buff(healed, haste);
        game.effect(effect, at, Some(healed));
    }
    game.shared_sound(id, "summoner.heal");
    true
}

fn ghost(game: &mut Game, id: UnitId) -> bool {
    let Some(unit) = game.unit(id) else {
        return false;
    };
    let mut buff = game.buff("ghost", GHOST_DURATION);
    buff.bonus.move_speed_percent = by_level(unit, 0.24, 0.48);
    let at = unit.position;
    game.add_buff(id, buff);
    game.effect("SummonerGhost", at, Some(id));
    game.shared_sound(id, "summoner.ghost");
    true
}

fn barrier(game: &mut Game, id: UnitId) -> bool {
    let Some(unit) = game.unit(id) else {
        return false;
    };
    let amount = by_level(unit, 120.0, 480.0);
    let at = unit.position;
    game.shield(id, amount, BARRIER_DURATION);
    game.effect("SummonerBarrier", at, Some(id));
    game.shared_sound(id, "summoner.barrier");
    true
}

fn enemy_champion_in_range(
    game: &mut Game,
    id: UnitId,
    slot: Slot,
    target: Target,
) -> Option<UnitId> {
    let range = range(game.unit(id)?, slot);
    let victim = target_in_range(game, id, &target, range, true).filter(|victim| {
        game.unit(*victim)
            .is_some_and(|unit| unit.champion().is_some())
    });
    if victim.is_none() && target.unit.is_some() {
        approach(game, id, slot, target);
    }
    victim
}

fn exhaust(game: &mut Game, id: UnitId, slot: Slot, target: Target) -> bool {
    let Some(victim) = enemy_champion_in_range(game, id, slot, target) else {
        return false;
    };
    let mut buff = game.buff("exhaust", EXHAUST_DURATION);
    buff.bonus.slow = EXHAUST_SLOW;
    buff.source = Some(id);
    game.add_buff(victim, buff);
    let at = position(game, victim);
    game.effect("SummonerExhaust", at, Some(victim));
    game.shared_sound(id, "summoner.exhaust");
    true
}

fn clarity(game: &mut Game, id: UnitId) -> bool {
    let Some(unit) = game.unit(id).cloned() else {
        return false;
    };
    let allies: Vec<(UnitId, f32)> = game
        .units
        .iter()
        .filter(|other| {
            other.team == unit.team
                && other.alive()
                && other.champion().is_some()
                && other.position.distance(unit.position) <= CLARITY_RANGE
        })
        .map(|other| (other.id, if other.id == id { 0.5 } else { 0.25 }))
        .collect();
    for (ally, share) in allies {
        if let Some(other) = game.unit_mut(ally) {
            other.mana = (other.mana + other.stats.max_mana * share).min(other.stats.max_mana);
        }
    }
    game.effect("SummonerClarity", unit.position, Some(id));
    game.shared_sound(id, "summoner.clarity");
    true
}

fn cleanse(game: &mut Game, id: UnitId) -> bool {
    let Some(unit) = game.unit_mut(id) else {
        return false;
    };
    unit.buffs
        .retain(|buff| !CLEANSED.contains(&buff.name) && buff.bonus.slow <= 0.0);
    let at = unit.position;
    game.refresh_stats(id);
    game.effect("SummonerCleanse", at, Some(id));
    game.shared_sound(id, "summoner.cleanse");
    true
}

fn ignite(game: &mut Game, id: UnitId, slot: Slot, target: Target) -> bool {
    let Some(victim) = enemy_champion_in_range(game, id, slot, target) else {
        return false;
    };
    let total = game
        .unit(id)
        .map_or(70.0, |unit| by_level(unit, 70.0, 410.0));
    let mut burn = game.buff("ignite", IGNITE_DURATION);
    burn.source = Some(id);
    burn.value = total / IGNITE_DURATION;
    burn.stacks = IGNITE_DURATION as u32;
    game.add_buff(victim, burn);
    let wounds = game.buff("grievous_wounds", IGNITE_DURATION);
    game.add_buff(victim, wounds);
    let at = position(game, victim);
    game.effect("SummonerIgnite", at, Some(victim));
    game.shared_sound(id, "summoner.ignite");
    true
}

pub fn afflicted(game: &mut Game, id: UnitId, name: &'static str) {
    if name != "ignite" {
        return;
    }
    let time = game.time;
    let Some(burn) = game.unit(id).and_then(|unit| unit.buff("ignite")).cloned() else {
        return;
    };
    let next_tick = burn.expires - burn.stacks as f32 + 0.5;
    if burn.stacks == 0 || time < next_tick {
        return;
    }
    if let Some(buff) = game.unit_mut(id).and_then(|unit| unit.buff_mut("ignite")) {
        buff.stacks -= 1;
    }
    let at = position(game, id);
    game.effect("SummonerIgniteTick", at, Some(id));
    game.damage(burn.source, id, burn.value, DamageKind::True, false);
}

fn mark(game: &mut Game, id: UnitId, slot: Slot, target: Target) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    if !unit.can_act() {
        return;
    }
    if let Some(marked) = marked(game, id) {
        if unit.has_buff("root") {
            return;
        }
        let to = game
            .unit(marked)
            .map_or(unit.position, |other| other.position);
        let speed = 1200.0 + unit.stats.move_speed;
        dash(game, id, to, speed, DashKind::Mark(marked));
        if let Some(other) = game.unit_mut(marked) {
            other
                .buffs
                .retain(|buff| !(buff.name == "mark" && buff.source == Some(id)));
        }
        game.shared_sound(id, "snowball.dash");
        return;
    }
    if unit.ability(slot).cooldown > 0.0 {
        return;
    }
    let position = unit.position;
    let facing = direction(position, Vec2::from_array(target.point), unit.facing);
    let team = unit.team;
    start_cooldown(game, id, slot, Summoner::Mark);
    if let Some(unit) = game.unit_mut(id) {
        unit.facing = facing.x.atan2(facing.y);
    }
    let missile = game.allocate();
    game.missiles.push(Missile {
        id: missile,
        owner: id,
        team,
        kind: clip_id("SummonerSnowball"),
        position,
        height: 100.0,
        speed: 1300.0,
        width: 60.0,
        motion: Motion::Line {
            direction: facing,
            remaining: MARK_RANGE,
            limit: Some(1),
        },
        hit: Hit::Mark,
        hit_units: Vec::new(),
        acceleration: None,
    });
    game.shared_sound(id, "snowball.throw");
}

fn marked(game: &Game, id: UnitId) -> Option<UnitId> {
    game.units
        .iter()
        .find(|other| {
            other.alive()
                && other
                    .buffs
                    .iter()
                    .any(|buff| buff.name == "mark" && buff.source == Some(id))
        })
        .map(|other| other.id)
}

pub fn summoner_recast(game: &Game, unit: &Unit, slot: Slot) -> bool {
    summoner(unit, slot) == Some(Summoner::Mark) && marked(game, unit.id).is_some()
}

fn mark_damage(game: &Game, id: UnitId) -> f32 {
    let level = game.unit(id).map_or(1, |unit| unit.level);
    10.0 + 5.0 * f32::from(level)
}

pub fn mark_hit(game: &mut Game, owner: UnitId, target: UnitId) {
    let damage = mark_damage(game, owner);
    game.damage(Some(owner), target, damage, DamageKind::True, false);
    let mut buff = game.buff("mark", 3.0);
    buff.source = Some(owner);
    game.add_sourced_buff(target, buff);
    game.shared_sound(target, "snowball.hit");
}

pub fn mark_landed(game: &mut Game, id: UnitId, target: UnitId) {
    let damage = mark_damage(game, id);
    game.damage(Some(id), target, damage, DamageKind::True, false);
    game.shared_sound(id, "snowball.dash_hit");
}
