use protocol::{DamageKind, UnitId};

use crate::Game;
use crate::stats::Bonus;
use crate::unit::Unit;

pub const SPELLBLADE: u32 = 3057;
pub const TRI_FORCE: u32 = 3078;
pub const BANE_BLADE: u32 = 3100;
pub const FROST_GAUNTLET: u32 = 6662;
pub const ARCHMAGES_CROWN: u32 = 3089;
pub const EDGE_OF_INFINITY: u32 = 3031;
pub const VIPERS_FANG: u32 = 3115;
pub const RUINED_BLADE: u32 = 3153;
pub const LEVIATHAN_SLAYER: u32 = 6672;
pub const SUNFIRE_SHIELD: u32 = 3068;
pub const THORN_ARMOR: u32 = 3075;
pub const TITANS_ARMOR: u32 = 3083;
pub const STASIS_HOURGLASS: u32 = 3157;
pub const HEALTH_POTION: u32 = 2003;
pub const STASIS: &str = "stasis";
pub const STASIS_COOLDOWN_BUFF: &str = "stasis_cooldown";
const SPELLBLADE_GLOW: &str = "spellblade_glow";
const STASIS_TIME: f32 = 2.5;
const STASIS_COOLDOWN: f32 = 120.0;
const POTION_TIME: f32 = 15.0;
const POTION_HEALING: f32 = 120.0;
const THORNS_WOUNDS: f32 = 3.0;
const IMMOLATE_TIME: f32 = 3.0;
const QUICKEN: &str = "quicken";
const QUICKEN_TIME: f32 = 2.0;
const QUICKEN_SPEED: f32 = 20.0;
const TITAN_HEART_HEALTH: f32 = 2000.0;
const TITAN_HEART_CALM: f32 = 8.0;
const TITAN_VITALITY: f32 = 0.12;
const CLAWING_SHADOWS: &str = "clawing_shadows";
const CLAWING_SHADOWS_HITS: u32 = 3;
const CLAWING_SHADOWS_DURATION: f32 = 6.0;
const CLAWING_SHADOWS_SLOW: f32 = 0.3;
const CLAWING_SHADOWS_SLOW_TIME: f32 = 1.0;
const KRAKEN: &str = "bring_it_down";
const KRAKEN_GLOW: &str = "bring_it_down_glow";
const KRAKEN_STACKS: u32 = 2;
const KRAKEN_DURATION: f32 = 4.0;
const KRAKEN_DAMAGE: f32 = 150.0;
const KRAKEN_DAMAGE_PER_LEVEL: f32 = 5.0;
const KRAKEN_RAMP_LEVEL: f32 = 9.0;
const KRAKEN_MISSING_HEALTH_AMP: f32 = 0.75;

#[derive(Debug, Clone, PartialEq)]
pub struct Trade {
    items: [Option<u32>; 6],
    stacks: [u8; 6],
    gold: f32,
}

pub fn cooldown(game: &Game, unit: &Unit, slot: usize) -> f32 {
    match unit.items.get(slot).copied().flatten() {
        Some(STASIS_HOURGLASS) => unit
            .buff(STASIS_COOLDOWN_BUFF)
            .map_or(0.0, |buff| (buff.expires - game.time).max(0.0)),
        _ => 0.0,
    }
}

pub fn activate(game: &mut Game, id: UnitId, slot: usize) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    if !unit.alive() {
        return;
    }
    match unit.items.get(slot).copied().flatten() {
        Some(STASIS_HOURGLASS) if !unit.has_buff(STASIS_COOLDOWN_BUFF) && !unit.is_stunned() => {
            let position = unit.position;
            let stasis = game.buff(STASIS, STASIS_TIME);
            let cooldown = game.buff(STASIS_COOLDOWN_BUFF, STASIS_COOLDOWN);
            game.add_buff(id, stasis);
            game.add_buff(id, cooldown);
            if let Some(unit) = game.unit_mut(id) {
                unit.path.clear();
                unit.action = crate::unit::Action::None;
                unit.trades.clear();
            }
            game.effect("Stasis", position, Some(id));
        }
        Some(HEALTH_POTION) => {
            let left = unit.buff("potion").map_or(0.0, |buff| {
                buff.value * buff.stacks as f32 * (buff.expires - game.time).max(0.0)
            });
            let mut potion = game.buff("potion", POTION_TIME);
            potion.stacks = unit.buff("potion").map_or(1, |buff| buff.stacks + 1);
            potion.value = (left + POTION_HEALING) / POTION_TIME / potion.stacks as f32;
            game.add_buff(id, potion);
            if let Some(unit) = game.unit_mut(id) {
                unit.stacks[slot] = unit.stacks[slot].saturating_sub(1);
                if unit.stacks[slot] == 0 {
                    unit.items[slot] = None;
                }
                unit.trades.clear();
            }
            game.refresh_stats(id);
        }
        _ => {}
    }
}

