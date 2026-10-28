mod ascendant;
mod cardmaster;
mod colossus;
mod emperor;
mod guardian;
mod sniper;
mod sorcerer;
mod summoners;
mod swordsman;
mod void_walker;
mod wanderer;
mod warlord;

#[cfg(test)]
mod tests;

use asset::data::{Spell, Targeting};
use glam::Vec2;
use protocol::{Slot, TICK, Target, Team, UnitId, UnitKind};

use crate::formula::Context;
use crate::items;
use crate::stats::{self, Bonus};
use crate::unit::{Action, DashKind, Order, Unit, slot_index};
use crate::{Game, Hit, Missile, facing_to, segment_distance};

pub use emperor::stops;
pub use summoners::FLASH_RANGE;
pub use wanderer::Wall;

const SPELLBLADE_WINDOW: f32 = 10.0;

pub struct Cast<'a> {
    pub id: UnitId,
    pub slot: Slot,
    pub target: Target,
    pub spell: &'a Spell,
    pub rank: u8,
    pub position: Vec2,
    pub team: Team,
    pub point: Vec2,
    pub facing: Vec2,
}

type CastFn = fn(&mut Game, UnitId, Slot, Target, &Spell, u8);
type MissileHitFn = fn(&mut Game, &Missile, UnitId, Slot, f32);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Zone {
    pub owner: UnitId,
    pub team: Team,
    pub name: &'static str,
    pub center: Vec2,
    pub radius: f32,
    pub armed: f32,
    pub expires: f32,
    pub next_tick: f32,
    pub damage: f32,
    pub value: f32,
}

pub struct Interaction {
    pub range: f32,
    pub windup: f32,
    pub clip: &'static str,
    pub effect: &'static str,
    pub targets: fn(&Game) -> Vec<UnitId>,
    pub ready: fn(&Game, UnitId) -> bool,
    pub finish: fn(&mut Game, UnitId, UnitId),
}

