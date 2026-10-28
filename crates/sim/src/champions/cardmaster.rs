use asset::clip_id;
use asset::data::Spell;
use glam::Vec2;
use protocol::{DamageKind, Slot, TICK, Target, UnitId};

use super::{
    Cast, Kit, begin, begin_channel, calculate, clamp_point, named_spell, rank, spell_of, spend,
};
use crate::stats;
use crate::unit::{Unit, slot_index};
use crate::{Game, Hit, Missile, Motion};

const CARD_RANGE: f32 = 1450.0;
const WILD_CARDS_HIT: &str = "wild_cards_hit";
const GATE_RANGE: f32 = 5500.0;
const GATE_CHANNEL: f32 = 1.5;
const PICK_WINDOW: f32 = 6.0;
const DECK_STACKS: f32 = 3.0;
const BLUE_MANA: f32 = 70.0;
const BLUE_MANA_PER_RANK: f32 = 20.0;
const RED_RADIUS: f32 = 325.0;
const RED_SLOW: f32 = 0.3;
const RED_SLOW_PER_RANK: f32 = 0.05;
const RED_SLOW_DURATION: f32 = 2.5;
const CARD_INTERVAL: f32 = 0.5;
const CARDS: [(&str, Card); 3] = [
    ("card_blue", Card::Blue),
    ("card_red", Card::Red),
    ("card_gold", Card::Gold),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Card {
    Blue,
    Red,
    Gold,
}

impl Card {
    fn sound(self, phase: &str) -> &'static str {
        match (self, phase) {
            (Self::Blue, "attack") => "w_blue_attack",
            (Self::Red, "attack") => "w_red_attack",
            (Self::Gold, "attack") => "w_gold_attack",
            (Self::Blue, "hit") => "w_blue_hit",
            (Self::Red, "hit") => "w_red_hit",
            (Self::Gold, "hit") => "w_gold_hit",
            (Self::Blue, _) => "w_blue",
            (Self::Red, _) => "w_red",
            (Self::Gold, _) => "w_gold",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Blue => "card_blue",
            Self::Red => "card_red",
            Self::Gold => "card_gold",
        }
    }
}

pub const KIT: Kit = Kit {
    died,
    recast,
    recasting,
    cast,
    fire,
    range,
    missile_hit,
    attack_clip,
    attack_sound,
    launch_attack,
    on_attack_hit,
    on_kill,
    tick,
    indicator,
    respawned,
    silent_hits: &[Slot::W],
    ..Kit::NONE
};

fn range(_: &Game, _: &Unit, slot: Slot, base: f32) -> f32 {
    match slot {
        Slot::Q => CARD_RANGE,
        Slot::R => GATE_RANGE,
        _ => base,
    }
}

fn recasting(unit: &Unit, slot: Slot) -> bool {
    match slot {
        Slot::W => unit.has_buff("pick_a_card"),
        Slot::R => unit.has_buff("destiny"),
        _ => false,
    }
}

fn recast(game: &mut Game, id: UnitId, slot: Slot, target: Target) -> bool {
    let Some(unit) = game.unit(id) else {
        return false;
    };
    match slot {
        Slot::W if unit.has_buff("pick_a_card") => {
            lock_card(game, id);
            true
        }
        Slot::R if unit.has_buff("destiny") => {
            gate(game, id, target);
            true
        }
        _ => false,
    }
}

fn cast(game: &mut Game, id: UnitId, slot: Slot, target: Target, spell: &Spell, rank: u8) {
    match slot {
        Slot::Q => {
            spend(game, id, slot, spell.cooldown(rank), spell.mana(rank));
            begin(game, id, slot, target, 0.25, 0.1, "Spell1");
            game.sound(id, "q");
        }
        Slot::W => {
            spend(game, id, slot, 0.0, spell.mana(rank));
            let first = (game.next_random() * 3.0).floor();
            let mut buff = game.buff("pick_a_card", PICK_WINDOW);
            buff.value = game.time - first * CARD_INTERVAL;
            game.add_buff(id, buff);
            if let Some(unit) = game.unit_mut(id) {
                unit.abilities[slot_index(Slot::W)].cooldown = 0.0;
            }
            let position = game.unit(id).map_or(Vec2::ZERO, |unit| unit.position);
            game.effect(&format!("PickACard{}", first as usize), position, Some(id));
            game.sound(id, "w");
        }
        Slot::R => {
            spend(game, id, slot, 0.0, spell.mana(rank));
            let duration = spell.value("RecastDuration", rank);
            let buff = game.buff("destiny", duration);
            game.add_buff(id, buff);
            let until = game.time + duration;
            let team = game.unit(id).map(|unit| unit.team);
            for unit in game
                .units
                .iter_mut()
                .filter(|unit| Some(unit.team) != team && unit.champion().is_some() && unit.alive())
            {
                unit.revealed_until = unit.revealed_until.max(until);
            }
            if let Some(unit) = game.unit_mut(id) {
                unit.abilities[slot_index(Slot::R)].cooldown = 0.0;
            }
            game.play(id, "Spell4", 1.0);
            game.sound(id, "r");
        }
        _ => {}
    }
}

