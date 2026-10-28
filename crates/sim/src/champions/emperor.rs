use asset::clip_id;
use asset::data::Spell;
use glam::Vec2;
use protocol::{DamageKind, Slot, TICK, Target, Team, UnitId, UnitKind};

use super::{
    Cast, Interaction, Kit, activated, begin, calculate, clamp_point, dash, direction,
    enemies_on_segment, max_charges, named_spell, rank, recharge_charges, spell_of, spend,
};

use crate::stats::{self, Bonus};
use crate::unit::{Action, DashKind, Tier, Unit, slot_index};
use crate::{Game, facing_to, items};

const COMMAND_CLIP: &str = "Attack_Command";
const SOLDIER_ATTACK_RANGE: f32 = 350.0;
const SOLDIER_LEASH: f32 = 660.0;
const SPEAR_OVERREACH: f32 = 50.0;
const SOLDIER_SPAWN_RANGE: f32 = 525.0;
const SOLDIER_LOCKOUT: f32 = 1.5;
const SHIFTING_SANDS_RADIUS: f32 = 120.0;
const EMPERORS_GUARD: &str = "emperors_guard";
const INACTIVE: &str = "soldier_inactive";
const CHARGE_DELAY: f32 = 0.25;
const SOLDIER_START: f32 = 175.0;
const CATCH_BEHIND: f32 = 325.0;
const CHARGE_DISTANCE: f32 = 575.0;
const CHARGE_SPEED: f32 = 1000.0;
const CHARGE_WIDTH: [f32; 3] = [620.0, 720.0, 820.0];
const WALL_WIDTH: [f32; 3] = [750.0, 850.0, 950.0];
const WALL_DURATION: f32 = 5.0;
const WALL_THICKNESS: f32 = 50.0;
const KNOCKBACK_LINE: f32 = 650.0;
const KNOCKBACK_TIME: f32 = 1.0;
const SUN_DISC_RANGE: f32 = 700.0;
const SUN_DISC_CAST: f32 = 0.5;
const SUN_DISC_ASSEMBLY: f32 = 5.0;
const SUN_DISC_HEALTH: f32 = 1750.0;
const SUN_DISC_TETHER: f32 = 2000.0;
const SUN_DISC_RESIST_LOSS: f32 = 100.0;
const SUN_DISC_COOLDOWN: &str = "sun_disc_cooldown";
const ASSEMBLING: &str = "sun_disc_assembling";

pub const KIT: Kit = Kit {
    spawned,
    ranked_up,
    recharge,
    ammo: Some(Slot::W),
    cast,
    fire,
    redirect_attack,
    launch_attack,
    dash_cast,
    tick,
    interaction: Some(Interaction {
        range: SUN_DISC_RANGE,
        windup: SUN_DISC_CAST,
        clip: "Spell_P",
        effect: "SunDiscCast",
        targets: ruins,
        ready: sun_disc_ready,
        finish: summon_sun_disc,
    }),
    passive_cooldown: Some(SUN_DISC_COOLDOWN),
    movement: &[Slot::E],
    ..Kit::NONE
};

fn shifting(unit: &Unit) -> bool {
    matches!(
        unit.action,
        Action::Dash {
            kind: DashKind::Spell(Slot::E, _),
            ..
        }
    )
}

fn dash_cast(unit: &Unit, slot: Slot) -> bool {
    slot != Slot::E && shifting(unit)
}

fn start(game: &mut Game, id: UnitId, slot: Slot, target: Target, timing: (f32, f32), clip: &str) {
    if game.unit(id).is_some_and(shifting) {
        game.play(id, clip, 1.0);
        super::fire(game, id, slot, target);
    } else {
        begin(game, id, slot, target, timing.0, timing.1, clip);
    }
}

fn spawned(game: &mut Game, id: UnitId) {
    if let Some(unit) = game.unit_mut(id) {
        unit.abilities[slot_index(Slot::W)].charges = 2;
    }
}

fn ranked_up(game: &mut Game, id: UnitId, slot: Slot) {
    if slot != Slot::W {
        return;
    }
    let max = spell_of(game, id, Slot::W)
        .and_then(|spell| spell.max_ammo.first().copied())
        .unwrap_or(2);
    if let Some(unit) = game.unit_mut(id)
        && unit.abilities[slot_index(Slot::W)].rank == 1
    {
        unit.abilities[slot_index(Slot::W)].charges = max;
    }
}

