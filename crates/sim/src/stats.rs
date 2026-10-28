use asset::data::{ItemStats, UnitStats};

pub const MAX_ATTACK_SPEED: f32 = 2.5;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Bonus {
    pub attack_damage: f32,
    pub ability_power: f32,
    pub armor: f32,
    pub magic_resist: f32,
    pub health: f32,
    pub mana: f32,
    pub health_regen_percent: f32,
    pub mana_regen_percent: f32,
    pub attack_speed: f32,
    pub cripple: f32,
    pub crit_chance: f32,
    pub crit_damage: f32,
    pub move_speed: f32,
    pub move_speed_percent: f32,
    pub slow: f32,
    pub ability_haste: f32,
    pub life_steal: f32,
    pub lethality: f32,
    pub armor_penetration: f32,
    pub crit_bonus_armor_penetration: f32,
    pub magic_penetration: f32,
    pub magic_penetration_percent: f32,
    pub ability_power_percent: f32,
    pub armor_percent: f32,
    pub magic_resist_percent: f32,
    pub damage_reduction: f32,
    pub magic_damage_reduction: f32,
    pub attack_range: f32,
    pub size: f32,
}

impl Bonus {
    pub fn item(stats: &ItemStats) -> Self {
        Self {
            attack_damage: stats.attack_damage,
            ability_power: stats.ability_power,
            armor: stats.armor,
            magic_resist: stats.magic_resist,
            health: stats.health,
            mana: stats.mana,
            health_regen_percent: stats.health_regen,
            mana_regen_percent: stats.mana_regen,
            attack_speed: stats.attack_speed,
            crit_chance: stats.crit_chance,
            move_speed: stats.move_speed,
            move_speed_percent: stats.move_speed_percent,
            ability_haste: stats.ability_haste,
            life_steal: stats.life_steal,
            lethality: stats.lethality,
            armor_penetration: stats.armor_penetration,
            magic_penetration: stats.magic_penetration,
            magic_penetration_percent: stats.magic_penetration_percent,
            ..Self::default()
        }
    }