pub struct Kit {
    pub spawned: fn(&mut Game, UnitId),
    pub respawned: fn(&mut Game, UnitId),
    pub died: fn(&mut Game, UnitId),
    pub ranked_up: fn(&mut Game, UnitId, Slot),
    pub recharge: fn(&mut Game, UnitId),
    pub ammo: Option<Slot>,
    pub recast: fn(&mut Game, UnitId, Slot, Target) -> bool,
    pub recasting: fn(&Unit, Slot) -> bool,
    pub cast: CastFn,
    pub fire: fn(&mut Game, &Cast),
    pub range: fn(&Game, &Unit, Slot, f32) -> f32,
    pub area: fn(&Game, &Unit, Slot) -> f32,
    pub dashed: fn(&mut Game, UnitId, Slot, Option<UnitId>),
    pub missile_hit: MissileHitFn,
    pub attack_clip: fn(&Unit) -> Option<&'static str>,
    pub attack_sound: fn(&Unit) -> Option<&'static str>,
    pub redirect_attack: fn(&mut Game, UnitId, UnitId) -> bool,
    pub launch_attack: fn(&mut Game, UnitId, UnitId) -> bool,
    pub modify_attack: fn(&mut Game, UnitId, UnitId, f32) -> f32,
    pub on_attack_hit: fn(&mut Game, UnitId, UnitId),
    pub on_crit: fn(&mut Game, UnitId, UnitId),
    pub critical: fn(&Unit) -> bool,
    pub bonus: fn(&Game, &Unit, Bonus) -> Bonus,
    pub stops_missile: fn(&Game, &Missile, UnitId) -> bool,
    pub on_kill: fn(&mut Game, UnitId, UnitId),
    pub on_takedown: fn(&mut Game, UnitId, UnitId),
    pub before_damage: fn(&mut Game, UnitId),
    pub tick: fn(&mut Game, UnitId),
    pub afflicted: fn(&mut Game, UnitId, &'static str),
    pub zone: fn(&mut Game, usize),
    pub indicator: fn(&Game, UnitId) -> Option<&'static str>,
    pub resource: fn(&Unit) -> Option<(f32, f32)>,
    pub dash_cast: fn(&Unit, Slot) -> bool,
    pub interaction: Option<Interaction>,
    pub passive_cooldown: Option<&'static str>,
    pub silent_hits: &'static [Slot],
    pub movement: &'static [Slot],
}

impl Kit {
    pub const NONE: Kit = Kit {
        spawned: |_, _| {},
        respawned: |_, _| {},
        died: |_, _| {},
        ranked_up: |_, _, _| {},
        recharge: |_, _| {},
        ammo: None,
        recast: |_, _, _, _| false,
        recasting: |_, _| false,
        cast: |_, _, _, _, _, _| {},
        fire: |_, _| {},
        range: |_, _, _, base| base,
        area: |_, _, _| 0.0,
        dashed: |_, _, _, _| {},
        missile_hit: |_, _, _, _, _| {},
        attack_clip: |_| None,
        attack_sound: |_| None,
        redirect_attack: |_, _, _| false,
        launch_attack: |_, _, _| false,
        modify_attack: |_, _, _, amount| amount,
        on_attack_hit: |_, _, _| {},
        on_crit: |_, _, _| {},
        critical: |_| false,
        bonus: |_, _, bonus| bonus,
        stops_missile: |_, _, _| false,
        on_kill: |_, _, _| {},
        on_takedown: |_, _, _| {},
        before_damage: |_, _| {},
        tick: |_, _| {},
        afflicted: |_, _, _| {},
        zone: |_, _| {},
        indicator: |_, _| None,
        resource: |_| None,
        dash_cast: |_, _| false,
        interaction: None,
        passive_cooldown: None,
        silent_hits: &[],
        movement: &[],
    };
}

pub fn kit(name: &str) -> Option<&'static Kit> {
    match name {
        "Emperor" => Some(&emperor::KIT),
        "Cardmaster" => Some(&cardmaster::KIT),
        "Colossus" => Some(&colossus::KIT),
        "Wanderer" => Some(&wanderer::KIT),
        "Guardian" => Some(&guardian::KIT),
        "Swordsman" => Some(&swordsman::KIT),
        "Warlord" => Some(&warlord::KIT),
        "VoidWalker" => Some(&void_walker::KIT),
        "Sorcerer" => Some(&sorcerer::KIT),
        "Ascendant" => Some(&ascendant::KIT),
        "Sniper" => Some(&sniper::KIT),
        _ => None,
    }
}

pub fn recasting(game: &Game, unit: &Unit, slot: Slot) -> bool {
    match slot {
        Slot::D | Slot::F => summoners::summoner_recast(game, unit, slot),
        _ => (kit_of(game, unit.id).recasting)(unit, slot),
    }
}

pub fn area(game: &Game, unit: &Unit, slot: Slot) -> f32 {
    (kit_of(game, unit.id).area)(game, unit, slot)
}

pub fn ammo_slot(game: &Game, id: UnitId) -> Option<Slot> {
    kit_of(game, id).ammo
}

fn kit_of(game: &Game, id: UnitId) -> &'static Kit {
    game.unit(id)
        .and_then(Unit::champion)
        .and_then(|champion| game.data.champions.get(usize::from(champion)))
        .and_then(|data| kit(&data.name))
        .unwrap_or(&Kit::NONE)
}

fn spell_of(game: &Game, id: UnitId, slot: Slot) -> Option<Spell> {
    let champion = game.unit(id)?.champion()?;
    let data = game.data.champions.get(usize::from(champion))?;
    data.spell(data.slots.get(slot_index(slot))?).cloned()
}

fn named_spell(game: &Game, id: UnitId, name: &str) -> Option<Spell> {
    let champion = game.unit(id)?.champion()?;
    game.data
        .champions
        .get(usize::from(champion))?
        .spell(name)
        .cloned()
}

pub fn calculate(game: &Game, id: UnitId, spell: &Spell, rank: u8, name: &str) -> f32 {
    let Some(unit) = game.unit(id) else {
        return 0.0;
    };
    let counter = unit.counter;
    Context {
        spell,
        rank,
        stats: &unit.stats,
        health: unit.health,
        buff_count: &|_| counter,
    }
    .calculate(name)
}

