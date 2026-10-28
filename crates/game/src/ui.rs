use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

use asset::clip_id;
use asset::data::{GameData, Resource, Spell, SummonerData};
use asset::ui::{Layout, Rect};
use protocol::{
    AuthError, BuffState, Player, Setup, Slot, Snapshot, Stats, Summoner, Team, UnitId, UnitState,
    You,
};
use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlElement, HtmlInputElement, KeyboardEvent};

use crate::banner::{Banner, Banners};
use crate::buffs;
use crate::callouts::{Callout, Callouts};
use crate::dom::{
    Listener, append, element, input, listen, on, rich_markup, set_style, set_text, show, text,
};
use crate::fade::Fade;
use crate::frame::{Frame, View};
use crate::relics::RelicTimers;
use crate::scoreboard::Scoreboard;
use crate::shop::Shop;
use crate::teamframes::{Ally, TeamFrames};

const ANNOUNCEMENT_TIME: f32 = 2.8;
const SUBJECT: &str = "@srctr name@";

pub const SLOT_KEYS: [&str; 6] = ["Q", "W", "E", "R", "D", "F"];
pub const SLOTS: [Slot; 6] = [Slot::Q, Slot::W, Slot::E, Slot::R, Slot::D, Slot::F];

#[derive(Debug, Clone, PartialEq)]
pub enum UiEvent {
    LogIn { name: String, password: String },
    SignUp { name: String, password: String },
    JoinTeam(Team),
    Start,
    Pick(u8),
    LockIn,
    Summoners([Summoner; 2]),
    Chat { text: String, all: bool },
    Buy(u32),
    Sell(u8),
    Undo,
    Sound(String),
    LevelUp(Slot),
    ToggleShop,
    ToggleCameraLock,
    ToggleStats,
    CreateLobby(String),
    JoinLobby(u64),
    LeaveLobby,
    SetAvatar(u8),
    LogOut,
    Ban(Option<u8>),
    StartVote(protocol::VoteKind),
    Vote(bool),
    OpenMenu,
    CloseMenu,
    ResetSettings,
    MenuTab(u8),
}

pub type Events = Rc<RefCell<Vec<UiEvent>>>;

pub struct Login {
    root: HtmlElement,
    error: HtmlElement,
}

impl Login {
    pub fn new(parent: &Element, events: &Events) -> Self {
        let root = element("form", "login");
        append(&root, &text("div", "logo", "OpenMOBA"));
        let name = input("text", "Name");
        let password = input("password", "Password");
        append(&root, &name);
        append(&root, &password);
        let error = text("div", "error", "");
        append(&root, &error);
        let buttons = element("div", "row");
        let log_in = text("button", "button primary", "Log in");
        let sign_up = text("button", "button", "Sign up");
        let _ = sign_up.set_attribute("type", "button");
        append(&buttons, &log_in);
        append(&buttons, &sign_up);
        append(&root, &buttons);
        append(parent, &root);

        let read = {
            let name = name.clone();
            let password = password.clone();
            move || (name.value().trim().to_owned(), password.value())
        };
        let submit_events = events.clone();
        let submit_read = read.clone();
        on(&root, "submit", move |event| {
            event.prevent_default();
            let (name, password) = submit_read();
            submit_events
                .borrow_mut()
                .push(UiEvent::LogIn { name, password });
        });
        let sign_events = events.clone();
        on(&sign_up, "click", move |_| {
            let (name, password) = read();
            sign_events
                .borrow_mut()
                .push(UiEvent::SignUp { name, password });
        });
        Self { root, error }
    }

    pub fn show(&self, visible: bool) {
        show(&self.root, visible);
    }

    pub fn error(&self, error: Option<AuthError>) {
        set_text(
            &self.error,
            match error {
                Some(AuthError::WrongNameOrPassword) => "Wrong name or password",
                Some(AuthError::NameTaken) => "That name is taken",
                Some(AuthError::Unavailable) => "Could not save your account. Please try again.",
                Some(AuthError::InvalidName) => {
                    "Name must be 1 to 16 letters, digits, spaces or underscores"
                }
                None => "",
            },
        );
    }
}

struct FloatingText {
    element: HtmlElement,
    position: [f32; 2],
    height: f32,
    age: f32,
}

const PLAYER: &str = "PlayerFrame/Player_Frame_Root";
const SPELLS: &str = "PlayerFrame/Player_Frame_Root/PlayerSpells";
const PORTRAIT: &str = "PlayerFrame/Player_Frame_Root/Player_Frame";
const BARS: &str = "PlayerFrame/Player_Frame_Root/PlayerResourceBars";
const INVENTORY: &str = "PlayerInventory/Inventory";
const STATS: &str = "PlayerStats/Player_Stats";
const ADVANCED_STATS: &str = "PlayerStats/Player_AdvancedStats";
const HEADER: &str = "LoLGameHeader/GameHeader";
const MINIMAP: &str = "LoLMinimap/Minimap";
const CAMERA_LOCK: &str = "LoLMinimap/Minimap/CameraLockButton";
const STATS_BUTTON: &str = "PlayerFrame/Player_Frame_Root/Player_Frame/Stats_Button";
const SCENES: [&str; 6] = [PLAYER, INVENTORY, STATS, ADVANCED_STATS, HEADER, MINIMAP];
const HIDDEN: [&str; 11] = [
    "PlayerFrame/Player_Frame_Root/PlayerSpells/Ability0/Ability0_CostBg",
    "PlayerFrame/Player_Frame_Root/PlayerSpells/Ability1/Ability1_CostBg",
    "PlayerFrame/Player_Frame_Root/PlayerSpells/Ability2/Ability2_CostBg",
    "PlayerFrame/Player_Frame_Root/PlayerSpells/Ability3/Ability3_CostBg",
    "PlayerStats/Player_Stats/Stats_CDR_Icon",
    "LoLGameHeader/GameHeader/FPS",
    "LoLGameHeader/GameHeader/LatencyText",
    "LoLMinimap/Minimap/JunglePathButton/JunglePathOn_ButtonDefault",
    "LoLMinimap/Minimap/JunglePathButton/JunglePathOff_ButtonDefault",
    "LoLMinimap/Minimap/JunglePathButton/JunglePath_HitRegion",
    "PlayerInventory/Inventory/AutobuildButton/Autobuild_Icon",
];
pub const STAT_NAMES: [&str; 8] = ["AD", "AP", "Armor", "MR", "AS", "AH", "Crit", "MS"];
const ADVANCED_STAT_NAMES: [&str; 8] = ["HPR", "APen", "LS", "AR", "HSP", "MPen", "SV", "Ten"];
pub const ITEM_KEYS: [&str; 6] = ["1", "2", "3", "5", "6", "7"];
const PIP_SIZE: f32 = 9.0;
const HP_BAR: &str =
    "PlayerFrame/Player_Frame_Root/PlayerResourceBars/PlayerHPMeter/PlayerHP_BarTextureGreen";
