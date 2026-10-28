use asset::clip_id;
use asset::data::Spell;
use protocol::{DamageKind, Slot, Target, UnitId};

use super::{
    Cast, Kit, Zone, approach, begin, calculate, cast_range, clamp_point, named_spell, rank,
    spell_of, spend, target_in_range,
};
use crate::stats::Bonus;
use crate::unit::Unit;
use crate::{Acceleration, Game, Hit, Missile, Motion};

const BALEFUL_TARGETS: usize = 2;
const DARK_MATTER: &str = "dark_matter";
const DARK_MATTER_WINDUP: f32 = 0.25;
const EVENT_HORIZON: &str = "event_horizon";
const CAGE_DURATION: f32 = 3.0;
const CAGE_EDGE: f32 = 40.0;
const EXECUTE_THRESHOLD: f32 = 2.0 / 3.0;

pub const KIT: Kit = Kit {
    cast,
    fire,
    range,
    missile_hit,
    on_takedown,
    tick,
    zone,
    ..Kit::NONE
};

fn range(_: &Game, unit: &Unit, slot: Slot, base: f32) -> f32 {
    match slot {
        Slot::R => base + unit.radius(),
        _ => base,
    }
}

fn cast(game: &mut Game, id: UnitId, slot: Slot, target: Target, spell: &Spell, rank: u8) {
    match slot {
        Slot::Q => {
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
        }
        Slot::W => {
            let cooldown = dark_matter_cooldown(game, id, spell, rank);
            spend(game, id, slot, cooldown, spell.mana(rank));
            begin(game, id, slot, target, DARK_MATTER_WINDUP, 0.1, "Spell2");
            game.sound(id, "w");
        }
        Slot::E => {
            spend(game, id, slot, spell.cooldown(rank), spell.mana(rank));
            begin(game, id, slot, target, 0.25, 0.1, "Spell3");
            game.sound(id, "e");
        }
        Slot::R => {
            let Some(unit) = game.unit(id) else {
                return;
            };
            if !target
                .unit
                .and_then(|other| game.unit(other))
                .is_some_and(|other| other.champion().is_some())
            {
                return;
            }
            let range = cast_range(game, unit, slot);
            if target_in_range(game, id, &target, range, true).is_none() {
                if target.unit.is_some() {
                    approach(game, id, slot, target);
                }
                return;
            }
            spend(game, id, slot, spell.cooldown(rank), spell.mana(rank));
            begin(game, id, slot, target, 0.25, 0.1, "Spell4");
            game.sound(id, "r");
        }
        _ => {}
    }
}

fn dark_matter_cooldown(game: &Game, id: UnitId, spell: &Spell, rank: u8) -> f32 {
    let passive = named_spell(game, id, "VeigarPassive").unwrap_or_default();
    let stacks = game.unit(id).map_or(0.0, |unit| unit.counter);
    let per_step = passive.value("PStacksPerDarkMatterCDR", 1).max(1.0);
    let step = passive.value("DarkMatterCDRIncrement", 1);
    spell.value("BaseCooldown", rank) * (1.0 - step).powf((stacks / per_step).floor())
}

fn dark_matter(game: &mut Game, cast: &Cast) {
    let Cast {
        id,
        spell,
        rank,
        position,
        team,
        point,
        ..
    } = *cast;
    let time = game.time;
    let center = clamp_point(position, point, spell.range(rank));
    let delay = (spell.value("ImpactDelay", rank) - DARK_MATTER_WINDUP).max(0.0);
    game.zones.push(Zone {
        owner: id,
        team,
        name: DARK_MATTER,
        center,
        radius: Spell::by_rank(&spell.cast_radius, rank),
        armed: time + delay,
        expires: time + delay + 1.0,
        next_tick: time + delay,
        damage: calculate(game, id, spell, rank, "TotalDamage"),
        value: 0.0,
    });
    game.lasting_effect("VeigarW", id, center, time + delay);
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
            let damage = calculate(game, id, spell, rank, "TotalDamage");
            let missile = game.allocate();
            game.missiles.push(Missile {
                id: missile,
                owner: id,
                team,
                kind: clip_id("VeigarBalefulStrikeMis"),
                position,
                height: 100.0,
                speed: 2200.0,
                width: spell.line_width.max(70.0),
                motion: Motion::Line {
                    direction: facing,
                    remaining: spell.range(rank),
                    limit: Some(BALEFUL_TARGETS),
                },
                hit: Hit::Spell(Slot::Q, damage),
                hit_units: Vec::new(),
                acceleration: None,
            });
        }
        Slot::W => dark_matter(game, cast),
        Slot::E => {
            let center = clamp_point(position, point, spell.range(rank));
            let delay = spell.value("CageDelay", rank).max(0.5);
            game.zones.push(Zone {
                owner: id,
                team,
                name: EVENT_HORIZON,
                center,
                radius: Spell::by_rank(&spell.cast_radius, rank).max(375.0),
                armed: time + delay,
                expires: time + delay + CAGE_DURATION,
                next_tick: time + delay,
                damage: 0.0,
                value: spell.value("StunDuration", rank),
            });
            game.lasting_effect("VeigarE", id, center, time + delay + CAGE_DURATION);
        }
        Slot::R => {
            let Some(target) = cast.target.unit else {
                return;
            };
            let damage = calculate(game, id, spell, rank, "MinDamage");
            let missile = game.allocate();
            game.missiles.push(Missile {
                id: missile,
                owner: id,
                team,
                kind: clip_id("VeigarR"),
                position,
                height: 100.0,
                speed: spell.missile_speed,
                width: 0.0,
                motion: Motion::Homing(target),
                hit: Hit::Spell(Slot::R, damage),
                hit_units: Vec::new(),
                acceleration: Some(Acceleration {
                    rate: spell.missile_acceleration,
                    max_speed: spell.missile_max_speed,
                }),
            });
        }
        _ => {}
    }
}