fn rank(game: &Game, id: UnitId, slot: Slot) -> u8 {
    game.unit(id).map_or(0, |unit| unit.ability(slot).rank)
}

pub fn spawned(game: &mut Game, id: UnitId) {
    (kit_of(game, id).spawned)(game, id);
}

pub fn respawned(game: &mut Game, id: UnitId) {
    (kit_of(game, id).respawned)(game, id);
}

pub fn died(game: &mut Game, id: UnitId) {
    (kit_of(game, id).died)(game, id);
}

pub fn ranked_up(game: &mut Game, id: UnitId, slot: Slot) {
    (kit_of(game, id).ranked_up)(game, id, slot);
    game.refresh_stats(id);
}

pub fn recharge(game: &mut Game, id: UnitId) {
    (kit_of(game, id).recharge)(game, id);
}

fn max_charges(spell: &Spell, rank: u8) -> u8 {
    let ammo: Vec<f32> = spell
        .max_ammo
        .iter()
        .map(|value| f32::from(*value))
        .collect();
    (Spell::by_rank(&ammo, rank) as u8).max(1)
}

fn recharge_charges(game: &mut Game, id: UnitId, slot: Slot) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    let ability = *unit.ability(slot);
    if ability.rank == 0 {
        return;
    }
    let spell = spell_of(game, id, slot).unwrap_or_default();
    let max = max_charges(&spell, ability.rank);
    let haste = unit.stats.ability_haste;
    let Some(unit) = game.unit_mut(id) else {
        return;
    };
    let ability = &mut unit.abilities[slot_index(slot)];
    if ability.charges >= max {
        ability.recharge = 0.0;
        return;
    }
    if ability.recharge <= 0.0 {
        ability.recharge =
            stats::cooldown(Spell::by_rank(&spell.ammo_recharge, ability.rank), haste);
        ability.recharge_total = ability.recharge;
    }
    ability.recharge -= TICK;
    if ability.recharge <= 0.0 {
        ability.charges += 1;
        ability.recharge = 0.0;
    }
}

fn spend(game: &mut Game, id: UnitId, slot: Slot, cooldown: f32, mana: f32) {
    if let Some(unit) = game.unit_mut(id) {
        unit.mana = (unit.mana - mana).max(0.0);
    }
    set_cooldown(game, id, slot, cooldown);
    activated(game, id, slot);
}

fn set_cooldown(game: &mut Game, id: UnitId, slot: Slot, cooldown: f32) {
    let Some(unit) = game.unit_mut(id) else {
        return;
    };
    let haste = unit.stats.ability_haste;
    let ability = &mut unit.abilities[slot_index(slot)];
    let cooldown = if matches!(slot, Slot::D | Slot::F) {
        cooldown
    } else {
        stats::cooldown(cooldown, haste)
    };
    ability.cooldown = cooldown;
    ability.cooldown_total = cooldown;
}

fn activated(game: &mut Game, id: UnitId, slot: Slot) {
    sheen_proc_ready(game, id);
    void_walker::ability_cast(game, id, slot);
}

pub fn glow(game: &mut Game, id: UnitId, marker: &'static str, on: bool, [start, end]: [&str; 2]) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    if on == unit.has_buff(marker) {
        return;
    }
    let position = unit.position;
    if on {
        let buff = game.buff(marker, f32::MAX);
        game.add_buff(id, buff);
        game.effect(start, position, Some(id));
    } else {
        if let Some(unit) = game.unit_mut(id) {
            unit.buffs.retain(|buff| buff.name != marker);
        }
        game.effect(end, position, Some(id));
    }
}

fn sheen_proc_ready(game: &mut Game, id: UnitId) {
    let ready = game.unit(id).is_some_and(|unit| {
        unit.items.iter().flatten().any(|item| {
            matches!(
                *item,
                items::SPELLBLADE | items::TRI_FORCE | items::BANE_BLADE | items::FROST_GAUNTLET
            )
        }) && !unit.has_buff("spellblade_cooldown")
    });
    if ready {
        let buff = game.buff("spellblade", SPELLBLADE_WINDOW);
        game.add_buff(id, buff);
    }
}