const RESOURCE_BARS: [(Resource, &str); 2] = [
    (
        Resource::Mana,
        "PlayerFrame/Player_Frame_Root/PlayerResourceBars/PlayerPARMeter/PlayerPar_BarTextureBlue",
    ),
    (
        Resource::Flow,
        "PlayerFrame/Player_Frame_Root/PlayerResourceBars/PlayerPARMeterSkins/PlayerPar_BarTextureWhite",
    ),
];
const HP_SHIELD: &str = "PlayerFrame/Player_Frame_Root/PlayerResourceBars/PlayerHPMeter/Shield";
const BUFFS: &str = "PlayerFrame/Player_Frame_Root/PlayerBuffs";
const BUFF_TEMPLATE: &str = "PlayerFrame/Player_Frame_Root/PlayerBuffs/PlayerBuffTemplate";
const BUFF_SWEEP: &str = "rgba(0, 0, 0, 0.39)";
const BUFF_ROWS: usize = 2;

pub const DISABLES: [&str; 5] = ["stun", "knockup", "suppression", "stasis", "airborne"];

pub fn free_to_cast(unit: &UnitState, summoner: Option<Summoner>, recast: bool) -> bool {
    let has = |name: &str| unit.buffs.contains(&clip_id(name));
    let disabled = DISABLES.iter().any(|name| has(name));
    !unit.dead
        && match summoner {
            Some(Summoner::Cleanse) => true,
            Some(Summoner::Flash) => !disabled && !has("root"),
            Some(Summoner::Mark) if recast => !disabled && !has("root"),
            Some(_) => !disabled,
            None => !disabled && !has("silence"),
        }
}

fn spell(index: usize, part: &str) -> String {
    match index {
        0..=3 => format!("{SPELLS}/Ability{index}/Ability{index}_{part}"),
        _ => {
            let summoner = index - 4;
            format!("{SPELLS}/Summoner{summoner}/Summoner{summoner}_{part}")
        }
    }
}

fn pip(index: usize, full: bool, pip: usize) -> String {
    let kind = if full { "FullPip" } else { "EmptyPip" };
    format!("{SPELLS}/Ability{index}/Pips/Ability{index}_{kind}{pip}")
}

fn level_up(index: usize) -> String {
    format!("{PLAYER}/LevelUp/LevelUp{index}_Button")
}

fn level_up_in(index: usize) -> String {
    format!("{PLAYER}/LevelUpFxIn/LevelUp{index}_ButtonIn")
}

fn item(slot: usize, part: &str) -> String {
    match part {
        "" | "HitArea" => format!("{INVENTORY}/InventorySlot{slot}/InventoryItem{slot}_{part}"),
        "Backdrop" => format!("{INVENTORY}/InventoryItem{slot}_Backdrop"),
        "HotKey" => format!("{INVENTORY}/InventoryHotkey/InventoryItem{slot}_HotKey"),
        _ => format!("{INVENTORY}/InventorySlotOverlays/InventoryItem{slot}_{part}"),
    }
}

pub fn stat_texts(stats: &Stats) -> [String; 8] {
    [
        format!("{:.0}", stats.attack_damage),
        format!("{:.0}", stats.ability_power),
        format!("{:.0}", stats.armor),
        format!("{:.0}", stats.magic_resist),
        format!("{:.2}", stats.attack_speed),
        format!("{:.0}", stats.ability_haste),
        format!("{:.0}%", stats.crit_chance * 100.0),
        format!("{:.0}", stats.move_speed),
    ]
}

fn advanced_stat_texts(stats: &Stats) -> [String; 8] {
    [
        format!("{:.0}", stats.health_regen * 5.0),
        format!(
            "{:.0} | {:.0}%",
            stats.lethality,
            stats.armor_penetration * 100.0
        ),
        format!("{:.0}%", stats.life_steal * 100.0),
        format!("{:.0}", stats.attack_range),
        "0%".to_owned(),
        format!(
            "{:.0} | {:.0}%",
            stats.magic_penetration,
            stats.magic_penetration_percent * 100.0
        ),
        "0%".to_owned(),
        "0%".to_owned(),
    ]
}

fn ranks(index: usize) -> u8 {
    if index == 3 { 3 } else { 5 }
}

pub fn max_rank(slot: Slot) -> usize {
    match slot {
        Slot::R => 3,
        Slot::D | Slot::F => 0,
        _ => 5,
    }
}