fn current_card(game: &Game, id: UnitId) -> Card {
    let started = game
        .unit(id)
        .and_then(|unit| unit.buff("pick_a_card"))
        .map_or(0.0, |buff| buff.value);
    match (((game.time - started) / CARD_INTERVAL) as u32) % 3 {
        0 => Card::Blue,
        1 => Card::Red,
        _ => Card::Gold,
    }
}

fn lock_card(game: &mut Game, id: UnitId) {
    let card = current_card(game, id);
    if let Some(unit) = game.unit_mut(id) {
        unit.buffs.retain(|buff| buff.name != "pick_a_card");
    }
    let buff = game.buff(card.name(), 6.0);
    game.add_buff(id, buff);
    start_cooldown(game, id, Slot::W);
    game.sound(id, card.sound("w"));
    let position = game.unit(id).map_or(Vec2::ZERO, |unit| unit.position);
    game.effect(card.name(), position, Some(id));
}

fn died(game: &mut Game, id: UnitId) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    let pending: Vec<Slot> = [("pick_a_card", Slot::W), ("destiny", Slot::R)]
        .into_iter()
        .filter(|(name, _)| unit.has_buff(name))
        .map(|(_, slot)| slot)
        .collect();
    for slot in pending {
        start_cooldown(game, id, slot);
    }
}

fn start_cooldown(game: &mut Game, id: UnitId, slot: Slot) {
    let spell = spell_of(game, id, slot).unwrap_or_default();
    let rank = rank(game, id, slot);
    if let Some(unit) = game.unit_mut(id) {
        let cooldown = stats::cooldown(spell.cooldown(rank), unit.stats.ability_haste);
        let ability = &mut unit.abilities[slot_index(slot)];
        ability.cooldown = cooldown;
        ability.cooldown_total = cooldown;
    }
}

fn gate(game: &mut Game, id: UnitId, target: Target) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    if !unit.can_act() || unit.is_silenced() || unit.has_buff("root") {
        return;
    }
    let destination = game.walkable_point(clamp_point(
        unit.position,
        Vec2::from_array(target.point),
        GATE_RANGE,
    ));
    if let Some(unit) = game.unit_mut(id) {
        unit.buffs.retain(|buff| buff.name != "destiny");
    }
    start_cooldown(game, id, Slot::R);
    begin_channel(
        game,
        id,
        Slot::R,
        Target {
            point: destination.to_array(),
            unit: None,
        },
        GATE_CHANNEL,
    );
    game.hold(id, "Channel");
    game.global_effect("Gate", id, destination, None);
    game.sound(id, "r_gate");
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
            let speed = named_spell(game, id, "SealFateMissile")
                .map_or(spell.missile_speed, |missile| missile.missile_speed);
            for angle in [-28.0_f32, 0.0, 28.0] {
                let direction = Vec2::from_angle(angle.to_radians()).rotate(facing);
                let missile = game.allocate();
                game.missiles.push(Missile {
                    id: missile,
                    owner: id,
                    team,
                    kind: clip_id("SealFateMissile"),
                    position,
                    height: 100.0,
                    speed,
                    width: 40.0,
                    motion: Motion::Line {
                        direction,
                        remaining: CARD_RANGE,
                        limit: None,
                    },
                    hit: Hit::Spell(Slot::Q, damage),
                    hit_units: Vec::new(),
                    acceleration: None,
                });
            }
        }
        Slot::R => {
            if let Some(unit) = game.unit_mut(id) {
                unit.position = point;
                unit.path.clear();
            }
            game.release(id, "Channel");
            game.owned_effect("GateArrive", id, point, Some(id));
        }
        _ => {}
    }
}