fn begin(
    game: &mut Game,
    id: UnitId,
    slot: Slot,
    target: Target,
    windup: f32,
    recovery: f32,
    clip: &str,
) {
    let point = Vec2::from_array(target.point);
    if let Some(unit) = game.unit_mut(id) {
        unit.facing = facing_to(unit.position, point, unit.facing);
        unit.action = Action::Cast {
            slot,
            target,
            elapsed: 0.0,
            windup,
            recovery,
            fired: false,
            mobile: false,
            channel: false,
        };
        unit.order = Order::Idle;
        unit.path.clear();
    }
    if !clip.is_empty() {
        game.play(id, clip, 1.0);
    }
}

fn begin_channel(game: &mut Game, id: UnitId, slot: Slot, target: Target, duration: f32) {
    begin(game, id, slot, target, duration, 0.0, "");
    if let Some(unit) = game.unit_mut(id)
        && let Action::Cast { channel, .. } = &mut unit.action
    {
        *channel = true;
    }
}

fn begin_mobile(
    game: &mut Game,
    id: UnitId,
    slot: Slot,
    target: Target,
    windup: f32,
    recovery: f32,
    clip: &str,
) {
    let kept = game.unit(id).map(|unit| (unit.order, unit.path.clone()));
    begin(game, id, slot, target, windup, recovery, clip);
    if let (Some(unit), Some((order, path))) = (game.unit_mut(id), kept) {
        unit.order = order;
        unit.path = path;
        if let Action::Cast { mobile, .. } = &mut unit.action {
            *mobile = true;
        }
    }
}

fn dash(game: &mut Game, id: UnitId, to: Vec2, speed: f32, kind: DashKind) {
    if let Some(unit) = game.unit_mut(id) {
        unit.action = Action::Dash { to, speed, kind };
        unit.order = Order::Idle;
        unit.path.clear();
    }
}

pub fn can_move_while_casting(unit: &Unit) -> bool {
    matches!(unit.action, Action::Cast { mobile: true, .. })
}

fn able(unit: &Unit, slot: Slot, kit: &Kit) -> bool {
    if unit.has_buff("root") && kit.movement.contains(&slot) {
        return false;
    }
    unit.can_act()
        || unit.alive()
            && !unit.is_stunned()
            && matches!(unit.action, Action::Dash { .. })
            && (kit.dash_cast)(unit, slot)
}

fn ready(unit: &Unit, slot: Slot, mana: f32, able: bool) -> bool {
    let ability = unit.ability(slot);
    able && !unit.is_silenced()
        && ability.rank > 0
        && ability.cooldown <= 0.0
        && unit.mana >= mana
        && !matches!(unit.action, Action::Cast { fired: false, .. })
}

fn ammo_ready(unit: &Unit, slot: Slot, mana: f32, able: bool) -> bool {
    let ability = unit.ability(slot);
    able && !unit.is_silenced()
        && ability.rank > 0
        && ability.charges > 0
        && ability.cooldown <= 0.0
        && unit.mana >= mana
        && !matches!(unit.action, Action::Cast { fired: false, .. })
}

pub fn command_cast(game: &mut Game, id: UnitId, slot: Slot, target: Target) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    if unit.dead || unit.champion().is_none() {
        return;
    }
    match slot {
        Slot::D | Slot::F => summoners::cast(game, id, slot, target),
        _ => {
            let kit = kit_of(game, id);
            if (kit.recasting)(unit, slot) && (!able(unit, slot, kit) || unit.is_silenced()) {
                return;
            }
            if (kit.recast)(game, id, slot, target) {
                return;
            }
            let Some(spell) = spell_of(game, id, slot) else {
                return;
            };
            let Some(unit) = game.unit(id) else {
                return;
            };
            let rank = unit.ability(slot).rank;
            let mana = spell.mana(rank);
            let able = able(unit, slot, kit);
            let available = if kit.ammo == Some(slot) {
                ammo_ready(unit, slot, mana, able)
            } else {
                ready(unit, slot, mana, able)
            };
            if !available {
                return;
            }
            if let Some(unit) = game.unit_mut(id)
                && let Action::Attack { .. } = unit.action
            {
                unit.action = Action::None;
            }
            (kit.cast)(game, id, slot, target, &spell, rank);
        }
    }
}