fn cooldown_text(seconds: f32) -> String {
    if seconds < 1.0 {
        format!("{seconds:.1}")
    } else {
        format!("{:.0}", seconds.ceil())
    }
}

fn place_pips(frame: &Frame, index: usize, count: usize) {
    let Some(region) = frame.rect(&format!(
        "{SPELLS}/Ability{index}/Pips/Ability{index}_PipRect"
    )) else {
        return;
    };
    let step = region.size[0] / count.max(1) as f32;
    for slot in 0..6 {
        for full in [false, true] {
            let name = pip(index, full, slot);
            if slot >= count {
                frame.show(&name, false);
                continue;
            }
            let x = region.position[0] + step * slot as f32 + (step - PIP_SIZE) / 2.0;
            frame.move_to(&name, [x, region.position[1]]);
        }
    }
}

fn level_up_button(frame: &Frame, events: &Events, index: usize, slot: Slot) {
    let events = events.clone();
    frame.show_button(&level_up(index), false);
    frame.on_click(&level_up(index), move |_| {
        events.borrow_mut().push(UiEvent::LevelUp(slot))
    });
}

fn buff_slots(frame: &mut Frame) -> [Vec<String>; 2] {
    let (Some(cell), Some(positive), Some(negative)) = (
        frame.rect(&format!("{BUFF_TEMPLATE}/PlayerBuffHitRegion")),
        frame.rect(&format!(
            "{BUFFS}/PlayerPositiveBuffLayout/PlayerPositiveBuffRegion"
        )),
        frame.rect(&format!(
            "{BUFFS}/PlayerNegativeBuffLayout/PlayerNegativeBuffRegion"
        )),
    ) else {
        return Default::default();
    };
    let [width, height] = cell.size;
    let mut layout = |region: Rect, from_right: bool, kind: &str| {
        let per_row = (region.size[0] / width).floor().max(1.0) as usize;
        (0..per_row * BUFF_ROWS)
            .map(|index| {
                let column = (index % per_row) as f32;
                let row = (index / per_row) as f32;
                let x = if from_right {
                    region.position[0] + region.size[0] - width * (column + 1.0)
                } else {
                    region.position[0] + width * column
                };
                let y = region.position[1] + region.size[1] - height * (row + 1.0);
                let name = format!("{BUFFS}/{kind}{index}");
                frame.instance(
                    BUFF_TEMPLATE,
                    &name,
                    [x - cell.position[0], y - cell.position[1]],
                );
                name
            })
            .collect()
    };
    [
        layout(positive, false, "Positive"),
        layout(negative, true, "Negative"),
    ]
}

