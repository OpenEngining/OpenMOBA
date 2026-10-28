use serde::{Deserialize, Serialize};

pub const TICK_RATE: u32 = 30;
pub const TICK: f32 = 1.0 / TICK_RATE as f32;

pub type UnitId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Team {
    Blue,
    Red,
}

impl Team {
    pub fn enemy(self) -> Self {
        match self {
            Team::Blue => Team::Red,
            Team::Red => Team::Blue,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Slot {
    Q,
    W,
    E,
    R,
    D,
    F,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Target {
    pub point: [f32; 2],
    pub unit: Option<UnitId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Command {
    Move([f32; 2]),
    AttackMove([f32; 2]),
    Attack(UnitId),
    Stop,
    Cast(Slot, Target),
    LevelUp(Slot),
    Buy(u32),
    Sell(u8),
    Undo,
    Emote(Emote),
    UseItem(u8, Target),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Emote {
    Joke,
    Taunt,
    Dance,
    Laugh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PingKind {
    Generic,
    Danger,
    OnMyWay,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClientMessage {
    SignUp { name: String, password: String },
    LogIn { name: String, password: String },
    Resume { token: String },
    JoinTeam(Team),
    Start,
    Pick(u8),
    LockIn,
    Summoners([Summoner; 2]),
    Command(Command),
    Chat { text: String, all: bool },
    Ping { position: [f32; 2], kind: PingKind },
    EndGame,
    Loading(u8),
    CreateLobby(String),
    JoinLobby(u64),
    LeaveLobby,
    SetAvatar(u8),
    LogOut,
    Ban(Option<u8>),
    StartVote(VoteKind),
    Vote(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthError {
    WrongNameOrPassword,
    NameTaken,
    InvalidName,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LobbyPlayer {
    pub name: String,
    pub team: Team,
    pub online: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lobby {
    pub players: Vec<LobbyPlayer>,
    pub phase: Phase,
    pub id: u64,
    pub name: String,
    pub host: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    Lobby,
    Select,
    Game,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectPlayer {
    pub name: String,
    pub team: Team,
    pub champion: Option<u8>,
    pub locked: bool,
    pub summoners: [Summoner; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Summoner {
    Flash,
    Mark,
    Heal,
    Ghost,
    Barrier,
    Exhaust,
    Clarity,
    Cleanse,
    Ignite,
}

impl Summoner {
    pub const ALL: [Self; 9] = [
        Self::Heal,
        Self::Ghost,
        Self::Barrier,
        Self::Exhaust,
        Self::Clarity,
        Self::Flash,
        Self::Mark,
        Self::Cleanse,
        Self::Ignite,
    ];

    pub const DEFAULT: [Self; 2] = [Self::Flash, Self::Mark];

    pub fn id(self) -> u32 {
        match self {
            Self::Cleanse => 1,
            Self::Exhaust => 3,
            Self::Flash => 4,
            Self::Ghost => 6,
            Self::Heal => 7,
            Self::Clarity => 13,
            Self::Ignite => 14,
            Self::Barrier => 21,
            Self::Mark => 32,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChampSelect {
    pub players: Vec<SelectPlayer>,
    pub remaining: f32,
    pub banning: bool,
    pub bans: Vec<(String, Option<u8>)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub avatar: u8,
    pub wins: u32,
    pub games: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomInfo {
    pub id: u64,
    pub name: String,
    pub host: String,
    pub players: usize,
    pub phase: Phase,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Directory {
    pub rooms: Vec<RoomInfo>,
    pub profile: Profile,
    pub leaderboard: Vec<Profile>,
    pub room: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoteKind {
    Surrender,
    EndGame,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoteStatus {
    pub kind: VoteKind,
    pub yes: usize,
    pub needed: usize,
    pub remaining: f32,
    pub voted: bool,
    pub result: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player {
    pub name: String,
    pub team: Team,
    pub champion: u8,
    pub unit: UnitId,
    pub summoners: [Summoner; 2],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Setup {
    pub first_wave: u32,
    pub you: UnitId,
    pub players: Vec<Player>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnitKind {
    Champion(u8),
    Minion(MinionKind),
    Turret,
    Shrine,
    Inhibitor,
    Nexus,
    Soldier,
    SunDisc,
}

pub fn sight_range(kind: UnitKind) -> f32 {
    match kind {
        UnitKind::Champion(_) => 1350.0,
        UnitKind::Minion(_) => 1100.0,
        UnitKind::Soldier => 700.0,
        UnitKind::Turret
        | UnitKind::Shrine
        | UnitKind::Inhibitor
        | UnitKind::Nexus
        | UnitKind::SunDisc => 1350.0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MinionKind {
    Melee,
    Caster,
    Siege,
    Super,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Animation {
    pub clip: u32,
    pub started: u32,
    pub speed: f32,
    pub hold: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnitState {
    pub id: UnitId,
    pub kind: UnitKind,
    pub team: Team,
    pub position: [f32; 2],
    pub facing: f32,
    pub health: f32,
    pub max_health: f32,
    pub shield: f32,
    pub mana: f32,
    pub max_mana: f32,
    pub level: u8,
    pub move_speed: f32,
    pub moving: bool,
    pub dashing: bool,
    pub dead: bool,
    pub targetable: bool,
    pub size: f32,
    pub respawn: Option<f32>,
    pub ultimate: Option<f32>,
    pub animation: Option<Animation>,
    pub buffs: Vec<u32>,
    pub stats: Option<Stats>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MissileState {
    pub id: u32,
    pub owner: UnitId,
    pub target: Option<UnitId>,
    pub kind: u32,
    pub position: [f32; 2],
    pub height: f32,
    pub facing: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AbilityState {
    pub rank: u8,
    pub cooldown: f32,
    pub cooldown_total: f32,
    pub charges: u8,
    pub ammo: bool,
    pub recast: bool,
    pub range: f32,
    pub area: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub attack_damage: f32,
    pub ability_power: f32,
    pub armor: f32,
    pub magic_resist: f32,
    pub attack_speed: f32,
    pub ability_haste: f32,
    pub crit_chance: f32,
    pub move_speed: f32,
    pub health_regen: f32,
    pub lethality: f32,
    pub armor_penetration: f32,
    pub magic_penetration: f32,
    pub magic_penetration_percent: f32,
    pub life_steal: f32,
    pub attack_range: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RelicState {
    pub position: [f32; 2],
    pub team: Team,
    pub inner: bool,
    pub ready_in: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PropState {
    pub id: u32,
    pub effect: u32,
    pub position: [f32; 2],
    pub facing: f32,
    pub team: Team,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct You {
    pub gold: f32,
    pub xp: f32,
    pub xp_needed: f32,
    pub skill_points: u8,
    pub abilities: [AbilityState; 6],
    pub passive_cooldown: Option<(f32, f32)>,
    pub items: [Option<u32>; 6],
    pub stacks: [u8; 6],
    pub item_cooldowns: [f32; 6],
    pub buffs: Vec<BuffState>,
    pub respawn: Option<f32>,
    pub can_shop: bool,
    pub can_undo: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BuffState {
    pub name: u32,
    pub source: Option<UnitId>,
    pub slot: Option<Slot>,
    pub remaining: f32,
    pub duration: f32,
    pub stacks: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Score {
    pub kills: u16,
    pub deaths: u16,
    pub assists: u16,
    pub spree: u16,
    pub minions: u16,
    pub items: [Option<u32>; 6],
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum DamageKind {
    Physical,
    Magic,
    True,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Event {
    Damage {
        source: Option<UnitId>,
        target: UnitId,
        amount: f32,
        kind: DamageKind,
        critical: bool,
    },
    Heal {
        target: UnitId,
        amount: f32,
    },
    Gold {
        target: UnitId,
        amount: f32,
    },
    Kill {
        killer: Option<UnitId>,
        victim: UnitId,
        assists: Vec<UnitId>,
        survivors: u8,
    },
    Effect {
        name: u32,
        position: [f32; 2],
        facing: f32,
        unit: Option<UnitId>,
        team: Option<Team>,
        global: bool,
    },
    Sound {
        unit: UnitId,
        key: String,
    },
    Ping {
        position: [f32; 2],
    },
    MissileLanded {
        id: u32,
        team: Team,
        position: [f32; 2],
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub tick: u32,
    pub units: Vec<UnitState>,
    pub missiles: Vec<MissileState>,
    pub relics: Vec<RelicState>,
    pub props: Vec<PropState>,
    pub interactables: Vec<UnitId>,
    pub scores: Vec<(UnitId, Score)>,
    pub you: Option<You>,
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ServerMessage {
    Session {
        token: String,
        name: String,
    },
    AuthFailed(AuthError),
    Lobby(Lobby),
    ChampSelect(ChampSelect),
    GameStarted(Setup),
    Chat {
        from: String,
        team: Team,
        text: String,
        all: bool,
    },
    Ping {
        from: String,
        position: [f32; 2],
        kind: PingKind,
    },
    Snapshot(Box<Snapshot>),
    GameOver {
        winner: Team,
    },
    Loading(Vec<(String, u8)>),
    Directory(Directory),
    Vote(Option<VoteStatus>),
    Notice(String),
}

pub fn encode<T: Serialize>(message: &T) -> Vec<u8> {
    postcard::to_allocvec(message).unwrap_or_default()
}

pub fn decode<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Option<T> {
    postcard::from_bytes(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip() {
        let message = ClientMessage::Command(Command::Cast(
            Slot::Q,
            Target {
                point: [1.0, 2.0],
                unit: Some(7),
            },
        ));
        assert_eq!(decode::<ClientMessage>(&encode(&message)), Some(message));
    }

    #[test]
    fn teams_are_enemies() {
        assert_eq!(Team::Blue.enemy(), Team::Red);
        assert_eq!(Team::Red.enemy(), Team::Blue);
    }
}
