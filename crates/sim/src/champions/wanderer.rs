use asset::clip_id;
use asset::data::Spell;
use glam::Vec2;
use protocol::{DamageKind, Slot, Target, Team, UnitId};

use super::{
    Cast, Kit, activated, approach, begin, calculate, dash, direction, enemies_on_segment,
    spell_of, target_in_range,
};
use crate::stats::{self, Bonus};
use crate::unit::{Action, DashKind, Unit, slot_index};
use crate::{Game, Hit, Missile, Motion, facing_to, items};

const WIND_WALLS: [&str; 5] = ["YasuoW1", "YasuoW2", "YasuoW3", "YasuoW4", "YasuoW5"];
const DASH_RANGE: f32 = 475.0;
const STAB_RANGE: f32 = 450.0;
const TORNADO_RANGE: f32 = 1100.0;
const MAX_FLOW: f32 = 100.0;
const WALL_DISTANCE: f32 = 250.0;
const LAST_BREATH_RANGE: f32 = 1400.0;
const SWEEPING_GHOST: f32 = 2.0;
const Q_CAST: f32 = 0.35;
const Q_CAST_STEP: f32 = 0.035;
const Q_CAST_STEP_SPEED: f32 = 0.24;
const THRUST_MIN_CAST: f32 = 0.175;
const WHIRLWIND_MIN_CAST: f32 = 0.28;
const Q_COOLDOWN_SPEED: f32 = 1.67;
const Q_MIN_COOLDOWN: f32 = 1.33 / 4.0;
const CRIT_MULTIPLIER: f32 = 2.0;
const EXCESS_CRIT_DAMAGE: f32 = 50.0;
const CRIT_DAMAGE_PENALTY: f32 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Wall {
    pub team: Team,
    pub from: Vec2,
    pub to: Vec2,
    pub armed: f32,
    pub expires: f32,
}

pub const KIT: Kit = Kit {
    cast,
    fire,
    range,
    dashed,
    missile_hit,
    before_damage,
    tick,
    resource: |unit| Some((unit.counter, MAX_FLOW)),
    dash_cast,
    movement: &[Slot::E],
    ..Kit::NONE
};

fn dash_cast(unit: &Unit, slot: Slot) -> bool {
    matches!(slot, Slot::Q | Slot::R)
        && matches!(
            unit.action,
            Action::Dash {
                kind: DashKind::Spell(Slot::E, _),
                ..
            }
        )
}

fn range(_: &Game, unit: &Unit, slot: Slot, base: f32) -> f32 {
    match slot {
        Slot::Q if tornado_ready(unit) => TORNADO_RANGE,
        Slot::Q => STAB_RANGE,
        Slot::W => WALL_DISTANCE,
        Slot::E => DASH_RANGE + unit.radius(),
        Slot::R => LAST_BREATH_RANGE,
        _ => base,
    }
}

fn tornado_ready(unit: &Unit) -> bool {
    unit.buff("gathering_storm")
        .is_some_and(|buff| buff.stacks >= 2)
}

fn bonus_attack_speed(unit: &Unit) -> f32 {
    (unit.stats.attack_speed / unit.stats.base_attack_speed.max(0.1) - 1.0).max(0.0)
}

fn q_cast_time(unit: &Unit) -> f32 {
    let floor = if tornado_ready(unit) {
        WHIRLWIND_MIN_CAST
    } else {
        THRUST_MIN_CAST
    };
    (Q_CAST - Q_CAST_STEP * bonus_attack_speed(unit) / Q_CAST_STEP_SPEED).max(floor)
}

fn q_cooldown(unit: &Unit, base: f32) -> f32 {
    (base * (1.0 - bonus_attack_speed(unit) / Q_COOLDOWN_SPEED)).max(base * Q_MIN_COOLDOWN)
}

