use asset::clip_id;
use asset::data::Spell;
use glam::Vec2;
use protocol::{DamageKind, Slot, TICK, Target, Team, UnitId, UnitKind};

use super::{
    Cast, Kit, Zone, activated, approach, begin, begin_channel, calculate, cast_range, clamp_point,
    enemies_on_segment, glow, named_spell, rank, recharge_charges, spell_of, spend,
    target_in_range,
};
use crate::stats::{self, Bonus};
use crate::unit::{Action, Unit, slot_index};
use crate::{Game, Hit, Missile, Motion, Prop};

const PASSIVE: &str = "JhinPassive";
const AMMO: &str = "whisper_ammo";
const RELOADING: &str = "reloading";
const TOP_UP: &str = "whisper_reload";
const IN_COMBAT: &str = "whisper_combat";
const FOURTH_SHOT: &str = "whisper_fourth";
const FOURTH_READY: &str = "whisper_fourth_ready";
const HASTE: &str = "every_moment_matters";
const CAUGHT_OUT: &str = "caught_out";
const LOTUS_HIT: &str = "lotus_hit";
const LOTUS_TRAP: &str = "lotus_trap";
const CURTAIN_CALL: &str = "curtain_call";
const CRIT_HASTE: f32 = 0.14;
const BONUS_AS_TO_AD: f32 = 0.3;
const FLOURISH_RANGE: f32 = 2520.0;
const FLOURISH_WIDTH: f32 = 40.0;
const TRAP_LIFETIME: f32 = 180.0;
const TRAP_SLOW_REFRESH: f32 = 0.25;
const CURTAIN_UNLOCK: f32 = 0.2;
const CURTAIN_SHOTS: u32 = 4;
const CURTAIN_HALF_CONE: f32 = 30.0;
const CURTAIN_RANGE: f32 = 3500.0;
const CURTAIN_SPEED: f32 = 5000.0;
const CURTAIN_WIDTH: f32 = 80.0;
const CURTAIN_START: f32 = 1.33;
const CURTAIN_SHOT_CLIP: f32 = 1.03;

pub const KIT: Kit = Kit {
    spawned,
    respawned: spawned,
    died,
    ranked_up,
    recharge,
    ammo: Some(Slot::E),
    recast,
    recasting,
    cast,
    fire,
    missile_hit,
    attack_clip,
    launch_attack,
    modify_attack,
    on_crit,
    critical,
    bonus,
    stops_missile,
    on_kill,
    tick,
    zone,
    silent_hits: &[Slot::R],
    ..Kit::NONE
};

fn passive(game: &Game, id: UnitId) -> Spell {
    named_spell(game, id, PASSIVE).unwrap_or_default()
}

fn ammo(unit: &Unit) -> u32 {
    unit.buff(AMMO).map_or(0, |buff| buff.stacks)
}

fn set_ammo(game: &mut Game, id: UnitId, shots: u32) {
    let mut buff = game.buff(AMMO, f32::MAX);
    buff.stacks = shots;
    game.add_buff(id, buff);
}

fn spawned(game: &mut Game, id: UnitId) {
    let shots = passive(game, id).value("MaxAmmo", 1) as u32;
    set_ammo(game, id, shots);
    if let Some(unit) = game.unit_mut(id) {
        unit.buffs
            .retain(|buff| !matches!(buff.name, RELOADING | TOP_UP | FOURTH_SHOT));
    }
}

fn ranked_up(game: &mut Game, id: UnitId, slot: Slot) {
    if slot != Slot::E {
        return;
    }
    let max = spell_of(game, id, Slot::E)
        .and_then(|spell| spell.max_ammo.first().copied())
        .unwrap_or(2);
    if let Some(unit) = game.unit_mut(id)
        && unit.abilities[slot_index(Slot::E)].rank == 1
    {
        unit.abilities[slot_index(Slot::E)].charges = max;
    }
}

fn recharge(game: &mut Game, id: UnitId) {
    recharge_charges(game, id, Slot::E);
}

fn critical(unit: &Unit) -> bool {
    ammo(unit) == 1
}