pub fn has(unit: &Unit, item: u32) -> bool {
    unit.items.contains(&Some(item))
}

pub fn passive_bonus(game: &Game, unit: &Unit) -> Bonus {
    let mut bonus = Bonus::default();
    if has(unit, TITANS_ARMOR) {
        let item_health: f32 = unit
            .items
            .iter()
            .flatten()
            .filter_map(|item| game.data.item(*item))
            .map(|item| item.stats.health)
            .sum();
        bonus.health += TITAN_VITALITY * item_health;
    }
    if has(unit, ARCHMAGES_CROWN) {
        bonus.ability_power_percent += 0.3;
    }
    if has(unit, EDGE_OF_INFINITY) {
        bonus.crit_damage += 0.3;
    }
    bonus
}

pub fn buy(game: &mut Game, id: UnitId, item: u32) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    if !(unit.dead || game.in_fountain(unit)) {
        return;
    }
    let Some(quote) = game.data.quote(item, &unit.items, &unit.stacks) else {
        return;
    };
    let cost = quote.cost as f32;
    if unit.gold < cost {
        return;
    }
    trade(game, id, quote.items, quote.stacks, -cost);
}

pub fn sell(game: &mut Game, id: UnitId, slot: usize) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    if !(unit.dead || game.in_fountain(unit)) {
        return;
    }
    let Some(item) = unit.items.get(slot).copied().flatten() else {
        return;
    };
    let mut items = unit.items;
    let mut stacks = unit.stacks;
    stacks[slot] = stacks[slot].saturating_sub(1);
    if stacks[slot] == 0 {
        items[slot] = None;
    }
    let refund = game.data.sell_price(item) as f32;
    trade(game, id, items, stacks, refund);
}

fn trade(game: &mut Game, id: UnitId, items: [Option<u32>; 6], stacks: [u8; 6], gold: f32) {
    if let Some(unit) = game.unit_mut(id) {
        unit.trades.push(Trade {
            items: unit.items,
            stacks: unit.stacks,
            gold,
        });
        unit.items = items;
        unit.stacks = stacks;
        unit.gold += gold;
    }
    game.refresh_stats(id);
}

pub fn undo(game: &mut Game, id: UnitId) {
    let Some(unit) = game.unit_mut(id) else {
        return;
    };
    let Some(trade) = unit.trades.pop() else {
        return;
    };
    if unit.gold < trade.gold {
        unit.trades.clear();
        return;
    }
    unit.items = trade.items;
    unit.stacks = trade.stacks;
    unit.gold -= trade.gold;
    game.refresh_stats(id);
}

pub fn on_attack_hit(game: &mut Game, attacker: UnitId, target: UnitId) {
    let Some(unit) = game.unit(attacker) else {
        return;
    };
    if unit.champion().is_none() {
        return;
    }
    let stats = unit.stats;
    let ranged = stats.attack_range > 300.0;
    if has(unit, VIPERS_FANG) {
        game.damage(
            Some(attacker),
            target,
            15.0 + 0.2 * stats.ability_power,
            DamageKind::Magic,
            false,
        );
    }
    let Some(unit) = game.unit(attacker) else {
        return;
    };
    if has(unit, RUINED_BLADE)
        && let Some(victim) = game.unit(target).filter(|victim| !victim.is_structure())
    {
        let percent = if ranged { 0.06 } else { 0.09 };
        let damage = (victim.health * percent).max(15.0);
        let champion = victim.champion().is_some();
        game.damage(Some(attacker), target, damage, DamageKind::Physical, false);
        if champion {
            clawing_shadows(game, attacker, target);
        }
    }
    let Some(unit) = game.unit(attacker) else {
        return;
    };
    if has(unit, TRI_FORCE) {
        let mut quicken = game.buff(QUICKEN, QUICKEN_TIME);
        quicken.bonus.move_speed = QUICKEN_SPEED;
        game.add_buff(attacker, quicken);
    }
    let Some(unit) = game.unit(attacker) else {
        return;
    };
    if has(unit, LEVIATHAN_SLAYER) {
        bring_it_down(game, attacker, target);
    }
    spellblade_hit(game, attacker, target);
}