fn cast(game: &mut Game, id: UnitId, slot: Slot, target: Target, spell: &Spell, rank: u8) {
    let Some(unit) = game.unit(id).cloned() else {
        return;
    };
    match slot {
        Slot::Q => {
            let cooldown = q_cooldown(&unit, spell.cooldown(rank));
            let windup = q_cast_time(&unit);
            let stacks = unit.buff("gathering_storm").map_or(0, |buff| buff.stacks);
            let dashing = matches!(
                unit.action,
                Action::Dash {
                    kind: DashKind::Spell(Slot::E, _),
                    ..
                }
            );
            if let Some(unit) = game.unit_mut(id) {
                let q = &mut unit.abilities[slot_index(Slot::Q)];
                q.cooldown = cooldown;
                q.cooldown_total = cooldown;
            }
            activated(game, id, slot);
            game.sound(
                id,
                match stacks {
                    0 => "q",
                    1 => "q2",
                    _ => "q3",
                },
            );
            if dashing {
                let mut buff = game.buff("sweeping_blade_q", 1.0);
                buff.stacks = stacks;
                game.add_buff(id, buff);
                game.play(id, "Spell3b", 1.0);
                return;
            }
            let clip = match stacks {
                0 => "Spell1A",
                1 => "Spell1B",
                _ => "Spell1C",
            };
            begin(game, id, slot, target, windup, 0.05, clip);
        }
        Slot::W => {
            if let Some(unit) = game.unit_mut(id) {
                let cooldown = stats::cooldown(spell.cooldown(rank), unit.stats.ability_haste);
                let w = &mut unit.abilities[slot_index(Slot::W)];
                w.cooldown = cooldown;
                w.cooldown_total = w.cooldown;
            }
            activated(game, id, slot);
            let position = unit.position;
            let facing = direction(position, Vec2::from_array(target.point), unit.facing);
            let center = position + facing * WALL_DISTANCE;
            let half = facing.perp() * spell.value("Width", rank).max(320.0) / 2.0;
            let expires = game.time + spell.value("WallLife", rank).max(4.0) + 0.25;
            game.walls.push(Wall {
                team: unit.team,
                from: center - half,
                to: center + half,
                armed: game.time,
                expires,
            });
            game.play(id, "Spell2", 1.0);
            game.sound(id, "w");
            game.effect("YasuoWCast", position, Some(id));
            let wall = WIND_WALLS[usize::from(rank.clamp(1, 5) - 1)];
            game.lasting_effect(wall, id, center, expires);
        }
        Slot::E => e(game, id, &unit, target, spell, rank),
        Slot::R => r(game, id, &unit, target, spell, rank),
        _ => {}
    }
}

fn e(game: &mut Game, id: UnitId, unit: &Unit, target: Target, spell: &Spell, rank: u8) {
    let Some(other) = target_in_range(game, id, &target, DASH_RANGE + unit.radius(), true) else {
        if target.unit.is_some() {
            approach(game, id, Slot::E, target);
        }
        return;
    };
    let marked = game.unit(other).is_some_and(|other| {
        other
            .buffs
            .iter()
            .any(|buff| buff.name == "sweeping_blade_mark" && buff.source == Some(id))
    });
    if marked || matches!(unit.action, Action::Dash { .. }) {
        return;
    }
    let other_position = game
        .unit(other)
        .map_or(unit.position, |other| other.position);
    let destination =
        unit.position + direction(unit.position, other_position, unit.facing) * DASH_RANGE;
    let speed = spell.value("BaseDashSpeed", rank) + unit.stats.move_speed * 0.6;
    let mut stacks = game
        .unit(id)
        .and_then(|unit| unit.buff("sweeping_blade_stacks"))
        .map_or(0, |buff| buff.stacks);
    let damage = calculate(game, id, spell, rank, "TotalDamage") * (1.0 + 0.25 * stacks as f32);
    stacks = (stacks + 1).min(4);
    let mut buff = game.buff(
        "sweeping_blade_stacks",
        spell.value("StackDuration", rank).max(5.0),
    );
    buff.stacks = stacks;
    game.add_buff(id, buff);
    let mut mark = game.buff(
        "sweeping_blade_mark",
        spell.value("PerTargetCooldown", rank).max(6.0),
    );
    mark.source = Some(id);
    game.add_sourced_buff(other, mark);
    let ghosted = game.buff("ghosted", SWEEPING_GHOST);
    game.add_buff(id, ghosted);
    game.damage(Some(id), other, damage, DamageKind::Magic, false);
    game.effect("YasuoEHit", other_position, Some(other));
    game.effect(&format!("YasuoETimer{rank}"), other_position, Some(other));
    if let Some(unit) = game.unit_mut(id) {
        let cooldown = stats::cooldown(spell.cooldown(rank), unit.stats.ability_haste);
        let e = &mut unit.abilities[slot_index(Slot::E)];
        e.cooldown = cooldown;
        e.cooldown_total = e.cooldown;
    }
    activated(game, id, Slot::E);
    dash(
        game,
        id,
        destination,
        speed,
        DashKind::Spell(Slot::E, Some(other)),
    );
    game.play(id, "Spell3", 1.0);
    game.sound(id, "e");
}