fn attack_clip(unit: &Unit) -> Option<&'static str> {
    critical(unit).then_some("Attack4")
}

fn engage(game: &mut Game, id: UnitId) {
    let window = passive(game, id).value("OutOfCombatTimeBeforeReload", 1);
    let combat = game.buff(IN_COMBAT, window);
    game.add_buff(id, combat);
    if let Some(unit) = game.unit_mut(id) {
        unit.buffs.retain(|buff| buff.name != TOP_UP);
    }
}

fn launch_attack(game: &mut Game, id: UnitId, target: UnitId) -> bool {
    let Some(unit) = game.unit(id) else {
        return false;
    };
    let (shots, position, team) = (ammo(unit), unit.position, unit.team);
    engage(game, id);
    let left = shots.saturating_sub(1);
    set_ammo(game, id, left);
    if left > 0 {
        return false;
    }
    let reload = passive(game, id).value("ReloadTime", 1);
    let reloading = game.buff(RELOADING, reload);
    game.add_buff(id, reloading);
    game.sound(id, "fourth");
    let marker = game.buff(FOURTH_SHOT, f32::MAX);
    game.add_buff(id, marker);
    let speed =
        named_spell(game, id, "JhinPassiveAttack").map_or(3000.0, |spell| spell.missile_speed);
    let missile = game.allocate();
    game.missiles.push(Missile {
        id: missile,
        owner: id,
        team,
        kind: clip_id("JhinPassiveAttack"),
        position,
        height: 100.0,
        speed,
        width: 0.0,
        motion: Motion::Homing(target),
        hit: Hit::Attack { critical: true },
        hit_units: Vec::new(),
        acceleration: None,
    });
    true
}

fn modify_attack(game: &mut Game, attacker: UnitId, target: UnitId, amount: f32) -> f32 {
    if !game
        .unit(attacker)
        .is_some_and(|unit| unit.has_buff(FOURTH_SHOT))
    {
        return amount;
    }
    if let Some(unit) = game.unit_mut(attacker) {
        unit.buffs.retain(|buff| buff.name != FOURTH_SHOT);
    }
    let execute = calculate(
        game,
        attacker,
        &passive(game, attacker),
        1,
        "FourthShotExecutePercent",
    );
    let missing = game
        .unit(target)
        .map_or(0.0, |unit| (unit.stats.max_health - unit.health).max(0.0));
    amount + execute * missing
}

fn hasten(game: &mut Game, id: UnitId) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    let passive = passive(game, id);
    let bonus_speed = game.raw_bonus(unit).attack_speed;
    let percent = CRIT_HASTE + passive.value("CritMoveSpeedPercentASRatio", 1) * bonus_speed;
    let mut haste = game.buff(HASTE, passive.value("HasteDuration", 1));
    haste.bonus.move_speed_percent = percent;
    game.add_buff(id, haste);
    let position = position_of(game, id);
    game.effect("JhinHaste", position, Some(id));
    game.sound(id, "haste");
}

fn on_crit(game: &mut Game, attacker: UnitId, target: UnitId) {
    let enemy = game
        .unit(target)
        .zip(game.unit(attacker))
        .is_some_and(|(target, attacker)| target.team != attacker.team);
    if enemy {
        hasten(game, attacker);
    }
}

fn bonus(game: &Game, unit: &Unit, bonus: Bonus) -> Bonus {
    let passive = passive(game, unit.id);
    let percent = calculate(game, unit.id, &passive, 1, "TotalADPercent")
        + BONUS_AS_TO_AD * bonus.attack_speed;
    let crit_damage = unit.base.crit_damage + bonus.crit_damage;
    Bonus {
        attack_speed: passive.value("PercentAttackSpeedPerLevel", 1) * stats::growth(unit.level),
        cripple: 0.0,
        attack_damage: bonus.attack_damage
            + percent * (unit.stats.base_attack_damage + bonus.attack_damage),
        crit_damage: bonus.crit_damage - passive.value("CritReductionPercent", 1) * crit_damage,
        ..bonus
    }
}