pub struct Hud {
    root: HtmlElement,
    frame: Frame,
    size: [f32; 3],
    previewing: Rc<Cell<Option<Slot>>>,
    level_ups: [Option<f32>; 4],
    level_ups_shown: [bool; 4],
    item_totals: [f32; 6],
    fade: Fade,
    ranges: [f32; 4],
    buff_slots: [Vec<String>; 2],
    shop: Shop,
    scoreboard: Scoreboard,
    tooltip: HtmlElement,
    banner: HtmlElement,
    banners: Banners,
    callouts: Callouts,
    team_frames: TeamFrames,
    relic_timers: RelicTimers,
    strings: BTreeMap<String, String>,
    announcing: bool,
    announcements: VecDeque<(Banner, Option<&'static str>)>,
    announcement_age: f32,
    floating: Vec<FloatingText>,
    layer: HtmlElement,
    data: Rc<GameData>,
    champion: usize,
    summoners: [Summoner; 2],
    picks: Vec<(UnitId, usize)>,
    item_slots: [Option<u32>; 6],
    stats_pinned: bool,
}

impl Hud {
    pub fn new(
        parent: &Element,
        events: &Events,
        data: Rc<GameData>,
        layout: &Layout,
        champion: usize,
        summoners: [Summoner; 2],
        players: &[Player],
    ) -> Self {
        let root = element("div", "hud");
        let layer = element("div", "floating-layer");
        append(&root, &layer);
        let mut frame = Frame::new(&root, layout, &SCENES);
        frame.add(HP_SHIELD, HP_BAR, "frame-shield");
        let buff_slots = buff_slots(&mut frame);
        frame.show_scene(ADVANCED_STATS, false);
        let camera_events = events.clone();
        let camera_sound = frame.sound(CAMERA_LOCK);
        frame.on_click(CAMERA_LOCK, move |_| {
            let mut events = camera_events.borrow_mut();
            events.extend(camera_sound.clone().map(UiEvent::Sound));
            events.push(UiEvent::ToggleCameraLock);
        });
        frame.show_button(STATS_BUTTON, true);
        let stats_events = events.clone();
        let stats_sound = frame.sound(STATS_BUTTON);
        frame.on_click(STATS_BUTTON, move |_| {
            let mut events = stats_events.borrow_mut();
            events.extend(stats_sound.clone().map(UiEvent::Sound));
            events.push(UiEvent::ToggleStats);
        });
        for name in HIDDEN {
            frame.show(name, false);
        }

        let tooltip = element("div", "tooltip");
        show(&tooltip, false);

        let champion_data = data.champions.get(champion).cloned().unwrap_or_default();
        frame.image(
            &format!("{PORTRAIT}/PlayerIcon_Base"),
            &format!("assets/icons/{}", champion_data.icon),
        );
        let passive = format!("{SPELLS}/Passive/Passive_");
        frame.image(
            &passive,
            &format!("assets/icons/{}", champion_data.passive.icon),
        );
        if let Some(view) = frame.interactive(&passive) {
            attach_tooltip(
                &view,
                &tooltip,
                &champion_data.passive.display_name,
                &champion_data.passive.description,
            );
        }
        let previewing = Rc::new(Cell::new(None));
        for (index, slot) in SLOTS.iter().copied().enumerate() {
            frame.text(&spell(index, "HotKey"), SLOT_KEYS[index]);
            let (icon, title, details) = match index {
                0..=3 => {
                    let ability = champion_data
                        .spell(&champion_data.slots.get(index).cloned().unwrap_or_default())
                        .cloned()
                        .unwrap_or_default();
                    let details = spell_details(&ability, ranks(index), None);
                    (ability.icon, ability.display_name, details)
                }
                _ => summoners
                    .get(index - 4)
                    .and_then(|summoner| data.summoner(summoner.id()))
                    .map(|summoner| {
                        (
                            summoner.icon.clone(),
                            summoner.display_name.clone(),
                            summoner_details(summoner),
                        )
                    })
                    .unwrap_or_default(),
            };
            frame.image(&spell(index, ""), &format!("assets/icons/{icon}"));
            let Some(view) = frame.interactive(&spell(index, "")) else {
                continue;
            };
            attach_tooltip(&view, &tooltip, &title, &details);
            let hover = frame.get(&spell(index, "Mouseover")).cloned();
            let entered = previewing.clone();
            let lit = hover.clone();
            on(&view, "mouseenter", move |_| {
                entered.set(Some(slot));
                if let Some(hover) = &lit {
                    show(hover, true);
                }
            });
            let left = previewing.clone();
            on(&view, "mouseleave", move |_| {
                left.set(None);
                if let Some(hover) = &hover {
                    show(hover, false);
                }
            });
            if index < 4 {
                place_pips(&frame, index, max_rank(slot));
                level_up_button(&frame, events, index, slot);
            }
        }

        for slot in 0..=ITEM_KEYS.len() {
            frame.show(&item(slot, "Backdrop"), true);
        }
        for (slot, key) in ITEM_KEYS.iter().enumerate() {
            frame.text(&item(slot, "HotKey"), key);
            frame.show(&item(slot, "HotKey"), true);
            let Some(area) = frame.interactive(&item(slot, "HitArea")) else {
                continue;
            };
            listen_tooltip(&area, &tooltip);
            let events = events.clone();
            on(&area, "contextmenu", move |event| {
                event.prevent_default();
                events.borrow_mut().push(UiEvent::Sell(slot as u8));
            });
            let hover = frame.get(&item(slot, "Mouseover")).cloned();
            let lit = hover.clone();
            on(&area, "mouseenter", move |_| {
                if let Some(hover) = &lit {
                    show(hover, true);
                }
            });
            on(&area, "mouseleave", move |_| {
                if let Some(hover) = &hover {
                    show(hover, false);
                }
            });
        }
        frame.text(
            &format!("{INVENTORY}/InventoryHotkey/InventoryItem6_HotKey"),
            "4",
        );
        frame.show(
            &format!("{INVENTORY}/InventoryHotkey/InventoryItem6_HotKey"),
            true,
        );
        for backdrop in [
            format!("{PLAYER}/HUDCenterFrame"),
            format!("{PORTRAIT}/PortraitFrame_Default"),
            format!("{INVENTORY}/InventoryFrame"),
            format!("{STATS}/StatsBg"),
        ] {
            frame.interactive(&backdrop);
        }
        if let Some(button) = frame.interactive(&format!("{INVENTORY}/ShopButton/Shop_HitArea")) {
            let events = events.clone();
            on(&button, "click", move |_| {
                events.borrow_mut().push(UiEvent::ToggleShop)
            });
        }

        let shop = Shop::new(
            &root,
            layout,
            data.clone(),
            events,
            &tooltip,
            &champion_data.role,
        );

        let scoreboard = Scoreboard::new(&root, layout);

        let banners = Banners::new(&root, layout);
        let callouts = Callouts::new(&root, layout);
        let team_frames = TeamFrames::new(&root, layout);
        let relic_timers = RelicTimers::new(&root, layout);
        let banner = text("div", "banner", "");
        show(&banner, false);
        append(&root, &banner);
        append(&root, &tooltip);
        append(parent, &root);

        Self {
            root,
            frame,
            size: [0.0, 0.0, 0.0],
            previewing,
            level_ups: [None; 4],
            level_ups_shown: [false; 4],
            item_totals: [0.0; 6],
            fade: Fade::default(),
            ranges: [0.0; 4],
            buff_slots,
            shop,
            scoreboard,
            tooltip,
            banner,
            banners,
            callouts,
            team_frames,
            relic_timers,
            strings: layout.strings.clone(),
            announcing: false,
            announcements: VecDeque::new(),
            announcement_age: 0.0,
            floating: Vec::new(),
            layer,
            data,
            champion,
            summoners,
            picks: players
                .iter()
                .map(|player| (player.unit, usize::from(player.champion)))
                .collect(),
            item_slots: [None; 6],
            stats_pinned: false,
        }
    }

    pub fn resize(&mut self, width: f32, height: f32, scale: f32) -> Option<[f32; 4]> {
        if self.size == [width, height, scale] {
            return None;
        }
        self.size = [width, height, scale];
        self.frame.resize(height * scale);
        self.shop.resize(height * scale);
        self.scoreboard.resize(height * scale);
        self.banners.resize(height * scale);
        self.callouts.resize(height * scale);
        self.team_frames.resize(height * scale);
        self.relic_timers.resize(height * scale);
        self.frame
            .place_scaled(&format!("{MINIMAP}/MinimapContent"), width, height, scale)
    }