fn r(game: &mut Game, id: UnitId, unit: &Unit, target: Target, spell: &Spell, rank: u8) {
    let point = Vec2::from_array(target.point);
    let position = unit.position;
    let search = spell.value("RCastSearchRadius", rank);
    let airborne_enemy = |other: &&Unit| {
        other.team != unit.team
            && other.alive()
            && other.champion().is_some()
            && matches!(
                other.action,
                Action::Airborne { .. }
                    | Action::Dash {
                        kind: DashKind::Knockback,
                        ..
                    }
            )
    };
    let Some((victim, at, reach)) = game
        .units
        .iter()
        .filter(airborne_enemy)
        .filter(|other| {
            other.position.distance(position) <= LAST_BREATH_RANGE
                && other.position.distance(point) <= search
        })
        .min_by(|a, b| {
            a.position
                .distance(point)
                .total_cmp(&b.position.distance(point))
        })
        .map(|other| (other.id, other.position, other.radius() + unit.radius()))
    else {
        return;
    };
    let landing = game.walkable_point(at + direction(position, at, unit.facing) * reach);
    if let Some(unit) = game.unit_mut(id) {
        let r = &mut unit.abilities[slot_index(Slot::R)];
        let haste = unit.stats.ability_haste;
        r.cooldown = stats::cooldown(spell.cooldown(rank), haste);
        r.cooldown_total = r.cooldown;
        unit.position = landing;
        unit.facing = facing_to(landing, at, unit.facing);
        unit.counter = MAX_FLOW;
        unit.action = Action::None;
        unit.path.clear();
        unit.buffs.retain(|buff| buff.name != "gathering_storm");
    }
    activated(game, id, Slot::R);
    let damage = calculate(game, id, spell, rank, "Damage");
    let radius = spell.value("RRadius", rank);
    let duration = spell.value("RKnockupDuration", rank);
    let victims: Vec<UnitId> = game
        .units
        .iter()
        .filter(airborne_enemy)
        .filter(|other| other.id == victim || other.position.distance(at) <= radius)
        .map(|other| other.id)
        .collect();
    for victim in victims {
        game.damage(Some(id), victim, damage, DamageKind::Physical, false);
        game.knock_up(victim, duration, Some((id, Slot::R)));
        let at = game.unit(victim).map_or(landing, |unit| unit.position);
        game.effect("YasuoRHit", at, Some(victim));
    }
    let mut buff = game.buff("last_breath", spell.value("RBuffDuration", rank));
    buff.bonus.crit_bonus_armor_penetration = spell.value("RPercentArmorPen", rank) / 100.0;
    game.add_buff(id, buff);
    game.play(id, "Spell4", 1.0);
    game.sound(id, "r");
    game.effect("YasuoR", at, Some(id));
    game.effect("LastBreathSword", landing, Some(id));
}

