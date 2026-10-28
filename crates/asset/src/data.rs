use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct UnitStats {
    pub health: f32,
    pub health_per_level: f32,
    pub health_regen: f32,
    pub health_regen_per_level: f32,
    pub mana: f32,
    pub mana_per_level: f32,
    pub mana_regen: f32,
    pub mana_regen_per_level: f32,
    pub attack_damage: f32,
    pub attack_damage_per_level: f32,
    pub armor: f32,
    pub armor_per_level: f32,
    pub magic_resist: f32,
    pub magic_resist_per_level: f32,
    pub move_speed: f32,
    pub attack_range: f32,
    pub attack_speed: f32,
    pub attack_speed_ratio: f32,
    pub attack_speed_per_level: f32,
    pub crit_damage: f32,
    pub selection_radius: f32,
    pub pathing_radius: f32,
    pub acquisition_range: f32,
    pub gameplay_radius: f32,
    pub selection_height: f32,
    pub gold_given: f32,
    pub xp_given: f32,
    pub attack: Attack,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Attack {
    pub total_time: f32,
    pub cast_time: f32,
    pub delay_offset_percent: f32,
    pub windup_modifier: f32,
    pub missile_speed: f32,
}

impl Default for Attack {
    fn default() -> Self {
        Self {
            total_time: 0.0,
            cast_time: 0.0,
            delay_offset_percent: 0.0,
            windup_modifier: 1.0,
            missile_speed: 0.0,
        }
    }
}

impl Attack {
    pub fn windup_fraction(&self) -> f32 {
        if self.total_time > 0.0 && self.cast_time > 0.0 {
            self.cast_time / self.total_time
        } else {
            0.3 + self.delay_offset_percent
        }
    }

    pub fn windup(&self, period: f32, base_period: f32) -> f32 {
        let fraction = self.windup_fraction();
        let base = base_period * fraction;
        base + (period * fraction - base) * self.windup_modifier
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stat {
    AbilityPower,
    Armor,
    AttackDamage,
    AttackSpeed,
    MagicResist,
    MoveSpeed,
    CritChance,
    CritDamage,
    MaxHealth,
    CurrentHealth,
    MissingHealth,
    Level,
    Unknown,
}

impl Stat {
    pub fn from_id(id: u64) -> Self {
        match id {
            0 => Stat::AbilityPower,
            1 => Stat::Armor,
            2 => Stat::AttackDamage,
            3 => Stat::AttackSpeed,
            5 => Stat::MagicResist,
            6 => Stat::MoveSpeed,
            8 => Stat::CritChance,
            9 => Stat::CritDamage,
            12 => Stat::MaxHealth,
            13 => Stat::CurrentHealth,
            14 => Stat::MissingHealth,
            _ => Stat::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatFormula {
    Total,
    Base,
    Bonus,
}

impl StatFormula {
    pub fn from_id(id: u64) -> Self {
        match id {
            1 => StatFormula::Base,
            2 => StatFormula::Bonus,
            _ => StatFormula::Total,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Part {
    Number(f32),
    Value(String),
    Effect(u8),
    StatByValue {
        value: String,
        stat: Stat,
        formula: StatFormula,
    },
    StatByCoefficient {
        coefficient: f32,
        stat: Stat,
        formula: StatFormula,
    },
    StatBySubPart {
        part: Box<Part>,
        stat: Stat,
        formula: StatFormula,
    },
    ByLevelBreakpoints {
        level1: f32,
        initial_per_level: f32,
        breakpoints: Vec<Breakpoint>,
    },
    ByLevelInterpolation {
        start: f32,
        end: f32,
    },
    Product(Box<Part>, Box<Part>),
    Sum(Vec<Part>),
    BuffCounter {
        buff: String,
        coefficient: f32,
    },
    MaxResource(f32),
    Calculation(String),
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Breakpoint {
    pub level: u8,
    pub additional: f32,
    pub per_level_after: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Calculation {
    pub name: String,
    pub parts: Vec<Part>,
    pub multiplier: Option<Part>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataValue {
    pub name: String,
    pub values: Vec<f32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Spell {
    pub name: String,
    pub targeting: Targeting,
    pub display_name: String,
    pub description: String,
    pub icon: String,
    pub animation: String,
    pub cooldown: Vec<f32>,
    pub mana: Vec<f32>,
    pub cast_range: Vec<f32>,
    pub cast_radius: Vec<f32>,
    pub cast_radius_secondary: Vec<f32>,
    pub cast_time: f32,
    pub total_time: f32,
    pub cast_frame: f32,
    pub missile_speed: f32,
    pub missile_acceleration: f32,
    pub missile_max_speed: f32,
    pub line_width: f32,
    pub max_ammo: Vec<u8>,
    pub ammo_recharge: Vec<f32>,
    pub values: Vec<DataValue>,
    pub effects: Vec<Vec<f32>>,
    pub calculations: Vec<Calculation>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Targeting {
    #[default]
    Unit,
    Myself,
    MyselfArea,
    Location,
    Area,
    Direction,
    Cone,
}

impl Targeting {
    pub fn from_name(name: &str) -> Self {
        match name {
            "Self" => Self::Myself,
            "SelfAoe" => Self::MyselfArea,
            "Location" | "LocationClamped" | "TargetOrLocation" => Self::Location,
            "Area" | "AreaClamped" => Self::Area,
            "Direction" | "DragDirection" => Self::Direction,
            "Cone" => Self::Cone,
            _ => Self::Unit,
        }
    }

    pub fn instant(self) -> bool {
        matches!(self, Self::Myself | Self::MyselfArea)
    }
}

impl Spell {
    pub fn value(&self, name: &str, rank: u8) -> f32 {
        self.values
            .iter()
            .find(|value| value.name.eq_ignore_ascii_case(name))
            .and_then(|value| value.values.get(usize::from(rank)).or(value.values.last()))
            .copied()
            .unwrap_or(0.0)
    }

    pub fn by_rank(values: &[f32], rank: u8) -> f32 {
        values
            .get(usize::from(rank))
            .or(values.last())
            .copied()
            .unwrap_or(0.0)
    }

    pub fn cooldown(&self, rank: u8) -> f32 {
        Self::by_rank(&self.cooldown, rank)
    }

    pub fn mana(&self, rank: u8) -> f32 {
        Self::by_rank(&self.mana, rank.saturating_sub(1))
    }

    pub fn range(&self, rank: u8) -> f32 {
        Self::by_rank(&self.cast_range, rank)
    }

    pub fn calculation(&self, name: &str) -> Option<&Calculation> {
        self.calculations
            .iter()
            .find(|calculation| calculation.name.eq_ignore_ascii_case(name))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChampionData {
    pub name: String,
    pub resource: Resource,
    pub display_name: String,
    pub title: String,
    pub role: String,
    pub icon: String,
    pub splash: String,
    pub tile: String,
    pub loading: String,
    pub stats: UnitStats,
    pub slots: [String; 4],
    pub passive: Spell,
    pub spells: Vec<Spell>,
    pub balance: Balance,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Balance {
    pub damage_dealt: f32,
    pub damage_taken: f32,
    pub attack_speed: f32,
    pub ability_haste: f32,
}

impl Default for Balance {
    fn default() -> Self {
        Self {
            damage_dealt: 1.0,
            damage_taken: 1.0,
            attack_speed: 1.0,
            ability_haste: 0.0,
        }
    }
}

impl ChampionData {
    pub fn key(&self) -> String {
        self.name.to_lowercase()
    }

    pub fn spell(&self, name: &str) -> Option<&Spell> {
        self.spells
            .iter()
            .find(|spell| spell.name.eq_ignore_ascii_case(name))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Resource {
    #[default]
    Mana,
    Flow,
    None,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UnitData {
    pub name: String,
    pub stats: UnitStats,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct ItemStats {
    pub attack_damage: f32,
    pub ability_power: f32,
    pub armor: f32,
    pub magic_resist: f32,
    pub health: f32,
    pub mana: f32,
    pub health_regen: f32,
    pub mana_regen: f32,
    pub attack_speed: f32,
    pub crit_chance: f32,
    pub move_speed: f32,
    pub move_speed_percent: f32,
    pub ability_haste: f32,
    pub life_steal: f32,
    pub lethality: f32,
    pub armor_penetration: f32,
    pub magic_penetration: f32,
    pub magic_penetration_percent: f32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: u32,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub price: u32,
    pub total_price: u32,
    pub from: Vec<u32>,
    pub stats: ItemStats,
    pub tier: u8,
    pub classes: u8,
    pub categories: Vec<String>,
    pub limits: Vec<Limit>,
    pub sell_back: f32,
    pub max_stack: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limit {
    pub group: String,
    pub max: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SummonerData {
    pub id: u32,
    pub display_name: String,
    pub description: String,
    pub cooldown: f32,
    pub icon: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GameData {
    pub champions: Vec<ChampionData>,
    pub units: Vec<UnitData>,
    pub items: Vec<Item>,
    pub summoners: Vec<SummonerData>,
    pub quick_buy: QuickBuy,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QuickBuy {
    pub consumables: Vec<u32>,
    pub boots: Vec<u32>,
}

impl GameData {
    pub fn summoner(&self, id: u32) -> Option<&SummonerData> {
        self.summoners.iter().find(|summoner| summoner.id == id)
    }

    pub fn to_bytes(&self) -> postcard::Result<Vec<u8>> {
        postcard::to_allocvec(self)
    }

    pub fn from_bytes(bytes: &[u8]) -> postcard::Result<Self> {
        postcard::from_bytes(bytes)
    }

    pub fn unit(&self, name: &str) -> Option<&UnitData> {
        self.units
            .iter()
            .find(|unit| unit.name.eq_ignore_ascii_case(name))
    }

    pub fn item(&self, id: u32) -> Option<&Item> {
        self.items.iter().find(|item| item.id == id)
    }

    pub fn purchase(&self, id: u32, inventory: &[Option<u32>]) -> Option<Purchase> {
        let item = self.item(id)?;
        let mut purchase = Purchase {
            cost: item.price,
            consumed: Vec::new(),
        };
        for component in &item.from {
            self.component(*component, inventory, &mut purchase);
        }
        Some(purchase)
    }

    fn component(&self, id: u32, inventory: &[Option<u32>], purchase: &mut Purchase) {
        let owned = inventory
            .iter()
            .enumerate()
            .position(|(slot, owned)| *owned == Some(id) && !purchase.consumed.contains(&slot));
        match (owned, self.item(id)) {
            (Some(slot), _) => purchase.consumed.push(slot),
            (None, Some(item)) => {
                purchase.cost += item.price;
                for component in &item.from {
                    self.component(*component, inventory, purchase);
                }
            }
            (None, None) => {}
        }
    }

    pub fn locked(&self, id: u32, inventory: &[Option<u32>]) -> bool {
        let (Some(item), Some(purchase)) = (self.item(id), self.purchase(id, inventory)) else {
            return true;
        };
        item.limits.iter().any(|limit| {
            inventory
                .iter()
                .enumerate()
                .filter(|(slot, _)| !purchase.consumed.contains(slot))
                .filter_map(|(_, owned)| owned.and_then(|owned| self.item(owned)))
                .filter(|owned| owned.limits.iter().any(|other| other.group == limit.group))
                .count()
                >= usize::from(limit.max)
        })
    }

    pub fn stack_slot(&self, id: u32, inventory: &[Option<u32>], stacks: &[u8]) -> Option<usize> {
        let max = self.item(id)?.max_stack;
        inventory
            .iter()
            .zip(stacks)
            .position(|(owned, count)| *owned == Some(id) && *count < max)
    }

    pub fn quote(&self, id: u32, items: &[Option<u32>; 6], stacks: &[u8; 6]) -> Option<Quote> {
        let (mut items, mut stacks) = (*items, *stacks);
        if let Some(slot) = self.stack_slot(id, &items, &stacks) {
            stacks[slot] += 1;
            let cost = self.item(id)?.total_price;
            return Some(Quote {
                items,
                stacks,
                cost,
            });
        }
        if self.locked(id, &items) {
            return None;
        }
        let purchase = self.purchase(id, &items)?;
        for slot in &purchase.consumed {
            items[*slot] = None;
            stacks[*slot] = 0;
        }
        let slot = items.iter().position(Option::is_none)?;
        items[slot] = Some(id);
        stacks[slot] = 1;
        Some(Quote {
            items,
            stacks,
            cost: purchase.cost,
        })
    }

    pub fn sell_price(&self, id: u32) -> u32 {
        self.item(id)
            .map_or(0, |item| (item.total_price as f32 * item.sell_back) as u32)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quote {
    pub items: [Option<u32>; 6],
    pub stacks: [u8; 6],
    pub cost: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Purchase {
    pub cost: u32,
    pub consumed: Vec<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_consume_components_and_need_a_free_slot() {
        let item = |id: u32, price: u32, from: Vec<u32>| Item {
            id,
            price,
            total_price: price + 100 * from.len() as u32,
            from,
            max_stack: 1,
            ..Item::default()
        };
        let data = GameData {
            items: vec![item(1, 100, Vec::new()), item(2, 300, vec![1])],
            ..GameData::default()
        };
        let mut items = [None; 6];
        items[0] = Some(1);
        let quote = data.quote(2, &items, &[1, 0, 0, 0, 0, 0]).unwrap();
        assert_eq!(quote.cost, 300);
        assert_eq!(quote.items[0], Some(2));
        let full = [Some(1); 6];
        assert_eq!(
            data.quote(2, &full, &[1; 6]).map(|quote| quote.cost),
            Some(300)
        );
        assert_eq!(data.quote(1, &full, &[1; 6]), None);
    }

    #[test]
    fn windup_only_partly_shrinks_with_attack_speed_for_low_modifiers() {
        let attack = Attack {
            delay_offset_percent: -0.12,
            windup_modifier: 0.5,
            ..Attack::default()
        };
        let base = 1.0 / 0.625;
        assert!((attack.windup(base, base) - base * 0.18).abs() < 1e-5);
        let doubled = attack.windup(base / 2.0, base);
        assert!((doubled - base * 0.18 * 0.75).abs() < 1e-5);
        let full = Attack {
            windup_modifier: 1.0,
            ..attack
        };
        assert!((full.windup(base / 2.0, base) - base * 0.09).abs() < 1e-5);
    }
    use alloc::borrow::ToOwned;
    use alloc::vec;

    fn item(id: u32, price: u32, from: &[u32], limits: &[&str]) -> Item {
        Item {
            id,
            price,
            from: from.to_vec(),
            limits: limits
                .iter()
                .map(|group| Limit {
                    group: (*group).to_owned(),
                    max: 1,
                })
                .collect(),
            sell_back: 0.7,
            ..Item::default()
        }
    }

    fn data() -> GameData {
        GameData {
            items: vec![
                item(1036, 350, &[], &[]),
                item(1028, 400, &[], &[]),
                item(3044, 450, &[1036, 1028], &[]),
                item(3057, 900, &[], &["spellblade"]),
                item(3078, 733, &[3044, 3057], &["spellblade", "trinity"]),
                item(3100, 1000, &[3057], &["spellblade"]),
            ],
            ..GameData::default()
        }
    }

    #[test]
    fn components_anywhere_in_the_recipe_tree_count_towards_the_price() {
        let data = data();
        let inventory = [Some(1036), None, Some(3057), Some(1036)];
        assert_eq!(
            data.purchase(3078, &inventory),
            Some(Purchase {
                cost: 733 + 450 + 400,
                consumed: vec![0, 2],
            })
        );
        assert_eq!(
            data.purchase(3078, &[]).map(|purchase| purchase.cost),
            Some(2833)
        );
    }

    #[test]
    fn ownership_limits_ignore_the_components_a_purchase_uses_up() {
        let data = data();
        assert!(!data.locked(3078, &[Some(3057)]));
        assert!(data.locked(3100, &[Some(3078)]));
        assert!(data.locked(3057, &[Some(3057)]));
        assert!(!data.locked(3044, &[Some(3078)]));
        assert!(data.locked(9999, &[]));
    }

    #[test]
    fn stackable_items_join_a_stack_that_has_room() {
        let mut data = data();
        data.items[0].max_stack = 5;
        let inventory = [Some(1028), Some(1036), Some(1036)];
        assert_eq!(data.stack_slot(1036, &inventory, &[1, 5, 2]), Some(2));
        assert_eq!(data.stack_slot(1036, &inventory, &[1, 5, 5]), None);
        assert_eq!(data.stack_slot(1028, &inventory, &[1, 5, 2]), None);
    }

    #[test]
    fn items_sell_for_their_sell_back_share() {
        let mut data = data();
        data.items[0].total_price = 350;
        assert_eq!(data.sell_price(1036), 245);
    }
}