pub fn continue_cast(game: &mut Game, id: UnitId, slot: Slot, target: Target) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    let goal = target
        .unit
        .and_then(|target| game.unit(target))
        .filter(|target| spell_target(game, unit, target))
        .map(|target| target.position);
    let Some(goal) = goal else {
        if let Some(unit) = game.unit_mut(id) {
            unit.order = Order::Idle;
        }
        return;
    };
    let range = cast_range(game, unit, slot);
    if unit.position.distance(goal) <= range {
        let target = Target {
            point: goal.to_array(),
            unit: target.unit,
        };
        if let Some(unit) = game.unit_mut(id) {
            unit.order = Order::Idle;
        }
        command_cast(game, id, slot, target);
    } else if let Some(unit) = game.unit_mut(id) {
        unit.path = vec![goal];
    }
}

fn usable(interaction: &Interaction, game: &Game, id: UnitId, target: UnitId) -> bool {
    (interaction.ready)(game, id) && (interaction.targets)(game).contains(&target)
}

pub fn passive_cooldown(game: &Game, unit: &Unit) -> Option<(f32, f32)> {
    let buff = unit.buff(kit_of(game, unit.id).passive_cooldown?)?;
    Some(((buff.expires - game.time).max(0.0), buff.duration))
}

pub fn interactables(game: &Game, id: UnitId) -> Vec<UnitId> {
    kit_of(game, id)
        .interaction
        .as_ref()
        .map_or_else(Vec::new, |interaction| (interaction.targets)(game))
}

pub fn can_interact(game: &Game, id: UnitId, target: UnitId) -> bool {
    kit_of(game, id)
        .interaction
        .as_ref()
        .is_some_and(|interaction| usable(interaction, game, id, target))
}

pub fn continue_interact(game: &mut Game, id: UnitId, target: UnitId) {
    let interaction = kit_of(game, id)
        .interaction
        .as_ref()
        .filter(|interaction| usable(interaction, game, id, target));
    let goal = game.unit(target).map(|target| target.position);
    let (Some(interaction), Some(goal), Some(unit)) = (interaction, goal, game.unit_mut(id)) else {
        if let Some(unit) = game.unit_mut(id) {
            unit.order = Order::Idle;
        }
        return;
    };
    if unit.position.distance(goal) > interaction.range {
        unit.path = vec![goal];
        return;
    }
    unit.order = Order::Idle;
    unit.path.clear();
    unit.facing = facing_to(unit.position, goal, unit.facing);
    unit.action = Action::Interact {
        target,
        elapsed: 0.0,
        windup: interaction.windup,
    };
    let position = unit.position;
    game.play(id, interaction.clip, 1.0);
    game.effect(interaction.effect, position, Some(id));
}

pub fn interacted(game: &mut Game, id: UnitId, target: UnitId) {
    if let Some(interaction) = kit_of(game, id)
        .interaction
        .as_ref()
        .filter(|interaction| usable(interaction, game, id, target))
    {
        (interaction.finish)(game, id, target);
    }
}

pub fn cast_range(game: &Game, unit: &Unit, slot: Slot) -> f32 {
    if matches!(slot, Slot::D | Slot::F) {
        return summoners::range(unit, slot);
    }
    let rank = unit.ability(slot).rank.max(1);
    let base = spell_of(game, unit.id, slot).map_or(0.0, |spell| match spell.targeting {
        Targeting::Myself => 0.0,
        Targeting::MyselfArea => Spell::by_rank(&spell.cast_radius, rank),
        _ => spell.range(rank),
    });
    (kit_of(game, unit.id).range)(game, unit, slot, base)
}

fn approach(game: &mut Game, id: UnitId, slot: Slot, target: Target) {
    if let Some(unit) = game.unit_mut(id) {
        unit.order = Order::Cast(slot, target);
    }
}