    pub fn preferences(&self, settings: &crate::settings::Settings) {
        show(&self.layer, settings.damage);
        for (index, key) in settings.keys.iter().take(6).enumerate() {
            self.frame
                .text(&spell(index, "HotKey"), crate::settings::label(key));
        }
    }

    pub fn camera_locked(&self, locked: bool) {
        self.frame.select(CAMERA_LOCK, locked);
    }

    pub fn advanced_stats(&self, held: bool) {
        self.frame
            .show_scene(ADVANCED_STATS, held || self.stats_pinned);
    }

    pub fn toggle_stats(&mut self) {
        self.stats_pinned = !self.stats_pinned;
        self.frame.select(STATS_BUTTON, self.stats_pinned);
        self.advanced_stats(false);
    }

    pub fn remove(&self) {
        self.root.remove();
    }

    pub fn toggle_shop(&mut self) {
        self.shop.toggle();
    }

    pub fn shop_open(&self) -> bool {
        self.shop.open()
    }

    pub fn show_scoreboard(&self, visible: bool) {
        self.scoreboard.show(visible);
    }

    pub fn announce(
        &mut self,
        mut banner: Banner,
        subject: Option<&str>,
        audio: Option<&'static str>,
    ) {
        let text = self
            .strings
            .get(&banner.text)
            .map_or("", |text| text.trim());
        banner.text = text.replace(SUBJECT, subject.unwrap_or_default());
        self.announcements.push_back((banner, audio));
    }

    pub fn update_announcements(&mut self, dt: f32) -> Option<&'static str> {
        self.announcement_age += dt;
        if self.announcing && self.announcement_age < ANNOUNCEMENT_TIME {
            return None;
        }
        match self.announcements.pop_front() {
            Some((next, audio)) => {
                self.banners.show(&next);
                self.announcing = true;
                self.announcement_age = 0.0;
                audio
            }
            None => {
                self.banners.hide();
                self.announcing = false;
                None
            }
        }
    }

    pub fn callout(&mut self, callout: Callout) {
        self.callouts.push(callout);
    }

    pub fn update_callouts(&mut self, dt: f32) {
        self.callouts.update(dt);
    }

    pub fn banner(&self, message: Option<&str>) {
        show(&self.banner, message.is_some());
        set_text(&self.banner, message.unwrap_or_default());
    }

    pub fn floating(&mut self, value: &str, class: &str, position: [f32; 2], height: f32) {
        let element = text("div", &format!("floating {class}"), value);
        append(&self.layer, &element);
        self.floating.push(FloatingText {
            element,
            position,
            height,
            age: 0.0,
        });
    }

    pub fn forget_backlog(&mut self) {
        self.announcements.clear();
        for text in self.floating.drain(..) {
            text.element.remove();
        }
    }

    pub fn update_floating(
        &mut self,
        dt: f32,
        project: impl Fn([f32; 2], f32) -> Option<(f32, f32)>,
    ) {
        for text in &mut self.floating {
            text.age += dt;
            match project(text.position, text.height + text.age * 120.0) {
                Some((x, y)) => {
                    set_style(
                        &text.element,
                        "transform",
                        &format!("translate({x:.0}px, {y:.0}px)"),
                    );
                    set_style(
                        &text.element,
                        "opacity",
                        &format!("{:.2}", (1.2 - text.age).clamp(0.0, 1.0)),
                    );
                }
                None => set_style(&text.element, "opacity", "0"),
            }
        }
        self.floating.retain(|text| {
            let keep = text.age < 1.2;
            if !keep {
                text.element.remove();
            }
            keep
        });
    }

    fn update_buffs(&self, you: &You) {
        let frame = &self.frame;
        let champion_of = |source: Option<UnitId>| {
            source
                .and_then(|id| self.picks.iter().find(|(unit, _)| *unit == id))
                .map_or(self.champion, |(_, champion)| *champion)
        };
        let shown: Vec<(&BuffState, buffs::Shown, String)> = you
            .buffs
            .iter()
            .filter_map(|buff| {
                let shown = buffs::shown(buff.name)?;
                let icon = match buffs::icon(buff, shown)? {
                    buffs::Icon::Passive => self
                        .data
                        .champions
                        .get(champion_of(buff.source))?
                        .passive
                        .icon
                        .clone(),
                    buffs::Icon::Ability(slot) => {
                        let champion = self.data.champions.get(champion_of(buff.source))?;
                        let index = SLOTS.iter().position(|candidate| *candidate == slot)?;
                        champion.spell(champion.slots.get(index)?)?.icon.clone()
                    }
                    buffs::Icon::Summoner(summoner) => {
                        self.data.summoner(summoner.id())?.icon.clone()
                    }
                    buffs::Icon::Item(id) => self.data.item(id)?.icon.clone(),
                    buffs::Icon::Origin => return None,
                };
                Some((buff, shown, icon))
            })
            .collect();
        for (positive, slots) in [true, false].into_iter().zip(&self.buff_slots) {
            let mut listed = shown
                .iter()
                .filter(|(_, shown, _)| shown.positive == positive);
            for slot in slots {
                let part = |name: &str| format!("{slot}/PlayerBuff{name}");
                let current = listed.next();
                frame.show(&part("Icon"), current.is_some());
                frame.show(&part("CooldownFX"), current.is_some());
                frame.show(&part("BorderPositive"), current.is_some() && positive);
                frame.show(&part("BorderNegative"), current.is_some() && !positive);
                let stacks = current.filter(|(buff, shown, _)| shown.stacks && buff.stacks > 1);
                frame.show(&part("Stacks"), stacks.is_some());
                let Some((buff, _, icon)) = current else {
                    continue;
                };
                frame.image(&part("Icon"), &format!("assets/icons/{icon}"));
                frame.sweep(&part("CooldownFX"), buffs::elapsed(buff), BUFF_SWEEP);
                if let Some((buff, _, _)) = stacks {
                    frame.text(&part("Stacks"), &buff.stacks.to_string());
                }
            }
        }
    }