fn reload(game: &mut Game, id: UnitId) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    let shots = ammo(unit);
    let reloading = unit.has_buff(RELOADING);
    let topping = unit.buff(TOP_UP).map(|buff| game.time - buff.value);
    let idle = !unit.has_buff(IN_COMBAT) && !unit.is_stunned() && !unit.is_silenced();
    let passive = passive(game, id);
    let max = passive.value("MaxAmmo", 1) as u32;
    let time = passive.value("ReloadTime", 1);
    if shots == 0 && !reloading {
        set_ammo(game, id, max);
        return;
    }
    match topping {
        Some(elapsed) if elapsed >= time => {
            if let Some(unit) = game.unit_mut(id) {
                unit.buffs.retain(|buff| buff.name != TOP_UP);
            }
            set_ammo(game, id, max);
        }
        None if shots < max && shots > 0 && idle && unit.alive() => {
            let mut buff = game.buff(TOP_UP, f32::MAX);
            buff.value = game.time;
            game.add_buff(id, buff);
            game.play(id, "Reload", 1.0);
            game.sound(id, "reload");
        }
        _ => {}
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
        }
        Slot::W => {
            spend(game, id, slot, spell.cooldown(rank), spell.mana(rank));
            begin(
                game,
                id,
                slot,
                target,
                spell.cast_time.max(0.75),
                0.1,
                "Spell2",
            );
            let position = position_of(game, id);
            game.effect("JhinWCharge", position, Some(id));
        }
        Slot::E => {
            if let Some(unit) = game.unit_mut(id) {
                unit.mana -= spell.mana(rank);
                let cooldown = stats::cooldown(spell.cooldown(rank), unit.stats.ability_haste);
                let ability = &mut unit.abilities[slot_index(Slot::E)];
                ability.charges -= 1;
                ability.cooldown = cooldown;
                ability.cooldown_total = cooldown;
            }
            activated(game, id, slot);
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
        Slot::R => curtain_call(game, id, target, spell, rank),
        _ => {}
    }
    engage(game, id);
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
            let Some(target) = cast.target.unit.filter(|target| {
                game.unit(*target)
                    .is_some_and(|unit| unit.alive() && !unit.untargetable())
            }) else {
                return;
            };
            let damage = calculate(game, id, spell, rank, "TotalDamage");
            grenade(game, id, "JhinQ", position, target, damage, Vec::new());
            game.sound(id, "q");
        }
        Slot::W => deadly_flourish(game, id, spell, rank, position, facing),
        Slot::E => {
            let center = clamp_point(position, point, spell.range(rank));
            let throw = named_spell(game, id, "JhinETrap").unwrap_or_default();
            let landing = position.distance(center) / throw.missile_speed.max(1.0);
            let time = game.time;
            game.zones.push(Zone {
                owner: id,
                team,
                name: LOTUS_TRAP,
                center,
                radius: spell.value("TrapAoERadius", rank),
                armed: time + landing + spell.value("TrapArmTime", rank),
                expires: time + landing + TRAP_LIFETIME,
                next_tick: time + landing,
                damage: calculate(game, id, spell, rank, "TotalDamage"),
                value: 0.0,
            });
            let missile = game.allocate();
            game.missiles.push(Missile {
                id: missile,
                owner: id,
                team,
                kind: clip_id("JhinETrap"),
                position,
                height: 100.0,
                speed: throw.missile_speed.max(1.0),
                width: 0.0,
                motion: Motion::Point(center),
                hit: Hit::Visual,
                hit_units: Vec::new(),
                acceleration: None,
            });
            game.sound(id, "e");
        }
        Slot::R => end_curtain(game, id),
        _ => {}
    }
}

fn grenade(
    game: &mut Game,
    owner: UnitId,
    kind: &str,
    from: Vec2,
    target: UnitId,
    damage: f32,
    hit: Vec<UnitId>,
) {
    let team = game.unit(owner).map_or(Team::Blue, |unit| unit.team);
    let speed = named_spell(game, owner, kind).map_or(1800.0, |spell| spell.missile_speed);
    let missile = game.allocate();
    game.missiles.push(Missile {
        id: missile,
        owner,
        team,
        kind: clip_id(kind),
        position: from,
        height: 100.0,
        speed,
        width: 0.0,
        motion: Motion::Homing(target),
        hit: Hit::Spell(Slot::Q, damage),
        hit_units: hit,
        acceleration: None,
    });
}