fn approach_point(game: &mut Game, id: UnitId, target: Target) {
    if let Some(unit) = game.unit_mut(id) {
        unit.path = vec![Vec2::from_array(target.point)];
        unit.order = Order::Move(Vec2::from_array(target.point));
    }
}

fn target_in_range(
    game: &Game,
    id: UnitId,
    target: &Target,
    range: f32,
    enemy: bool,
) -> Option<UnitId> {
    let unit = game.unit(id)?;
    let other = game.unit(target.unit?)?;
    if !spell_target(game, unit, other) || (enemy && other.team == unit.team) {
        return None;
    }
    (unit.position.distance(other.position) <= range + other.radius()).then_some(other.id)
}

fn spell_target(game: &Game, unit: &Unit, other: &Unit) -> bool {
    other.alive()
        && !other.untargetable()
        && other.kind != UnitKind::Soldier
        && !other.is_structure()
        && game.detected(unit.team, other)
}

fn clamp_point(from: Vec2, to: Vec2, range: f32) -> Vec2 {
    let offset = to - from;
    if offset.length() > range {
        from + offset.normalize() * range
    } else {
        to
    }
}

fn direction(from: Vec2, to: Vec2, facing: f32) -> Vec2 {
    (to - from)
        .try_normalize()
        .unwrap_or(Vec2::new(facing.sin(), facing.cos()))
}

fn enemies_in_cone(
    game: &Game,
    team: Team,
    origin: Vec2,
    direction: Vec2,
    range: f32,
    half_angle: f32,
) -> Vec<UnitId> {
    let cosine = half_angle.to_radians().cos();
    game.units
        .iter()
        .filter(|unit| {
            let offset = unit.position - origin;
            unit.team != team
                && unit.alive()
                && !unit.untargetable()
                && !unit.is_structure()
                && unit.kind != UnitKind::Soldier
                && offset.length() <= range + unit.radius()
                && offset.normalize_or_zero().dot(direction) >= cosine
        })
        .map(|unit| unit.id)
        .collect()
}

fn enemies_on_segment(game: &Game, team: Team, from: Vec2, to: Vec2, width: f32) -> Vec<UnitId> {
    game.units
        .iter()
        .filter(|unit| {
            unit.team != team
                && unit.alive()
                && !unit.untargetable()
                && !unit.is_structure()
                && unit.kind != UnitKind::Soldier
                && segment_distance(from, to, unit.position) <= width + unit.radius()
        })
        .map(|unit| unit.id)
        .collect()
}

pub fn fire(game: &mut Game, id: UnitId, slot: Slot, target: Target) {
    let Some(spell) = spell_of(game, id, slot) else {
        return;
    };
    let Some(unit) = game.unit(id) else {
        return;
    };
    let point = Vec2::from_array(target.point);
    let cast = Cast {
        id,
        slot,
        target,
        spell: &spell,
        rank: unit.ability(slot).rank,
        position: unit.position,
        team: unit.team,
        point,
        facing: direction(unit.position, point, unit.facing),
    };
    (kit_of(game, id).fire)(game, &cast);
}

pub fn dash_finished(game: &mut Game, id: UnitId, kind: DashKind) {
    match kind {
        DashKind::Spell(slot, target) => (kit_of(game, id).dashed)(game, id, slot, target),
        DashKind::Mark(target) => summoners::mark_landed(game, id, target),
        DashKind::Plain | DashKind::Knockback => {}
    }
}

pub fn missile_hit(game: &mut Game, missile: &Missile, target: UnitId) {
    match missile.hit {
        Hit::Spell(slot, amount) => {
            let suffix = match slot {
                Slot::Q => "q_hit",
                Slot::W => "w_hit",
                Slot::E => "e_hit",
                _ => "r_hit",
            };
            let kit = kit_of(game, missile.owner);
            if !kit.silent_hits.contains(&slot) {
                game.sound_on(missile.owner, target, suffix);
            }
            (kit.missile_hit)(game, missile, target, slot, amount)
        }
        Hit::Mark => summoners::mark_hit(game, missile.owner, target),
        Hit::Attack { .. } | Hit::Visual => {}
    }
}

