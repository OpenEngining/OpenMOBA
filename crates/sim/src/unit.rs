use asset::data::UnitStats;
use glam::Vec2;
use protocol::{Animation, MinionKind, Slot, Summoner, Target, Team, UnitId, UnitKind};

use crate::items::Trade;
use crate::stats::{Bonus, Stats};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Order {
    Idle,
    Move(Vec2),
    AttackMove(Vec2),
    Attack(UnitId),
    Cast(Slot, Target),
    Interact(UnitId),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    None,
    Attack {
        target: UnitId,
        elapsed: f32,
        windup: f32,
        critical: bool,
    },
    Cast {
        slot: Slot,
        target: Target,
        elapsed: f32,
        windup: f32,
        recovery: f32,
        fired: bool,
        mobile: bool,
        channel: bool,
    },
    Dash {
        to: Vec2,
        speed: f32,
        kind: DashKind,
    },
    Airborne {
        until: f32,
    },
    Interact {
        target: UnitId,
        elapsed: f32,
        windup: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DashKind {
    Plain,
    Spell(Slot, Option<UnitId>),
    Knockback,
    Mark(UnitId),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Buff {
    pub name: &'static str,
    pub expires: f32,
    pub duration: f32,
    pub stacks: u32,
    pub bonus: Bonus,
    pub source: Option<UnitId>,
    pub slot: Option<Slot>,
    pub value: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shield {
    pub name: &'static str,
    pub amount: f32,
    pub expires: f32,
    pub magic_only: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Ability {
    pub rank: u8,
    pub cooldown: f32,
    pub cooldown_total: f32,
    pub charges: u8,
    pub recharge: f32,
    pub recharge_total: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Outer,
    Inner,
    Nexus,
    Fountain,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Unit {
    pub id: UnitId,
    pub kind: UnitKind,
    pub team: Team,
    pub summoners: [Summoner; 2],
    pub data: String,
    pub base: UnitStats,
    pub stats: Stats,
    pub position: Vec2,
    pub facing: f32,
    pub path: Vec<Vec2>,
    pub health: f32,
    pub mana: f32,
    pub shields: Vec<Shield>,
    pub level: u8,
    pub xp: f32,
    pub gold: f32,
    pub dead: bool,
    pub respawn_at: f32,
    pub order: Order,
    pub action: Action,
    pub attack_ready_at: f32,
    pub attack_index: u32,
    pub buffs: Vec<Buff>,
    pub items: [Option<u32>; 6],
    pub stacks: [u8; 6],
    pub trades: Vec<Trade>,
    pub abilities: [Ability; 6],
    pub skill_points: u8,
    pub animation: Option<Animation>,
    pub owner: Option<UnitId>,
    pub waypoint: usize,
    pub targetable: bool,
    pub invulnerable: bool,
    pub tier: Option<Tier>,
    pub expires: Option<f32>,
    pub last_hit_by: Vec<(UnitId, f32)>,
    pub hurt_at: f32,
    pub dealt_at: f32,
    pub no_cooldowns: bool,
    pub turret_target_bonus: f32,
    pub counter: f32,
    pub spawn: Vec2,
    pub last_position: Vec2,
    pub route: Vec<Vec2>,
    pub route_goal: Option<Vec2>,
    pub revealed_until: f32,
}

impl Unit {
    pub fn new(
        id: UnitId,
        kind: UnitKind,
        team: Team,
        data: &str,
        base: UnitStats,
        position: Vec2,
    ) -> Self {
        Self {
            id,
            kind,
            team,
            summoners: Summoner::DEFAULT,
            data: data.to_owned(),
            base,
            stats: Stats::default(),
            position,
            facing: 0.0,
            path: Vec::new(),
            health: 0.0,
            mana: 0.0,
            shields: Vec::new(),
            level: 1,
            xp: 0.0,
            gold: 0.0,
            dead: false,
            respawn_at: 0.0,
            order: Order::Idle,
            action: Action::None,
            attack_ready_at: 0.0,
            attack_index: 0,
            buffs: Vec::new(),
            items: [None; 6],
            stacks: [0; 6],
            trades: Vec::new(),
            abilities: [Ability::default(); 6],
            skill_points: 0,
            animation: None,
            owner: None,
            waypoint: 0,
            targetable: true,
            invulnerable: false,
            tier: None,
            expires: None,
            last_hit_by: Vec::new(),
            hurt_at: f32::MIN,
            dealt_at: f32::MIN,
            no_cooldowns: false,
            turret_target_bonus: 0.0,
            counter: 0.0,
            spawn: position,
            last_position: position,
            route: Vec::new(),
            route_goal: None,
            revealed_until: 0.0,
        }
    }

    pub fn champion(&self) -> Option<u8> {
        match self.kind {
            UnitKind::Champion(index) => Some(index),
            _ => None,
        }
    }

    pub fn is_structure(&self) -> bool {
        matches!(
            self.kind,
            UnitKind::Turret
                | UnitKind::Shrine
                | UnitKind::Inhibitor
                | UnitKind::Nexus
                | UnitKind::SunDisc
        )
    }

    pub fn solid(&self) -> bool {
        self.is_structure() && self.kind != UnitKind::Shrine
    }

    pub fn is_minion(&self) -> bool {
        matches!(self.kind, UnitKind::Minion(_))
    }

    pub fn minion(&self) -> Option<MinionKind> {
        match self.kind {
            UnitKind::Minion(kind) => Some(kind),
            _ => None,
        }
    }

    pub fn radius(&self) -> f32 {
        self.base.gameplay_radius.max(1.0)
    }

    pub fn alive(&self) -> bool {
        !self.dead
    }

    pub fn shield(&self) -> f32 {
        self.shields.iter().map(|shield| shield.amount).sum()
    }

    pub fn has_buff(&self, name: &str) -> bool {
        self.buffs.iter().any(|buff| buff.name == name)
    }

    pub fn buff(&self, name: &str) -> Option<&Buff> {
        self.buffs.iter().find(|buff| buff.name == name)
    }

    pub fn buff_mut(&mut self, name: &str) -> Option<&mut Buff> {
        self.buffs.iter_mut().find(|buff| buff.name == name)
    }

    pub fn unstoppable(&self) -> bool {
        self.has_buff("unstoppable") || self.has_buff(crate::items::STASIS)
    }

    pub fn untargetable(&self) -> bool {
        !self.targetable || self.has_buff(crate::items::STASIS)
    }

    pub fn is_stunned(&self) -> bool {
        self.buffs
            .iter()
            .any(|buff| matches!(buff.name, "stun" | "knockup" | "suppression" | "stasis"))
            || matches!(self.action, Action::Airborne { .. })
    }

    pub fn is_silenced(&self) -> bool {
        self.has_buff("silence")
    }

    pub fn ignores_collision(&self) -> bool {
        self.has_buff("ghost")
            || self.has_buff("ghosted")
            || self.has_buff("judgment")
            || self.has_buff("void_stone")
    }

    pub fn camouflage(&self) -> Option<f32> {
        self.buff(CAMOUFLAGE).map(|buff| buff.value)
    }

    pub fn is_disarmed(&self) -> bool {
        self.has_buff("disarm") || self.has_buff("reloading")
    }

    pub fn is_large_minion(&self) -> bool {
        matches!(self.minion(), Some(MinionKind::Siege | MinionKind::Super))
    }

    pub fn can_act(&self) -> bool {
        self.alive() && !self.is_stunned() && !matches!(self.action, Action::Dash { .. })
    }

    pub fn ability(&self, slot: Slot) -> &Ability {
        &self.abilities[slot_index(slot)]
    }

    pub fn ability_mut(&mut self, slot: Slot) -> &mut Ability {
        &mut self.abilities[slot_index(slot)]
    }
}

pub fn slot_index(slot: Slot) -> usize {
    match slot {
        Slot::Q => 0,
        Slot::W => 1,
        Slot::E => 2,
        Slot::R => 3,
        Slot::D => 4,
        Slot::F => 5,
    }
}

pub const CAMOUFLAGE: &str = "camouflage";

pub const SLOTS: [Slot; 6] = [Slot::Q, Slot::W, Slot::E, Slot::R, Slot::D, Slot::F];