fn recharge(game: &mut Game, id: UnitId) {
    recharge_charges(game, id, Slot::W);
}

pub(super) fn soldiers(game: &Game, owner: UnitId) -> Vec<UnitId> {
    game.units
        .iter()
        .filter(|unit| {
            unit.kind == UnitKind::Soldier
                && unit.owner == Some(owner)
                && unit.alive()
                && !unit.has_buff(EMPERORS_GUARD)
        })
        .map(|unit| unit.id)
        .collect()
}

fn cast(game: &mut Game, id: UnitId, slot: Slot, target: Target, spell: &Spell, rank: u8) {
    let point = Vec2::from_array(target.point);
    match slot {
        Slot::Q => {
            if soldiers(game, id).is_empty() {
                return;
            }
            spend(game, id, slot, spell.cooldown(rank), spell.mana(rank));
            start(game, id, slot, target, (0.25, 0.1), "Spell1");
            game.sound(id, "q");
        }
        Slot::W => {
            if let Some(unit) = game.unit_mut(id) {
                unit.mana -= spell.mana(rank);
                let ability = &mut unit.abilities[slot_index(Slot::W)];
                ability.charges -= 1;
                ability.cooldown = SOLDIER_LOCKOUT;
                ability.cooldown_total = SOLDIER_LOCKOUT;
            }
            activated(game, id, slot);
            start(game, id, slot, target, (0.25, 0.0), "Spell2");
            game.sound(id, "w");
        }
        Slot::E => {
            let position = game.unit(id).map_or(Vec2::ZERO, |unit| unit.position);
            let soldier = soldiers(game, id)
                .into_iter()
                .filter_map(|soldier| game.unit(soldier))
                .filter(|soldier| soldier.position.distance(position) <= 1100.0)
                .min_by(|a, b| {
                    a.position
                        .distance(point)
                        .total_cmp(&b.position.distance(point))
                })
                .map(|soldier| (soldier.id, soldier.position));
            let Some((soldier, destination)) = soldier else {
                return;
            };
            spend(game, id, slot, spell.cooldown(rank), spell.mana(rank));
            let shield = calculate(game, id, spell, rank, "TotalShield");
            game.shield(id, shield, spell.value("ShieldDuration", rank));
            game.effect("AzirEShield", position, Some(id));
            game.sound(id, "e_shield");
            game.sound(id, "e");
            let speed = spell.value("DashSpeed", rank).max(1700.0);
            dash(
                game,
                id,
                destination,
                speed,
                DashKind::Spell(Slot::E, Some(soldier)),
            );
            game.play(id, "Spell3", 1.0);
        }
        Slot::R => {
            spend(game, id, slot, spell.cooldown(rank), spell.mana(rank));
            start(game, id, slot, target, (CHARGE_DELAY, 0.25), "Spell4");
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
        facing,
        ..
    } = *cast;
    match cast.slot {
        Slot::Q => {
            let damage = calculate(game, id, spell, rank, "TotalDamage");
            let goal = clamp_point(position, point, spell.range(rank).max(740.0));
            let slow = -spell.value("SlowAmount", rank);
            let speed = spell.value("SoldierDashSpeed", rank);
            let list = soldiers(game, id);
            let count = list.len() as f32;
            let mut hit = Vec::new();
            game.effect("AzirQ", goal, None);
            for (index, soldier) in list.into_iter().enumerate() {
                let spread = (index as f32 - (count - 1.0) / 2.0) * 120.0;
                let to = goal + facing.perp() * spread;
                let from = game.unit(soldier).map_or(to, |soldier| soldier.position);
                for victim in enemies_on_segment(game, team, from, to, 80.0) {
                    if hit.contains(&victim) {
                        continue;
                    }
                    hit.push(victim);
                    game.damage(Some(id), victim, damage, DamageKind::Magic, false);
                    game.slow(victim, slow, 1.0, Some((id, Slot::Q)));
                    let at = game.unit(victim).map_or(to, |victim| victim.position);
                    game.effect("AzirQHit", at, Some(victim));
                }
                if let Some(soldier) = game.unit_mut(soldier) {
                    soldier.action = Action::Dash {
                        to,
                        speed,
                        kind: DashKind::Plain,
                    };
                }
                game.owned_effect("AzirQSoldier", id, from, Some(soldier));
                game.play(soldier, "Spell1", 1.0);
            }
        }
        Slot::W => {
            let at = game.walkable_point(clamp_point(position, point, SOLDIER_SPAWN_RANGE));
            let duration = named_spell(game, id, "AzirW")
                .map_or(10.0, |spell| spell.value("SoldierDuration", rank).max(10.0));
            spawn_soldier(game, id, team, at, duration);
        }
        Slot::R => emperors_divide(game, cast),
        _ => {}
    }
}

fn emperors_divide(game: &mut Game, cast: &Cast) {
    let Cast {
        id,
        spell,
        rank,
        position,
        team,
        facing,
        ..
    } = *cast;
    let index = usize::from(rank.clamp(1, 3) - 1);
    let count = spell.value("NumberOfSoldiers", rank).max(1.0) as usize;
    let charge_width = CHARGE_WIDTH[index];
    let wall_width = WALL_WIDTH[index];
    let damage = calculate(game, id, spell, rank, "TotalDamage");
    let side = facing.perp();
    let start = position - facing * SOLDIER_START;
    let end = start + facing * CHARGE_DISTANCE;
    let travel = CHARGE_DISTANCE / CHARGE_SPEED;
    let time = game.time;
    let base = game.unit_stats("AzirSoldier");
    let spacing = charge_width / count as f32;
    for slot in 0..count {
        let offset = (slot as f32 - (count as f32 - 1.0) / 2.0) * spacing;
        let soldier = game.allocate();
        let mut unit = Unit::new(
            soldier,
            UnitKind::Soldier,
            team,
            "AzirSoldier",
            base,
            start + side * offset,
        );
        unit.owner = Some(id);
        unit.targetable = false;
        unit.invulnerable = true;
        unit.expires = Some(time + travel + WALL_DURATION);
        unit.stats = stats::compute(&unit.base, 1, &Bonus::default());
        unit.health = 1.0;
        unit.facing = facing.x.atan2(facing.y);
        unit.action = Action::Dash {
            to: end + side * offset,
            speed: CHARGE_SPEED,
            kind: DashKind::Plain,
        };
        unit.buffs
            .push(game.buff(EMPERORS_GUARD, travel + WALL_DURATION));
        game.units.push(unit);
        game.play(soldier, "Dash2", 1.0);
        game.owned_effect("AzirRSoldier", id, start + side * offset, Some(soldier));
    }
    let victims: Vec<(UnitId, f32)> = game
        .units
        .iter()
        .filter(|other| {
            other.team != team
                && other.alive()
                && !other.untargetable()
                && !other.is_structure()
                && other.kind != UnitKind::Soldier
        })
        .filter_map(|other| {
            let along = (other.position - position).dot(facing);
            let across = (other.position - position).dot(side);
            ((-CATCH_BEHIND..=CHARGE_DISTANCE - SOLDIER_START).contains(&along)
                && across.abs() <= charge_width / 2.0 + other.radius())
            .then_some((other.id, across))
        })
        .collect();
    if !victims.is_empty() {
        game.sound(id, "r_hit");
    }
    for (victim, across) in victims {
        game.damage(Some(id), victim, damage, DamageKind::Magic, false);
        let at = game.unit(victim).map_or(end, |victim| victim.position);
        game.effect("AzirRHit", at, Some(victim));
        let to = position + facing * KNOCKBACK_LINE + side * across;
        game.knock_back(victim, to, at.distance(to) / KNOCKBACK_TIME);
    }
    game.barriers.push(super::Wall {
        team,
        from: end - side * wall_width / 2.0,
        to: end + side * wall_width / 2.0,
        armed: time + travel,
        expires: time + travel + WALL_DURATION,
    });
    game.sound(id, "r");
}

pub fn stops(barrier: &super::Wall, from: Vec2, to: Vec2, radius: f32) -> bool {
    let along = barrier.to - barrier.from;
    let length = along.length();
    if length <= 0.0 {
        return false;
    }
    let direction = along / length;
    let normal = direction.perp();
    let side = |point: Vec2| (point - barrier.from).dot(normal);
    let within = (-radius..=length + radius).contains(&(to - barrier.from).dot(direction));
    let (before, after) = (side(from), side(to));
    let reach = radius + WALL_THICKNESS;
    let crossed = (before > 0.0) != (after > 0.0);
    let entering = after.abs() < reach && after.abs() < before.abs();
    within && (crossed || entering)
}

fn spawn_soldier(game: &mut Game, owner: UnitId, team: Team, at: Vec2, duration: f32) {
    let base = game.unit_stats("AzirSoldier");
    let id = game.allocate();
    let mut soldier = Unit::new(id, UnitKind::Soldier, team, "AzirSoldier", base, at);
    soldier.owner = Some(owner);
    soldier.targetable = false;
    soldier.invulnerable = true;
    soldier.expires = Some(game.time + duration);
    soldier.stats = stats::compute(&soldier.base, 1, &Bonus::default());
    soldier.health = 1.0;
    soldier.facing = game
        .unit(owner)
        .map_or(0.0, |unit| facing_to(unit.position, at, unit.facing));
    game.units.push(soldier);
    game.play(id, "Spawn", 1.0);
    game.effect("AzirSoldierSpawn", at, Some(id));
    game.owned_effect("AzirSoldierGarb", owner, at, Some(id));
}

fn dissipate(game: &mut Game, soldier: UnitId) {
    if let Some(position) = game.unit(soldier).map(|unit| unit.position) {
        game.effect("AzirSoldierEnd", position, Some(soldier));
    }
}

fn expire_soldiers(game: &mut Game, id: UnitId) {
    let time = game.time;
    let expiring: Vec<(UnitId, Vec2, bool)> = game
        .units
        .iter()
        .filter(|unit| {
            unit.kind == UnitKind::Soldier
                && unit.owner == Some(id)
                && unit.alive()
                && unit.expires.is_some_and(|expires| expires <= time + TICK)
        })
        .map(|unit| (unit.id, unit.position, unit.has_buff(EMPERORS_GUARD)))
        .collect();
    for (soldier, position, guard) in expiring {
        if guard {
            game.owned_effect("AzirRSoldierEnd", id, position, Some(soldier));
        } else {
            dissipate(game, soldier);
        }
    }
}

fn decay_near_turrets(game: &mut Game, id: UnitId) {
    let turrets: Vec<(Team, Vec2, f32)> = game
        .units
        .iter()
        .filter(|unit| unit.kind == UnitKind::Turret && unit.alive())
        .map(|unit| {
            (
                unit.team,
                unit.position,
                unit.stats.attack_range + unit.radius(),
            )
        })
        .collect();
    for soldier in soldiers(game, id) {
        if let Some(unit) = game.unit_mut(soldier)
            && turrets.iter().any(|(team, position, range)| {
                *team != unit.team && unit.position.distance(*position) <= *range
            })
            && let Some(expires) = unit.expires.as_mut()
        {
            *expires -= TICK;
        }
    }
}

fn tether(game: &mut Game, id: UnitId) {
    let Some(position) = game.unit(id).map(|unit| unit.position) else {
        return;
    };
    for soldier in soldiers(game, id) {
        let Some(unit) = game.unit(soldier) else {
            continue;
        };
        let away = unit.position.distance(position) > SOLDIER_LEASH;
        if away == unit.has_buff(INACTIVE) {
            continue;
        }
        if away {
            let buff = game.buff(INACTIVE, f32::MAX);
            game.add_buff(soldier, buff);
            game.hold(soldier, "Inactive_Loop");
        } else {
            if let Some(unit) = game.unit_mut(soldier) {
                unit.buffs.retain(|buff| buff.name != INACTIVE);
            }
            game.release(soldier, "Inactive_Loop");
            game.play(soldier, "Reactivate", 1.0);
        }
    }
}

fn tick(game: &mut Game, id: UnitId) {
    tether(game, id);
    decay_near_turrets(game, id);
    expire_soldiers(game, id);
    shifting_sands(game, id);
}

fn shifting_sands(game: &mut Game, id: UnitId) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    let Action::Dash {
        speed,
        kind: kind @ DashKind::Spell(Slot::E, Some(soldier)),
        ..
    } = unit.action
    else {
        return;
    };
    let (position, team) = (unit.position, unit.team);
    let followed = game
        .unit(soldier)
        .filter(|soldier| soldier.alive())
        .map(|soldier| soldier.position);
    if let Some(to) = followed
        && let Some(unit) = game.unit_mut(id)
    {
        unit.action = Action::Dash { to, speed, kind };
    }
    let victims: Vec<UnitId> =
        game.enemies_near(team, position, SHIFTING_SANDS_RADIUS)
            .into_iter()
            .filter(|victim| {
                game.unit(*victim).is_some_and(|victim| {
                    !victim.is_structure()
                        && !victim.buffs.iter().any(|buff| {
                            buff.name == "shifting_sands_hit" && buff.source == Some(id)
                        })
                })
            })
            .collect();
    if victims.is_empty() {
        return;
    }
    let spell = spell_of(game, id, Slot::E).unwrap_or_default();
    let damage = calculate(game, id, &spell, rank(game, id, Slot::E), "TotalDamage");
    let mut champion = false;
    for victim in victims {
        let mut mark = game.buff("shifting_sands_hit", 1.0);
        mark.source = Some(id);
        game.add_buff(victim, mark);
        game.damage(Some(id), victim, damage, DamageKind::Magic, false);
        champion |= game
            .unit(victim)
            .is_some_and(|victim| victim.champion().is_some());
    }
    if !champion {
        return;
    }
    let w = spell_of(game, id, Slot::W).unwrap_or_default();
    let max = max_charges(&w, rank(game, id, Slot::W));
    if let Some(unit) = game.unit_mut(id) {
        unit.action = Action::None;
        let charges = &mut unit.abilities[slot_index(Slot::W)].charges;
        *charges = (*charges + 1).min(max);
    }
}