pub fn missile_arrived(_game: &mut Game, _missile: &Missile) {}

pub fn blocked_by_wind_wall(game: &mut Game, missile: &Missile, from: Vec2) -> bool {
    wanderer::blocks(game, missile, from)
}

pub fn attack_clip<'a>(game: &Game, id: UnitId, clip: &'a str) -> &'a str {
    game.unit(id)
        .and_then(|unit| (kit_of(game, id).attack_clip)(unit))
        .unwrap_or(clip)
}

pub fn attack_sound(game: &Game, id: UnitId) -> Option<&'static str> {
    game.unit(id)
        .and_then(|unit| (kit_of(game, id).attack_sound)(unit))
}

pub fn redirect_attack(game: &mut Game, id: UnitId, target: UnitId) -> bool {
    (kit_of(game, id).redirect_attack)(game, id, target)
}

pub fn launch_attack(game: &mut Game, id: UnitId, target: UnitId) -> bool {
    (kit_of(game, id).launch_attack)(game, id, target)
}

pub fn modify_attack(game: &mut Game, attacker: UnitId, target: UnitId, amount: f32) -> f32 {
    (kit_of(game, attacker).modify_attack)(game, attacker, target, amount)
}

pub fn on_attack_hit(game: &mut Game, attacker: UnitId, target: UnitId) {
    (kit_of(game, attacker).on_attack_hit)(game, attacker, target);
}

pub fn on_crit(game: &mut Game, attacker: UnitId, target: UnitId) {
    (kit_of(game, attacker).on_crit)(game, attacker, target);
}

pub fn forced_critical(game: &Game, unit: &Unit) -> bool {
    (kit_of(game, unit.id).critical)(unit)
}

pub fn stops_missile(game: &Game, missile: &Missile, target: UnitId) -> bool {
    (kit_of(game, missile.owner).stops_missile)(game, missile, target)
}

pub fn bonus(game: &Game, unit: &Unit, bonus: Bonus) -> Bonus {
    (kit_of(game, unit.id).bonus)(game, unit, bonus)
}

pub fn on_kill(game: &mut Game, killer: Option<UnitId>, victim: UnitId) {
    if let Some(killer) = killer {
        (kit_of(game, killer).on_kill)(game, killer, victim);
    }
}

pub fn on_takedown(game: &mut Game, champion: UnitId, victim: UnitId) {
    (kit_of(game, champion).on_takedown)(game, champion, victim);
}

pub fn prevent_death(_game: &mut Game, _victim: UnitId) -> bool {
    false
}

pub fn tick(game: &mut Game, id: UnitId) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    if unit.dead {
        return;
    }
    if unit.kind == UnitKind::SunDisc {
        emperor::sun_disc(game, id);
        return;
    }
    (kit_of(game, id).tick)(game, id);
    let afflictions: Vec<(&'static str, UnitId)> = game
        .unit(id)
        .into_iter()
        .flat_map(|unit| &unit.buffs)
        .filter_map(|buff| Some((buff.name, buff.source?)))
        .collect();
    for (name, source) in afflictions {
        summoners::afflicted(game, id, name);
        (kit_of(game, source).afflicted)(game, id, name);
    }
}

pub fn zones(game: &mut Game) {
    for index in 0..game.zones.len() {
        let owner = game.zones[index].owner;
        (kit_of(game, owner).zone)(game, index);
    }
    let time = game.time;
    game.zones.retain(|zone| zone.expires > time);
}

pub fn resource(game: &Game, unit: &Unit) -> (f32, f32) {
    (kit_of(game, unit.id).resource)(unit).unwrap_or((unit.mana, unit.stats.max_mana))
}

pub fn indicator(game: &Game, id: UnitId) -> Option<&'static str> {
    (kit_of(game, id).indicator)(game, id)
}

pub fn before_damage(game: &mut Game, target: UnitId, source: Option<UnitId>) {
    let from_champion = source
        .and_then(|source| game.unit(source))
        .is_some_and(|unit| unit.champion().is_some());
    if from_champion {
        (kit_of(game, target).before_damage)(game, target);
    }
}