    pub fn previewing(&self) -> Option<Slot> {
        self.previewing.get()
    }

    pub fn update(&mut self, snapshot: &Snapshot, setup: &Setup, time: f32) {
        self.update_team(snapshot, setup);
        self.relic_timers.update(&snapshot.relics);
        let Some(you) = &snapshot.you else {
            return;
        };
        let frame = &self.frame;
        let unit = snapshot.units.iter().find(|unit| unit.id == setup.you);
        if let Some(unit) = unit {
            frame.text(&format!("{PORTRAIT}/PlayerLevel"), &unit.level.to_string());
            let total = (unit.health + unit.shield).max(unit.max_health).max(1.0);
            let health = unit.health / total;
            let shielded = health + unit.shield / total;
            let fade = self.fade.update(health, time);
            frame.fill(HP_BAR, health);
            frame.span(HP_SHIELD, health, shielded);
            frame.span(
                &format!("{BARS}/PlayerHPMeter/PlayerHP_BarFade"),
                shielded,
                fade,
            );
            frame.follow(
                &format!("{BARS}/PlayerHPMeter/PlayerHP_BarTip"),
                HP_BAR,
                shielded.max(fade),
            );
            frame.text(
                &format!("{BARS}/PlayerHPMeter/PlayerHP_ValueText"),
                &format!("{:.0} / {:.0}", unit.health, unit.max_health),
            );
            let resource = self
                .data
                .champions
                .get(self.champion)
                .map_or(Resource::None, |champion| champion.resource);
            let mana = unit.max_mana > 0.0;
            let mana_fraction = unit.mana / unit.max_mana.max(1.0);
            for (kind, bar) in RESOURCE_BARS {
                frame.show(bar, mana && kind == resource);
                frame.fill(bar, mana_fraction);
            }
            frame.follow(
                &format!("{BARS}/PlayerPARMeter/PlayerPar_BarTip"),
                RESOURCE_BARS[0].1,
                if mana { mana_fraction } else { 0.0 },
            );
            frame.text(
                &format!("{BARS}/PlayerPARMeter/PlayerPar_ValueText"),
                &if mana {
                    format!("{:.0} / {:.0}", unit.mana, unit.max_mana)
                } else {
                    String::new()
                },
            );
            frame.desaturate(&format!("{PORTRAIT}/PlayerIcon_Base"), unit.dead);
        }
        frame.fill_up(
            &format!("{PORTRAIT}/PlayerExp_BarTexture"),
            you.xp / you.xp_needed.max(1.0),
        );
        let respawn = format!("{PORTRAIT}/PlayerRespawnTimer");
        frame.show(&respawn, you.respawn.is_some());
        frame.text(
            &respawn,
            &you.respawn
                .map(|seconds| format!("{:.0}", seconds.ceil()))
                .unwrap_or_default(),
        );
        let level = unit.map_or(1, |unit| unit.level);
        let mana = unit.map_or(0.0, |unit| unit.mana);
        let champion = self.data.champions.get(self.champion);
        let passive = |name: &str| format!("{SPELLS}/Passive/Passive_{name}");
        frame.desaturate(&passive(""), you.passive_cooldown.is_some());
        frame.show(&passive("CooldownFX"), you.passive_cooldown.is_some());
        frame.show(&passive("Cooldown"), you.passive_cooldown.is_some());
        if let Some((remaining, total)) = you.passive_cooldown {
            frame.radial(&passive("CooldownFX"), remaining / total.max(0.01));
            frame.text(&passive("Cooldown"), &cooldown_text(remaining));
        }
        for (index, slot) in SLOTS.iter().copied().enumerate() {
            let ability = you.abilities[index];
            let learned = ability.rank > 0 || index >= 4;
            let cooling = learned && ability.cooldown > 0.0 && !ability.recast;
            let cost = champion
                .filter(|_| index < 4)
                .and_then(|champion| champion.spell(champion.slots.get(index)?))
                .map_or(0.0, |spell| spell.mana(ability.rank.max(1)));
            let summoner = index.checked_sub(4).map(|index| self.summoners[index]);
            let locked = !unit.is_some_and(|unit| free_to_cast(unit, summoner, ability.recast));
            let available = learned && !cooling && !locked;
            frame.desaturate(&spell(index, ""), !available);
            frame.show(&spell(index, "OverlayUnavailable"), !learned);
            frame.show(&spell(index, "BorderAvailable"), available);
            frame.show(&spell(index, "BorderUnavailable"), !available);
            frame.show(
                &spell(index, "IconOom"),
                learned && !cooling && !ability.recast && cost > mana,
            );
            frame.show(&spell(index, "IconLoc"), learned && locked);
            frame.show(&spell(index, "CooldownFX"), cooling);
            frame.show(&spell(index, "Cooldown"), cooling);
            if cooling {
                frame.radial(
                    &spell(index, "CooldownFX"),
                    ability.cooldown / ability.cooldown_total.max(0.01),
                );
                frame.text(&spell(index, "Cooldown"), &cooldown_text(ability.cooldown));
            }
            let charges = ability.ammo && learned;
            frame.show(&spell(index, "Ammo"), charges);
            if charges {
                frame.text(&spell(index, "Ammo"), &ability.charges.to_string());
            }
            if index >= 4 {
                continue;
            }
            if self.ranges[index] != ability.range {
                self.ranges[index] = ability.range;
                if let (Some(view), Some(data)) = (
                    frame.get(&spell(index, "")),
                    champion.and_then(|champion| champion.spell(champion.slots.get(index)?)),
                ) {
                    let range = Some(ability.range).filter(|range| *range > 0.0);
                    set_tooltip(
                        view,
                        Some((
                            &data.display_name,
                            &spell_details(data, ranks(index), range),
                        )),
                    );
                }
            }
            for pip_index in 0..max_rank(slot) {
                let full = pip_index < usize::from(ability.rank);
                frame.show(&pip(index, true, pip_index), full);
                frame.show(&pip(index, false, pip_index), !full);
            }
            let can_level = you.skill_points > 0 && can_level(slot, ability.rank, level);
            let started = &mut self.level_ups[index];
            match (can_level, *started) {
                (true, None) => *started = Some(time),
                (false, Some(_)) => *started = None,
                _ => {}
            }
            if self.level_ups_shown[index] != can_level {
                self.level_ups_shown[index] = can_level;
                frame.show_button(&level_up(index), can_level);
            }
            let growing =
                started.is_some_and(|start| time - start < frame.duration(&level_up_in(index)));
            frame.show(&level_up_in(index), growing);
            if let (true, Some(start)) = (growing, *started) {
                frame.animate(&level_up_in(index), time - start);
            }
        }
        if self.item_slots != you.items {
            self.item_slots = you.items;
            for (slot, owned) in you.items.iter().enumerate() {
                let icon = item(slot, "");
                match owned.and_then(|id| self.data.item(id)) {
                    Some(owned) => {
                        frame.image(&icon, &format!("assets/icons/{}", owned.icon));
                        frame.show(&icon, true);
                        if let Some(area) = frame.get(&item(slot, "HitArea")) {
                            set_tooltip(area, Some((&owned.name, &owned.description)));
                            refresh_hovered(area, &self.tooltip);
                        }
                    }
                    None => {
                        frame.show(&icon, false);
                        if let Some(area) = frame.get(&item(slot, "HitArea")) {
                            set_tooltip(area, None);
                            refresh_hovered(area, &self.tooltip);
                        }
                    }
                }
            }
        }
        for (slot, count) in you.stacks.iter().enumerate() {
            frame.show(&item(slot, "Count"), *count > 1);
            frame.text(&item(slot, "Count"), &count.to_string());
        }
        for (slot, cooldown) in you.item_cooldowns.iter().copied().enumerate() {
            let total = &mut self.item_totals[slot];
            *total = if cooldown > 0.0 {
                total.max(cooldown)
            } else {
                0.0
            };
            let cooling = cooldown > 0.0 && you.items[slot].is_some();
            frame.show(&item(slot, "CooldownFX"), cooling);
            frame.show(&item(slot, "Cooldown"), cooling);
            if cooling {
                frame.radial(&item(slot, "CooldownFX"), cooldown / total.max(0.01));
                frame.text(&item(slot, "Cooldown"), &cooldown_text(cooldown));
            }
        }
        self.update_buffs(you);
        let gold = format!("{:.0}", you.gold.floor());
        frame.text(&format!("{INVENTORY}/ShopButton/Shop_GoldText"), &gold);
        self.shop.update(you);
        if let Some(stats) = unit.and_then(|unit| unit.stats.as_ref()) {
            for (name, value) in STAT_NAMES.iter().zip(stat_texts(stats)) {
                frame.text(&format!("{STATS}/Stats_{name}_Text"), &value);
            }
            for (name, value) in ADVANCED_STAT_NAMES.iter().zip(advanced_stat_texts(stats)) {
                frame.text(&format!("{ADVANCED_STATS}/Stats_{name}_Text"), &value);
            }
        }
        let team_of = |unit: UnitId| {
            setup
                .players
                .iter()
                .find(|player| player.unit == unit)
                .map(|player| player.team)
        };
        let kills = |team: Team| {
            snapshot
                .scores
                .iter()
                .filter(|(unit, _)| team_of(*unit) == Some(team))
                .map(|(_, score)| u32::from(score.kills))
                .sum::<u32>()
        };
        frame.text(
            &format!("{HEADER}/TB_TeamKills_T1Score"),
            &kills(Team::Blue).to_string(),
        );
        frame.text(
            &format!("{HEADER}/TB_TeamKills_T2Score"),
            &kills(Team::Red).to_string(),
        );
        let seconds = time.max(0.0) as u32;
        frame.text(
            &format!("{HEADER}/GameTime"),
            &format!("{:02}:{:02}", seconds / 60, seconds % 60),
        );
        if let Some((_, score)) = snapshot.scores.iter().find(|(unit, _)| *unit == setup.you) {
            frame.text(
                &format!("{HEADER}/TB_Player_KDA_Text"),
                &format!("{}/{}/{}", score.kills, score.deaths, score.assists),
            );
            frame.text(
                &format!("{HEADER}/TB_Player_CS_Text"),
                &score.minions.to_string(),
            );
        }
    }