fn soldiers_in_reach(
    game: &Game,
    id: UnitId,
    (target, radius): (Vec2, f32),
) -> Vec<(UnitId, Vec2)> {
    let Some(position) = game.unit(id).map(|unit| unit.position) else {
        return Vec::new();
    };
    soldiers(game, id)
        .into_iter()
        .filter_map(|soldier| game.unit(soldier))
        .filter(|soldier| {
            soldier.position.distance(position) <= SOLDIER_LEASH
                && soldier.position.distance(target) <= SOLDIER_ATTACK_RANGE + radius
        })
        .map(|soldier| (soldier.id, soldier.position))
        .collect()
}

fn soldier_target(game: &Game, target: UnitId) -> Option<(Vec2, f32)> {
    game.unit(target)
        .filter(|unit| !unit.is_structure())
        .map(|unit| (unit.position, unit.radius()))
}

fn redirect_attack(game: &mut Game, id: UnitId, target: UnitId) -> bool {
    let Some(reach) = soldier_target(game, target) else {
        return false;
    };
    if soldiers_in_reach(game, id, reach).is_empty() {
        return false;
    }
    let Some(index) = game.index(id) else {
        return false;
    };
    game.start_attack(index, target);
    if let Some(animation) = game.unit_mut(id).and_then(|unit| unit.animation.as_mut()) {
        animation.clip = clip_id(COMMAND_CLIP);
    }
    true
}