fn zone(game: &mut Game, index: usize) {
    let zone = game.zones[index];
    if game.time < zone.armed {
        return;
    }
    match zone.name {
        DARK_MATTER => {
            game.zones[index].expires = game.time;
            for victim in game.enemies_near(zone.team, zone.center, zone.radius) {
                if !game.unit(victim).is_some_and(Unit::is_structure) {
                    strike(game, zone.owner, victim, zone.damage);
                }
            }
            game.effect("VeigarWBlast", zone.center, Some(zone.owner));
            game.sound(zone.owner, "w_hit");
        }
        EVENT_HORIZON => {
            let caged: Vec<UnitId> = game
                .units
                .iter()
                .filter(|unit| {
                    unit.team != zone.team
                        && unit.alive()
                        && !unit.untargetable()
                        && !unit.is_structure()
                        && !unit.buffs.iter().any(|buff| {
                            buff.name == EVENT_HORIZON && buff.source == Some(zone.owner)
                        })
                        && (unit.position.distance(zone.center) - zone.radius).abs()
                            <= unit.radius() + CAGE_EDGE
                })
                .map(|unit| unit.id)
                .collect();
            for victim in caged {
                game.stun(victim, zone.value, Some((zone.owner, Slot::E)));
                let mut mark = game.buff(EVENT_HORIZON, zone.expires - game.time);
                mark.source = Some(zone.owner);
                game.add_buff(victim, mark);
                game.sound(zone.owner, "e_hit");
                if game
                    .unit(victim)
                    .is_some_and(|unit| unit.champion().is_some())
                {
                    empower(game, zone.owner, ability_stacks(game, zone.owner));
                }
            }
        }
        _ => {}
    }
}

fn missile_hit(game: &mut Game, missile: &Missile, target: UnitId, slot: Slot, damage: f32) {
    let owner = missile.owner;
    match slot {
        Slot::Q => {
            strike(game, owner, target, damage);
            let Some(victim) = game.unit(target).filter(|unit| unit.dead) else {
                return;
            };
            let large = victim.is_large_minion();
            let passive = named_spell(game, owner, "VeigarPassive").unwrap_or_default();
            let stacks = if large {
                passive.value("dQKillStacksLarge", 1)
            } else {
                passive.value("dQKillStacks", 1)
            };
            empower(game, owner, stacks);
            if let Some(position) = game.unit(owner).map(|unit| unit.position) {
                game.effect("VeigarQPowerup", position, Some(owner));
            }
        }
        Slot::R => {
            let missing = game.unit(target).map_or(0.0, |unit| {
                1.0 - unit.health / unit.stats.max_health.max(1.0)
            });
            let spell = spell_of(game, owner, Slot::R).unwrap_or_default();
            let most = spell
                .value("MaxExecuteMult", rank(game, owner, Slot::R))
                .max(2.0);
            let multiplier = 1.0 + (most - 1.0) * (missing / EXECUTE_THRESHOLD).min(1.0);
            let executed = multiplier >= most;
            strike(game, owner, target, damage * multiplier);
            let at = game
                .unit(target)
                .map_or(missile.position, |unit| unit.position);
            game.effect(
                if executed { "VeigarRMax" } else { "VeigarRHit" },
                at,
                Some(target),
            );
        }
        _ => {}
    }
}

fn strike(game: &mut Game, owner: UnitId, target: UnitId, damage: f32) {
    game.damage(Some(owner), target, damage, DamageKind::Magic, false);
    if game
        .unit(target)
        .is_some_and(|unit| unit.champion().is_some())
    {
        empower(game, owner, ability_stacks(game, owner));
    }
}

fn ability_stacks(game: &Game, id: UnitId) -> f32 {
    named_spell(game, id, "VeigarPassive")
        .unwrap_or_default()
        .value("dAbilityStacks", 1)
        .max(1.0)
}

fn on_takedown(game: &mut Game, champion: UnitId, _victim: UnitId) {
    let stacks = named_spell(game, champion, "VeigarPassive")
        .unwrap_or_default()
        .value("dTakedownStacks", 1);
    empower(game, champion, stacks);
}

fn empower(game: &mut Game, id: UnitId, stacks: f32) {
    if let Some(unit) = game.unit_mut(id) {
        unit.counter += stacks;
    }
}

fn tick(game: &mut Game, id: UnitId) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    let stacks = unit.counter;
    if unit
        .buff("phenomenal_evil")
        .is_some_and(|buff| buff.bonus.ability_power == stacks)
    {
        return;
    }
    let mut buff = game.buff("phenomenal_evil", f32::MAX);
    buff.stacks = stacks as u32;
    buff.bonus = Bonus {
        ability_power: stacks,
        ..Bonus::default()
    };
    game.add_buff(id, buff);
}