fn missile_hit(game: &mut Game, missile: &Missile, target: UnitId, slot: Slot, amount: f32) {
    let owner = missile.owner;
    match slot {
        Slot::Q => {
            let struck = game.unit(target).is_some_and(|unit| {
                unit.buff(WILD_CARDS_HIT)
                    .is_some_and(|buff| buff.source == Some(owner))
            });
            if struck {
                return;
            }
            let mut mark = game.buff(WILD_CARDS_HIT, CARD_RANGE / missile.speed.max(1.0));
            mark.source = Some(owner);
            game.add_buff(target, mark);
            game.damage(Some(owner), target, amount, DamageKind::Magic, false);
        }
        Slot::W => {
            if let Some((_, card)) = CARDS
                .into_iter()
                .find(|(name, _)| clip_id(name) == missile.kind)
            {
                game.sound_on(owner, target, card.sound("hit"));
                resolve_card(game, owner, target, card);
                game.on_hit(owner, target);
            }
        }
        _ => {}
    }
}

fn resolve_card(game: &mut Game, owner: UnitId, target: UnitId, card: Card) {
    let spell = spell_of(game, owner, Slot::W).unwrap_or_default();
    let rank = rank(game, owner, Slot::W);
    let team = game
        .unit(owner)
        .map_or(protocol::Team::Blue, |unit| unit.team);
    match card {
        Card::Blue => {
            let damage = calculate(game, owner, &spell, rank, "BlueDamage");
            game.damage(Some(owner), target, damage, DamageKind::Magic, false);
            let restore = BLUE_MANA + BLUE_MANA_PER_RANK * f32::from(rank.saturating_sub(1));
            if let Some(unit) = game.unit_mut(owner) {
                unit.mana = (unit.mana + restore).min(unit.stats.max_mana);
            }
            if let Some(position) = game.unit(owner).map(|unit| unit.position) {
                game.effect("BlueCardMana", position, Some(owner));
            }
        }
        Card::Red => {
            let damage = calculate(game, owner, &spell, rank, "RedDamage");
            let center = game.unit(target).map_or(Vec2::ZERO, |unit| unit.position);
            for victim in game.enemies_near(team, center, RED_RADIUS) {
                let structure = game.unit(victim).is_some_and(Unit::is_structure);
                if structure && victim != target {
                    continue;
                }
                game.damage(Some(owner), victim, damage, DamageKind::Magic, false);
                if !structure {
                    let slow = RED_SLOW + RED_SLOW_PER_RANK * f32::from(rank.saturating_sub(1));
                    game.slow(victim, slow, RED_SLOW_DURATION, Some((owner, Slot::W)));
                }
            }
            game.effect("RedCardExplosion", center, None);
        }
        Card::Gold => {
            let damage = calculate(game, owner, &spell, rank, "GoldDamage");
            game.damage(Some(owner), target, damage, DamageKind::Magic, false);
            if !game.unit(target).is_some_and(Unit::is_structure) {
                game.stun(
                    target,
                    1.0 + 0.25 * f32::from(rank.saturating_sub(1)),
                    Some((owner, Slot::W)),
                );
            }
        }
    }
}

fn held_card(unit: &Unit) -> Option<Card> {
    CARDS
        .into_iter()
        .find(|(name, _)| unit.has_buff(name))
        .map(|(_, card)| card)
}

fn attack_clip(unit: &Unit) -> Option<&'static str> {
    held_card(unit).map(|_| "Spell1")
}

fn attack_sound(unit: &Unit) -> Option<&'static str> {
    held_card(unit).map(|card| card.sound("attack"))
}

fn launch_attack(game: &mut Game, id: UnitId, target: UnitId) -> bool {
    let Some(unit) = game.unit(id) else {
        return false;
    };
    let Some(card) = held_card(unit) else {
        return false;
    };
    let (position, team) = (unit.position, unit.team);
    if let Some(unit) = game.unit_mut(id) {
        unit.buffs.retain(|buff| buff.name != card.name());
    }
    let missile = game.allocate();
    game.missiles.push(Missile {
        id: missile,
        owner: id,
        team,
        kind: clip_id(card.name()),
        position,
        height: 100.0,
        speed: 1500.0,
        width: 0.0,
        motion: Motion::Homing(target),
        hit: Hit::Spell(Slot::W, 0.0),
        hit_units: Vec::new(),
        acceleration: None,
    });
    true
}