    fn update_team(&self, snapshot: &Snapshot, setup: &Setup) {
        let team = setup
            .players
            .iter()
            .find(|player| player.unit == setup.you)
            .map(|player| player.team);
        let allies: Vec<Ally> = setup
            .players
            .iter()
            .filter(|player| Some(player.team) == team && player.unit != setup.you)
            .filter_map(|player| {
                let champion = self.data.champions.get(usize::from(player.champion))?;
                let unit = snapshot.units.iter().find(|unit| unit.id == player.unit)?;
                Some(Ally {
                    icon: &champion.icon,
                    resource: champion.resource,
                    unit,
                })
            })
            .collect();
        self.team_frames.update(&allies);
    }

    pub fn update_scoreboard(&self, snapshot: &Snapshot, setup: &Setup) {
        self.scoreboard.update(snapshot, setup, &self.data);
    }

    pub fn hide_tooltip(&self) {
        show(&self.tooltip, false);
    }
}

pub fn can_level(slot: Slot, rank: u8, level: u8) -> bool {
    match slot {
        Slot::R => [6, 11, 16]
            .get(usize::from(rank))
            .is_some_and(|required| level >= *required),
        Slot::D | Slot::F => false,
        _ => rank < 5 && level > rank * 2,
    }
}

fn per_rank(values: impl Iterator<Item = f32>) -> Option<String> {
    let values: Vec<f32> = values.collect();
    let first = *values.first()?;
    if values.iter().all(|value| *value <= 0.0) {
        return None;
    }
    let format = |value: &f32| {
        let rounded = (value * 100.0).round() / 100.0;
        format!("{rounded}")
    };
    Some(if values.iter().all(|value| *value == first) {
        format(&first)
    } else {
        values.iter().map(format).collect::<Vec<_>>().join(" / ")
    })
}