fn clawing_shadows(game: &mut Game, attacker: UnitId, target: UnitId) {
    let Some(victim) = game.unit(target) else {
        return;
    };
    let mine =
        |buff: &crate::unit::Buff| buff.name == CLAWING_SHADOWS && buff.source == Some(attacker);
    let stacks = victim
        .buffs
        .iter()
        .find(|buff| mine(buff))
        .map_or(0, |buff| buff.stacks)
        + 1;
    let mut mark = game.buff(CLAWING_SHADOWS, CLAWING_SHADOWS_DURATION);
    mark.source = Some(attacker);
    mark.stacks = stacks;
    if stacks < CLAWING_SHADOWS_HITS {
        game.add_sourced_buff(target, mark);
    } else if let Some(victim) = game.unit_mut(target) {
        victim.buffs.retain(|buff| !mine(buff));
    }
    if stacks >= CLAWING_SHADOWS_HITS {
        game.slow(
            target,
            CLAWING_SHADOWS_SLOW,
            CLAWING_SHADOWS_SLOW_TIME,
            None,
        );
    }
}

fn bring_it_down(game: &mut Game, attacker: UnitId, target: UnitId) {
    let Some(unit) = game.unit(attacker) else {
        return;
    };
    let stacks = unit.buff(KRAKEN).map_or(0, |buff| buff.stacks);
    if stacks < KRAKEN_STACKS {
        let mut buff = game.buff(KRAKEN, KRAKEN_DURATION);
        buff.stacks = stacks + 1;
        game.add_buff(attacker, buff);
        return;
    }
    let level = f32::from(unit.level);
    let base = KRAKEN_DAMAGE + KRAKEN_DAMAGE_PER_LEVEL * (level - KRAKEN_RAMP_LEVEL + 1.0).max(0.0);
    let Some(victim) = game.unit(target) else {
        return;
    };
    let missing = 1.0 - victim.health / victim.stats.max_health.max(1.0);
    let position = victim.position;
    if let Some(unit) = game.unit_mut(attacker) {
        unit.buffs.retain(|buff| buff.name != KRAKEN);
    }
    game.effect("KrakenHit", position, Some(target));
    game.damage(
        Some(attacker),
        target,
        base * (1.0 + KRAKEN_MISSING_HEALTH_AMP * missing),
        DamageKind::Physical,
        false,
    );
}

pub fn spellblade_hit(game: &mut Game, attacker: UnitId, target: UnitId) {
    let Some(unit) = game.unit(attacker) else {
        return;
    };
    if !unit.has_buff("spellblade") {
        return;
    }
    let stats = unit.stats;
    let (amount, kind) = if has(unit, BANE_BLADE) {
        (
            0.75 * stats.base_attack_damage + 0.5 * stats.ability_power,
            DamageKind::Magic,
        )
    } else if has(unit, TRI_FORCE) {
        (2.0 * stats.base_attack_damage, DamageKind::Physical)
    } else if has(unit, FROST_GAUNTLET) {
        (1.5 * stats.base_attack_damage, DamageKind::Physical)
    } else {
        (stats.base_attack_damage, DamageKind::Physical)
    };
    let slows = has(unit, FROST_GAUNTLET);
    let cooldown = game.buff("spellblade_cooldown", 1.5);
    if let Some(unit) = game.unit_mut(attacker) {
        unit.buffs.retain(|buff| buff.name != "spellblade");
        unit.buffs.push(cooldown);
    }
    game.damage(Some(attacker), target, amount, kind, false);
    if slows {
        game.slow(target, 0.3, 1.5, None);
    }
}