fn launch_attack(game: &mut Game, id: UnitId, target: UnitId) -> bool {
    let Some(team) = game.unit(id).map(|unit| unit.team) else {
        return false;
    };
    let Some(reach) = soldier_target(game, target) else {
        return false;
    };
    let attackers = soldiers_in_reach(game, id, reach);
    if attackers.is_empty() {
        return false;
    }
    let spell = spell_of(game, id, Slot::W).unwrap_or_default();
    let level_rank = game
        .unit(id)
        .map_or(1, |unit| unit.ability(Slot::W).rank.max(1));
    let damage = calculate(game, id, &spell, level_rank, "TotalDamage");
    let secondary = calculate(game, id, &spell, level_rank, "SecondaryTargetDamageMod");
    let subsequent = spell.value("SubsequentDamageMod", level_rank) / 100.0;
    let (target_position, _) = reach;
    for (index, (soldier, from)) in attackers.into_iter().enumerate() {
        let direction = direction(from, target_position, 0.0);
        let scale = if index == 0 { 1.0 } else { subsequent };
        let length = from.distance(target_position) + SPEAR_OVERREACH;
        for victim in enemies_on_segment(game, team, from, from + direction * length, 40.0) {
            let share = if victim == target { 1.0 } else { secondary };
            game.damage(
                Some(id),
                victim,
                damage * scale * share,
                DamageKind::Magic,
                false,
            );
        }
        if let Some(soldier) = game.unit_mut(soldier) {
            soldier.facing = direction.x.atan2(direction.y);
        }
        game.play(soldier, "Attack1", 1.0);
    }
    items::on_attack_hit(game, id, target);
    true
}