fn on_attack_hit(game: &mut Game, attacker: UnitId, target: UnitId) {
    let Some(unit) = game.unit(attacker) else {
        return;
    };
    let rank = unit.ability(Slot::E).rank;
    if rank == 0 {
        return;
    }
    if unit.counter < DECK_STACKS {
        let position = unit.position;
        let stacks = unit.counter + 1.0;
        if let Some(unit) = game.unit_mut(attacker) {
            unit.counter = stacks;
        }
        if stacks >= DECK_STACKS {
            game.effect("StackedDeckReady", position, Some(attacker));
        }
        return;
    }
    let spell = spell_of(game, attacker, Slot::E).unwrap_or_default();
    let target_unit = game.unit(target);
    let structure = target_unit.is_some_and(Unit::is_structure);
    let position = target_unit.map_or(Vec2::ZERO, |unit| unit.position);
    let effectiveness = if structure {
        spell.value("TowerEffectiveness", rank)
    } else {
        1.0
    };
    let damage = calculate(game, attacker, &spell, rank, "BonusDamage") * effectiveness;
    if let Some(unit) = game.unit_mut(attacker) {
        unit.counter = 0.0;
    }
    game.damage(Some(attacker), target, damage, DamageKind::Magic, false);
    game.effect("StackedDeckHit", position, Some(target));
    game.sound_on(attacker, target, "e_hit");
    game.effect("StackedDeckSpent", position, Some(attacker));
}

fn respawned(game: &mut Game, id: UnitId) {
    let full = game
        .unit(id)
        .is_some_and(|unit| unit.ability(Slot::E).rank > 0);
    if let Some(unit) = game.unit_mut(id)
        && full
    {
        unit.counter = DECK_STACKS;
    }
    if full && let Some(position) = game.unit(id).map(|unit| unit.position) {
        game.effect("StackedDeckReady", position, Some(id));
    }
}

fn on_kill(game: &mut Game, killer: UnitId, _victim: UnitId) {
    let roll = game.next_random();
    game.give_gold(killer, 1.0 + (roll * 6.0).floor());
}

fn shuffle(game: &mut Game, id: UnitId) {
    let Some(started) = game
        .unit(id)
        .and_then(|unit| unit.buff("pick_a_card"))
        .map(|buff| buff.value)
    else {
        return;
    };
    let flip = |time: f32| ((time - started) / CARD_INTERVAL).floor();
    if flip(game.time) != flip(game.time - TICK) {
        game.sound(id, "w_cycle");
    }
}

fn tick(game: &mut Game, id: UnitId) {
    shuffle(game, id);
    let Some(unit) = game.unit(id) else {
        return;
    };
    let expiring = |name: &str| {
        unit.buff(name)
            .is_some_and(|buff| buff.expires <= game.time + TICK)
    };
    let unused = [("pick_a_card", Slot::W), ("destiny", Slot::R)]
        .into_iter()
        .filter(|(name, _)| expiring(name))
        .map(|(_, slot)| slot)
        .collect::<Vec<_>>();
    for slot in unused {
        start_cooldown(game, id, slot);
    }
    let Some(unit) = game.unit(id) else {
        return;
    };
    let rank = unit.ability(Slot::E).rank;
    if rank == 0 {
        return;
    }
    let spell = spell_of(game, id, Slot::E).unwrap_or_default();
    let mut buff = game.buff("stacked_deck", f32::MAX);
    buff.bonus.attack_speed = spell.value("AttackSpeedBonus", rank) / 100.0;
    buff.stacks = unit.counter as u32;
    let current = unit
        .buff("stacked_deck")
        .map(|buff| (buff.bonus.attack_speed, buff.stacks));
    if current != Some((buff.bonus.attack_speed, buff.stacks)) {
        game.add_buff(id, buff);
    }
}

fn indicator(game: &Game, id: UnitId) -> Option<&'static str> {
    game.unit(id)
        .filter(|unit| unit.has_buff("pick_a_card"))
        .map(|_| match current_card(game, id) {
            Card::Blue => "cycle_blue",
            Card::Red => "cycle_red",
            Card::Gold => "cycle_gold",
        })
}