fn bounce(game: &mut Game, missile: &Missile, target: UnitId, damage: f32) {
    let owner = missile.owner;
    let spell = spell_of(game, owner, Slot::Q).unwrap_or_default();
    let rank = rank(game, owner, Slot::Q);
    let killed = game.unit(target).is_some_and(|unit| unit.dead);
    let at = game
        .unit(target)
        .map_or(missile.position, |unit| unit.position);
    let mut hit = missile.hit_units.clone();
    hit.push(target);
    game.effect(
        if hit.len() == 1 {
            "JhinQHit"
        } else {
            "JhinQBounceHit"
        },
        at,
        Some(target),
    );
    if hit.len() > 1 {
        game.sound_on(owner, target, "q_bounce");
    }
    if hit.len() > spell.value("NumberOfBounces", rank) as usize {
        return;
    }
    let damage = if killed {
        game.effect("JhinQKill", at, Some(target));
        damage * (1.0 + spell.value("PercentAmpOnKill", rank))
    } else {
        damage
    };
    let range = spell.value("BounceRange", rank);
    let team = missile.team;
    let next = game
        .units
        .iter()
        .filter(|unit| {
            unit.team != team
                && unit.alive()
                && !unit.untargetable()
                && !unit.is_structure()
                && unit.kind != UnitKind::Soldier
                && !hit.contains(&unit.id)
                && unit.position.distance(at) <= range + unit.radius()
        })
        .min_by(|a, b| a.position.distance(at).total_cmp(&b.position.distance(at)))
        .map(|unit| unit.id);
    if let Some(next) = next {
        grenade(game, owner, "JhinQMisBounce", at, next, damage, hit);
    }
}

fn caught_out(game: &Game, sniper: UnitId, target: UnitId) -> bool {
    let Some(unit) = game.unit(target) else {
        return false;
    };
    let Some(team) = game.unit(sniper).map(|unit| unit.team) else {
        return false;
    };
    let window = spell_of(game, sniper, Slot::W)
        .unwrap_or_default()
        .value("SpottingDuration", 1);
    unit.has_buff(CAUGHT_OUT)
        || unit.last_hit_by.iter().any(|(by, at)| {
            game.time - at <= window
                && game
                    .unit(*by)
                    .is_some_and(|by| by.team == team && by.champion().is_some())
        })
}

fn deadly_flourish(game: &mut Game, id: UnitId, spell: &Spell, rank: u8, from: Vec2, facing: Vec2) {
    let team = game.unit(id).map_or(Team::Blue, |unit| unit.team);
    let end = from + facing * FLOURISH_RANGE;
    let mut hits: Vec<(f32, UnitId)> = enemies_on_segment(game, team, from, end, FLOURISH_WIDTH)
        .into_iter()
        .filter_map(|victim| {
            game.unit(victim)
                .map(|unit| (unit.position.distance(from), victim))
        })
        .collect();
    hits.sort_by(|a, b| a.0.total_cmp(&b.0));
    let damage = calculate(game, id, spell, rank, "TotalDamage");
    let mut stop = end;
    for (_, victim) in hits {
        let Some(unit) = game.unit(victim) else {
            continue;
        };
        let (champion, at) = (unit.champion().is_some(), unit.position);
        let marked = champion && caught_out(game, id, victim);
        let amount = if champion {
            damage
        } else {
            damage * spell.value("MinionMod", rank)
        };
        game.damage(Some(id), victim, amount, DamageKind::Physical, false);
        game.effect(
            if champion {
                "JhinWHit"
            } else {
                "JhinWMinionHit"
            },
            at,
            Some(victim),
        );
        if champion {
            if marked {
                game.root(
                    victim,
                    spell.value("RootDuration", rank),
                    Some((id, Slot::W)),
                );
                game.effect("JhinRoot", at, Some(victim));
                hasten(game, id);
            }
            stop = at;
            break;
        }
    }
    game.effect("JhinWBeam", stop, Some(id));
    game.sound(id, "w");
}