fn spellblade_effect(unit: &Unit) -> Option<&'static str> {
    if !unit.has_buff("spellblade") {
        return None;
    }
    [
        (BANE_BLADE, "SpellbladeLichBane"),
        (TRI_FORCE, "SpellbladeTrinityForce"),
        (FROST_GAUNTLET, "SpellbladeIceborn"),
        (SPELLBLADE, "SpellbladeSheen"),
    ]
    .into_iter()
    .find(|(item, _)| has(unit, *item))
    .map(|(_, effect)| effect)
}

pub fn tick(game: &mut Game, id: UnitId) {
    let Some(unit) = game.unit(id) else {
        return;
    };
    let effect = spellblade_effect(unit);
    let kraken_ready = unit
        .buff(KRAKEN)
        .is_some_and(|buff| buff.stacks >= KRAKEN_STACKS);
    crate::champions::glow(
        game,
        id,
        SPELLBLADE_GLOW,
        effect.is_some(),
        [effect.unwrap_or_default(), "SpellbladeEnd"],
    );
    crate::champions::glow(
        game,
        id,
        KRAKEN_GLOW,
        kraken_ready,
        ["KrakenReady", "KrakenSpent"],
    );
    let Some(unit) = game.unit(id) else {
        return;
    };
    if !unit.trades.is_empty()
        && !(unit.dead || game.in_fountain(unit))
        && let Some(unit) = game.unit_mut(id)
    {
        unit.trades.clear();
    }
    let Some(unit) = game.unit(id) else {
        return;
    };
    if unit.dead || !game.tick.is_multiple_of(30) {
        return;
    }
    let team = unit.team;
    let position = unit.position;
    if let Some(potion) = unit.buff("potion") {
        let heal = potion.value * potion.stacks as f32;
        game.heal(id, heal);
    }
    let Some(unit) = game.unit(id) else {
        return;
    };
    let bonus_health = unit.stats.max_health
        - (unit.base.health + unit.base.health_per_level * crate::stats::growth(unit.level));
    let fighting = game.time - unit.hurt_at.max(unit.dealt_at) <= IMMOLATE_TIME;
    if has(unit, SUNFIRE_SHIELD) && fighting {
        let dealt_at = unit.dealt_at;
        let damage = 20.0 + 0.01 * bonus_health.max(0.0);
        for enemy in game.enemies_near(team, position, 325.0) {
            if game.unit(enemy).is_some_and(Unit::is_structure) {
                continue;
            }
            game.damage(Some(id), enemy, damage, DamageKind::Magic, false);
        }
        if let Some(unit) = game.unit_mut(id) {
            unit.dealt_at = dealt_at;
        }
    }
    if let Some(unit) = game.unit(id)
        && has(unit, TITANS_ARMOR)
        && bonus_health >= TITAN_HEART_HEALTH
        && game.time - unit.hurt_at >= TITAN_HEART_CALM
    {
        let heal = unit.stats.max_health * 0.025;
        game.heal(id, heal);
    }
}

pub fn on_attacked(game: &mut Game, attacker: UnitId, target: UnitId) {
    let Some(unit) = game.unit(target) else {
        return;
    };
    if !has(unit, THORN_ARMOR) {
        return;
    }
    let bonus_armor = unit.stats.armor - unit.stats.base_armor;
    game.damage(
        Some(target),
        attacker,
        20.0 + 0.1 * bonus_armor,
        DamageKind::Magic,
        false,
    );
    let time = game.time;
    let Some(source) = game.unit(attacker) else {
        return;
    };
    let position = source.position;
    let wounds_needed = source.champion().is_some()
        && source
            .buff("grievous_wounds")
            .is_none_or(|buff| buff.expires < time + THORNS_WOUNDS);
    game.effect("Thorns", position, Some(attacker));
    if wounds_needed {
        let wounds = game.buff("grievous_wounds", THORNS_WOUNDS);
        game.add_buff(attacker, wounds);
    }
}