fn ruins(game: &Game) -> Vec<UnitId> {
    game.units
        .iter()
        .filter(|ruin| {
            ruin.kind == UnitKind::Turret
                && ruin.dead
                && matches!(ruin.tier, Some(Tier::Outer | Tier::Inner))
                && !game.units.iter().any(|disc| {
                    disc.kind == UnitKind::SunDisc
                        && disc.alive()
                        && disc.position.distance(ruin.position) < 1.0
                })
        })
        .map(|ruin| ruin.id)
        .collect()
}

fn sun_disc_ready(game: &Game, id: UnitId) -> bool {
    game.unit(id).is_some_and(|emperor| {
        emperor.alive()
            && !emperor.is_stunned()
            && !emperor.is_silenced()
            && !emperor.has_buff(SUN_DISC_COOLDOWN)
    })
}

fn summon_sun_disc(game: &mut Game, id: UnitId, ruin: UnitId) {
    let (Some(team), Some(site)) = (
        game.unit(id).map(|emperor| emperor.team),
        game.unit(ruin).map(|ruin| ruin.position),
    ) else {
        return;
    };
    let cooldown = named_spell(game, id, "AzirPassive").map_or(90.0, |spell| spell.cooldown(1));
    let cooldown = game.buff(SUN_DISC_COOLDOWN, cooldown);
    game.add_buff(id, cooldown);
    let mut base = game.unit_stats("AzirSunDisc");
    base.health = SUN_DISC_HEALTH;
    let disc = game.allocate();
    let mut unit = Unit::new(disc, UnitKind::SunDisc, team, "AzirSunDisc", base, site);
    unit.owner = Some(id);
    unit.stats = stats::compute(&unit.base, 1, &Bonus::default());
    unit.health = unit.stats.max_health;
    game.units.push(unit);
    for name in [ASSEMBLING, "disarm"] {
        let buff = game.buff(name, SUN_DISC_ASSEMBLY);
        game.add_buff(disc, buff);
    }
    sun_disc(game, disc);
    game.play(disc, "Spawn", 1.0);
    game.effect("AzirSunDiscSpawn", site, Some(disc));
    game.sound(id, "passive");
}