fn fire(game: &mut Game, cast: &Cast) {
    if cast.slot == Slot::Q {
        q(game, cast);
    }
}

fn q(game: &mut Game, cast: &Cast) {
    let Cast {
        id,
        spell,
        rank,
        position,
        team,
        facing,
        ..
    } = *cast;
    let Some(unit) = game.unit(id) else {
        return;
    };
    let stacks = unit.buff("gathering_storm").map_or(0, |buff| buff.stacks);
    let critical_roll = game.next_random();
    let critical = game
        .unit(id)
        .is_some_and(|unit| critical_roll < unit.stats.crit_chance);
    let damage = calculate(
        game,
        id,
        spell,
        rank,
        if critical {
            "TotalDamageCrit"
        } else {
            "TotalDamage"
        },
    );
    if stacks >= 2 {
        if let Some(unit) = game.unit_mut(id) {
            unit.buffs.retain(|buff| buff.name != "gathering_storm");
        }
        game.effect("YasuoTornadoSpent", position, Some(id));
        let missile = game.allocate();
        game.missiles.push(Missile {
            id: missile,
            owner: id,
            team,
            kind: clip_id("YasuoQ3Mis"),
            position,
            height: 50.0,
            speed: 1200.0,
            width: 90.0,
            motion: Motion::Line {
                direction: facing,
                remaining: TORNADO_RANGE,
                limit: None,
            },
            hit: Hit::Spell(Slot::Q, damage),
            hit_units: Vec::new(),
            acceleration: None,
        });
        return;
    }
    let reach = STAB_RANGE;
    let victims = enemies_on_segment(game, team, position, position + facing * reach, 40.0);
    let hit_any = !victims.is_empty();
    for victim in victims {
        game.damage(Some(id), victim, damage, DamageKind::Physical, critical);
        items::spellblade_hit(game, id, victim);
        let at = game.unit(victim).map_or(position, |unit| unit.position);
        game.effect("YasuoQHit", at, Some(victim));
    }
    game.effect("YasuoQ", position + facing * reach * 0.5, Some(id));
    if hit_any {
        add_storm_stack(game, id, spell, rank);
    }
}

fn add_storm_stack(game: &mut Game, id: UnitId, spell: &Spell, rank: u8) {
    let stacks = game
        .unit(id)
        .and_then(|unit| unit.buff("gathering_storm"))
        .map_or(0, |buff| buff.stacks);
    let mut buff = game.buff(
        "gathering_storm",
        spell.value("GatheringStormDuration", rank).max(6.0),
    );
    buff.stacks = (stacks + 1).min(2);
    game.add_buff(id, buff);
    if stacks == 1
        && let Some(position) = game.unit(id).map(|unit| unit.position)
    {
        game.effect("YasuoTornadoReady", position, Some(id));
        game.sound(id, "q_ready");
    }
}

fn dashed(game: &mut Game, id: UnitId, slot: Slot, _target: Option<UnitId>) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    if slot != Slot::E {
        return;
    }
    let Some(stacks) = unit.buff("sweeping_blade_q").map(|buff| buff.stacks) else {
        return;
    };
    let (position, team, rank) = (unit.position, unit.team, unit.ability(Slot::Q).rank);
    if let Some(unit) = game.unit_mut(id) {
        unit.buffs.retain(|buff| buff.name != "sweeping_blade_q");
    }
    let spell = spell_of(game, id, Slot::Q).unwrap_or_default();
    let damage = calculate(game, id, &spell, rank, "TotalDamage");
    let victims = game.enemies_near(team, position, 215.0);
    let hit_any = !victims.is_empty();
    for victim in victims {
        if game.unit(victim).is_some_and(Unit::is_structure) {
            continue;
        }
        game.damage(Some(id), victim, damage, DamageKind::Physical, false);
        if stacks >= 2 {
            game.knock_up(victim, 1.0, Some((id, Slot::Q)));
        }
        let at = game.unit(victim).map_or(position, |unit| unit.position);
        game.effect("YasuoQHit", at, Some(victim));
    }
    if stacks >= 2 {
        if let Some(unit) = game.unit_mut(id) {
            unit.buffs.retain(|buff| buff.name != "gathering_storm");
        }
        game.effect("YasuoTornadoSpent", position, Some(id));
        game.effect("YasuoEQ3", position, Some(id));
    } else {
        if hit_any {
            add_storm_stack(game, id, &spell, rank);
        }
        game.effect("YasuoEQ", position, Some(id));
    }
}