pub fn summoner_details(summoner: &SummonerData) -> String {
    format!(
        "{}<br><br><attention>Cooldown:</attention> {}",
        summoner.description, summoner.cooldown
    )
}

pub fn spell_details(spell: &Spell, ranks: u8, range: Option<f32>) -> String {
    let mut text = spell.description.clone();
    let lines = [
        (
            "Cooldown",
            per_rank((1..=ranks).map(|rank| spell.cooldown(rank))),
            "",
        ),
        (
            "Cost",
            per_rank((1..=ranks).map(|rank| spell.mana(rank))),
            " Mana",
        ),
        (
            "Range",
            match range {
                Some(range) => per_rank([range].into_iter()),
                None => per_rank((1..=ranks).map(|rank| spell.range(rank))),
            },
            "",
        ),
    ];
    let mut first = true;
    for (label, value, suffix) in lines {
        if let Some(value) = value {
            text.push_str(if first { "<br><br>" } else { "<br>" });
            first = false;
            text.push_str(&format!("<attention>{label}:</attention> {value}{suffix}"));
        }
    }
    text
}

pub fn attach_tooltip(target: &HtmlElement, tooltip: &HtmlElement, title: &str, body: &str) {
    set_tooltip(target, Some((title, body)));
    listen_tooltip(target, tooltip);
}

pub fn owned_tooltip(
    target: &HtmlElement,
    tooltip: &HtmlElement,
    title: &str,
    body: &str,
) -> [Listener; 2] {
    set_tooltip(target, Some((title, body)));
    tooltip_listeners(target, tooltip)
}

fn set_tooltip(target: &HtmlElement, content: Option<(&str, &str)>) {
    match content {
        Some((title, body)) => {
            let _ = target.set_attribute(
                "data-tooltip",
                &format!(
                    "<div class=\"tooltip-title\">{}</div><div class=\"tooltip-body\">{}</div>",
                    rich_markup(title),
                    rich_markup(body)
                ),
            );
        }
        None => {
            let _ = target.remove_attribute("data-tooltip");
        }
    }
}

fn listen_tooltip(target: &HtmlElement, tooltip: &HtmlElement) {
    std::mem::forget(tooltip_listeners(target, tooltip));
}

fn tooltip_listeners(target: &HtmlElement, tooltip: &HtmlElement) -> [Listener; 2] {
    let shown = tooltip.clone();
    let entered = listen(target, "mouseenter", move |event| {
        let Some(target) = event
            .current_target()
            .and_then(|target| target.dyn_into::<HtmlElement>().ok())
        else {
            return;
        };
        let Some(html) = target.get_attribute("data-tooltip") else {
            return;
        };
        shown.set_inner_html(&html);
        show(&shown, true);
        let rect = target.get_bounding_client_rect();
        set_style(&shown, "left", &format!("{:.0}px", rect.left()));
        set_style(
            &shown,
            "bottom",
            &format!(
                "{:.0}px",
                web_sys::window()
                    .and_then(|window| window.inner_height().ok())
                    .and_then(|height| height.as_f64())
                    .unwrap_or(800.0)
                    - rect.top()
                    + 8.0
            ),
        );
    });
    let hidden = tooltip.clone();
    let left = listen(target, "mouseleave", move |_| show(&hidden, false));
    [entered, left]
}

fn refresh_hovered(target: &HtmlElement, tooltip: &HtmlElement) {
    if !target.matches(":hover").unwrap_or(false) {
        return;
    }
    match target.get_attribute("data-tooltip") {
        Some(html) => tooltip.set_inner_html(&html),
        None => show(tooltip, false),
    }
}

pub fn is_typing(event: &KeyboardEvent) -> bool {
    event
        .target()
        .and_then(|target| target.dyn_into::<HtmlInputElement>().ok())
        .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spell_tooltips_list_values_per_rank() {
        let spell = Spell {
            description: "Throws cards.".into(),
            cooldown: vec![6.0, 6.0, 5.75, 5.5, 5.25, 5.0],
            mana: vec![60.0, 70.0, 80.0, 90.0, 100.0],
            cast_range: vec![1450.0; 6],
            ..Spell::default()
        };
        assert_eq!(
            spell_details(&spell, 5, None),
            "Throws cards.<br><br><attention>Cooldown:</attention> 6 / 5.75 / 5.5 / 5.25 / 5<br><attention>Cost:</attention> 60 / 70 / 80 / 90 / 100 Mana<br><attention>Range:</attention> 1450"
        );
        assert!(
            spell_details(&spell, 5, Some(825.0)).ends_with("<attention>Range:</attention> 825")
        );
    }
}