pub fn sun_disc(game: &mut Game, disc: UnitId) {
    let Some(unit) = game.unit(disc) else {
        return;
    };
    let assembling = unit.has_buff(ASSEMBLING);
    let position = unit.position;
    let owner = unit.owner.and_then(|owner| game.unit(owner));
    let tethered = owner.is_some_and(|emperor| {
        emperor.alive() && emperor.position.distance(position) <= SUN_DISC_TETHER
    });
    let spell = unit
        .owner
        .and_then(|owner| named_spell(game, owner, "AzirPassive"))
        .unwrap_or_default();
    let calculation = |name| {
        unit.owner
            .map_or(0.0, |owner| calculate(game, owner, &spell, 1, name))
    };
    let damage = calculation("TowerDamage");
    let resists = calculation("BonusResists") - if tethered { 0.0 } else { SUN_DISC_RESIST_LOSS };
    let decay = unit.stats.max_health / spell.value("TowerDisintegrationTime", 1).max(1.0) * TICK;
    let Some(unit) = game.unit_mut(disc) else {
        return;
    };
    unit.base.attack_damage = damage;
    unit.base.armor = resists.max(0.0);
    unit.base.magic_resist = resists.max(0.0);
    if !assembling {
        unit.health -= decay;
    }
    let decayed = unit.health <= 0.0;
    game.refresh_stats(disc);
    if decayed {
        game.kill(disc, None);
    }
}