fn zone(game: &mut Game, index: usize) {
    let zone = game.zones[index];
    if zone.name != LOTUS_TRAP {
        return;
    }
    let time = game.time;
    let spell = spell_of(game, zone.owner, Slot::E).unwrap_or_default();
    let rank = rank(game, zone.owner, Slot::E).max(1);
    match zone.value as u8 {
        0 if time >= zone.next_tick => {
            let id = game.allocate();
            game.props.push(Prop {
                id,
                owner: zone.owner,
                team: zone.team,
                effect: "JhinTrap",
                position: zone.center,
                facing: 0.0,
                hidden: true,
                expires: f32::MAX,
            });
            game.zones[index].value = 1.0;
        }
        1 if time + TICK >= zone.expires => remove_trap(game, &zone),
        1 if time >= zone.armed => {
            let trigger = spell.value("TrapTriggerRadius", rank);
            let stepped: Vec<UnitId> = game
                .enemies_near(zone.team, zone.center, trigger)
                .into_iter()
                .filter(|victim| {
                    game.unit(*victim)
                        .is_some_and(|unit| unit.champion().is_some())
                })
                .collect();
            if stepped.is_empty() {
                return;
            }
            for victim in stepped {
                let mark = game.buff(CAUGHT_OUT, spell.value("TrapRevealDuration", rank));
                game.add_buff(victim, mark);
            }
            remove_trap(game, &zone);
            bloom(game, index, &spell, rank, "JhinTrapBloom");
        }
        2 if time >= zone.next_tick => detonate(game, index, &spell, rank),
        2 => {
            for victim in game.enemies_near(zone.team, zone.center, zone.radius) {
                if !game.unit(victim).is_some_and(Unit::is_structure) {
                    game.slow(
                        victim,
                        spell.value("TrapSlowAmount", rank),
                        TRAP_SLOW_REFRESH,
                        Some((zone.owner, Slot::E)),
                    );
                }
            }
        }
        _ => {}
    }
}

fn remove_trap(game: &mut Game, zone: &Zone) {
    game.props
        .retain(|prop| !(prop.owner == zone.owner && prop.position == zone.center));
}

fn bloom(game: &mut Game, index: usize, spell: &Spell, rank: u8, effect: &str) {
    let time = game.time;
    let zone = &mut game.zones[index];
    zone.value = 2.0;
    zone.next_tick = time + spell.value("TrapDetonationTime", rank);
    zone.expires = zone.next_tick + 1.0;
    let (owner, center) = (zone.owner, zone.center);
    game.owned_effect(effect, owner, center, None);
    game.sound(owner, "e_hit");
}

fn detonate(game: &mut Game, index: usize, spell: &Spell, rank: u8) {
    let zone = game.zones[index];
    game.zones[index].expires = game.time;
    let reduced = spell.value("ReducedDamagePercent", rank);
    for victim in game.enemies_near(zone.team, zone.center, zone.radius) {
        let Some(unit) = game.unit(victim).filter(|unit| !unit.is_structure()) else {
            continue;
        };
        let lesser = unit.minion().is_some() || unit.has_buff(LOTUS_HIT);
        let at = unit.position;
        let amount = if lesser {
            zone.damage * reduced
        } else {
            zone.damage
        };
        game.damage(Some(zone.owner), victim, amount, DamageKind::Magic, false);
        let hit = game.buff(LOTUS_HIT, spell.value("TrapDetonationTime", rank));
        game.add_buff(victim, hit);
        game.effect("JhinTrapHit", at, Some(victim));
    }
    game.effect("JhinTrapBoom", zone.center, Some(zone.owner));
}

fn on_kill(game: &mut Game, killer: UnitId, victim: UnitId) {
    let Some(unit) = game.unit(victim).filter(|unit| unit.champion().is_some()) else {
        return;
    };
    let (center, time) = (unit.position, game.time);
    let Some(team) = game.unit(killer).map(|unit| unit.team) else {
        return;
    };
    let spell = spell_of(game, killer, Slot::E).unwrap_or_default();
    let rank = rank(game, killer, Slot::E);
    if rank == 0 {
        return;
    }
    game.zones.push(Zone {
        owner: killer,
        team,
        name: LOTUS_TRAP,
        center,
        radius: spell.value("TrapAoERadius", rank),
        armed: time,
        expires: time + TRAP_LIFETIME,
        next_tick: time,
        damage: calculate(game, killer, &spell, rank, "TotalDamage"),
        value: 2.0,
    });
    let index = game.zones.len() - 1;
    bloom(game, index, &spell, rank, "JhinTrapBloomKill");
}

