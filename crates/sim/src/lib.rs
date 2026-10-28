pub mod champions;
pub mod formula;
pub mod items;
pub mod layout;
pub mod map;
pub mod nav;
pub mod stats;
pub mod unit;
pub mod vision;

use std::sync::Arc;

use asset::clip_id;
use asset::data::{Balance, GameData, UnitStats};
use glam::Vec2;
use layout::{Layout, Structure, team_index};
use protocol::{
    Animation, Command, DamageKind, Emote, Event, MinionKind, MissileState, Player, PropState,
    RelicState, Score, Setup, Slot, Snapshot, Summoner, TICK, Target, Team, UnitId, UnitKind,
    UnitState, You,
};
use stats::Bonus;
use unit::{Ability, Action, Buff, DashKind, Order, SLOTS, Shield, Tier, Unit, slot_index};

pub const START_GOLD: f32 = 1400.0;
const ANIMATION_FPS: f32 = 30.0;
pub const START_LEVEL: u8 = 3;
pub const MAX_LEVEL: u8 = 18;
const ATTACK_CLIPS: [&str; 4] = ["Attack1", "Attack2", "Attack3", "Attack4"];
pub const PASSIVE_GOLD: f32 = 5.5;
pub const PASSIVE_XP: f32 = 5.0;
pub const FIRST_WAVE: f32 = 25.0;
pub const WAVE_INTERVAL: f32 = 30.0;
pub const MINION_INTERVAL: f32 = 0.8;
const REROUTE_DISTANCE: f32 = 40.0;
const REVEAL_TIME: f32 = 1.5;
const EXHAUST_WEAKNESS: f32 = 0.35;
const GRIEVOUS_WOUNDS: f32 = 0.4;
pub const KILL_GOLD: f32 = 300.0;
pub const ASSIST_WINDOW: f32 = 10.0;
pub const XP_RANGE: f32 = 1600.0;
pub const RELIC_RESPAWN: f32 = 90.0;
const RELIC_OUTER_SPAWN: f32 = 105.0;
const RELIC_INNER_SPAWN: f32 = 150.0;
const RELIC_BEAM_DELAY: f32 = 2.5;
const RELIC_BEAM_RADIUS: f32 = 850.0;
const RELIC_PICKUP_RESTORE: f32 = 0.08;
const RELIC_BEAM_RESTORE: f32 = 0.16;
pub const RELIC_RADIUS: f32 = 120.0;
pub const INHIBITOR_RESPAWN: f32 = 240.0;
const SUN_DISC_BOUNTY: f32 = 50.0;
pub const XP_PER_LEVEL: [f32; 17] = [
    280.0, 380.0, 480.0, 580.0, 680.0, 780.0, 880.0, 980.0, 1080.0, 1180.0, 1280.0, 1380.0, 1480.0,
    1580.0, 1680.0, 1780.0, 1880.0,
];
pub const KILL_XP: [f32; 18] = [
    42.0, 114.0, 144.0, 174.0, 204.0, 234.0, 308.0, 392.0, 486.0, 590.0, 640.0, 690.0, 740.0,
    790.0, 840.0, 890.0, 940.0, 990.0,
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Motion {
    Homing(UnitId),
    Line {
        direction: Vec2,
        remaining: f32,
        limit: Option<usize>,
    },
    Point(Vec2),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    Attack { critical: bool },
    Mark,
    Spell(Slot, f32),
    Visual,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Missile {
    pub id: u32,
    pub owner: UnitId,
    pub team: Team,
    pub kind: u32,
    pub position: Vec2,
    pub height: f32,
    pub speed: f32,
    pub width: f32,
    pub motion: Motion,
    pub hit: Hit,
    pub hit_units: Vec<UnitId>,
    pub acceleration: Option<Acceleration>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Acceleration {
    pub rate: f32,
    pub max_speed: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Prop {
    pub id: u32,
    pub owner: UnitId,
    pub team: Team,
    pub effect: &'static str,
    pub position: Vec2,
    pub facing: f32,
    pub hidden: bool,
    pub expires: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Relic {
    pub position: Vec2,
    pub team: Team,
    pub inner: bool,
    pub ready_at: f32,
    pub beam: Option<(f32, Team)>,
}

fn relics(layout: &Layout) -> Vec<Relic> {
    let side = |position: Vec2| {
        let [blue, red] = layout.fountains;
        if position.distance(blue) <= position.distance(red) {
            Team::Blue
        } else {
            Team::Red
        }
    };
    let depth = |position: Vec2| position.distance(layout.fountains[team_index(side(position))]);
    layout
        .relics
        .iter()
        .map(|position| {
            let inner = layout
                .relics
                .iter()
                .filter(|other| side(**other) == side(*position))
                .all(|other| depth(*other) <= depth(*position));
            Relic {
                position: *position,
                team: side(*position),
                inner,
                ready_at: if inner {
                    RELIC_INNER_SPAWN
                } else {
                    RELIC_OUTER_SPAWN
                },
                beam: None,
            }
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Seat {
    pub name: String,
    pub team: Team,
    pub champion: u8,
    pub unit: UnitId,
    pub summoners: [Summoner; 2],
    pub score: Score,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entrant {
    pub name: String,
    pub team: Team,
    pub champion: Option<u8>,
    pub summoners: [Summoner; 2],
}

impl Entrant {
    pub fn new(name: &str, team: Team, champion: Option<u8>) -> Self {
        Self {
            name: name.to_owned(),
            team,
            champion,
            summoners: Summoner::DEFAULT,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cheat {
    Level(u8),
    Gold(f32),
    NoCooldowns,
    Heal,
    Minions,
}

impl Cheat {
    pub fn parse(text: &str) -> Option<Self> {
        let mut words = text.strip_prefix('/')?.split_whitespace();
        let command = words.next()?;
        let number = words
            .next()
            .and_then(|word| word.parse::<f32>().ok())
            .filter(|number| number.is_finite());
        match command {
            "level" => Some(Self::Level(number?.clamp(1.0, f32::from(MAX_LEVEL)) as u8)),
            "gold" => Some(Self::Gold(number?.max(0.0))),
            "nocd" => Some(Self::NoCooldowns),
            "heal" => Some(Self::Heal),
            "minions" => Some(Self::Minions),
            _ => None,
        }
    }
}

pub struct Game {
    pub data: Arc<GameData>,
    pub layout: Layout,
    pub tick: u32,
    pub time: f32,
    pub units: Vec<Unit>,
    pub missiles: Vec<Missile>,
    pub seats: Vec<Seat>,
    pub relics: Vec<Relic>,
    pub events: Vec<Event>,
    pub walls: Vec<champions::Wall>,
    pub barriers: Vec<champions::Wall>,
    pub zones: Vec<champions::Zone>,
    pub props: Vec<Prop>,
    pub winner: Option<Team>,
    next_id: u32,
    next_wave: f32,
    wave: u32,
    waves_paused: bool,
    marching: Vec<(f32, Team, MinionKind)>,
    random: u64,
    passive_timer: f32,
}

fn facing_to(from: Vec2, to: Vec2, fallback: f32) -> f32 {
    let delta = to - from;
    if delta.length_squared() < 1e-3 {
        fallback
    } else {
        delta.x.atan2(delta.y)
    }
}

impl Game {
    pub fn new(data: Arc<GameData>, layout: Layout, entrants: &[Entrant], seed: u64) -> Self {
        let mut game = Self {
            data,
            relics: relics(&layout),
            tick: 0,
            time: 0.0,
            units: Vec::new(),
            missiles: Vec::new(),
            seats: Vec::new(),
            events: Vec::new(),
            walls: Vec::new(),
            barriers: Vec::new(),
            zones: Vec::new(),
            props: Vec::new(),
            winner: None,
            next_id: 1,
            next_wave: layout.first_wave,
            wave: 0,
            waves_paused: false,
            marching: Vec::new(),
            random: seed.max(1),
            passive_timer: 0.0,
            layout,
        };
        game.spawn_structures();
        let mut taken: Vec<(Team, u8)> = Vec::new();
        for entrant in entrants {
            let team = &entrant.team;
            let available: Vec<u8> = (0..game.data.champions.len() as u8)
                .filter(|champion| !taken.contains(&(*team, *champion)))
                .collect();
            let champion = match entrant.champion {
                Some(champion) => champion,
                None if available.is_empty() => {
                    (game.next_random() * game.data.champions.len() as f32) as u8
                }
                None => {
                    available
                        [(game.next_random() * available.len() as f32) as usize % available.len()]
                }
            };
            taken.push((*team, champion));
            let unit = game.spawn_champion(*team, champion);
            if let Some(spawned) = game.unit_mut(unit) {
                spawned.summoners = entrant.summoners;
            }
            game.seats.push(Seat {
                name: entrant.name.clone(),
                team: *team,
                champion,
                unit,
                summoners: entrant.summoners,
                score: Score {
                    kills: 0,
                    deaths: 0,
                    assists: 0,
                    spree: 0,
                    minions: 0,
                    items: [None; 6],
                },
            });
        }
        game
    }

    pub fn setup_for(&self, name: &str) -> Option<Setup> {
        let you = self.seats.iter().find(|seat| seat.name == name)?.unit;
        Some(Setup {
            first_wave: self.layout.first_wave.round() as u32,
            you,
            players: self
                .seats
                .iter()
                .map(|seat| Player {
                    name: seat.name.clone(),
                    team: seat.team,
                    champion: seat.champion,
                    unit: seat.unit,
                    summoners: seat.summoners,
                })
                .collect(),
        })
    }

    pub fn next_random(&mut self) -> f32 {
        self.random ^= self.random << 13;
        self.random ^= self.random >> 7;
        self.random ^= self.random << 17;
        (self.random >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn allocate(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    pub fn index(&self, id: UnitId) -> Option<usize> {
        self.units.iter().position(|unit| unit.id == id)
    }

    pub fn unit(&self, id: UnitId) -> Option<&Unit> {
        self.units.iter().find(|unit| unit.id == id)
    }

    pub fn unit_mut(&mut self, id: UnitId) -> Option<&mut Unit> {
        self.units.iter_mut().find(|unit| unit.id == id)
    }

    pub fn walkable_point(&self, point: Vec2) -> Vec2 {
        self.layout
            .nav
            .as_ref()
            .map_or(point, |nav| nav::nearest_walkable(nav, point))
    }

    pub fn unit_stats(&self, name: &str) -> UnitStats {
        self.data
            .unit(name)
            .map(|unit| unit.stats)
            .unwrap_or_default()
    }

    fn spawn_structures(&mut self) {
        for placement in self.layout.structures.clone() {
            let (kind, data) = match placement.structure {
                Structure::Turret(Tier::Fountain) => (
                    UnitKind::Shrine,
                    match placement.team {
                        Team::Blue => "HA_AP_OrderShrineTurret",
                        Team::Red => "HA_AP_ChaosTurretShrine",
                    },
                ),
                Structure::Turret(_) => (UnitKind::Turret, "Turret"),
                Structure::Inhibitor => (UnitKind::Inhibitor, "Inhibitor"),
                Structure::Nexus => (UnitKind::Nexus, "Nexus"),
            };
            let mut base = self.unit_stats(data);
            if let Structure::Turret(tier) = placement.structure {
                base = turret_stats(base, tier);
            }
            let id = self.allocate();
            let mut unit = Unit::new(id, kind, placement.team, data, base, placement.position);
            unit.facing = placement.facing;
            unit.tier = match placement.structure {
                Structure::Turret(tier) => Some(tier),
                _ => None,
            };
            unit.stats = stats::compute(&unit.base, 1, &Bonus::default());
            unit.health = unit.stats.max_health;
            unit.targetable = !matches!(unit.tier, Some(Tier::Fountain));
            unit.invulnerable = matches!(unit.tier, Some(Tier::Fountain));
            if let Some(nav) = self.layout.nav.as_mut()
                && unit.solid()
            {
                nav::block(nav, unit.position, unit.base.pathing_radius);
            }
            self.units.push(unit);
        }
        self.update_structure_protection();
    }

    fn spawn_champion(&mut self, team: Team, champion: u8) -> UnitId {
        let data = self
            .data
            .champions
            .get(usize::from(champion))
            .cloned()
            .unwrap_or_default();
        let id = self.allocate();
        let fountain = self.layout.fountain(team);
        let offset = Vec2::from_angle(id as f32 * 1.3) * 150.0;
        let mut unit = Unit::new(
            id,
            UnitKind::Champion(champion),
            team,
            &data.name,
            data.stats,
            fountain + offset,
        );
        unit.level = START_LEVEL;
        unit.skill_points = START_LEVEL;
        unit.gold = START_GOLD;
        unit.xp = XP_PER_LEVEL[..usize::from(START_LEVEL - 1)].iter().sum();
        unit.abilities[slot_index(Slot::D)] = Ability {
            rank: 1,
            ..Ability::default()
        };
        unit.abilities[slot_index(Slot::F)] = Ability {
            rank: 1,
            ..Ability::default()
        };
        unit.facing = facing_to(unit.position, self.layout.fountain(team.enemy()), 0.0);
        self.units.push(unit);
        self.refresh_stats(id);
        if let Some(unit) = self.unit_mut(id) {
            unit.health = unit.stats.max_health;
            unit.mana = unit.stats.max_mana;
        }
        champions::spawned(self, id);
        id
    }

    fn balance(&self, id: UnitId) -> Balance {
        self.unit(id)
            .and_then(Unit::champion)
            .and_then(|champion| self.data.champions.get(usize::from(champion)))
            .map_or_else(Balance::default, |champion| champion.balance)
    }

    pub fn raw_bonus(&self, unit: &Unit) -> Bonus {
        let items = unit
            .items
            .iter()
            .flatten()
            .filter_map(|item| self.data.item(*item))
            .fold(Bonus::default(), |bonus, item| {
                bonus.combine(Bonus::item(&item.stats))
            });
        let buffs = unit
            .buffs
            .iter()
            .fold(Bonus::default(), |bonus, buff| bonus.combine(buff.bonus));
        items
            .combine(buffs)
            .combine(items::passive_bonus(self, unit))
            .combine(Bonus {
                ability_haste: self.balance(unit.id).ability_haste,
                ..Bonus::default()
            })
    }

    pub fn refresh_stats(&mut self, id: UnitId) {
        let Some(index) = self.index(id) else {
            return;
        };
        let balance = self.balance(id);
        let bonus = {
            let unit = &self.units[index];
            champions::bonus(self, unit, self.raw_bonus(unit))
        };
        let unit = &mut self.units[index];
        let previous = unit.stats;
        unit.stats = stats::compute(&unit.base, unit.level, &bonus);
        unit.stats.attack_speed =
            (unit.stats.attack_speed * balance.attack_speed).min(stats::MAX_ATTACK_SPEED);
        if previous.max_health > 0.0 && unit.stats.max_health > previous.max_health && unit.alive()
        {
            unit.health += unit.stats.max_health - previous.max_health;
        }
        if unit.stats.max_mana != previous.max_mana {
            unit.mana = (unit.mana + (unit.stats.max_mana - previous.max_mana).max(0.0))
                .min(unit.stats.max_mana);
        }
        unit.health = unit.health.min(unit.stats.max_health);
    }

    pub fn play(&mut self, id: UnitId, clip: &str, speed: f32) {
        self.animate(id, clip, speed, false);
    }

    pub fn hold(&mut self, id: UnitId, clip: &str) {
        self.animate(id, clip, 1.0, true);
    }

    pub fn release(&mut self, id: UnitId, clip: &str) {
        if let Some(unit) = self.unit_mut(id)
            && unit
                .animation
                .is_some_and(|animation| animation.hold && animation.clip == clip_id(clip))
        {
            unit.animation = None;
        }
    }

    fn animate(&mut self, id: UnitId, clip: &str, speed: f32, hold: bool) {
        let tick = self.tick;
        if let Some(unit) = self.unit_mut(id) {
            unit.animation = Some(Animation {
                clip: clip_id(clip),
                started: tick,
                speed,
                hold,
            });
        }
    }

    pub fn effect(&mut self, name: &str, position: Vec2, unit: Option<UnitId>) {
        self.push_effect(name, position, unit, None, false);
    }

    pub fn owned_effect(
        &mut self,
        name: &str,
        owner: UnitId,
        position: Vec2,
        unit: Option<UnitId>,
    ) {
        let team = self.unit(owner).map(|owner| owner.team);
        self.push_effect(name, position, unit, team, false);
    }

    pub fn global_effect(
        &mut self,
        name: &str,
        owner: UnitId,
        position: Vec2,
        unit: Option<UnitId>,
    ) {
        let team = self.unit(owner).map(|owner| owner.team);
        self.push_effect(name, position, unit, team, true);
    }

    pub fn lasting_effect(
        &mut self,
        name: &'static str,
        owner: UnitId,
        position: Vec2,
        expires: f32,
    ) {
        let Some((team, facing)) = self.unit(owner).map(|unit| (unit.team, unit.facing)) else {
            return;
        };
        let id = self.allocate();
        self.props.push(Prop {
            id,
            owner,
            team,
            effect: name,
            position,
            facing,
            hidden: false,
            expires,
        });
    }

    fn push_effect(
        &mut self,
        name: &str,
        position: Vec2,
        unit: Option<UnitId>,
        team: Option<Team>,
        global: bool,
    ) {
        let facing = unit
            .and_then(|id| self.unit(id))
            .map_or(0.0, |unit| unit.facing);
        self.events.push(Event::Effect {
            name: clip_id(name),
            position: position.to_array(),
            facing,
            unit,
            team,
            global,
        });
    }

    pub fn sound(&mut self, unit: UnitId, suffix: &str) {
        self.sound_on(unit, unit, suffix);
    }

    pub fn sound_on(&mut self, owner: UnitId, anchor: UnitId, suffix: &str) {
        if let Some(champion) = self.champion_key(owner) {
            self.shared_sound(anchor, &format!("{champion}.{suffix}"));
        }
    }

    pub fn shared_sound(&mut self, anchor: UnitId, key: &str) {
        self.events.push(Event::Sound {
            unit: anchor,
            key: key.to_owned(),
        });
    }

    fn champion_key(&self, id: UnitId) -> Option<String> {
        let champion = self.unit(id)?.champion()?;
        self.data
            .champions
            .get(usize::from(champion))
            .map(|data| data.key())
    }

    fn hit_sound(&mut self, attacker: &Unit, target: UnitId, critical: bool) {
        let key = match attacker.kind {
            UnitKind::Champion(_) => {
                let suffix = if critical { "crit_hit" } else { "attack_hit" };
                self.sound_on(attacker.id, target, suffix);
                return;
            }
            UnitKind::Minion(MinionKind::Melee) => "minion.hit",
            UnitKind::Minion(MinionKind::Caster) => "minion.ranged_hit",
            UnitKind::Minion(MinionKind::Siege) => "minion.siege_hit",
            UnitKind::Minion(MinionKind::Super) => "minion.super_hit",
            UnitKind::Turret | UnitKind::Shrine => "turret.hit",
            _ => return,
        };
        self.shared_sound(target, key);
    }

    pub fn command(&mut self, id: UnitId, command: Command) {
        let Some(index) = self.index(id) else {
            return;
        };
        if self.winner.is_some() {
            return;
        }
        let order = matches!(
            command,
            Command::Move(_) | Command::AttackMove(_) | Command::Attack(_) | Command::Stop
        );
        if order && self.units[index].dead {
            return;
        }
        match command {
            Command::Move(point) => {
                let unit = &mut self.units[index];
                cancel_attack_windup(unit);
                unit.order = Order::Move(Vec2::from_array(point));
                unit.path = vec![Vec2::from_array(point)];
            }
            Command::AttackMove(point) => {
                let unit = &mut self.units[index];
                cancel_attack_windup(unit);
                unit.order = Order::AttackMove(Vec2::from_array(point));
            }
            Command::Attack(target) if champions::can_interact(self, id, target) => {
                let unit = &mut self.units[index];
                cancel_attack_windup(unit);
                unit.order = Order::Interact(target);
            }
            Command::Attack(target) => {
                if self.valid_target(&self.units[index], target).is_some() {
                    self.units[index].order = Order::Attack(target);
                }
            }
            Command::Stop => {
                let unit = &mut self.units[index];
                cancel_attack_windup(unit);
                unit.order = Order::Idle;
                unit.path.clear();
            }
            Command::Cast(slot, target) => champions::command_cast(self, id, slot, target),
            Command::LevelUp(slot) => self.level_up_ability(id, slot),
            Command::Buy(item) => items::buy(self, id, item),
            Command::Sell(slot) => items::sell(self, id, usize::from(slot)),
            Command::Undo => items::undo(self, id),
            Command::UseItem(slot, _) => items::activate(self, id, usize::from(slot)),
            Command::Emote(emote) => self.emote(id, emote),
        }
    }

    fn emote(&mut self, id: UnitId, emote: Emote) {
        let Some(unit) = self.unit_mut(id) else {
            return;
        };
        if !unit.can_act() || !matches!(unit.action, Action::None) {
            return;
        }
        unit.path.clear();
        unit.order = Order::Idle;
        let clip = match emote {
            Emote::Joke => "Joke",
            Emote::Taunt => "Taunt",
            Emote::Dance => "Dance",
            Emote::Laugh => "Laugh",
        };
        self.play(id, clip, 1.0);
    }

    fn level_up_ability(&mut self, id: UnitId, slot: Slot) {
        let Some(unit) = self.unit_mut(id) else {
            return;
        };
        if unit.skill_points == 0 || matches!(slot, Slot::D | Slot::F) {
            return;
        }
        let level = unit.level;
        let ability = &unit.abilities[slot_index(slot)];
        let max_rank = if slot == Slot::R { 3 } else { 5 };
        let required = match slot {
            Slot::R => [6, 11, 16]
                .get(usize::from(ability.rank))
                .copied()
                .unwrap_or(99),
            _ => (ability.rank * 2 + 1).min(99),
        };
        if ability.rank >= max_rank || level < required {
            return;
        }
        unit.skill_points -= 1;
        unit.abilities[slot_index(slot)].rank += 1;
        champions::ranked_up(self, id, slot);
    }

    pub fn step(&mut self) {
        if self.winner.is_some() {
            self.tick += 1;
            return;
        }
        self.tick += 1;
        self.time += TICK;
        self.spawn_waves();
        self.march_out();
        self.passive_income();
        let ids: Vec<UnitId> = self.units.iter().map(|unit| unit.id).collect();
        for id in &ids {
            self.update_timers(*id);
        }
        for id in &ids {
            self.think(*id);
        }
        for id in &ids {
            self.advance_action(*id);
        }
        for id in &ids {
            champions::tick(self, *id);
            items::tick(self, *id);
        }
        champions::zones(self);
        for unit in &mut self.units {
            unit.last_position = unit.position;
        }
        for id in &ids {
            self.advance_movement(*id);
        }
        self.separate_units();
        self.hold_barriers();
        self.update_missiles();
        self.update_relics();
        let time = self.time;
        self.props.retain(|prop| prop.expires > time);
        self.respawn();
        self.cleanup();
        self.update_structure_protection();
    }

    fn passive_income(&mut self) {
        self.passive_timer += TICK;
        if self.passive_timer < 1.0 {
            return;
        }
        self.passive_timer -= 1.0;
        let gold = if self.time > 15.0 { PASSIVE_GOLD } else { 0.0 };
        let ids: Vec<UnitId> = self
            .units
            .iter()
            .filter(|unit| unit.champion().is_some())
            .map(|unit| unit.id)
            .collect();
        for id in ids {
            if let Some(unit) = self.unit_mut(id) {
                unit.gold += gold;
            }
            self.grant_xp(id, PASSIVE_XP);
        }
    }

    fn update_timers(&mut self, id: UnitId) {
        let time = self.time;
        let Some(index) = self.index(id) else {
            return;
        };
        let mut changed = false;
        {
            let unit = &mut self.units[index];
            let before = unit.buffs.len();
            unit.buffs.retain(|buff| buff.expires > time);
            changed |= before != unit.buffs.len();
            unit.shields
                .retain(|shield| shield.expires > time && shield.amount > 0.0);
            unit.last_hit_by.retain(|(_, at)| time - at < ASSIST_WINDOW);
            for ability in &mut unit.abilities {
                ability.cooldown = (ability.cooldown - TICK).max(0.0);
            }
            if unit.no_cooldowns {
                for ability in &mut unit.abilities {
                    ability.cooldown = ability.cooldown.min(TICK);
                }
                unit.mana = unit.stats.max_mana;
            }
            if unit.alive() && unit.champion().is_some() {
                unit.health =
                    (unit.health + unit.stats.health_regen * TICK).min(unit.stats.max_health);
                unit.mana = (unit.mana + unit.stats.mana_regen * TICK).min(unit.stats.max_mana);
            }
            if let Some(expires) = unit.expires
                && time >= expires
            {
                unit.dead = true;
            }
            if let Action::Airborne { until } = unit.action
                && time >= until
            {
                unit.action = Action::None;
            }
        }
        champions::recharge(self, id);
        if changed || self.units[index].champion().is_some() && self.tick.is_multiple_of(6) {
            self.refresh_stats(id);
        }
    }

    pub fn update_structure_protection(&mut self) {
        for team in [Team::Blue, Team::Red] {
            let alive = |tier: Tier, units: &[Unit]| {
                units
                    .iter()
                    .any(|unit| unit.team == team && unit.tier == Some(tier) && unit.alive())
            };
            let outer = alive(Tier::Outer, &self.units);
            let inner = alive(Tier::Inner, &self.units);
            let inhibitor = self
                .units
                .iter()
                .any(|unit| unit.team == team && unit.kind == UnitKind::Inhibitor && unit.alive());
            let nexus_turrets = alive(Tier::Nexus, &self.units);
            for unit in self.units.iter_mut().filter(|unit| unit.team == team) {
                unit.invulnerable = match (unit.kind, unit.tier) {
                    (_, Some(Tier::Fountain)) => true,
                    (_, Some(Tier::Inner)) => outer,
                    (UnitKind::Inhibitor, _) => inner,
                    (_, Some(Tier::Nexus)) => inhibitor,
                    (UnitKind::Nexus, _) => nexus_turrets,
                    _ => unit.invulnerable && !unit.is_structure(),
                };
            }
        }
    }

    fn spawn_waves(&mut self) {
        if self.waves_paused {
            self.next_wave = self.next_wave.max(self.time);
        }
        if self.waves_paused || self.time < self.next_wave {
            return;
        }
        self.wave += 1;
        self.next_wave += self.layout.wave_interval;
        for team in [Team::Blue, Team::Red] {
            let enemy_inhibitor_down = !self.units.iter().any(|unit| {
                unit.team == team.enemy() && unit.kind == UnitKind::Inhibitor && unit.alive()
            });
            let mut kinds = vec![MinionKind::Melee; 3];
            if self.wave.is_multiple_of(3) {
                kinds.push(MinionKind::Siege);
            }
            kinds.extend([MinionKind::Caster; 3]);
            if enemy_inhibitor_down {
                kinds.insert(0, MinionKind::Super);
            }
            for (slot, kind) in kinds.into_iter().enumerate() {
                let at = self.time + slot as f32 * self.layout.minion_interval;
                self.marching.push((at, team, kind));
            }
        }
    }

    fn march_out(&mut self) {
        let time = self.time;
        let (due, waiting) = std::mem::take(&mut self.marching)
            .into_iter()
            .partition(|(at, ..)| *at <= time);
        self.marching = waiting;
        for (_, team, kind) in due {
            let name = minion_name(team, kind);
            let mut base = self.unit_stats(name);
            let upgrades = self.time / 90.0;
            base.health += base.health * 0.04 * upgrades;
            base.attack_damage += base.attack_damage * 0.03 * upgrades;
            let id = self.allocate();
            let barracks = self.layout.barracks[team_index(team)];
            let position = self.walkable_point(barracks);
            let lane = self.layout.lane_for(team);
            let mut unit = Unit::new(id, UnitKind::Minion(kind), team, name, base, position);
            unit.stats = stats::compute(&unit.base, 1, &Bonus::default());
            unit.health = unit.stats.max_health;
            unit.facing = facing_to(position, lane.first().copied().unwrap_or(position), 0.0);
            self.units.push(unit);
        }
    }

    pub fn enemies_near(&self, team: Team, position: Vec2, radius: f32) -> Vec<UnitId> {
        self.units
            .iter()
            .filter(|unit| {
                unit.team != team
                    && unit.alive()
                    && !unit.untargetable()
                    && unit.kind != UnitKind::Soldier
                    && unit.position.distance(position) <= radius + unit.radius()
            })
            .map(|unit| unit.id)
            .collect()
    }

    fn in_attack_range(&self, attacker: &Unit, target: &Unit) -> bool {
        attacker.position.distance(target.position)
            <= attacker.stats.attack_range + attacker.radius() + target.radius()
    }

    fn think(&mut self, id: UnitId) {
        let Some(index) = self.index(id) else {
            return;
        };
        let unit = &self.units[index];
        if !unit.can_act() {
            return;
        }
        match unit.kind {
            UnitKind::Minion(_) => self.think_minion(index),
            UnitKind::Turret | UnitKind::Shrine | UnitKind::SunDisc => self.think_turret(index),
            UnitKind::Champion(_) => self.think_champion(index),
            UnitKind::Soldier | UnitKind::Inhibitor | UnitKind::Nexus => {}
        }
    }

    fn valid_target(&self, attacker: &Unit, target: UnitId) -> Option<&Unit> {
        self.unit(target).filter(|target| {
            target.alive()
                && !target.untargetable()
                && target.team != attacker.team
                && target.kind != UnitKind::Soldier
                && self.detected(attacker.team, target)
        })
    }

    pub fn detected(&self, team: Team, target: &Unit) -> bool {
        vision::detects(&self.units, team, target)
    }

    fn think_champion(&mut self, index: usize) {
        let unit = &self.units[index];
        if !matches!(unit.action, Action::None) {
            return;
        }
        let id = unit.id;
        match unit.order {
            Order::Idle => {
                let target = self
                    .units
                    .iter()
                    .filter(|other| {
                        other.team != unit.team
                            && other.alive()
                            && !other.untargetable()
                            && other.kind != UnitKind::Soldier
                            && !(other.is_structure() && other.invulnerable)
                            && self.in_attack_range(unit, other)
                            && self.detected(unit.team, other)
                    })
                    .min_by(|a, b| {
                        a.position
                            .distance(unit.position)
                            .total_cmp(&b.position.distance(unit.position))
                    })
                    .map(|other| other.id);
                if let Some(target) = target {
                    self.try_attack(index, target);
                }
            }
            Order::Move(_) => {}
            Order::AttackMove(point) => {
                let target = self
                    .units
                    .iter()
                    .filter(|other| {
                        other.team != unit.team
                            && other.alive()
                            && !other.untargetable()
                            && other.kind != UnitKind::Soldier
                            && self.detected(unit.team, other)
                            && other.position.distance(unit.position)
                                <= unit
                                    .base
                                    .acquisition_range
                                    .max(unit.stats.attack_range + 100.0)
                    })
                    .min_by(|a, b| {
                        a.position
                            .distance(point)
                            .total_cmp(&b.position.distance(point))
                    })
                    .map(|other| other.id);
                match target {
                    Some(target) => self.units[index].order = Order::Attack(target),
                    None => self.units[index].path = vec![point],
                }
            }
            Order::Attack(target) => {
                if self.valid_target(unit, target).is_none() {
                    let unit = &mut self.units[index];
                    unit.order = Order::Idle;
                    unit.path.clear();
                    return;
                }
                if champions::redirect_attack(self, id, target) {
                    return;
                }
                self.try_attack(index, target);
            }
            Order::Cast(slot, target) => champions::continue_cast(self, id, slot, target),
            Order::Interact(target) => champions::continue_interact(self, id, target),
        }
    }

    pub fn try_attack(&mut self, index: usize, target: UnitId) {
        let Some(target_unit) = self.unit(target) else {
            return;
        };
        let target_position = target_unit.position;
        let in_range = self.in_attack_range(&self.units[index], target_unit);
        let unit = &mut self.units[index];
        if !in_range {
            unit.path = vec![target_position];
            return;
        }
        unit.path.clear();
        self.start_attack(index, target);
    }

    pub fn start_attack(&mut self, index: usize, target: UnitId) {
        let time = self.time;
        let Some(target_position) = self.unit(target).map(|unit| unit.position) else {
            return;
        };
        let unit = &mut self.units[index];
        if unit.is_disarmed() {
            unit.path = vec![target_position];
            return;
        }
        unit.path.clear();
        if !unit.is_structure() {
            unit.facing = facing_to(unit.position, target_position, unit.facing);
        }
        if time < unit.attack_ready_at || !matches!(unit.action, Action::None) {
            return;
        }
        let period = 1.0 / unit.stats.attack_speed.max(0.1);
        let base_period = 1.0 / unit.stats.base_attack_speed.max(0.1);
        let windup = unit
            .base
            .attack
            .windup(period, base_period)
            .clamp(period * 0.05, period * 0.9);
        let critical = unit.stats.crit_chance > 0.0;
        let id = unit.id;
        unit.attack_ready_at = time + period;
        unit.attack_index += 1;
        let roll = self.next_random();
        let forced = champions::forced_critical(self, &self.units[index]);
        let unit = &mut self.units[index];
        let critical = forced || critical && roll < unit.stats.crit_chance;
        unit.action = Action::Attack {
            target,
            elapsed: 0.0,
            windup,
            critical,
        };
        let base_windup = unit.base.attack.windup(base_period, base_period);
        let standard = if critical {
            "Crit"
        } else {
            ATTACK_CLIPS[unit.attack_index as usize % ATTACK_CLIPS.len()]
        };
        let clip = champions::attack_clip(self, id, standard);
        let speed = match self.swing(id, critical).filter(|_| clip == standard) {
            Some(swing) => swing / windup,
            None => (base_windup / windup).max(1.0),
        };
        self.play(id, clip, speed);
        if let Some(key) = champions::attack_sound(self, id) {
            self.sound(id, key);
        }
    }

    fn swing(&self, id: UnitId, critical: bool) -> Option<f32> {
        let champion = self.unit(id)?.champion()?;
        let data = self.data.champions.get(usize::from(champion))?;
        let attack = if critical {
            "CritAttack"
        } else {
            "BasicAttack"
        };
        let spell = data.spell(&format!("{}{attack}", data.name))?;
        (spell.cast_frame > 0.0).then_some(spell.cast_frame / ANIMATION_FPS)
    }

    fn think_minion(&mut self, index: usize) {
        let unit = &self.units[index];
        if !matches!(unit.action, Action::None) {
            return;
        }
        let range = unit.base.acquisition_range.max(700.0);
        let current = match unit.order {
            Order::Attack(target) => self
                .valid_target(unit, target)
                .filter(|target| target.position.distance(unit.position) <= range + 200.0)
                .map(|target| target.id),
            _ => None,
        };
        let should_retarget = current.is_none() || self.tick % 8 == (unit.id % 8);
        let target = if should_retarget {
            self.pick_minion_target(index, range).or(current)
        } else {
            current
        };
        match target {
            Some(target) => {
                self.units[index].order = Order::Attack(target);
                self.try_attack(index, target);
            }
            None => {
                let team = self.units[index].team;
                let lane = self.layout.lane_for(team);
                let unit = &mut self.units[index];
                unit.order = Order::Idle;
                while unit.waypoint < lane.len()
                    && unit.position.distance(lane[unit.waypoint]) < 150.0
                {
                    unit.waypoint += 1;
                }
                let goal = lane.get(unit.waypoint).copied().or(lane.last().copied());
                if let Some(goal) = goal {
                    unit.path = vec![goal];
                }
            }
        }
    }

    fn pick_minion_target(&self, index: usize, range: f32) -> Option<UnitId> {
        let unit = &self.units[index];
        let candidates = self.units.iter().filter(|other| {
            other.team != unit.team
                && other.alive()
                && !other.untargetable()
                && other.kind != UnitKind::Soldier
                && !(other.is_structure() && other.invulnerable)
                && other.position.distance(unit.position) <= range + other.radius()
                && self.detected(unit.team, other)
        });
        candidates
            .map(|other| {
                let distance = other.position.distance(unit.position);
                let priority = match other.kind {
                    UnitKind::Champion(_)
                        if self.attacked_allied_champion(other.id, unit, range) =>
                    {
                        0.0
                    }
                    UnitKind::Minion(_) => 1.0,
                    UnitKind::Turret
                    | UnitKind::Shrine
                    | UnitKind::Inhibitor
                    | UnitKind::Nexus
                    | UnitKind::SunDisc => 2.0,
                    UnitKind::Champion(_) => 3.0,
                    UnitKind::Soldier => 9.0,
                };
                (priority * 10000.0 + distance, other.id)
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, id)| id)
    }

    fn attacked_allied_champion(&self, attacker: UnitId, guard: &Unit, range: f32) -> bool {
        self.units.iter().any(|unit| {
            unit.team == guard.team
                && unit.champion().is_some()
                && unit.position.distance(guard.position) <= range + unit.radius()
                && unit
                    .last_hit_by
                    .iter()
                    .any(|(by, at)| *by == attacker && self.time - at < 2.0)
        })
    }

    fn think_turret(&mut self, index: usize) {
        let unit = &self.units[index];
        let range = unit.stats.attack_range + unit.radius();
        let current = match unit.order {
            Order::Attack(target) => self
                .valid_target(unit, target)
                .filter(|target| target.position.distance(unit.position) <= range + target.radius())
                .map(|target| target.id),
            _ => None,
        };
        let aggro = self
            .units
            .iter()
            .filter(|other| {
                other.team != unit.team
                    && other.alive()
                    && !other.untargetable()
                    && other.champion().is_some()
                    && other.position.distance(unit.position) <= range + other.radius()
                    && self.attacked_allied_champion(other.id, unit, range)
            })
            .map(|other| other.id)
            .next();
        let target = aggro.or(current).or_else(|| {
            self.units
                .iter()
                .filter(|other| {
                    other.team != unit.team
                        && other.alive()
                        && !other.untargetable()
                        && other.kind != UnitKind::Soldier
                        && !other.is_structure()
                        && other.position.distance(unit.position) <= range + other.radius()
                })
                .min_by(|a, b| {
                    let key = |other: &Unit| {
                        let priority = if other.is_minion() { 0.0 } else { 1.0 };
                        priority * 10000.0 + other.position.distance(unit.position)
                    };
                    key(a).total_cmp(&key(b))
                })
                .map(|other| other.id)
        });
        let unit = &mut self.units[index];
        match target {
            Some(target) => {
                if unit.order != Order::Attack(target) {
                    unit.turret_target_bonus = 0.0;
                }
                unit.order = Order::Attack(target);
                if matches!(unit.action, Action::None) {
                    self.try_attack(index, target);
                }
            }
            None => {
                unit.order = Order::Idle;
                unit.turret_target_bonus = 0.0;
            }
        }
    }

    fn advance_action(&mut self, id: UnitId) {
        let Some(index) = self.index(id) else {
            return;
        };
        match self.units[index].action {
            Action::Attack {
                target,
                elapsed,
                windup,
                critical,
            } => {
                if self.units[index].is_stunned() {
                    self.units[index].action = Action::None;
                    return;
                }
                let elapsed = elapsed + TICK;
                if elapsed < windup {
                    self.units[index].action = Action::Attack {
                        target,
                        elapsed,
                        windup,
                        critical,
                    };
                    return;
                }
                self.units[index].action = Action::None;
                self.launch_attack(id, target, critical);
            }
            Action::Cast {
                slot,
                target,
                elapsed,
                windup,
                recovery,
                fired,
                ..
            } => {
                let elapsed = elapsed + TICK;
                let fire = !fired && elapsed >= windup;
                let action = &mut self.units[index].action;
                if elapsed >= windup + recovery {
                    *action = Action::None;
                } else if let Action::Cast {
                    elapsed: current,
                    fired: done,
                    ..
                } = action
                {
                    *current = elapsed;
                    *done = fired || fire;
                }
                if fire {
                    champions::fire(self, id, slot, target);
                }
            }
            Action::Interact {
                target,
                elapsed,
                windup,
            } => {
                let elapsed = elapsed + TICK;
                if elapsed < windup {
                    self.units[index].action = Action::Interact {
                        target,
                        elapsed,
                        windup,
                    };
                    return;
                }
                self.units[index].action = Action::None;
                champions::interacted(self, id, target);
            }
            Action::None | Action::Dash { .. } | Action::Airborne { .. } => {}
        }
    }

    fn launch_attack(&mut self, id: UnitId, target: UnitId, critical: bool) {
        let Some(attacker) = self.unit(id) else {
            return;
        };
        if self.valid_target(attacker, target).is_none() {
            return;
        }
        if champions::launch_attack(self, id, target) {
            return;
        }
        let Some(attacker) = self.unit(id) else {
            return;
        };
        let speed = attacker.base.attack.missile_speed;
        let ranged = attacker.stats.attack_range > 300.0;
        if ranged {
            let missile_speed = if speed > 0.0 { speed } else { 1500.0 };
            let attack = if critical {
                "CritAttack"
            } else {
                "BasicAttack"
            };
            let kind = clip_id(&format!("{}{attack}", attacker.data));
            let position = attacker.position;
            let team = attacker.team;
            let missile_id = self.allocate();
            self.missiles.push(Missile {
                id: missile_id,
                owner: id,
                team,
                kind,
                position,
                height: 100.0,
                speed: missile_speed,
                width: 0.0,
                motion: Motion::Homing(target),
                hit: Hit::Attack { critical },
                hit_units: Vec::new(),
                acceleration: None,
            });
        } else {
            if attacker.champion().is_some() {
                let attack = if critical {
                    "CritAttack"
                } else {
                    "BasicAttack"
                };
                let name = format!("{}{attack}Hit", attacker.data);
                let at = self
                    .unit(target)
                    .map_or(attacker.position, |unit| unit.position);
                self.effect(&name, at, Some(target));
            }
            self.attack_hit(id, target, critical);
        }
    }

    pub fn attack_hit(&mut self, attacker: UnitId, target: UnitId, critical: bool) {
        let Some(source) = self.unit(attacker).cloned() else {
            return;
        };
        self.hit_sound(&source, target, critical);
        let mut amount = source.stats.attack_damage;
        if critical {
            amount *= source.stats.crit_damage;
        }
        let is_turret = matches!(
            source.kind,
            UnitKind::Turret | UnitKind::Shrine | UnitKind::SunDisc
        );
        let target_is_champion = self
            .unit(target)
            .is_some_and(|unit| unit.champion().is_some());
        let target_minion = self.unit(target).and_then(Unit::minion);
        if is_turret {
            if target_is_champion {
                let bonus = source.turret_target_bonus;
                amount *= 1.0 + bonus;
                if let Some(turret) = self.unit_mut(attacker) {
                    turret.turret_target_bonus = (bonus + 0.4).min(1.2);
                }
            } else if let Some(kind) = target_minion {
                let health = self.unit(target).map_or(0.0, |unit| unit.stats.max_health);
                amount = health
                    * match kind {
                        MinionKind::Melee => 0.45,
                        MinionKind::Caster => 0.7,
                        MinionKind::Siege | MinionKind::Super => 0.14,
                    };
            }
        }
        if source.is_minion() && self.unit(target).is_some_and(Unit::is_structure) {
            amount *= 0.5;
        }
        amount = champions::modify_attack(self, attacker, target, amount);
        let kind = if source.kind == UnitKind::SunDisc {
            DamageKind::Magic
        } else {
            DamageKind::Physical
        };
        let dealt = self.damage(Some(attacker), target, amount, kind, critical);
        let life_steal = self
            .unit(attacker)
            .map_or(0.0, |unit| unit.stats.life_steal);
        if life_steal > 0.0 {
            self.heal(attacker, dealt * life_steal);
        }
        self.on_hit(attacker, target);
        if critical {
            champions::on_crit(self, attacker, target);
        }
    }

    pub fn on_hit(&mut self, attacker: UnitId, target: UnitId) {
        champions::on_attack_hit(self, attacker, target);
        items::on_attack_hit(self, attacker, target);
        if self
            .unit(attacker)
            .is_some_and(|unit| unit.champion().is_some())
        {
            items::on_attacked(self, attacker, target);
        }
    }

    pub fn heal(&mut self, id: UnitId, amount: f32) {
        if let Some(unit) = self.unit_mut(id)
            && unit.alive()
        {
            let wounded = if unit.has_buff("grievous_wounds") {
                1.0 - GRIEVOUS_WOUNDS
            } else {
                1.0
            };
            let healed = (amount * wounded)
                .min(unit.stats.max_health - unit.health)
                .max(0.0);
            unit.health += healed;
            if healed > 1.0 {
                self.events.push(Event::Heal {
                    target: id,
                    amount: healed,
                });
            }
        }
    }

    pub fn shield(&mut self, id: UnitId, amount: f32, duration: f32) {
        self.add_shield(id, amount, duration, false);
    }

    pub fn magic_shield(&mut self, id: UnitId, amount: f32, duration: f32) {
        self.add_shield(id, amount, duration, true);
    }

    fn add_shield(&mut self, id: UnitId, amount: f32, duration: f32, magic_only: bool) {
        let expires = self.time + duration;
        if let Some(unit) = self.unit_mut(id) {
            unit.shields.push(Shield {
                name: "",
                amount,
                expires,
                magic_only,
            });
        }
    }

    pub fn add_sourced_buff(&mut self, id: UnitId, buff: Buff) {
        if let Some(unit) = self.unit_mut(id)
            && unit.alive()
        {
            unit.buffs
                .retain(|existing| existing.name != buff.name || existing.source != buff.source);
            unit.buffs.push(buff);
        }
        self.refresh_stats(id);
    }

    pub fn add_buff(&mut self, id: UnitId, buff: Buff) {
        if buff.name == "silence" {
            self.interrupt(id, false);
        }
        if let Some(unit) = self.unit_mut(id)
            && unit.alive()
        {
            match unit
                .buffs
                .iter_mut()
                .find(|existing| existing.name == buff.name)
            {
                Some(existing) => *existing = buff,
                None => unit.buffs.push(buff),
            }
        }
        self.refresh_stats(id);
    }

    pub fn buff(&self, name: &'static str, duration: f32) -> Buff {
        Buff {
            name,
            expires: self.time + duration,
            duration,
            stacks: 1,
            bonus: Bonus::default(),
            source: None,
            slot: None,
            value: 0.0,
        }
    }

    pub fn crowd_control(
        &self,
        name: &'static str,
        duration: f32,
        origin: Option<(UnitId, Slot)>,
    ) -> Buff {
        let mut buff = self.buff(name, duration);
        buff.source = origin.map(|(source, _)| source);
        buff.slot = origin.map(|(_, slot)| slot);
        buff
    }

    pub fn slow(
        &mut self,
        target: UnitId,
        amount: f32,
        duration: f32,
        origin: Option<(UnitId, Slot)>,
    ) {
        if self
            .unit(target)
            .is_none_or(|unit| unit.dead || unit.has_buff(items::STASIS))
        {
            return;
        }
        let mut buff = self.crowd_control("slow", duration, origin);
        buff.bonus.slow = amount;
        if let Some(unit) = self.unit_mut(target) {
            unit.buffs.retain(|existing| {
                existing.name != "slow"
                    || existing.source != buff.source
                    || existing.slot != buff.slot
            });
            unit.buffs.push(buff);
        }
        self.refresh_stats(target);
    }

    pub fn root(&mut self, target: UnitId, duration: f32, origin: Option<(UnitId, Slot)>) {
        if self.unit(target).is_some_and(Unit::unstoppable) {
            return;
        }
        let buff = self.crowd_control("root", duration, origin);
        self.add_buff(target, buff);
    }

    pub fn stun(&mut self, target: UnitId, duration: f32, origin: Option<(UnitId, Slot)>) {
        if self.unit(target).is_some_and(Unit::unstoppable) {
            return;
        }
        let buff = self.crowd_control("stun", duration, origin);
        self.add_buff(target, buff);
        if let Some(unit) = self.unit_mut(target) {
            cancel_attack_windup(unit);
        }
        self.interrupt(target, false);
    }

    pub fn interrupt(&mut self, target: UnitId, channels_only: bool) {
        if let Some(unit) = self.unit_mut(target)
            && match unit.action {
                Action::Cast {
                    fired: false,
                    channel,
                    ..
                } => channel || !channels_only,
                Action::Interact { .. } => !channels_only,
                _ => false,
            }
        {
            unit.action = Action::None;
            if unit.animation.is_some_and(|animation| animation.hold) {
                unit.animation = None;
            }
        }
    }

    pub fn knock_back(&mut self, target: UnitId, to: Vec2, speed: f32) {
        if let Some(unit) = self.unit_mut(target)
            && unit.alive()
            && !unit.unstoppable()
            && !unit.is_structure()
        {
            unit.action = Action::Dash {
                to,
                speed,
                kind: DashKind::Knockback,
            };
            unit.path.clear();
        }
    }

    pub fn knock_up(&mut self, target: UnitId, duration: f32, origin: Option<(UnitId, Slot)>) {
        if self
            .unit(target)
            .is_none_or(|unit| unit.dead || unit.unstoppable())
        {
            return;
        }
        let until = self.time + duration;
        let mut buff = self.crowd_control("knockup", duration, origin);
        buff.value = duration;
        self.add_buff(target, buff);
        if let Some(unit) = self.unit_mut(target) {
            if unit.is_structure() {
                return;
            }
            unit.action = Action::Airborne { until };
            unit.path.clear();
        }
        self.play(target, "Knockup", 1.0);
    }

    pub fn damage(
        &mut self,
        source: Option<UnitId>,
        target: UnitId,
        raw: f32,
        kind: DamageKind,
        critical: bool,
    ) -> f32 {
        let attacker = source
            .and_then(|id| self.unit(id))
            .map(|unit| (unit.stats, unit.level, unit.team, unit.owner));
        let Some(index) = self.index(target) else {
            return 0.0;
        };
        let unit = &self.units[index];
        if unit.dead || unit.invulnerable || unit.has_buff(items::STASIS) || raw <= 0.0 {
            return 0.0;
        }
        if let Some((_, _, team, _)) = attacker
            && team == unit.team
        {
            return 0.0;
        }
        champions::before_damage(self, target, source);
        let unit = &self.units[index];
        let multiplier = match (kind, attacker) {
            (DamageKind::Physical, Some((stats, level, _, _))) => stats::physical_mitigation(
                unit.stats.armor,
                if critical {
                    stats.crit_bonus_armor_penetration
                        * (unit.stats.armor - unit.stats.base_armor).max(0.0)
                } else {
                    0.0
                },
                stats.lethality,
                stats.armor_penetration,
                level,
            ),
            (DamageKind::Physical, None) => stats::resist_multiplier(unit.stats.armor),
            (DamageKind::Magic, Some((stats, _, _, _))) => stats::magic_mitigation(
                unit.stats.magic_resist,
                stats.magic_penetration,
                stats.magic_penetration_percent,
            ),
            (DamageKind::Magic, None) => stats::resist_multiplier(unit.stats.magic_resist),
            (DamageKind::True, _) => 1.0,
        };
        let exhausted = source
            .and_then(|id| self.unit(id))
            .is_some_and(|unit| unit.has_buff("exhaust"));
        let dealer = attacker.and_then(|(_, _, _, owner)| owner).or(source);
        let balance = dealer.map_or(1.0, |dealer| self.balance(dealer).damage_dealt)
            * self.balance(target).damage_taken;
        let unit = &self.units[index];
        let weakened = if exhausted {
            1.0 - EXHAUST_WEAKNESS
        } else {
            1.0
        };
        let resisted = match kind {
            DamageKind::Magic => unit.stats.magic_damage_reduction,
            DamageKind::Physical | DamageKind::True => 0.0,
        };
        let mut amount = raw
            * multiplier
            * weakened
            * balance
            * (1.0 - unit.stats.damage_reduction)
            * (1.0 - resisted);
        let dealt = amount;
        let unit = &mut self.units[index];
        for shield in unit
            .shields
            .iter_mut()
            .filter(|shield| !shield.magic_only || kind == DamageKind::Magic)
        {
            let absorbed = shield.amount.min(amount);
            shield.amount -= absorbed;
            amount -= absorbed;
        }
        unit.health -= amount;
        if dealt > 0.0 {
            unit.hurt_at = self.time;
        }
        unit.trades.clear();
        let credit = source.map(|source| {
            attacker
                .and_then(|(_, _, _, owner)| owner)
                .unwrap_or(source)
        });
        if let Some(credit) = credit {
            unit.last_hit_by.retain(|(by, _)| *by != credit);
            unit.last_hit_by.push((credit, self.time));
        }
        let killed = unit.health <= 0.0;
        let reveal = self.time + REVEAL_TIME;
        let time = self.time;
        if let Some(source) = credit.and_then(|credit| self.unit_mut(credit)) {
            if dealt > 0.0 {
                source.dealt_at = time;
            }
            if source.champion().is_some() {
                source.revealed_until = reveal;
                source.trades.clear();
            }
        }
        self.events.push(Event::Damage {
            source: credit,
            target,
            amount: dealt,
            kind,
            critical,
        });
        if killed {
            self.kill(target, credit);
        }
        dealt
    }

    fn kill(&mut self, victim: UnitId, killer: Option<UnitId>) {
        if champions::prevent_death(self, victim) {
            return;
        }
        champions::died(self, victim);
        let Some(index) = self.index(victim) else {
            return;
        };
        let time = self.time;
        let unit = &mut self.units[index];
        unit.health = 0.0;
        unit.dead = true;
        unit.action = Action::None;
        unit.order = Order::Idle;
        unit.path.clear();
        unit.buffs
            .retain(|buff| buff.name == items::STASIS_COOLDOWN_BUFF);
        unit.shields.clear();
        let victim_unit = unit.clone();
        let credit = self.credit(&victim_unit, killer);
        let assisters = self.assisters(&victim_unit, credit);
        let survivors = self
            .units
            .iter()
            .filter(|unit| {
                unit.team == victim_unit.team && unit.champion().is_some() && unit.alive()
            })
            .count();
        self.events.push(Event::Kill {
            killer: credit,
            victim,
            assists: assisters.clone(),
            survivors: u8::try_from(survivors).unwrap_or(u8::MAX),
        });
        self.play(victim, "Death", 1.0);
        match victim_unit.kind {
            UnitKind::Champion(_) => {
                let killer = credit;
                let level = victim_unit.level;
                let respawn = respawn_time(level);
                if let Some(unit) = self.unit_mut(victim) {
                    unit.respawn_at = time + respawn;
                }
                if let Some(seat) = self.seats.iter_mut().find(|seat| seat.unit == victim) {
                    seat.score.deaths += 1;
                    seat.score.spree = 0;
                }
                if let Some(killer) = killer.filter(|killer| {
                    self.unit(*killer)
                        .is_some_and(|unit| unit.champion().is_some())
                }) {
                    self.give_gold(killer, KILL_GOLD);
                    if let Some(seat) = self.seats.iter_mut().find(|seat| seat.unit == killer) {
                        seat.score.kills += 1;
                        seat.score.spree += 1;
                    }
                }
                for assister in &assisters {
                    self.give_gold(*assister, KILL_GOLD * 0.5 / assisters.len() as f32);
                    if let Some(seat) = self.seats.iter_mut().find(|seat| seat.unit == *assister) {
                        seat.score.assists += 1;
                    }
                }
                let takedowns = killer
                    .filter(|killer| {
                        self.unit(*killer)
                            .is_some_and(|unit| unit.champion().is_some())
                    })
                    .into_iter()
                    .chain(assisters.iter().copied());
                for champion in takedowns.collect::<Vec<_>>() {
                    champions::on_takedown(self, champion, victim);
                }
                let xp = KILL_XP[usize::from(level.clamp(1, 18) - 1)];
                self.share_xp(victim_unit.team.enemy(), victim_unit.position, xp);
            }
            UnitKind::Minion(_) => {
                if let Some(killer) = killer.filter(|killer| {
                    self.unit(*killer)
                        .is_some_and(|unit| unit.champion().is_some())
                }) {
                    self.give_gold(killer, victim_unit.base.gold_given);
                    if let Some(seat) = self.seats.iter_mut().find(|seat| seat.unit == killer) {
                        seat.score.minions += 1;
                    }
                }
                self.share_xp(
                    victim_unit.team.enemy(),
                    victim_unit.position,
                    victim_unit.base.xp_given.max(30.0),
                );
            }
            UnitKind::Turret => {
                for champion in self.champions_of(victim_unit.team.enemy()) {
                    self.give_gold(champion, 100.0);
                }
            }
            UnitKind::Inhibitor => {
                if let Some(unit) = self.unit_mut(victim) {
                    unit.respawn_at = time + INHIBITOR_RESPAWN;
                }
            }
            UnitKind::Nexus => self.winner = Some(victim_unit.team.enemy()),
            UnitKind::SunDisc => {
                self.effect("AzirSunDiscDeath", victim_unit.position, Some(victim));
                if let Some(killer) = killer.filter(|killer| {
                    self.unit(*killer)
                        .is_some_and(|unit| unit.champion().is_some())
                }) {
                    self.give_gold(killer, SUN_DISC_BOUNTY);
                }
            }
            UnitKind::Shrine | UnitKind::Soldier => {}
        }
        champions::on_kill(self, killer, victim);
    }

    fn assisters(&self, victim: &Unit, killer: Option<UnitId>) -> Vec<UnitId> {
        if victim.champion().is_none() {
            return Vec::new();
        }
        victim
            .last_hit_by
            .iter()
            .rev()
            .map(|(by, _)| *by)
            .filter(|by| {
                Some(*by) != killer && self.unit(*by).is_some_and(|unit| unit.champion().is_some())
            })
            .collect()
    }

    fn credit(&self, victim: &Unit, killer: Option<UnitId>) -> Option<UnitId> {
        let champion = |id: &UnitId| self.unit(*id).is_some_and(|unit| unit.champion().is_some());
        if victim.champion().is_none() || killer.as_ref().is_some_and(champion) {
            return killer;
        }
        victim
            .last_hit_by
            .iter()
            .rev()
            .map(|(by, _)| *by)
            .find(champion)
            .or(killer)
    }

    pub fn champions_of(&self, team: Team) -> Vec<UnitId> {
        self.units
            .iter()
            .filter(|unit| unit.team == team && unit.champion().is_some())
            .map(|unit| unit.id)
            .collect()
    }

    pub fn give_gold(&mut self, id: UnitId, amount: f32) {
        if let Some(unit) = self.unit_mut(id) {
            unit.gold += amount;
            self.events.push(Event::Gold { target: id, amount });
        }
    }

    fn share_xp(&mut self, team: Team, position: Vec2, total: f32) {
        let nearby: Vec<UnitId> = self
            .units
            .iter()
            .filter(|unit| {
                unit.team == team
                    && unit.alive()
                    && unit.champion().is_some()
                    && unit.position.distance(position) <= XP_RANGE
            })
            .map(|unit| unit.id)
            .collect();
        for id in &nearby {
            self.grant_xp(*id, total / nearby.len() as f32);
        }
    }

    pub fn cheat(&mut self, id: UnitId, cheat: Cheat) {
        match cheat {
            Cheat::Level(level) => {
                let needed: f32 = XP_PER_LEVEL[..usize::from(level - 1)].iter().sum();
                let missing = self.unit(id).map_or(0.0, |unit| needed - unit.xp);
                if missing > 0.0 {
                    self.grant_xp(id, missing);
                }
            }
            Cheat::Gold(amount) => {
                if let Some(unit) = self.unit_mut(id) {
                    unit.gold += amount;
                }
            }
            Cheat::NoCooldowns => {
                if let Some(unit) = self.unit_mut(id) {
                    unit.no_cooldowns = !unit.no_cooldowns;
                }
            }
            Cheat::Heal => {
                if let Some(unit) = self.unit_mut(id) {
                    unit.health = unit.stats.max_health;
                    unit.mana = unit.stats.max_mana;
                }
            }
            Cheat::Minions => self.waves_paused = !self.waves_paused,
        }
    }

    pub fn grant_xp(&mut self, id: UnitId, amount: f32) {
        let Some(unit) = self.unit_mut(id) else {
            return;
        };
        unit.xp += amount;
        let mut leveled = false;
        while unit.level < MAX_LEVEL
            && unit.xp >= XP_PER_LEVEL[..usize::from(unit.level)].iter().sum::<f32>()
        {
            unit.level += 1;
            unit.skill_points += 1;
            leveled = true;
        }
        if leveled {
            self.refresh_stats(id);
            let position = self.unit(id).map_or(Vec2::ZERO, |unit| unit.position);
            self.effect("LevelUp", position, Some(id));
        }
    }

    fn advance_movement(&mut self, id: UnitId) {
        let Some(index) = self.index(id) else {
            return;
        };
        let nav = self.layout.nav.as_ref();
        let unit = &mut self.units[index];
        if unit.dead || unit.is_structure() {
            return;
        }
        if let Action::Dash { to, speed, kind } = unit.action {
            let offset = to - unit.position;
            let step = speed * TICK;
            if offset.length() <= step {
                unit.position = nav.map_or(to, |nav| nav::nearest_walkable(nav, to));
                unit.action = Action::None;
                champions::dash_finished(self, id, kind);
            } else {
                unit.position += offset.normalize() * step;
                if kind != DashKind::Knockback {
                    unit.facing = facing_to(Vec2::ZERO, offset, unit.facing);
                }
            }
            return;
        }
        if unit.is_stunned()
            || unit.has_buff("root")
            || matches!(unit.action, Action::Attack { .. } | Action::Interact { .. })
        {
            return;
        }
        if let Action::Cast { .. } = unit.action
            && !champions::can_move_while_casting(unit)
        {
            return;
        }
        let Some(&goal) = unit.path.first() else {
            unit.route.clear();
            return;
        };
        if let Some(nav) = nav
            && (unit.route.is_empty()
                || unit
                    .route_goal
                    .is_none_or(|previous| previous.distance(goal) > REROUTE_DISTANCE))
        {
            unit.route = nav::route(nav, unit.position, goal);
            unit.route_goal = Some(goal);
        }
        let step = unit.stats.move_speed * TICK;
        let stride = |unit: &Unit| {
            let next = unit.route.first().copied().unwrap_or(goal);
            unit.position + (next - unit.position).clamp_length_max(step)
        };
        if let Some(nav) = nav
            && nav::walkable(nav, unit.position)
            && !nav::walkable(nav, stride(unit))
        {
            unit.route = nav::route(nav, unit.position, goal);
            unit.route_goal = Some(goal);
        }
        let next = unit.route.first().copied().unwrap_or(goal);
        let offset = next - unit.position;
        let distance = offset.length();
        unit.facing = turn_towards(unit.facing, offset.x.atan2(offset.y), 30.0 * TICK);
        if distance <= step {
            unit.position = next;
            if !unit.route.is_empty() {
                unit.route.remove(0);
            }
            if unit.route.is_empty() {
                unit.path.remove(0);
                unit.route_goal = None;
                if unit.path.is_empty() && matches!(unit.order, Order::Move(_)) {
                    unit.order = Order::Idle;
                }
            }
        } else {
            unit.position += offset / distance * step;
        }
    }

    fn hold_barriers(&mut self) {
        let time = self.time;
        self.barriers.retain(|barrier| barrier.expires > time);
        let barriers: Vec<champions::Wall> = self
            .barriers
            .iter()
            .filter(|barrier| barrier.armed <= time)
            .copied()
            .collect();
        if barriers.is_empty() {
            return;
        }
        let mut stopped = Vec::new();
        for unit in &mut self.units {
            if unit.dead
                || unit.is_structure()
                || unit.kind == UnitKind::Soldier
                || matches!(
                    unit.action,
                    Action::Dash {
                        kind: DashKind::Knockback,
                        ..
                    }
                )
            {
                continue;
            }
            let radius = unit.radius();
            let blocked = barriers.iter().any(|barrier| {
                barrier.team != unit.team
                    && champions::stops(barrier, unit.last_position, unit.position, radius)
            });
            if blocked {
                unit.position = unit.last_position;
                if let Action::Dash { kind, .. } = unit.action {
                    unit.action = Action::None;
                    stopped.push((unit.id, kind));
                }
            }
        }
        for (id, kind) in stopped {
            champions::dash_finished(self, id, kind);
        }
    }

    fn separate_units(&mut self) {
        let count = self.units.len();
        let mut pushes = vec![Vec2::ZERO; count];
        for a in 0..count {
            let first = &self.units[a];
            if first.dead
                || first.kind == UnitKind::Soldier
                || matches!(first.action, Action::Dash { .. })
                || (first.is_structure() && !first.solid())
            {
                continue;
            }
            for b in (a + 1)..count {
                let second = &self.units[b];
                if second.dead
                    || second.kind == UnitKind::Soldier
                    || matches!(second.action, Action::Dash { .. })
                    || (second.is_structure() && (first.is_structure() || !second.solid()))
                    || (!first.solid()
                        && !second.solid()
                        && (first.ignores_collision() || second.ignores_collision()))
                {
                    continue;
                }
                let minimum =
                    first.base.pathing_radius.max(20.0) + second.base.pathing_radius.max(20.0);
                let offset = first.position - second.position;
                let distance = offset.length();
                if distance >= minimum {
                    continue;
                }
                let away = if distance < 1e-3 {
                    Vec2::from_angle((first.id ^ second.id) as f32)
                } else {
                    offset / distance
                };
                let push = away * (minimum - distance) * 0.5;
                if second.solid() {
                    pushes[a] += push * 2.0;
                } else if first.solid() {
                    pushes[b] -= push * 2.0;
                } else {
                    pushes[a] += push;
                    pushes[b] -= push;
                }
            }
        }
        for (unit, push) in self.units.iter_mut().zip(pushes) {
            let pushed = unit.position + push.clamp_length_max(8.0);
            if self
                .layout
                .nav
                .as_ref()
                .is_none_or(|nav| nav::walkable(nav, pushed))
            {
                unit.position = pushed;
            }
        }
    }

    fn update_missiles(&mut self) {
        let mut missiles = std::mem::take(&mut self.missiles);
        let mut keep = Vec::with_capacity(missiles.len());
        for mut missile in missiles.drain(..) {
            let flying = 'step: {
                if let Some(acceleration) = missile.acceleration {
                    missile.speed =
                        (missile.speed + acceleration.rate * TICK).min(acceleration.max_speed);
                }
                let step = missile.speed * TICK;
                match missile.motion {
                    Motion::Homing(target) => {
                        let Some(goal) = self
                            .unit(target)
                            .filter(|unit| unit.alive())
                            .map(|unit| unit.position)
                        else {
                            break 'step false;
                        };
                        let offset = goal - missile.position;
                        if offset.length() <= step + 20.0 {
                            self.resolve_hit(&missile, target);
                            break 'step false;
                        }
                        let from = missile.position;
                        missile.position += offset.normalize() * step;
                        let turret = self.unit(missile.owner).is_some_and(Unit::is_structure);
                        if !turret && champions::blocked_by_wind_wall(self, &missile, from) {
                            break 'step false;
                        }
                    }
                    Motion::Point(goal) => {
                        let offset = goal - missile.position;
                        if offset.length() <= step {
                            missile.position = goal;
                            champions::missile_arrived(self, &missile);
                            break 'step false;
                        }
                        missile.position += offset.normalize() * step;
                    }
                    Motion::Line {
                        direction,
                        remaining,
                        limit,
                    } => {
                        let travel = step.min(remaining);
                        let from = missile.position;
                        missile.position += direction * travel;
                        let mut hits: Vec<(f32, UnitId)> = self
                            .units
                            .iter()
                            .filter(|unit| {
                                unit.team != missile.team
                                    && unit.alive()
                                    && !unit.untargetable()
                                    && !unit.is_structure()
                                    && unit.kind != UnitKind::Soldier
                                    && !missile.hit_units.contains(&unit.id)
                                    && segment_distance(from, missile.position, unit.position)
                                        <= missile.width + unit.radius()
                            })
                            .map(|unit| (unit.position.distance(from), unit.id))
                            .collect();
                        hits.sort_by(|a, b| a.0.total_cmp(&b.0));
                        let mut stopped = false;
                        for (_, hit) in hits {
                            missile.hit_units.push(hit);
                            self.resolve_hit(&missile, hit);
                            if limit.is_some_and(|limit| missile.hit_units.len() >= limit)
                                || champions::stops_missile(self, &missile, hit)
                            {
                                stopped = true;
                                break;
                            }
                        }
                        if stopped || champions::blocked_by_wind_wall(self, &missile, from) {
                            break 'step false;
                        }
                        let remaining = remaining - travel;
                        if remaining <= 0.0 {
                            champions::missile_arrived(self, &missile);
                            break 'step false;
                        }
                        missile.motion = Motion::Line {
                            direction,
                            remaining,
                            limit,
                        };
                    }
                }
                true
            };
            if !flying {
                self.events.push(Event::MissileLanded {
                    id: missile.id,
                    team: missile.team,
                    position: missile.position.to_array(),
                });
                continue;
            }
            keep.push(missile);
        }
        keep.append(&mut self.missiles);
        self.missiles = keep;
    }

    fn resolve_hit(&mut self, missile: &Missile, target: UnitId) {
        match missile.hit {
            Hit::Attack { critical } => self.attack_hit(missile.owner, target, critical),
            _ => champions::missile_hit(self, missile, target),
        }
    }

    fn restore_missing(&mut self, id: UnitId, share: f32) {
        let Some((health, mana)) = self.unit(id).map(|unit| {
            (
                unit.stats.max_health - unit.health,
                unit.stats.max_mana - unit.mana,
            )
        }) else {
            return;
        };
        self.heal(id, health.max(0.0) * share);
        if let Some(unit) = self.unit_mut(id) {
            unit.mana = (unit.mana + mana.max(0.0) * share).min(unit.stats.max_mana);
        }
    }

    fn update_relics(&mut self) {
        let time = self.time;
        for relic_index in 0..self.relics.len() {
            let relic = self.relics[relic_index];
            if let Some((strikes, team)) = relic.beam
                && time >= strikes
            {
                self.relics[relic_index].beam = None;
                let allies: Vec<UnitId> = self
                    .units
                    .iter()
                    .filter(|unit| {
                        unit.alive()
                            && unit.team == team
                            && unit.champion().is_some()
                            && unit.position.distance(relic.position) <= RELIC_BEAM_RADIUS
                    })
                    .map(|unit| unit.id)
                    .collect();
                for ally in allies {
                    self.restore_missing(ally, RELIC_BEAM_RESTORE);
                }
            }
            if time < relic.ready_at {
                continue;
            }
            let taker = self
                .units
                .iter()
                .find(|unit| {
                    unit.alive()
                        && unit.champion().is_some()
                        && unit.position.distance(relic.position) < RELIC_RADIUS + unit.radius()
                })
                .map(|unit| unit.id);
            if let Some(taker) = taker {
                let team = self.unit(taker).map_or(Team::Blue, |unit| unit.team);
                self.relics[relic_index].ready_at = time + RELIC_BEAM_DELAY + RELIC_RESPAWN;
                self.relics[relic_index].beam = Some((time + RELIC_BEAM_DELAY, team));
                self.restore_missing(taker, RELIC_PICKUP_RESTORE);
                self.effect("HealthRelic", relic.position, None);
            }
        }
    }

    fn respawn(&mut self) {
        let time = self.time;
        let mut respawned = Vec::new();
        for unit in self.units.iter_mut() {
            if unit.dead && unit.respawn_at > 0.0 && time >= unit.respawn_at {
                unit.dead = false;
                unit.respawn_at = 0.0;
                unit.health = 0.0;
                unit.buffs
                    .retain(|buff| buff.name == items::STASIS_COOLDOWN_BUFF);
                unit.shields.clear();
                if unit.champion().is_some() {
                    unit.position = self.layout.fountains[team_index(unit.team)];
                    unit.last_position = unit.position;
                    unit.mana = f32::MAX;
                }
                respawned.push(unit.id);
            }
        }
        for id in respawned {
            self.refresh_stats(id);
            if let Some(unit) = self.unit_mut(id) {
                unit.health = unit.stats.max_health;
                unit.mana = unit.stats.max_mana;
                unit.animation = None;
            }
            self.play(id, "Respawn", 1.0);
            let position = self.unit(id).map_or(Vec2::ZERO, |unit| unit.position);
            self.effect("Respawn", position, Some(id));
            champions::respawned(self, id);
        }
    }

    fn cleanup(&mut self) {
        self.units.retain(|unit| {
            !(unit.dead
                && (unit.is_minion() || unit.kind == UnitKind::Soldier)
                && unit.respawn_at == 0.0
                && unit.health <= 0.0
                || unit.dead && matches!(unit.kind, UnitKind::Soldier | UnitKind::SunDisc))
        });
    }

    pub fn in_fountain(&self, unit: &Unit) -> bool {
        unit.position.distance(self.layout.fountain(unit.team)) <= self.layout.fountain_radius
    }

    pub fn snapshot(&self, viewer: Option<UnitId>) -> Snapshot {
        let nav = self.layout.nav.as_ref();
        let team = viewer.and_then(|id| self.unit(id)).map(|unit| unit.team);
        let visible = team.map(|team| vision::visible_to(&self.units, nav, team, self.time));
        let seen = |id: UnitId| visible.as_ref().is_none_or(|visible| visible.contains(&id));
        let hidden = team
            .map(|team| vision::hidden_in_brush(&self.units, nav, team, self.time))
            .unwrap_or_default();
        let point_seen = |point: Vec2| {
            team.is_none_or(|team| vision::point_visible(&self.units, nav, team, point))
        };
        let units = self
            .units
            .iter()
            .filter(|unit| {
                unit.alive() || unit.champion().is_some() || unit.is_structure() || unit.is_minion()
            })
            .filter(|unit| seen(unit.id))
            .map(|unit| (unit, champions::resource(self, unit)))
            .map(|(unit, (mana, max_mana))| UnitState {
                id: unit.id,
                kind: unit.kind,
                team: unit.team,
                position: unit.position.to_array(),
                facing: unit.facing,
                health: unit.health.max(0.0),
                max_health: unit.stats.max_health,
                shield: unit.shield(),
                mana,
                max_mana,
                level: unit.level,
                move_speed: unit.stats.move_speed,
                moving: unit.alive()
                    && !unit.path.is_empty()
                    && !unit.is_stunned()
                    && matches!(unit.action, Action::None),
                dashing: unit.alive()
                    && matches!(unit.action, Action::Dash { .. } | Action::Airborne { .. }),
                dead: unit.dead,
                targetable: !unit.untargetable() && !unit.invulnerable,
                size: 1.0 + unit.stats.bonus_size,
                respawn: (unit.dead && unit.champion().is_some())
                    .then(|| (unit.respawn_at - self.time).max(0.0)),
                ultimate: unit
                    .champion()
                    .filter(|_| team.is_none_or(|team| team == unit.team))
                    .map(|_| unit.ability(Slot::R))
                    .filter(|ultimate| ultimate.rank > 0)
                    .map(|ultimate| {
                        if ultimate.cooldown_total > 0.0 {
                            (ultimate.cooldown / ultimate.cooldown_total).clamp(0.0, 1.0)
                        } else {
                            0.0
                        }
                    }),
                animation: unit.animation,
                buffs: unit
                    .buffs
                    .iter()
                    .map(|buff| clip_id(buff.name))
                    .chain(champions::indicator(self, unit.id).map(clip_id))
                    .chain(hidden.contains(&unit.id).then(|| clip_id("brush")))
                    .collect(),
                stats: unit.champion().map(|_| shown_stats(&unit.stats)),
            })
            .collect();
        let you = viewer.and_then(|id| self.unit(id)).map(|unit| You {
            gold: unit.gold,
            xp: unit.xp
                - XP_PER_LEVEL[..usize::from(unit.level - 1)]
                    .iter()
                    .sum::<f32>(),
            xp_needed: XP_PER_LEVEL
                .get(usize::from(unit.level - 1))
                .copied()
                .unwrap_or(0.0),
            skill_points: unit.skill_points,
            passive_cooldown: champions::passive_cooldown(self, unit),
            abilities: std::array::from_fn(|index| {
                let ability = unit.abilities[index];
                let ammo = champions::ammo_slot(self, unit.id) == Some(unit::SLOTS[index]);
                let (cooldown, cooldown_total) = if ammo && ability.charges == 0 {
                    (
                        ability.cooldown.max(ability.recharge),
                        ability.recharge_total,
                    )
                } else {
                    (ability.cooldown, ability.cooldown_total)
                };
                protocol::AbilityState {
                    rank: ability.rank,
                    cooldown,
                    cooldown_total,
                    charges: ability.charges,
                    ammo,
                    recast: champions::recasting(self, unit, unit::SLOTS[index]),
                    range: champions::cast_range(self, unit, unit::SLOTS[index]),
                    area: champions::area(self, unit, unit::SLOTS[index]),
                }
            }),
            items: unit.items,
            stacks: unit.stacks,
            item_cooldowns: std::array::from_fn(|slot| items::cooldown(self, unit, slot)),
            buffs: unit
                .buffs
                .iter()
                .map(|buff| protocol::BuffState {
                    name: clip_id(buff.name),
                    source: buff.source,
                    slot: buff.slot,
                    remaining: (buff.expires - self.time).max(0.0),
                    duration: buff.duration,
                    stacks: buff.stacks,
                })
                .collect(),
            respawn: unit.dead.then_some((unit.respawn_at - self.time).max(0.0)),
            can_shop: unit.dead || self.in_fountain(unit),
            can_undo: !unit.trades.is_empty(),
        });
        Snapshot {
            tick: self.tick,
            units,
            missiles: self
                .missiles
                .iter()
                .filter(|missile| {
                    point_seen(missile.position)
                        || self
                            .unit(missile.owner)
                            .is_some_and(|owner| Some(owner.team) == team)
                })
                .map(|missile| MissileState {
                    id: missile.id,
                    owner: missile.owner,
                    target: match missile.motion {
                        Motion::Homing(target) => Some(target),
                        _ => None,
                    },
                    kind: missile.kind,
                    position: missile.position.to_array(),
                    height: missile.height,
                    facing: match missile.motion {
                        Motion::Line { direction, .. } => direction.x.atan2(direction.y),
                        _ => 0.0,
                    },
                })
                .collect(),
            relics: self
                .relics
                .iter()
                .map(|relic| RelicState {
                    position: relic.position.to_array(),
                    team: relic.team,
                    inner: relic.inner,
                    ready_in: (relic.ready_at - self.time).max(0.0),
                })
                .collect(),
            props: self
                .props
                .iter()
                .filter(|prop| {
                    if prop.hidden {
                        team.is_none_or(|team| team == prop.team)
                    } else {
                        point_seen(prop.position)
                    }
                })
                .map(|prop| PropState {
                    id: prop.id,
                    effect: clip_id(prop.effect),
                    position: prop.position.to_array(),
                    facing: prop.facing,
                    team: prop.team,
                })
                .collect(),
            interactables: viewer
                .map(|viewer| champions::interactables(self, viewer))
                .unwrap_or_default(),
            scores: self
                .seats
                .iter()
                .map(|seat| {
                    let items = self.unit(seat.unit).map_or([None; 6], |unit| unit.items);
                    (
                        seat.unit,
                        Score {
                            items,
                            ..seat.score
                        },
                    )
                })
                .collect(),
            you,
            events: self
                .events
                .iter()
                .filter(|event| match event {
                    Event::Effect { global: true, .. } => true,
                    Event::Effect { position, unit, .. } => match unit.and_then(|id| self.unit(id))
                    {
                        Some(unit) if unit.camouflage().is_some() => seen(unit.id),
                        _ => unit.is_some_and(seen) || point_seen(Vec2::from(*position)),
                    },
                    Event::Damage { target, .. }
                    | Event::Heal { target, .. }
                    | Event::Sound { unit: target, .. } => seen(*target),
                    Event::MissileLanded {
                        team: owner,
                        position,
                        ..
                    } => Some(*owner) == team || point_seen(Vec2::from(*position)),
                    _ => true,
                })
                .cloned()
                .collect(),
        }
    }

    pub fn clear_events(&mut self) {
        self.events.clear();
    }
}

fn cancel_attack_windup(unit: &mut Unit) {
    if let Action::Attack { .. } = unit.action {
        unit.action = Action::None;
        unit.attack_ready_at =
            0.0_f32.max(unit.attack_ready_at - 1.0 / unit.stats.attack_speed.max(0.1));
        unit.animation = None;
    }
}

pub fn turn_towards(from: f32, to: f32, max_step: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let delta = (to - from + PI).rem_euclid(TAU) - PI;
    from + delta.clamp(-max_step, max_step)
}

pub fn segment_distance(a: Vec2, b: Vec2, point: Vec2) -> f32 {
    let segment = b - a;
    let length = segment.length_squared();
    if length < 1e-6 {
        return point.distance(a);
    }
    let t = ((point - a).dot(segment) / length).clamp(0.0, 1.0);
    point.distance(a + segment * t)
}

pub fn respawn_time(level: u8) -> f32 {
    6.0 + f32::from(level) * 2.0
}

fn minion_name(team: Team, kind: MinionKind) -> &'static str {
    match (team, kind) {
        (Team::Blue, MinionKind::Melee) => "HA_OrderMinionMelee",
        (Team::Blue, MinionKind::Caster) => "HA_OrderMinionRanged",
        (Team::Blue, MinionKind::Siege) => "HA_OrderMinionSiege",
        (Team::Blue, MinionKind::Super) => "HA_OrderMinionSuper",
        (Team::Red, MinionKind::Melee) => "HA_ChaosMinionMelee",
        (Team::Red, MinionKind::Caster) => "HA_ChaosMinionRanged",
        (Team::Red, MinionKind::Siege) => "HA_ChaosMinionSiege",
        (Team::Red, MinionKind::Super) => "HA_ChaosMinionSuper",
    }
}

fn shown_stats(stats: &stats::Stats) -> protocol::Stats {
    protocol::Stats {
        attack_damage: stats.attack_damage,
        ability_power: stats.ability_power,
        armor: stats.armor,
        magic_resist: stats.magic_resist,
        attack_speed: stats.attack_speed,
        ability_haste: stats.ability_haste,
        crit_chance: stats.crit_chance,
        move_speed: stats.move_speed,
        health_regen: stats.health_regen,
        lethality: stats.lethality,
        armor_penetration: stats.armor_penetration,
        magic_penetration: stats.magic_penetration,
        magic_penetration_percent: stats.magic_penetration_percent,
        life_steal: stats.life_steal,
        attack_range: stats.attack_range,
    }
}

fn turret_stats(mut base: UnitStats, tier: Tier) -> UnitStats {
    let (health, damage) = match tier {
        Tier::Outer => (4000.0, 180.0),
        Tier::Inner => (4000.0, 200.0),
        Tier::Nexus => (3600.0, 160.0),
        Tier::Fountain => (9999.0, 1000.0),
    };
    if base.health <= 0.0 {
        base.health = health;
    }
    base.health = base.health.max(health);
    base.attack_damage = base.attack_damage.max(damage);
    if base.attack_range <= 0.0 {
        base.attack_range = 775.0;
    }
    if base.attack_speed <= 0.0 {
        base.attack_speed = 0.83;
    }
    base.armor = base.armor.max(40.0);
    base.magic_resist = base.magic_resist.max(40.0);
    base.gameplay_radius = base.gameplay_radius.max(88.0);
    base.pathing_radius = base.pathing_radius.max(88.0);
    if base.attack.missile_speed <= 0.0 {
        base.attack.missile_speed = 1200.0;
    }
    if tier == Tier::Fountain {
        base.attack_range = 1250.0;
        base.attack_speed = 2.0;
    }
    base
}

pub fn slot_ready(unit: &Unit, slot: Slot) -> bool {
    let ability = unit.ability(slot);
    ability.rank > 0 && (ability.cooldown <= 0.0 || ability.charges > 0)
}

pub fn all_slots() -> [Slot; 6] {
    SLOTS
}

pub fn target_point(target: &Target) -> Vec2 {
    Vec2::from_array(target.point)
}

#[cfg(test)]
mod tests;