    pub fn combine(mut self, other: Self) -> Self {
        self.attack_damage += other.attack_damage;
        self.ability_power += other.ability_power;
        self.armor += other.armor;
        self.magic_resist += other.magic_resist;
        self.health += other.health;
        self.mana += other.mana;
        self.health_regen_percent += other.health_regen_percent;
        self.mana_regen_percent += other.mana_regen_percent;
        self.attack_speed += other.attack_speed;
        self.cripple = 1.0 - (1.0 - self.cripple) * (1.0 - other.cripple);
        self.crit_chance += other.crit_chance;
        self.crit_damage += other.crit_damage;
        self.move_speed += other.move_speed;
        self.move_speed_percent += other.move_speed_percent;
        self.slow = self.slow.max(other.slow);
        self.ability_haste += other.ability_haste;
        self.life_steal += other.life_steal;
        self.lethality += other.lethality;
        self.armor_penetration =
            1.0 - (1.0 - self.armor_penetration) * (1.0 - other.armor_penetration);
        self.crit_bonus_armor_penetration += other.crit_bonus_armor_penetration;
        self.magic_penetration += other.magic_penetration;
        self.magic_penetration_percent =
            1.0 - (1.0 - self.magic_penetration_percent) * (1.0 - other.magic_penetration_percent);
        self.ability_power_percent += other.ability_power_percent;
        self.armor_percent += other.armor_percent;
        self.magic_resist_percent += other.magic_resist_percent;
        self.damage_reduction =
            1.0 - (1.0 - self.damage_reduction) * (1.0 - other.damage_reduction);
        self.magic_damage_reduction =
            1.0 - (1.0 - self.magic_damage_reduction) * (1.0 - other.magic_damage_reduction);
        self.attack_range += other.attack_range;
        self.size += other.size;
        self
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Stats {
    pub level: u8,
    pub base_max_health: f32,
    pub max_health: f32,
    pub max_mana: f32,
    pub health_regen: f32,
    pub mana_regen: f32,
    pub base_attack_damage: f32,
    pub attack_damage: f32,
    pub ability_power: f32,
    pub base_armor: f32,
    pub armor: f32,
    pub base_magic_resist: f32,
    pub magic_resist: f32,
    pub attack_speed: f32,
    pub base_attack_speed: f32,
    pub attack_range: f32,
    pub crit_chance: f32,
    pub crit_damage: f32,
    pub base_move_speed: f32,
    pub move_speed: f32,
    pub ability_haste: f32,
    pub life_steal: f32,
    pub lethality: f32,
    pub armor_penetration: f32,
    pub crit_bonus_armor_penetration: f32,
    pub magic_penetration: f32,
    pub magic_penetration_percent: f32,
    pub damage_reduction: f32,
    pub magic_damage_reduction: f32,
    pub bonus_size: f32,
}

pub fn growth(level: u8) -> f32 {
    let steps = f32::from(level.max(1) - 1);
    steps * (0.7025 + 0.0175 * steps)
}

pub fn compute(base: &UnitStats, level: u8, bonus: &Bonus) -> Stats {
    let growth = growth(level);
    let base_health = base.health + base.health_per_level * growth;
    let base_mana = base.mana + base.mana_per_level * growth;
    let base_attack_damage = base.attack_damage + base.attack_damage_per_level * growth;
    let base_armor = base.armor + base.armor_per_level * growth;
    let base_magic_resist = base.magic_resist + base.magic_resist_per_level * growth;
    let base_health_regen = base.health_regen + base.health_regen_per_level * growth;
    let base_mana_regen = base.mana_regen + base.mana_regen_per_level * growth;
    let level_attack_speed = base.attack_speed_per_level / 100.0 * growth;
    let ratio = if base.attack_speed_ratio > 0.0 {
        base.attack_speed_ratio
    } else {
        base.attack_speed
    };
    let attack_speed = ((base.attack_speed + ratio * (level_attack_speed + bonus.attack_speed))
        * (1.0 - bonus.cripple.clamp(0.0, 1.0)))
    .min(MAX_ATTACK_SPEED);
    let flat_speed = base.move_speed + bonus.move_speed;
    let raw_speed =
        flat_speed * (1.0 + bonus.move_speed_percent) * (1.0 - bonus.slow.clamp(0.0, 0.99));
    Stats {
        level,
        base_max_health: base_health,
        max_health: base_health + bonus.health,
        max_mana: base_mana + bonus.mana,
        health_regen: base_health_regen * (1.0 + bonus.health_regen_percent),
        mana_regen: base_mana_regen * (1.0 + bonus.mana_regen_percent),
        base_attack_damage,
        attack_damage: base_attack_damage + bonus.attack_damage,
        ability_power: bonus.ability_power * (1.0 + bonus.ability_power_percent),
        base_armor,
        armor: (base_armor + bonus.armor) * (1.0 + bonus.armor_percent),
        base_magic_resist,
        magic_resist: (base_magic_resist + bonus.magic_resist) * (1.0 + bonus.magic_resist_percent),
        attack_speed,
        base_attack_speed: base.attack_speed,
        attack_range: base.attack_range + bonus.attack_range,
        crit_chance: bonus.crit_chance.min(1.0),
        crit_damage: base.crit_damage + bonus.crit_damage,
        base_move_speed: base.move_speed,
        move_speed: soft_cap(raw_speed),
        ability_haste: bonus.ability_haste,
        life_steal: bonus.life_steal,
        lethality: bonus.lethality,
        armor_penetration: bonus.armor_penetration,
        crit_bonus_armor_penetration: bonus.crit_bonus_armor_penetration,
        magic_penetration: bonus.magic_penetration,
        magic_penetration_percent: bonus.magic_penetration_percent,
        damage_reduction: bonus.damage_reduction.clamp(0.0, 0.9),
        magic_damage_reduction: bonus.magic_damage_reduction.clamp(0.0, 0.9),
        bonus_size: bonus.size,
    }
}

fn soft_cap(speed: f32) -> f32 {
    let speed = if speed > 490.0 {
        415.0 + (490.0 - 415.0) * 0.8 + (speed - 490.0) * 0.5
    } else if speed > 415.0 {
        415.0 + (speed - 415.0) * 0.8
    } else if speed < 220.0 {
        220.0 * 0.5 + speed * 0.5
    } else {
        speed
    };
    speed.max(110.0)
}

pub fn cooldown(base: f32, ability_haste: f32) -> f32 {
    base * 100.0 / (100.0 + ability_haste.max(0.0))
}

pub fn physical_mitigation(
    armor: f32,
    ignored: f32,
    lethality: f32,
    armor_penetration: f32,
    attacker_level: u8,
) -> f32 {
    let effective = if armor > 0.0 {
        let after_percent = (armor * (1.0 - armor_penetration) - ignored).max(0.0);
        let flat = lethality * (0.6 + 0.4 * f32::from(attacker_level) / 18.0);
        (after_percent - flat).max(0.0)
    } else {
        armor
    };
    resist_multiplier(effective)
}

pub fn magic_mitigation(magic_resist: f32, flat: f32, percent: f32) -> f32 {
    let effective = if magic_resist > 0.0 {
        (magic_resist * (1.0 - percent) - flat).max(0.0)
    } else {
        magic_resist
    };
    resist_multiplier(effective)
}

pub fn resist_multiplier(resist: f32) -> f32 {
    if resist >= 0.0 {
        100.0 / (100.0 + resist)
    } else {
        2.0 - 100.0 / (100.0 - resist)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emperor() -> UnitStats {
        UnitStats {
            health: 575.0,
            health_per_level: 108.0,
            attack_damage: 56.0,
            attack_damage_per_level: 3.5,
            armor: 25.0,
            armor_per_level: 5.0,
            move_speed: 330.0,
            attack_speed: 0.625,
            attack_speed_ratio: 0.694,
            attack_speed_per_level: 5.0,
            crit_damage: 1.75,
            ..UnitStats::default()
        }
    }

    #[test]
    fn level_one_uses_base_stats() {
        let stats = compute(&emperor(), 1, &Bonus::default());
        assert_eq!(stats.max_health, 575.0);
        assert_eq!(stats.attack_damage, 56.0);
        assert_eq!(stats.attack_speed, 0.625);
        assert_eq!(stats.move_speed, 330.0);
    }

    #[test]
    fn regen_is_stored_per_second() {
        let base = UnitStats {
            health_regen: 1.1,
            health_regen_per_level: 0.12,
            mana_regen: 1.6,
            ..emperor()
        };
        let stats = compute(&base, 1, &Bonus::default());
        assert!((stats.health_regen * 5.0 - 5.5).abs() < 1e-4);
        assert!((stats.mana_regen * 5.0 - 8.0).abs() < 1e-4);
        let doubled = Bonus {
            health_regen_percent: 1.0,
            ..Bonus::default()
        };
        let stats = compute(&base, 18, &doubled);
        assert!((stats.health_regen - (1.1 + 0.12 * 17.0) * 2.0).abs() < 1e-4);
    }

    #[test]
    fn level_eighteen_adds_seventeen_full_growths() {
        let stats = compute(&emperor(), 18, &Bonus::default());
        assert!((stats.max_health - (575.0 + 108.0 * 17.0)).abs() < 0.01);
        assert!((stats.armor - (25.0 + 5.0 * 17.0)).abs() < 0.01);
    }

    #[test]
    fn bonus_attack_speed_uses_the_ratio_and_caps() {
        let bonus = Bonus {
            attack_speed: 0.5,
            ..Bonus::default()
        };
        let stats = compute(&emperor(), 1, &bonus);
        assert!((stats.attack_speed - (0.625 + 0.694 * 0.5)).abs() < 1e-5);
        let bonus = Bonus {
            attack_speed: 10.0,
            ..Bonus::default()
        };
        assert_eq!(
            compute(&emperor(), 1, &bonus).attack_speed,
            MAX_ATTACK_SPEED
        );
    }

    #[test]
    fn cripples_cut_total_attack_speed_and_stack_multiplicatively() {
        let cripple = |amount| Bonus {
            attack_speed: 0.5,
            cripple: amount,
            ..Bonus::default()
        };
        let bonus = cripple(0.2).combine(cripple(0.2));
        let stats = compute(&emperor(), 1, &bonus);
        assert!((stats.attack_speed - (0.625 + 0.694) * 0.64).abs() < 1e-5);
    }

    #[test]
    fn armor_halves_damage_at_one_hundred() {
        assert_eq!(resist_multiplier(100.0), 0.5);
        assert!((physical_mitigation(100.0, 0.0, 0.0, 0.0, 1) - 0.5).abs() < 1e-6);
        assert!(resist_multiplier(-50.0) > 1.0);
    }

    #[test]
    fn haste_converts_to_cooldown() {
        assert_eq!(cooldown(10.0, 100.0), 5.0);
    }

    #[test]
    fn move_speed_soft_caps() {
        let bonus = Bonus {
            move_speed: 300.0,
            ..Bonus::default()
        };
        assert!(compute(&emperor(), 1, &bonus).move_speed < 630.0);
    }
}