fn curtain_call(game: &mut Game, id: UnitId, target: Target, spell: &Spell, rank: u8) {
    spend(game, id, Slot::R, 0.0, spell.mana(rank));
    let mut buff = game.buff(CURTAIN_CALL, f32::MAX);
    buff.stacks = CURTAIN_SHOTS;
    buff.value = f32::MIN;
    game.add_buff(id, buff);
    begin_channel(game, id, Slot::R, target, 10.0);
    game.play(id, "Spell4", 1.0);
    let facing = game.unit(id).map_or(0.0, |unit| unit.facing);
    let mut cone = game.buff("curtain_call_cone", f32::MAX);
    cone.value = facing;
    game.add_buff(id, cone);
    let position = position_of(game, id);
    game.global_effect("JhinR", id, position, Some(id));
    game.sound(id, "r");
}

fn curtain_elapsed(unit: &Unit) -> Option<f32> {
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

fn recasting(unit: &Unit, slot: Slot) -> bool {
    slot == Slot::R && unit.has_buff(CURTAIN_CALL)
}

fn recast(game: &mut Game, id: UnitId, slot: Slot, target: Target) -> bool {
    let Some(unit) = game.unit(id) else {
        return false;
    };
    if !unit.has_buff(CURTAIN_CALL) {
        return false;
    }
    if slot == Slot::R {
        curtain_shot(game, id, target);
    }
    true
}

fn curtain_shot(game: &mut Game, id: UnitId, target: Target) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    let (Some(elapsed), Some(buff), Some(cone)) = (
        curtain_elapsed(unit),
        unit.buff(CURTAIN_CALL),
        unit.buff("curtain_call_cone"),
    ) else {
        return;
    };
    let spell = spell_of(game, id, Slot::R).unwrap_or_default();
    let rank = rank(game, id, Slot::R);
    let (shots, last, center) = (buff.stacks, buff.value, cone.value);
    if elapsed < CURTAIN_UNLOCK
        || game.time - last < spell.value("MInimumDelayBetweenShots", rank)
        || shots == 0
    {
        return;
    }
    let (position, team) = (unit.position, unit.team);
    let aimed = Vec2::from_array(target.point) - position;
    let angle = aimed.x.atan2(aimed.y);
    let offset = (angle - center + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    let angle = center
        + offset.clamp(
            -CURTAIN_HALF_CONE.to_radians(),
            CURTAIN_HALF_CONE.to_radians(),
        );
    let direction = Vec2::new(angle.sin(), angle.cos());
    let last_shot = shots == 1;
    let damage = calculate(game, id, &spell, rank, "DamageCalc")
        * if last_shot {
            spell.value("FourthShotMultiplier", rank)
        } else {
            1.0
        };
    let missile = game.allocate();
    game.missiles.push(Missile {
        id: missile,
        owner: id,
        team,
        kind: clip_id(if last_shot {
            "JhinRShotMis4"
        } else {
            "JhinRShotMis"
        }),
        position,
        height: 100.0,
        speed: CURTAIN_SPEED,
        width: CURTAIN_WIDTH,
        motion: Motion::Line {
            direction,
            remaining: CURTAIN_RANGE,
            limit: None,
        },
        hit: Hit::Spell(Slot::R, damage),
        hit_units: Vec::new(),
        acceleration: None,
    });
    let time = game.time;
    if let Some(unit) = game.unit_mut(id) {
        unit.facing = angle;
        if let Some(buff) = unit.buff_mut(CURTAIN_CALL) {
            buff.stacks = shots - 1;
            buff.value = time;
        }
    }
    game.play(id, "Spell4_Shoot1", 1.0);
    game.sound(id, "r_cast");
    if last_shot {
        if let Some(unit) = game.unit_mut(id) {
            unit.action = Action::None;
        }
        end_curtain(game, id);
    }
}

fn stops_missile(game: &Game, missile: &Missile, target: UnitId) -> bool {
    matches!(missile.hit, Hit::Spell(Slot::R, _))
        && game
            .unit(target)
            .is_some_and(|unit| unit.champion().is_some())
}

fn curtain_hit(game: &mut Game, missile: &Missile, target: UnitId, damage: f32) {
    let owner = missile.owner;
    let spell = spell_of(game, owner, Slot::R).unwrap_or_default();
    let rank = rank(game, owner, Slot::R);
    let Some(unit) = game.unit(target) else {
        return;
    };
    let missing = 1.0 - unit.health / unit.stats.max_health.max(1.0);
    let (champion, at) = (unit.champion().is_some(), unit.position);
    let amount = damage * (1.0 + spell.value("PercentMissingAmp", rank) * missing);
    game.damage(Some(owner), target, amount, DamageKind::Physical, false);
    let big = missile.kind == clip_id("JhinRShotMis4");
    game.effect(
        if big { "JhinRBigHit" } else { "JhinRHit" },
        at,
        Some(target),
    );
    if champion {
        game.slow(
            target,
            spell.value("SlowPercent", rank),
            spell.value("SlowDuration", rank),
            Some((owner, Slot::R)),
        );
    }
}

fn end_curtain(game: &mut Game, id: UnitId) {
    let spell = spell_of(game, id, Slot::R).unwrap_or_default();
    let rank = rank(game, id, Slot::R);
    if let Some(unit) = game.unit_mut(id) {
        unit.buffs
            .retain(|buff| !matches!(buff.name, CURTAIN_CALL | "curtain_call_cone"));
        let cooldown = stats::cooldown(spell.cooldown(rank), unit.stats.ability_haste);
        let ability = &mut unit.abilities[slot_index(Slot::R)];
        ability.cooldown = cooldown;
        ability.cooldown_total = cooldown;
    }
    game.release(id, "Spell4_Idle");
    let position = position_of(game, id);
    game.effect("JhinREnd", position, Some(id));
}

fn died(game: &mut Game, id: UnitId) {
    if game
        .unit(id)
        .is_some_and(|unit| unit.has_buff(CURTAIN_CALL))
    {
        end_curtain(game, id);
    }
}

fn curtain_tick(game: &mut Game, id: UnitId) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    let Some(buff) = unit.buff(CURTAIN_CALL) else {
        return;
    };
    let last = buff.value;
    let Some(elapsed) = curtain_elapsed(unit) else {
        end_curtain(game, id);
        return;
    };
    if !unit.path.is_empty() {
        if let Some(unit) = game.unit_mut(id) {
            unit.action = Action::None;
        }
        end_curtain(game, id);
        return;
    }
    let idle = unit
        .animation
        .is_some_and(|animation| animation.hold && animation.clip == clip_id("Spell4_Idle"));
    if !idle && elapsed >= CURTAIN_START && game.time - last >= CURTAIN_SHOT_CLIP {
        game.hold(id, "Spell4_Idle");
    }
}

fn missile_hit(game: &mut Game, missile: &Missile, target: UnitId, slot: Slot, damage: f32) {
    match slot {
        Slot::Q => {
            game.damage(
                Some(missile.owner),
                target,
                damage,
                DamageKind::Physical,
                false,
            );
            bounce(game, missile, target, damage);
        }
        Slot::R => curtain_hit(game, missile, target, damage),
        _ => {}
    }
}

fn tick(game: &mut Game, id: UnitId) {
    reload(game, id);
    curtain_tick(game, id);
    let ready = game.unit(id).is_some_and(|unit| ammo(unit) == 1);
    glow(
        game,
        id,
        FOURTH_READY,
        ready,
        ["JhinFourthReady", "JhinFourthSpent"],
    );
}

fn position_of(game: &Game, id: UnitId) -> Vec2 {
    game.unit(id).map_or(Vec2::ZERO, |unit| unit.position)
}
