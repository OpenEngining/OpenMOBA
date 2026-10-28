use asset::data::{Calculation, Part, Spell, Stat, StatFormula};

use crate::stats::Stats;

pub struct Context<'a> {
    pub spell: &'a Spell,
    pub rank: u8,
    pub stats: &'a Stats,
    pub health: f32,
    pub buff_count: &'a dyn Fn(&str) -> f32,
}

impl Context<'_> {
    pub fn calculate(&self, name: &str) -> f32 {
        self.spell
            .calculation(name)
            .map_or(0.0, |calculation| self.evaluate(calculation, 0))
    }

    fn evaluate(&self, calculation: &Calculation, depth: u8) -> f32 {
        if depth > 8 {
            return 0.0;
        }
        let sum: f32 = calculation
            .parts
            .iter()
            .map(|part| self.part(part, depth))
            .sum();
        match &calculation.multiplier {
            Some(multiplier) => sum * self.part(multiplier, depth),
            None => sum,
        }
    }

    fn stat(&self, stat: Stat, formula: StatFormula) -> f32 {
        let stats = self.stats;
        let (total, base) = match stat {
            Stat::AbilityPower => (stats.ability_power, 0.0),
            Stat::Armor => (stats.armor, stats.base_armor),
            Stat::AttackDamage => (stats.attack_damage, stats.base_attack_damage),
            Stat::AttackSpeed => (stats.attack_speed, stats.base_attack_speed),
            Stat::MagicResist => (stats.magic_resist, stats.base_magic_resist),
            Stat::MoveSpeed => (stats.move_speed, stats.base_move_speed),
            Stat::CritChance => (stats.crit_chance, 0.0),
            Stat::CritDamage => (stats.crit_damage, 0.0),
            Stat::MaxHealth => (stats.max_health, stats.base_max_health),
            Stat::CurrentHealth => (self.health, self.health),
            Stat::MissingHealth => (stats.max_health - self.health, 0.0),
            Stat::Level => (f32::from(stats.level), f32::from(stats.level)),
            Stat::Unknown => (0.0, 0.0),
        };
        match formula {
            StatFormula::Total => total,
            StatFormula::Base => base,
            StatFormula::Bonus => total - base,
        }
    }

    fn part(&self, part: &Part, depth: u8) -> f32 {
        let level = f32::from(self.stats.level.max(1));
        match part {
            Part::Number(value) => *value,
            Part::Value(name) => self.spell.value(name, self.rank),
            Part::Effect(index) => self
                .spell
                .effects
                .get(usize::from(index.saturating_sub(1)))
                .map_or(0.0, |values| Spell::by_rank(values, self.rank)),
            Part::StatByValue {
                value,
                stat,
                formula,
            } => self.spell.value(value, self.rank) * self.stat(*stat, *formula),
            Part::StatByCoefficient {
                coefficient,
                stat,
                formula,
            } => coefficient * self.stat(*stat, *formula),
            Part::StatBySubPart {
                part,
                stat,
                formula,
            } => self.part(part, depth + 1) * self.stat(*stat, *formula),
            Part::ByLevelBreakpoints {
                level1,
                initial_per_level,
                breakpoints,
            } => {
                let mut value = *level1;
                let mut per_level = *initial_per_level;
                for current in 2..=self.stats.level {
                    for breakpoint in breakpoints
                        .iter()
                        .filter(|breakpoint| breakpoint.level == current)
                    {
                        value += breakpoint.additional;
                        if breakpoint.per_level_after != 0.0 {
                            per_level = breakpoint.per_level_after;
                        }
                    }
                    value += per_level;
                }
                value
            }
            Part::ByLevelInterpolation { start, end } => {
                start + (end - start) * (level - 1.0) / 17.0
            }
            Part::Product(a, b) => self.part(a, depth + 1) * self.part(b, depth + 1),
            Part::Sum(parts) => parts.iter().map(|part| self.part(part, depth + 1)).sum(),
            Part::BuffCounter { buff, coefficient } => (self.buff_count)(buff) * coefficient,
            Part::MaxResource(coefficient) => coefficient * self.stats.max_mana,
            Part::Calculation(name) => self
                .spell
                .calculation(name)
                .map_or(0.0, |calculation| self.evaluate(calculation, depth + 1)),
            Part::Unknown => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use asset::data::{Breakpoint, DataValue};

    fn spell() -> Spell {
        Spell {
            name: "Test".into(),
            values: vec![
                DataValue {
                    name: "BaseDamage".into(),
                    values: vec![0.0, 60.0, 80.0, 100.0],
                },
                DataValue {
                    name: "APRatio".into(),
                    values: vec![0.5; 4],
                },
            ],
            calculations: vec![
                Calculation {
                    name: "TotalDamage".into(),
                    parts: vec![
                        Part::Value("BaseDamage".into()),
                        Part::StatByValue {
                            value: "APRatio".into(),
                            stat: Stat::AbilityPower,
                            formula: StatFormula::Total,
                        },
                        Part::StatByCoefficient {
                            coefficient: 1.5,
                            stat: Stat::AttackDamage,
                            formula: StatFormula::Bonus,
                        },
                    ],
                    multiplier: None,
                },
                Calculation {
                    name: "Doubled".into(),
                    parts: vec![Part::Calculation("TotalDamage".into())],
                    multiplier: Some(Part::Number(2.0)),
                },
                Calculation {
                    name: "Scaling".into(),
                    parts: vec![Part::ByLevelBreakpoints {
                        level1: 10.0,
                        initial_per_level: 1.0,
                        breakpoints: vec![Breakpoint {
                            level: 3,
                            additional: 5.0,
                            per_level_after: 2.0,
                        }],
                    }],
                    multiplier: None,
                },
            ],
            ..Spell::default()
        }
    }

    fn context<'a>(spell: &'a Spell, stats: &'a Stats) -> Context<'a> {
        Context {
            spell,
            rank: 2,
            stats,
            health: stats.max_health,
            buff_count: &|_| 0.0,
        }
    }

    #[test]
    fn sums_values_and_stat_ratios() {
        let spell = spell();
        let stats = Stats {
            level: 4,
            ability_power: 100.0,
            attack_damage: 80.0,
            base_attack_damage: 60.0,
            ..Stats::default()
        };
        let context = context(&spell, &stats);
        assert_eq!(context.calculate("TotalDamage"), 80.0 + 50.0 + 30.0);
        assert_eq!(context.calculate("doubled"), 320.0);
        assert_eq!(context.calculate("Scaling"), 10.0 + 1.0 + 5.0 + 2.0 + 2.0);
        assert_eq!(context.calculate("Missing"), 0.0);
    }

    #[test]
    fn max_resource_parts_scale_with_max_mana() {
        let spell = Spell {
            calculations: vec![Calculation {
                name: "ManaScaled".into(),
                parts: vec![Part::Number(70.0), Part::MaxResource(0.02)],
                multiplier: None,
            }],
            ..Spell::default()
        };
        let stats = Stats {
            max_mana: 1000.0,
            ..Stats::default()
        };
        assert_eq!(context(&spell, &stats).calculate("ManaScaled"), 90.0);
    }

    #[test]
    fn bonus_health_and_move_speed_exclude_the_base() {
        let spell = spell();
        let stats = Stats {
            max_health: 1500.0,
            base_max_health: 1100.0,
            move_speed: 400.0,
            base_move_speed: 340.0,
            ..Stats::default()
        };
        let context = context(&spell, &stats);
        assert_eq!(context.stat(Stat::MaxHealth, StatFormula::Bonus), 400.0);
        assert_eq!(context.stat(Stat::MoveSpeed, StatFormula::Bonus), 60.0);
    }
}