fn missile_hit(game: &mut Game, missile: &Missile, target: UnitId, _slot: Slot, damage: f32) {
    game.damage(
        Some(missile.owner),
        target,
        damage,
        DamageKind::Physical,
        false,
    );
    game.knock_up(target, 1.0, Some((missile.owner, Slot::Q)));
    if let Some(at) = game.unit(target).map(|unit| unit.position) {
        game.effect("YasuoTornadoHit", at, Some(target));
    }
}

fn tick(game: &mut Game, id: UnitId) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    let traveled = unit.position.distance(unit.last_position);
    let per_stack = match unit.level {
        0..=6 => 59.0,
        7..=12 => 52.5,
        _ => 46.0,
    };
    let wanderer = intent(game, unit);
    if let Some(unit) = game.unit_mut(id) {
        unit.counter = (unit.counter + traveled / per_stack).min(MAX_FLOW);
    }
    let current = game
        .unit(id)
        .and_then(|unit| unit.buff("way_of_the_wanderer"))
        .map(|buff| buff.bonus);
    if current != Some(wanderer) {
        let mut buff = game.buff("way_of_the_wanderer", f32::MAX);
        buff.bonus = wanderer;
        game.add_buff(id, buff);
    }
}

fn intent(game: &Game, unit: &Unit) -> Bonus {
    let own = unit
        .buff("way_of_the_wanderer")
        .map_or(Bonus::default(), |buff| buff.bonus);
    let items: f32 = unit
        .items
        .iter()
        .flatten()
        .filter_map(|item| game.data.item(*item))
        .map(|item| item.stats.crit_chance)
        .sum();
    let buffs: f32 = unit
        .buffs
        .iter()
        .filter(|buff| buff.name != "way_of_the_wanderer")
        .map(|buff| buff.bonus.crit_chance)
        .sum();
    let crit = items + buffs;
    let excess = (crit * CRIT_MULTIPLIER - 1.0).max(0.0);
    Bonus {
        crit_chance: crit,
        attack_damage: excess * EXCESS_CRIT_DAMAGE,
        crit_damage: -(unit.stats.crit_damage - own.crit_damage) * CRIT_DAMAGE_PENALTY,
        ..Bonus::default()
    }
}

fn before_damage(game: &mut Game, id: UnitId) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    if unit.counter < MAX_FLOW {
        return;
    }
    let level = f32::from(unit.level);
    let amount = 125.0 + (600.0 - 125.0) * (level - 1.0) / 17.0;
    let position = unit.position;
    if let Some(unit) = game.unit_mut(id) {
        unit.counter = 0.0;
    }
    game.shield(id, amount, 1.0);
    game.effect("YasuoFlowShield", position, Some(id));
    game.sound(id, "shield");
}

pub fn blocks(game: &mut Game, missile: &Missile, from: Vec2) -> bool {
    let time = game.time;
    game.walls.retain(|wall| wall.expires > time);
    let blocked = game.walls.iter().any(|wall| {
        wall.team != missile.team && segments_cross(from, missile.position, wall.from, wall.to)
    });
    if blocked {
        game.effect("YasuoWBlock", missile.position, None);
    }
    blocked
}

fn segments_cross(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let cross = |o: Vec2, p: Vec2, q: Vec2| (p - o).perp_dot(q - o);
    let d1 = cross(c, d, a);
    let d2 = cross(c, d, b);
    let d3 = cross(a, b, c);
    let d4 = cross(a, b, d);
    (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0)
}
