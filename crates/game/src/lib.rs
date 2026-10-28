mod animator;
mod announcer;
mod assets;
mod audio;
mod banner;
mod buffs;
mod callouts;
mod camera;
mod chat;
mod dom;
mod download;
mod fade;
mod fog;
mod frame;
mod lounge;
mod map;
mod menu;
mod net;
mod overlay;
mod particles;
mod puppet;
mod relics;
mod render;
mod scoreboard;
mod select;
mod settings;
mod shop;
mod status;
mod target;
mod teamframes;
mod ui;
mod world;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use asset::clip_id;
use asset::data::{ChampionData, GameData, Resource};
use asset::data::{Spell, Targeting};
use asset::map::{HeightField, MapData};
use asset::ui::Layout;
use assets::Library;
use camera::Camera;
use glam::{Mat4, Quat, Vec2, Vec3, Vec4};
use lounge::Lounge;
use net::{Connection, Status};
use overlay::{Bounds, Overlay, Viewer, bar_height};
use protocol::{
    AbilityState, ClientMessage, Command, DamageKind, Emote, Event, Lobby, MinionKind,
    MissileState, Phase, PingKind, ServerMessage, Setup, Slot, Snapshot, Summoner, TICK_RATE,
    Target, Team, UnitId, UnitKind, UnitState, You, sight_range,
};
use puppet::Puppet;
use render::{Decal, Renderer};
use ui::{Events, Hud, Login, SLOTS, UiEvent};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::{
    Element, HtmlCanvasElement, HtmlElement, KeyboardEvent, MouseEvent, WebGl2RenderingContext,
    WheelEvent, Window,
};

const TOKEN_KEY: &str = "lll-token";
const LAUNCH_SETTLE: f32 = 300.0;
const SHADOW_RADIUS: f32 = 80.0;
const GROUND_RADIUS: f32 = 9000.0;
const EDGE_MARGIN: f32 = 12.0;
const PAN_SPEED: f32 = 2400.0;
const LANE_VIEW: f32 = 1200.0;
const MOVE_REPEAT: f32 = 0.25;
const RECONNECT_DELAY: f32 = 2.0;
const CORPSE_TIME: f32 = 1.6;
const SCOREBOARD_REFRESH: f32 = 0.5;
const GROUND_REFINEMENT: usize = 3;
const CAMERA_HEIGHT_SMOOTHING: f32 = 6.0;

const LEFT_BUTTON: i16 = 0;
const MIDDLE_BUTTON: i16 = 1;
const RIGHT_BUTTON: i16 = 2;

const WIND: Vec4 = Vec4::new(0.75, 0.92, 1.0, 0.6);
const PING_TIME: f32 = 3.0;
const INDICATOR: Vec4 = Vec4::new(0.55, 0.85, 1.0, 0.7);
const INDICATOR_WIDTH: f32 = 30.0;
const CONE_HALF_ANGLE: f32 = 25.0;
const STASIS: Vec4 = Vec4::new(1.0, 0.82, 0.3, 0.9);
const CAMOUFLAGE_OPACITY: f32 = 0.4;
const BRUSH_OPACITY: f32 = 0.5;
const RELIC_GLOW: &str = "HealthRelicIdle";
const SUN_DISC: &str = "sun_disc";
const CHARGE_TIME: &str = "ChargeTime";
const LOADING_MINIMUM: f64 = 2.5;
const BACKGROUND_INTERVAL: i32 = 250;
const END_SCOREBOARD: f32 = 4.0;
const VOICE_COOLDOWN: f64 = 6.0;
const VOICE_CHANCE: f64 = 0.35;
const LEGENDARY: u8 = 5;
const SPELL_VOICE_CHANCE: f64 = 0.6;
const SPELL_VOICE_COOLDOWN: f64 = 3.0;
const WELCOME_AT: f32 = 1.0;
const STAGE_GRACE: f32 = 5.0;
const RED: Vec4 = Vec4::new(1.0, 0.3, 0.25, 0.85);
const ENEMY_OUTLINE: Vec4 = Vec4::new(1.0, 0.15, 0.1, 1.0);
const ALLY_OUTLINE: Vec4 = Vec4::new(0.25, 0.6, 1.0, 1.0);
const TURRET_WARNING: f32 = 350.0;

type FrameCallback = Closure<dyn Fn(f64)>;

#[derive(Default)]
struct Input {
    cursor: Option<Vec2>,
    right_held: bool,
    middle_drag: Option<Vec2>,
    minimap_drag: bool,
    center_held: bool,
    arrows: Vec<String>,
    attack_move: bool,
    casting: Option<Slot>,
    charge: Option<(Slot, bool)>,
    repeat: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Connecting,
    Login,
    Lobby,
    Select,
    Game,
}

struct Ping {
    position: Vec2,
    kind: PingKind,
    age: f32,
}

struct Flight {
    origin: Vec2,
    offset: Vec3,
}

struct Corpse {
    unit: UnitState,
    puppet: Puppet,
    age: f32,
}

struct Effect {
    name: u32,
    position: Vec2,
    facing: f32,
    unit: Option<UnitId>,
    hostile: bool,
}

struct Match {
    announcer: announcer::Announcer,
    loading: Option<select::LoadingView>,
    reported: Option<u8>,
    created: f64,
    timeline_stage: u8,
    over: f32,
    sounds: Vec<(String, Option<Vec2>)>,
    clips: Vec<(String, u32, UnitId, Vec3, f32)>,
    idle: Vec<UnitId>,
    relic_glows: Vec<Vec<u32>>,
    props: HashMap<u32, Vec<u32>>,
    setup: Setup,
    team: Team,
    hud: Hud,
    overlay: Overlay,
    timeline: world::Timeline,
    puppets: HashMap<UnitId, Puppet>,
    clickers: HashMap<UnitId, Puppet>,
    corpses: Vec<Corpse>,
    effects: Vec<Effect>,
    landed: Vec<u32>,
    pings: Vec<Ping>,
    selected: Option<UnitId>,
    target: target::TargetFrame,
    units: Vec<UnitState>,
    missiles: Vec<MissileState>,
    flights: HashMap<u32, Flight>,
    bounds: Option<Bounds>,
    winner: Option<Team>,
    clock: f32,
    centered: bool,
    killed: Vec<UnitId>,
    scoreboard: Option<f32>,
}

struct App {
    window: Window,
    canvas: HtmlCanvasElement,
    root: Element,
    renderer: Renderer,
    library: Library,
    arrivals: Rc<RefCell<Vec<(u8, assets::Model)>>>,
    requested: HashSet<u8>,
    pending: Rc<Cell<usize>>,
    map: Option<map::MapRenderer>,
    fog: fog::Fog,
    audio: Option<audio::Audio>,
    particles: Option<particles::Particles>,
    last_voice: f64,
    heights: Option<HeightField>,
    map_bounds: Option<Bounds>,
    data: Rc<GameData>,
    layout: Rc<Layout>,
    connection: Connection,
    reconnect: f32,
    resumed: bool,
    resuming: bool,
    events: Events,
    screen: Screen,
    status: HtmlElement,
    login: Login,
    lobby_view: Lounge,
    select_view: select::ChampSelectView,
    chat: chat::Chat,
    menu: menu::Menu,
    settings: menu::Preferences,
    notice: HtmlElement,
    notice_time: f32,
    vote_notice_time: f32,
    loading_error: HtmlElement,
    name: String,
    lobby: Option<Lobby>,
    game: Option<Match>,
    camera: Camera,
    input: Input,
    hovered: Option<UnitId>,
    last_frame: Option<f64>,
}

#[wasm_bindgen]
pub async fn start(canvas: HtmlCanvasElement, root: Element) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or("no window")?;
    let gl: WebGl2RenderingContext = canvas
        .get_context("webgl2")?
        .ok_or("no webgl2")?
        .dyn_into()?;
    let renderer = Renderer::new(gl.clone())?;
    download::load_manifest().await;
    let data = GameData::from_bytes(&download::fetch("assets/data.bin").await?)
        .map_err(|error| error.to_string())?;
    let layout = match download::fetch("assets/ui/layout.bin").await {
        Ok(bytes) => Layout::from_bytes(&bytes).map_err(|error| error.to_string())?,
        Err(error) => {
            web_sys::console::warn_1(&error);
            Layout::default()
        }
    };
    let pending = Rc::new(Cell::new(0));
    let booting = Loading::new(&pending);
    let events: Events = Rc::default();
    let status = dom::text("div", "status", "Connecting");
    dom::append(&root, &status);
    let login = Login::new(&root, &events);
    let data = Rc::new(data);
    let lobby_view = Lounge::new(&root, &events, data.clone());
    let select_view = select::ChampSelectView::new(&root, &events, &data);
    let chat = chat::Chat::new(&root, &layout, &events);
    chat.show(false);
    let settings = Rc::new(RefCell::new(settings::Settings::load()));
    let menu = menu::Menu::new(&root, &events, settings.clone());
    let notice = dom::text("div", "toast", "");
    let _ = notice.set_attribute("role", "status");
    dom::show(&notice, false);
    dom::append(&root, &notice);
    let loading_error = dom::element("div", "asset-error");
    dom::append(
        &loading_error,
        &dom::text("h2", "", "Some game assets could not load."),
    );
    dom::append(
        &loading_error,
        &dom::text(
            "p",
            "muted",
            "The match will wait. Retry to reload the missing assets.",
        ),
    );
    let retry = dom::text("button", "button primary", "Retry");
    dom::on(&retry, "click", |_| {
        if let Some(window) = web_sys::window() {
            let _ = window.location().reload();
        }
    });
    dom::append(&loading_error, &retry);
    dom::show(&loading_error, false);
    dom::append(&root, &loading_error);
    login.show(false);
    lobby_view.show(false);
    let app = Rc::new(RefCell::new(App {
        window: window.clone(),
        canvas: canvas.clone(),
        root,
        renderer: renderer.clone(),
        library: Library::default(),
        arrivals: Rc::default(),
        requested: HashSet::new(),
        pending: pending.clone(),
        map: None,
        fog: fog::Fog::new(&gl)?,
        audio: None,
        particles: None,
        last_voice: 0.0,
        heights: None,
        map_bounds: None,
        data,
        layout: Rc::new(layout),
        connection: Connection::open()?,
        reconnect: 0.0,
        resumed: false,
        resuming: false,
        events,
        screen: Screen::Connecting,
        status,
        login,
        lobby_view,
        select_view,
        chat,
        menu,
        settings,
        notice,
        notice_time: 0.0,
        vote_notice_time: 0.0,
        loading_error,
        name: String::new(),
        lobby: None,
        game: None,
        camera: Camera::new(Vec2::ZERO),
        input: Input::default(),
        hovered: None,
        last_frame: None,
    }));

    listen(&canvas, "mousedown", App::mouse_down, &app)?;
    listen(&window, "mousedown", App::minimap_down, &app)?;
    listen(
        &window,
        "contextmenu",
        |app, event: MouseEvent| {
            if app.screen == Screen::Game {
                event.prevent_default();
            }
        },
        &app,
    )?;
    listen(&window, "mouseup", App::mouse_up, &app)?;
    listen(&window, "mousemove", App::mouse_move, &app)?;
    listen(&canvas, "wheel", App::wheel, &app)?;
    listen(&window, "keydown", App::key_down, &app)?;
    listen(&window, "keyup", App::key_up, &app)?;
    listen(&window, "blur", |app, _: web_sys::Event| app.blur(), &app)?;
    listen(
        &dom::document(),
        "mouseout",
        |app, event: MouseEvent| {
            if event.related_target().is_none() {
                app.input.cursor = None;
            }
        },
        &app,
    )?;
    animate(app.clone());
    pump_in_background(app.clone())?;

    if let Ok(audio) = audio::Audio::load().await {
        app.borrow_mut().audio = Some(audio);
    }
    let map_data = MapData::from_bytes(&download::fetch("assets/map/data.bin").await?)
        .map_err(|error| error.to_string())?;
    app.borrow_mut().map_bounds = Some(Bounds {
        min: Vec2::from(map_data.bounds_min),
        max: Vec2::from(map_data.bounds_max),
    });
    app.borrow_mut().heights = Some(map_data.nav.heights);
    for folder in ["map/", "vfx/", "units/"] {
        download::expect_folder(folder);
    }
    let loading = Loading::new(&pending);
    let (map_gl, map_app) = (gl.clone(), app.clone());
    spawn_local(async move {
        let _loading = loading;
        match map::MapRenderer::load(&map_gl, "assets/map").await {
            Ok(map) => map_app.borrow_mut().map = Some(map),
            Err(error) => web_sys::console::warn_1(&error),
        }
    });
    let loading = Loading::new(&pending);
    let particles_app = app.clone();
    spawn_local(async move {
        let _loading = loading;
        match particles::Particles::load(&gl).await {
            Ok(particles) => particles_app.borrow_mut().particles = Some(particles),
            Err(error) => web_sys::console::warn_1(&error),
        }
    });
    let loading = Loading::new(&pending);
    spawn_local(async move {
        let _loading = loading;
        match assets::load_units(&renderer).await {
            Ok(units) => {
                let mut app = app.borrow_mut();
                let champions = std::mem::take(&mut app.library.champions);
                app.library = Library { champions, ..units };
            }
            Err(error) => web_sys::console::warn_1(&error),
        }
    });
    drop(booting);
    Ok(())
}

struct Loading(Rc<Cell<usize>>);

impl Loading {
    fn new(pending: &Rc<Cell<usize>>) -> Self {
        pending.set(pending.get() + 1);
        Self(pending.clone())
    }
}

impl Drop for Loading {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

fn listen<E: JsCast + 'static>(
    target: &web_sys::EventTarget,
    name: &str,
    handler: impl Fn(&mut App, E) + 'static,
    app: &Rc<RefCell<App>>,
) -> Result<(), JsValue> {
    let app = app.clone();
    let closure = Closure::<dyn Fn(web_sys::Event)>::new(move |event: web_sys::Event| {
        handler(&mut app.borrow_mut(), event.unchecked_into())
    });
    target.add_event_listener_with_callback(name, closure.as_ref().unchecked_ref())?;
    closure.forget();
    Ok(())
}

fn animate(app: Rc<RefCell<App>>) {
    let frame: Rc<RefCell<Option<FrameCallback>>> = Rc::new(RefCell::new(None));
    let next = frame.clone();
    let window = app.borrow().window.clone();
    *frame.borrow_mut() = Some(Closure::new(move |now: f64| {
        app.borrow_mut().frame(now);
        if let Some(callback) = next.borrow().as_ref() {
            let _ = app
                .borrow()
                .window
                .request_animation_frame(callback.as_ref().unchecked_ref());
        }
    }));
    if let Some(callback) = frame.borrow().as_ref() {
        let _ = window.request_animation_frame(callback.as_ref().unchecked_ref());
    }
}

fn pump_in_background(app: Rc<RefCell<App>>) -> Result<(), JsValue> {
    let window = app.borrow().window.clone();
    let closure = Closure::<dyn Fn()>::new(move || {
        let mut app = app.borrow_mut();
        let now = app.now();
        if app
            .last_frame
            .is_none_or(|last| now * 1000.0 - last > BACKGROUND_INTERVAL as f64)
        {
            app.handle_network(0.0);
            app.handle_ui();
            app.receive_assets();
            app.report_loading();
            if let Some(game) = app.game.as_mut() {
                game.sounds.clear();
                game.effects.retain(|effect| status::is_status(effect.name));
                game.landed.clear();
                game.hud.forget_backlog();
            }
        }
    });
    window.set_interval_with_callback_and_timeout_and_arguments_0(
        closure.as_ref().unchecked_ref(),
        BACKGROUND_INTERVAL,
    )?;
    closure.forget();
    Ok(())
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

fn stored_token() -> Option<String> {
    storage()?.get_item(TOKEN_KEY).ok()?.or_else(|| {
        web_sys::window()?
            .session_storage()
            .ok()??
            .get_item(TOKEN_KEY)
            .ok()?
    })
}

fn store_token(token: Option<&str>) {
    if let Some(legacy) =
        web_sys::window().and_then(|window| window.session_storage().ok().flatten())
    {
        let _ = legacy.remove_item(TOKEN_KEY);
    }
    if let Some(storage) = storage() {
        let _ = match token {
            Some(token) => storage.set_item(TOKEN_KEY, token),
            None => storage.remove_item(TOKEN_KEY),
        };
    }
}

fn structure_radius(kind: UnitKind) -> f32 {
    match kind {
        UnitKind::Turret | UnitKind::Shrine | UnitKind::SunDisc => 90.0,
        UnitKind::Inhibitor => 180.0,
        UnitKind::Nexus => 290.0,
        _ => 0.0,
    }
}

fn pick_radius(kind: UnitKind) -> f32 {
    match kind {
        UnitKind::Champion(_) => 75.0,
        UnitKind::Minion(_) => 55.0,
        UnitKind::Soldier => 55.0,
        structure => structure_radius(structure),
    }
}

fn keep_on_lane(focus: Vec2, from: Vec2, to: Vec2, width: f32) -> Vec2 {
    let lane = to - from;
    let direction = lane.normalize_or_zero();
    let along = ((focus - from).dot(lane) / lane.length_squared().max(1.0)).clamp(0.0, 1.0);
    let center = from + lane * along;
    let offset = focus - center;
    center + (offset - direction * offset.dot(direction)).clamp_length_max(width)
}

fn arrow(code: &str) -> Vec2 {
    match code {
        "ArrowLeft" => Vec2::NEG_X,
        "ArrowRight" => Vec2::X,
        "ArrowUp" => Vec2::Y,
        "ArrowDown" => Vec2::NEG_Y,
        _ => Vec2::ZERO,
    }
}

fn champion_key(data: &GameData, kind: UnitKind) -> Option<String> {
    match kind {
        UnitKind::Champion(index) => data
            .champions
            .get(usize::from(index))
            .map(ChampionData::key),
        _ => None,
    }
}

fn effect_key(data: &GameData, kind: UnitKind) -> Option<String> {
    match kind {
        UnitKind::SunDisc => Some(SUN_DISC.to_owned()),
        _ => champion_key(data, kind),
    }
}

fn clip_sound(data: &GameData, kind: UnitKind, clip: u32) -> Option<String> {
    let named = |name: &str| clip_id(name) == clip;
    if let Some(champion) = champion_key(data, kind) {
        let action = if named("Attack1") || named("Attack2") || named("Attack3") || named("Attack4")
        {
            "attack"
        } else if named("Crit") {
            "crit"
        } else if named("Joke") {
            "vo.joke"
        } else if named("Taunt") {
            "vo.taunt"
        } else if named("Laugh") {
            "vo.laugh"
        } else if named("Death") {
            "vo.death"
        } else {
            return None;
        };
        return Some(format!("{champion}.{action}"));
    }
    let attacking = named("Attack1") || named("Attack2") || named("Attack3") || named("Attack4");
    match kind {
        UnitKind::Minion(MinionKind::Melee) if attacking => Some("minion.melee_attack".into()),
        UnitKind::Minion(MinionKind::Caster) if attacking => Some("minion.ranged_attack".into()),
        UnitKind::Minion(MinionKind::Siege) if attacking => Some("minion.siege_attack".into()),
        UnitKind::Minion(MinionKind::Super) if attacking => Some("minion.super_attack".into()),
        UnitKind::Minion(_) if named("Death") => Some("minion.death".into()),
        UnitKind::Turret | UnitKind::Shrine if attacking => Some("turret.attack".into()),
        UnitKind::Soldier if attacking => Some("soldier.attack".into()),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Shape {
    Line(f32, f32),
    Circle(f32, f32),
    Target(f32),
    Cone(f32, f32),
}

fn summoner_shape(summoner: Summoner, range: f32) -> Option<Shape> {
    match summoner {
        Summoner::Mark => Some(Shape::Line(range, INDICATOR_WIDTH * 2.0)),
        Summoner::Exhaust | Summoner::Ignite => Some(Shape::Target(range)),
        Summoner::Flash
        | Summoner::Heal
        | Summoner::Ghost
        | Summoner::Barrier
        | Summoner::Clarity
        | Summoner::Cleanse => None,
    }
}

fn ability_ready(data: &GameData, game: &Match, slot: Slot) -> bool {
    let (Some(unit), Some(ability), Some(index)) = (
        game.you(),
        game.ability(slot),
        SLOTS.iter().position(|candidate| *candidate == slot),
    ) else {
        return false;
    };
    let summoner = game.summoner(slot);
    if !ui::free_to_cast(unit, summoner, ability.recast) {
        return false;
    }
    if summoner.is_some() {
        return ability.cooldown <= 0.0 || ability.recast;
    }
    let cost = match unit.kind {
        UnitKind::Champion(champion) => data
            .champions
            .get(usize::from(champion))
            .and_then(|champion| champion.spell(champion.slots.get(index)?))
            .map_or(0.0, |spell| spell.mana(ability.rank.max(1))),
        _ => 0.0,
    };
    ability.rank > 0 && (ability.recast || ability.cooldown <= 0.0 && unit.mana >= cost)
}

fn resource_of(data: &GameData, kind: UnitKind) -> Resource {
    match kind {
        UnitKind::Champion(index) => data
            .champions
            .get(usize::from(index))
            .map_or(Resource::None, |champion| champion.resource),
        _ => Resource::None,
    }
}

fn emote_for(code: &str) -> Option<Emote> {
    match code {
        "Digit1" => Some(Emote::Joke),
        "Digit2" => Some(Emote::Taunt),
        "Digit3" => Some(Emote::Dance),
        "Digit4" => Some(Emote::Laugh),
        _ => None,
    }
}

fn item_slot_for(code: &str) -> Option<u8> {
    let key = code.strip_prefix("Digit")?;
    ui::ITEM_KEYS
        .iter()
        .position(|candidate| *candidate == key)
        .map(|slot| slot as u8)
}

fn slot_for(code: &str) -> Option<Slot> {
    match code {
        "KeyQ" => Some(Slot::Q),
        "KeyW" => Some(Slot::W),
        "KeyE" => Some(Slot::E),
        "KeyR" => Some(Slot::R),
        "KeyD" => Some(Slot::D),
        "KeyF" => Some(Slot::F),
        _ => None,
    }
}

impl Match {
    fn new(app: &App, setup: Setup) -> Self {
        let player = setup.players.iter().find(|player| player.unit == setup.you);
        let champion = player.map_or(0, |player| usize::from(player.champion));
        let team = player.map_or(Team::Blue, |player| player.team);
        let summoners = player.map_or(Summoner::DEFAULT, |player| player.summoners);
        let hud = Hud::new(
            &app.root,
            &app.events,
            app.data.clone(),
            &app.layout,
            champion,
            summoners,
            &setup.players,
        );
        let overlay = Overlay::new(&app.root, &app.layout);
        let loading = select::LoadingView::new(&app.root, &setup, &app.data);
        Self {
            announcer: announcer::Announcer::new(setup.you, team),
            loading: Some(loading),
            reported: None,
            created: app.now(),
            timeline_stage: 0,
            over: 0.0,
            sounds: Vec::new(),
            clips: Vec::new(),
            idle: Vec::new(),
            relic_glows: Vec::new(),
            props: HashMap::new(),
            setup,
            team,
            hud,
            overlay,
            timeline: world::Timeline::default(),
            puppets: HashMap::new(),
            clickers: HashMap::new(),
            corpses: Vec::new(),
            effects: Vec::new(),
            landed: Vec::new(),
            pings: Vec::new(),
            selected: None,
            target: target::TargetFrame::new(&app.root, &app.layout),
            units: Vec::new(),
            missiles: Vec::new(),
            flights: HashMap::new(),
            bounds: None,
            winner: None,
            clock: 0.0,
            centered: false,
            killed: Vec::new(),
            scoreboard: None,
        }
    }

    fn remove(&self) {
        self.hud.remove();
        self.overlay.remove();
        self.target.remove();
        if let Some(loading) = &self.loading {
            loading.remove();
        }
    }

    fn you(&self) -> Option<&UnitState> {
        self.units.iter().find(|unit| unit.id == self.setup.you)
    }

    fn ability(&self, slot: Slot) -> Option<AbilityState> {
        let index = SLOTS.iter().position(|candidate| *candidate == slot)?;
        let you = self.timeline.latest()?.you.as_ref()?;
        Some(you.abilities[index])
    }

    fn summoner(&self, slot: Slot) -> Option<Summoner> {
        let player = self
            .setup
            .players
            .iter()
            .find(|player| player.unit == self.setup.you)?;
        match slot {
            Slot::D => Some(player.summoners[0]),
            Slot::F => Some(player.summoners[1]),
            _ => None,
        }
    }

    fn viewer(&self) -> Viewer {
        Viewer {
            you: self.setup.you,
            team: self.team,
        }
    }

    fn icon_of(&self, data: &GameData, unit: UnitId) -> Option<(String, bool)> {
        let player = self
            .setup
            .players
            .iter()
            .find(|player| player.unit == unit)?;
        let champion = data.champions.get(usize::from(player.champion))?;
        Some((champion.icon.clone(), player.team == self.team))
    }

    fn portrait(
        &self,
        data: &GameData,
        units: &[UnitState],
        id: UnitId,
    ) -> Option<banner::Portrait> {
        if let Some((icon, ally)) = self.icon_of(data, id) {
            return Some(banner::Portrait { icon, ally });
        }
        let unit = units.iter().find(|unit| unit.id == id)?;
        Some(banner::Portrait {
            icon: format!("{}.png", assets::portrait_file(unit.kind, unit.team)?),
            ally: unit.team == self.team,
        })
    }

    fn click(&mut self, effect: &str, position: Vec2) {
        self.effects.push(Effect {
            name: clip_id(effect),
            position,
            facing: 0.0,
            unit: None,
            hostile: false,
        });
    }

    fn receive(&mut self, snapshot: Snapshot, now: f64, data: &GameData) {
        let latest = |id: UnitId| {
            self.timeline
                .latest()
                .and_then(|snapshot| snapshot.units.iter().find(|unit| unit.id == id))
                .or_else(|| snapshot.units.iter().find(|unit| unit.id == id))
                .map(|unit| Vec2::from(unit.position))
        };
        let mut floating = Vec::new();
        self.announcer.sync(&snapshot.scores);
        let mut kills = Vec::new();
        for event in &snapshot.events {
            match event {
                Event::Damage {
                    source,
                    target,
                    amount,
                    kind,
                    critical,
                } => {
                    let mine = *source == Some(self.setup.you);
                    let taken = *target == self.setup.you;
                    if (mine || taken)
                        && *amount >= 1.0
                        && let Some(position) = latest(*target)
                    {
                        let class = match (taken, kind) {
                            (true, _) => "taken",
                            (false, DamageKind::Physical) => "physical",
                            (false, DamageKind::Magic) => "magic",
                            (false, DamageKind::True) => "true",
                        };
                        let class = if *critical {
                            format!("{class} critical")
                        } else {
                            class.to_owned()
                        };
                        floating.push((format!("{:.0}", amount), class, position));
                    }
                }
                Event::Heal { target, amount } if *target == self.setup.you && *amount >= 5.0 => {
                    if let Some(position) = latest(*target) {
                        floating.push((format!("+{:.0}", amount), "heal".to_owned(), position));
                    }
                }
                Event::Gold { target, amount } if *target == self.setup.you && *amount >= 1.0 => {
                    self.sounds.push(("gold".to_owned(), None));
                    if let Some(position) = latest(*target) {
                        floating.push((format!("+{:.0}", amount), "gold".to_owned(), position));
                    }
                }
                Event::Kill {
                    killer,
                    victim,
                    assists,
                    survivors,
                } => {
                    self.killed.push(*victim);
                    if *victim == self.setup.you {
                        self.sounds.push(("death".to_owned(), None));
                    }
                    kills.push((*killer, *victim, assists.clone(), *survivors));
                }
                Event::Effect {
                    name,
                    position,
                    facing,
                    unit,
                    team,
                    ..
                } => {
                    let sound = if *name == clip_id("Flash") {
                        Some("flash")
                    } else if *name == clip_id("LevelUp") && *unit == Some(self.setup.you) {
                        Some("level_up")
                    } else {
                        None
                    };
                    if let Some(sound) = sound {
                        self.sounds
                            .push((sound.to_owned(), Some(Vec2::from(*position))));
                    }
                    self.effects.push(Effect {
                        name: *name,
                        position: Vec2::from(*position),
                        facing: *facing,
                        unit: *unit,
                        hostile: team.is_some_and(|team| team != self.team),
                    });
                }
                Event::Sound { unit, key } => {
                    if let Some(position) = latest(*unit) {
                        self.sounds.push((key.clone(), Some(position)));
                    }
                }
                Event::MissileLanded { id, .. } => self.landed.push(*id),
                _ => {}
            }
        }
        for (value, class, position) in floating {
            self.hud
                .floating(&value, &class, position.to_array(), 200.0);
        }
        let mut marks = Vec::new();
        for &(killer, victim, _, survivors) in &kills {
            let unit = |id: UnitId| snapshot.units.iter().find(|unit| unit.id == id);
            let player = |id: UnitId| self.setup.players.iter().find(|player| player.unit == id);
            let victim_spree = self.announcer.spree(victim);
            let Some((victim_kind, victim_team)) = player(victim)
                .map(|player| (UnitKind::Champion(player.champion), player.team))
                .or_else(|| unit(victim).map(|unit| (unit.kind, unit.team)))
            else {
                continue;
            };
            let death_sound = match victim_kind {
                UnitKind::Turret => Some("turret.death"),
                UnitKind::Inhibitor => Some("inhibitor.death"),
                UnitKind::Nexus => Some("nexus.death"),
                _ => None,
            };
            if let Some(sound) = death_sound
                && let Some(victim_unit) = unit(victim)
            {
                self.sounds
                    .push((sound.to_owned(), Some(Vec2::from(victim_unit.position))));
            }
            let victim_kind = match victim_kind {
                UnitKind::Champion(_) => announcer::Victim::Champion,
                UnitKind::Turret => announcer::Victim::Turret,
                UnitKind::Inhibitor => announcer::Victim::Inhibitor,
                UnitKind::Nexus => announcer::Victim::Nexus,
                _ => announcer::Victim::Other,
            };
            let killer_champion = killer
                .and_then(player)
                .map(|player| (player.unit, player.team));
            let killer_id = killer_champion.map(|(id, _)| id);
            let announcements = self.announcer.kill(announcer::Kill {
                time: snapshot.tick as f32 / TICK_RATE as f32,
                killer: killer_champion,
                victim,
                victim_team,
                victim_kind,
                victim_team_alive: usize::from(survivors),
            });
            let killer_spree = killer_id.map_or(0, |id| self.announcer.spree(id));
            marks.push((victim, callouts::Mark::of(killer_spree, victim_spree)));
            let tone = if victim_team == self.team {
                banner::Tone::Enemy
            } else {
                banner::Tone::Ally
            };
            for item in announcements {
                let portrait = |id: UnitId| {
                    self.icon_of(data, id)
                        .map(|(icon, ally)| banner::Portrait { icon, ally })
                };
                let subject = item.killer.and_then(|id| {
                    self.setup
                        .players
                        .iter()
                        .find(|player| player.unit == id)
                        .map(|player| player.name.clone())
                });
                self.hud.announce(
                    banner::Banner {
                        text: item.text.to_owned(),
                        style: item.style,
                        tone,
                        left: item.killer.and_then(portrait),
                        right: item.victim.and_then(portrait),
                    },
                    subject.as_deref(),
                    Some(item.audio),
                );
            }
        }
        for (killer, victim, assists, _) in kills {
            if let Some((icon, ally)) = self.icon_of(data, victim) {
                let portrait = |id: UnitId| self.portrait(data, &snapshot.units, id);
                let mark = marks
                    .iter()
                    .find(|(marked, _)| *marked == victim)
                    .map_or(callouts::Mark::Normal, |(_, mark)| *mark);
                self.hud.callout(callouts::Callout {
                    killer: killer.and_then(portrait),
                    victim: banner::Portrait { icon, ally },
                    assists: assists.into_iter().filter_map(portrait).collect(),
                    mark,
                });
            }
        }
        let before = self
            .timeline
            .latest()
            .map_or(&[][..], |latest| latest.units.as_slice());
        for (unit, name) in status::changes(before, &snapshot.units) {
            if let Some(position) = latest(unit) {
                let facing = snapshot
                    .units
                    .iter()
                    .find(|state| state.id == unit)
                    .map_or(0.0, |state| state.facing);
                self.effects.push(Effect {
                    name: clip_id(name),
                    position,
                    facing,
                    unit: Some(unit),
                    hostile: false,
                });
            }
        }
        if self.bounds.is_none() {
            self.bounds = Bounds::around(&snapshot.units);
        }
        self.timeline.push(snapshot, now);
    }
}

impl App {
    fn blur(&mut self) {
        self.input = Input::default();
        if let Some(game) = self.game.as_mut() {
            game.hud.advanced_stats(false);
            if game.winner.is_none() {
                game.scoreboard = None;
                game.hud.show_scoreboard(false);
            }
        }
    }

    fn you(&self) -> Option<&You> {
        self.game.as_ref()?.timeline.latest()?.you.as_ref()
    }

    fn load_champion(&mut self, index: u8) {
        if self.library.champions.contains_key(&index) || !self.requested.insert(index) {
            return;
        }
        let Some(champion) = self.data.champions.get(usize::from(index)) else {
            return;
        };
        let key = champion.key();
        download::expect_folder(&format!("champions/{key}."));
        download::expect_folder(&format!("champions/{key}_"));
        let loading = Loading::new(&self.pending);
        let (renderer, arrivals) = (self.renderer.clone(), self.arrivals.clone());
        spawn_local(async move {
            let _loading = loading;
            match assets::load_model(&renderer, "champions", &key).await {
                Ok(model) => arrivals.borrow_mut().push((index, model)),
                Err(error) => web_sys::console::warn_1(&error),
            }
        });
    }

    fn receive_assets(&mut self) {
        let arrived: Vec<_> = self.arrivals.borrow_mut().drain(..).collect();
        self.library.champions.extend(arrived);
    }

    fn report_loading(&mut self) {
        let ready = self.assets_ready();
        let Some(game) = self.game.as_mut() else {
            return;
        };
        let Some(view) = game.loading.as_ref() else {
            return;
        };
        let (received, expected) = download::progress();
        let percent = if ready {
            100
        } else {
            (received * 100 / expected.max(1)).min(99) as u8
        };
        view.show_own(percent, if ready { expected } else { received }, expected);
        if game.reported != Some(percent) {
            game.reported = Some(percent);
            self.connection.send(&ClientMessage::Loading(percent));
        }
    }

    fn assets_ready(&self) -> bool {
        self.arrivals.borrow().is_empty()
            && self.pending.get() == 0
            && self.map.is_some()
            && self.particles.is_some()
            && self.library.minions.len() == 8
            && self.library.structures.len() == 8
            && self.library.soldier.is_some()
            && self.library.sun_disc.is_some()
            && self.library.clicker.is_some()
            && self.game.as_ref().is_none_or(|game| {
                game.setup
                    .players
                    .iter()
                    .all(|player| self.library.champions.contains_key(&player.champion))
            })
            && !self.audio.as_ref().is_some_and(audio::Audio::loading)
    }

    fn end_match(&mut self) {
        if let Some(game) = self.game.take() {
            game.remove();
        }
        if let Some(particles) = self.particles.as_mut() {
            particles.clear();
        }
    }

    fn can_level(&self, slot: Slot) -> bool {
        let (Some(you), Some(unit), Some(index)) = (
            self.you(),
            self.game.as_ref().and_then(Match::you),
            SLOTS.iter().position(|candidate| *candidate == slot),
        ) else {
            return false;
        };
        you.skill_points > 0 && ui::can_level(slot, you.abilities[index].rank, unit.level)
    }

    fn now(&self) -> f64 {
        self.window
            .performance()
            .map_or(0.0, |performance| performance.now())
            / 1000.0
    }

    fn size(&self) -> Vec2 {
        Vec2::new(
            self.canvas.client_width().max(1) as f32,
            self.canvas.client_height().max(1) as f32,
        )
    }

    fn aspect(&self) -> f32 {
        let size = self.size();
        size.x / size.y
    }

    fn terrain(&self, position: Vec2) -> f32 {
        self.heights
            .as_ref()
            .map_or(0.0, |heights| heights.height_at(position.x, position.y))
    }

    fn ground(&self, position: Vec2) -> Vec3 {
        Vec3::new(position.x, self.terrain(position), position.y)
    }

    fn ground_under(&self, cursor: Vec2) -> Option<Vec2> {
        let size = self.size();
        let ndc = Vec2::new(cursor.x / size.x * 2.0 - 1.0, 1.0 - cursor.y / size.y * 2.0);
        let mut height = self.camera.height;
        let mut hit = None;
        for _ in 0..GROUND_REFINEMENT {
            let point = self.camera.point_at_height(ndc, self.aspect(), height)?;
            height = self.terrain(point);
            hit = Some(point);
        }
        hit
    }

    fn project(&self, view_projection: &Mat4, position: Vec2, height: f32) -> Option<Vec2> {
        let clip = *view_projection * (self.ground(position) + Vec3::Y * height).extend(1.0);
        if clip.w <= 0.0 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        let size = self.size();
        Some(Vec2::new(
            (ndc.x + 1.0) * 0.5 * size.x,
            (1.0 - ndc.y) * 0.5 * size.y,
        ))
    }

    fn send(&self, command: Command) {
        self.connection.send(&ClientMessage::Command(command));
    }

    fn show_screen(&mut self, screen: Screen) {
        if screen != self.screen
            && let Some(audio) = self.audio.as_mut()
        {
            match screen {
                Screen::Select => audio.loop_music("champ_select.music"),
                Screen::Game => audio.loop_music("map.ambience"),
                _ => audio.stop_music(),
            }
        }
        if screen == Screen::Login {
            self.end_match();
        }
        self.screen = screen;
        self.menu.in_game(screen == Screen::Game);
        self.menu.set_open(false);
        self.login.show(screen == Screen::Login);
        self.lobby_view.show(screen == Screen::Lobby);
        self.select_view.show(screen == Screen::Select);
        self.chat.show(matches!(
            screen,
            Screen::Lobby | Screen::Select | Screen::Game
        ));
        dom::show(&self.status, screen == Screen::Connecting);
    }

    fn handle_ui(&mut self) {
        let events: Vec<UiEvent> = self.events.borrow_mut().drain(..).collect();
        for event in events {
            match event {
                UiEvent::LogIn { name, password } => {
                    self.resumed = false;
                    self.connection
                        .send(&ClientMessage::LogIn { name, password });
                }
                UiEvent::SignUp { name, password } => {
                    self.resumed = false;
                    self.connection
                        .send(&ClientMessage::SignUp { name, password });
                }
                UiEvent::JoinTeam(team) => self.connection.send(&ClientMessage::JoinTeam(team)),
                UiEvent::Start => self.connection.send(&ClientMessage::Start),
                UiEvent::StartVote(kind) => {
                    self.menu.set_open(false);
                    self.connection.send(&ClientMessage::StartVote(kind));
                    self.sound("ui.click");
                }
                UiEvent::Vote(yes) => self.connection.send(&ClientMessage::Vote(yes)),
                UiEvent::CreateLobby(name) => {
                    self.connection.send(&ClientMessage::CreateLobby(name))
                }
                UiEvent::JoinLobby(id) => self.connection.send(&ClientMessage::JoinLobby(id)),
                UiEvent::LeaveLobby => self.connection.send(&ClientMessage::LeaveLobby),
                UiEvent::SetAvatar(avatar) => {
                    self.connection.send(&ClientMessage::SetAvatar(avatar))
                }
                UiEvent::Ban(champion) => self.connection.send(&ClientMessage::Ban(champion)),
                UiEvent::OpenMenu => {
                    self.blur();
                    self.menu.set_open(true);
                }
                UiEvent::CloseMenu => self.menu.set_open(false),
                UiEvent::MenuTab(tab) => self.lobby_view.tab(tab),
                UiEvent::ResetSettings => {
                    *self.settings.borrow_mut() = settings::Settings::default();
                    self.settings.borrow().save();
                    self.menu.set_open(true);
                }
                UiEvent::LogOut => {
                    self.connection.send(&ClientMessage::LogOut);
                    store_token(None);
                    self.name.clear();
                    self.lobby = None;
                    self.show_screen(Screen::Login);
                }
                UiEvent::Pick(champion) => {
                    if self.select_view.banning() {
                        self.select_view.choose_ban(champion);
                    } else {
                        self.connection.send(&ClientMessage::Pick(champion));
                    }
                    self.sound("ui.click");
                }
                UiEvent::Summoners(summoners) => {
                    self.connection.send(&ClientMessage::Summoners(summoners));
                    self.sound("ui.click");
                }
                UiEvent::LockIn => {
                    if self.select_view.banning() {
                        self.connection
                            .send(&ClientMessage::Ban(self.select_view.picked()));
                        continue;
                    }
                    self.connection.send(&ClientMessage::LockIn);
                    self.sound("ui.lock_in");
                    if let Some(champion) = self
                        .select_view
                        .picked()
                        .and_then(|champion| self.data.champions.get(usize::from(champion)))
                    {
                        let key = format!("{}.vo.pick", champion.key());
                        self.sound(&key);
                    }
                }
                UiEvent::Chat { text, all } => {
                    if !text.trim().is_empty() {
                        self.connection.send(&ClientMessage::Chat { text, all });
                    }
                    self.chat.close();
                }
                UiEvent::Buy(item) => {
                    let affordable = self.you().is_some_and(|you| {
                        you.can_shop
                            && self
                                .data
                                .quote(item, &you.items, &you.stacks)
                                .is_some_and(|quote| quote.cost as f32 <= you.gold)
                    });
                    if affordable {
                        self.send(Command::Buy(item));
                        let legendary = self
                            .data
                            .item(item)
                            .is_some_and(|item| item.tier == LEGENDARY);
                        self.sound(if legendary { "buy_legendary" } else { "buy" });
                    }
                }
                UiEvent::ToggleCameraLock => self.toggle_camera(),
                UiEvent::ToggleStats => {
                    if let Some(game) = self.game.as_mut() {
                        game.hud.toggle_stats();
                    }
                }
                UiEvent::ToggleShop => {
                    if let Some(game) = self.game.as_mut() {
                        game.hud.toggle_shop();
                        game.hud.hide_tooltip();
                    }
                }
                UiEvent::Sell(slot) => {
                    let sellable = self.you().is_some_and(|you| {
                        you.can_shop
                            && you
                                .items
                                .get(usize::from(slot))
                                .is_some_and(Option::is_some)
                    });
                    if sellable {
                        self.send(Command::Sell(slot));
                        self.sound("sell");
                    }
                }
                UiEvent::Undo => {
                    if self.you().is_some_and(|you| you.can_undo) {
                        self.send(Command::Undo);
                        self.sound("undo");
                    }
                }
                UiEvent::Sound(key) => self.sound(&key),
                UiEvent::LevelUp(slot) => {
                    self.send(Command::LevelUp(slot));
                    self.sound("spell_level_up");
                }
            }
        }
    }

    fn handle_network(&mut self, dt: f32) {
        match self.connection.status() {
            Status::Closed => {
                self.reconnect += dt;
                dom::set_text(&self.status, "Disconnected");
                dom::show(&self.status, true);
                if self.reconnect > RECONNECT_DELAY {
                    self.reconnect = 0.0;
                    if let Ok(connection) = Connection::open() {
                        self.connection = connection;
                        self.resumed = false;
                    }
                }
                return;
            }
            Status::Connecting => return,
            Status::Open => {}
        }
        if !self.resumed {
            self.resumed = true;
            match stored_token() {
                Some(token) => {
                    dom::set_text(&self.status, "Connecting");
                    self.resuming = true;
                    self.connection.send(&ClientMessage::Resume { token });
                }
                None => self.show_screen(Screen::Login),
            }
        }
        let now = self.now();
        for message in self.connection.drain() {
            match message {
                ServerMessage::Session { token, name } => {
                    store_token(Some(&token));
                    self.resuming = false;
                    self.name = name;
                    self.login.error(None);
                    if self.game.is_none() || self.screen == Screen::Login {
                        self.end_match();
                        self.show_screen(Screen::Lobby);
                    }
                }
                ServerMessage::AuthFailed(error) => {
                    if self.resuming {
                        self.resuming = false;
                        store_token(None);
                    } else {
                        self.login.error(Some(error));
                    }
                    self.show_screen(Screen::Login);
                }
                ServerMessage::Lobby(lobby) => {
                    if lobby.id == 0 {
                        continue;
                    }
                    self.lobby_view.update(&lobby, &self.name);
                    if lobby.phase == Phase::Lobby {
                        if self.game.is_some() {
                            self.end_match();
                            self.set_grayscale(false);
                        }
                        if self.screen != Screen::Login {
                            self.show_screen(Screen::Lobby);
                        }
                    }
                    self.lobby = Some(lobby);
                }
                ServerMessage::Directory(directory) => {
                    if directory.room.is_none() {
                        self.lobby = None;
                        self.end_match();
                        self.set_grayscale(false);
                        if self.screen != Screen::Login {
                            self.show_screen(Screen::Lobby);
                        }
                    }
                    self.lobby_view.directory(&directory);
                }
                ServerMessage::Vote(vote) => {
                    self.vote_notice_time =
                        if vote.as_ref().is_some_and(|vote| vote.result.is_some()) {
                            5.0
                        } else {
                            0.0
                        };
                    self.menu.vote(vote.as_ref());
                }
                ServerMessage::Notice(message) => {
                    dom::set_text(&self.notice, &message);
                    self.notice_time = 5.0;
                }
                ServerMessage::ChampSelect(select) => {
                    self.select_view.update(&select, &self.name, &self.data);
                    let picking = select.players.iter().any(|player| player.name == self.name);
                    if picking && self.game.is_none() && self.screen != Screen::Login {
                        self.show_screen(Screen::Select);
                    }
                }
                ServerMessage::GameStarted(setup) => {
                    self.end_match();
                    for player in &setup.players {
                        self.load_champion(player.champion);
                    }
                    let playing: Vec<String> = setup
                        .players
                        .iter()
                        .filter_map(|player| self.data.champions.get(usize::from(player.champion)))
                        .map(ChampionData::key)
                        .collect();
                    let others: Vec<String> = self
                        .data
                        .champions
                        .iter()
                        .map(ChampionData::key)
                        .filter(|key| !playing.contains(key))
                        .collect();
                    if let Some(audio) = self.audio.as_mut() {
                        let keys: Vec<String> = audio
                            .keys()
                            .filter(|key| {
                                let owner = key.split('.').next().unwrap_or_default();
                                !others.iter().any(|other| other == owner)
                            })
                            .cloned()
                            .collect();
                        for key in keys {
                            audio.preload(&key);
                        }
                    }
                    self.game = Some(Match::new(self, setup));
                    self.input = Input {
                        cursor: self.input.cursor,
                        ..Input::default()
                    };
                    self.camera.locked = true;
                    self.show_screen(Screen::Game);
                }
                ServerMessage::Snapshot(snapshot) => {
                    let data = self.data.clone();
                    if let Some(game) = self.game.as_mut() {
                        game.receive(*snapshot, now, &data);
                    }
                }
                ServerMessage::Chat {
                    from,
                    team,
                    text,
                    all,
                } => {
                    let champion = self.game.as_ref().and_then(|game| {
                        let player = game
                            .setup
                            .players
                            .iter()
                            .find(|player| player.name == from)?;
                        Some(
                            self.data
                                .champions
                                .get(usize::from(player.champion))?
                                .display_name
                                .clone(),
                        )
                    });
                    self.chat.push(chat::Line {
                        all,
                        ally: Some(team) == self.my_team(),
                        from: &from,
                        champion: champion.as_deref(),
                        text: &text,
                    });
                }
                ServerMessage::Ping { position, kind, .. } => {
                    if let Some(game) = self.game.as_mut() {
                        let sound = match kind {
                            PingKind::Danger => "ping.danger",
                            _ => "ping.generic",
                        };
                        game.sounds.push((sound.to_owned(), None));
                        let effect = match kind {
                            PingKind::Generic => "PingGeneric",
                            PingKind::Danger => "PingDanger",
                            PingKind::OnMyWay => "PingOnMyWay",
                            PingKind::Missing => "PingMissing",
                        };
                        game.click(effect, Vec2::from(position));
                        game.pings.push(Ping {
                            position: Vec2::from(position),
                            kind,
                            age: 0.0,
                        });
                    }
                }
                ServerMessage::Loading(players) => {
                    if let Some(view) = self.game.as_ref().and_then(|game| game.loading.as_ref()) {
                        view.update(&players);
                    }
                }
                ServerMessage::GameOver { winner } => {
                    if let Some(game) = self.game.as_mut() {
                        game.winner = Some(winner);
                        let won = winner == game.team;
                        for key in if won {
                            ["announcer.victory", "victory"]
                        } else {
                            ["announcer.defeat", "defeat"]
                        } {
                            game.sounds.push((key.to_owned(), None));
                        }
                        game.hud.banner(Some(if winner == game.team {
                            "Victory"
                        } else {
                            "Defeat"
                        }));
                    }
                }
            }
        }
    }

    fn set_grayscale(&self, enabled: bool) {
        dom::set_style(
            self.canvas.unchecked_ref(),
            "filter",
            if enabled {
                "grayscale(0.85) brightness(0.8)"
            } else {
                "none"
            },
        );
    }

    fn cast_shape(&self, game: &Match, slot: Slot) -> Option<Shape> {
        let you = game.you()?;
        let index = SLOTS.iter().position(|candidate| *candidate == slot)?;
        let ability = game.ability(slot)?;
        if let Some(summoner) = game.summoner(slot) {
            return summoner_shape(summoner, ability.range);
        }
        if ability.area > 0.0 {
            return Some(Shape::Circle(ability.range, ability.area));
        }
        let UnitKind::Champion(champion) = you.kind else {
            return None;
        };
        let champion = self.data.champions.get(usize::from(champion))?;
        let spell = champion.spell(champion.slots.get(index)?)?;
        let radius = Spell::by_rank(&spell.cast_radius, ability.rank.max(1));
        let range = ability.range;
        match spell.targeting {
            Targeting::Myself | Targeting::MyselfArea => None,
            Targeting::Unit => Some(Shape::Target(range)),
            Targeting::Cone => Some(Shape::Cone(range, CONE_HALF_ANGLE)),
            Targeting::Area => Some(Shape::Circle(range, radius.max(INDICATOR_WIDTH))),
            Targeting::Location | Targeting::Direction => Some(Shape::Line(
                range,
                spell.line_width.max(INDICATOR_WIDTH) * 2.0,
            )),
        }
    }

    fn charges_up(&self, game: &Match, slot: Slot) -> bool {
        let index = SLOTS.iter().position(|candidate| *candidate == slot);
        let champion = match game.you().map(|you| you.kind) {
            Some(UnitKind::Champion(champion)) => self.data.champions.get(usize::from(champion)),
            _ => None,
        };
        champion
            .zip(index)
            .and_then(|(champion, index)| champion.spell(champion.slots.get(index)?))
            .is_some_and(|spell| spell.values.iter().any(|value| value.name == CHARGE_TIME))
    }

    fn hovered_unit(&self, game: &Match) -> Option<UnitId> {
        self.unit_at_cursor(game, |unit| {
            !unit.dead && unit.targetable && unit.team != game.team
                || game.clickers.contains_key(&unit.id)
        })
    }

    fn unit_at_cursor(&self, game: &Match, include: impl Fn(&UnitState) -> bool) -> Option<UnitId> {
        let cursor = self.input.cursor?;
        let view_projection = self.camera.view_projection(self.aspect());
        game.units
            .iter()
            .filter(|unit| include(unit))
            .filter_map(|unit| {
                let position = Vec2::from(unit.position);
                let base = self.project(&view_projection, position, 0.0)?;
                let top = self.project(&view_projection, position, bar_height(unit.kind) * 0.7)?;
                let edge = self.project(
                    &view_projection,
                    position + Vec2::new(pick_radius(unit.kind), 0.0),
                    0.0,
                )?;
                let radius = edge.distance(base);
                let segment = top - base;
                let along = ((cursor - base).dot(segment) / segment.length_squared().max(1.0))
                    .clamp(0.0, 1.0);
                let distance = cursor.distance(base + segment * along);
                (distance <= radius).then_some((unit.id, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    fn target(&self) -> Target {
        Target {
            point: self
                .input
                .cursor
                .and_then(|cursor| self.ground_under(cursor))
                .unwrap_or_default()
                .to_array(),
            unit: self.hovered,
        }
    }

    fn minimap_point(&self, event: &MouseEvent) -> Option<Vec2> {
        let game = self.game.as_ref()?;
        let bounds = game.bounds.as_ref()?;
        let rect = game.overlay.minimap.get_bounding_client_rect();
        let fraction = Vec2::new(
            (event.client_x() as f64 - rect.left()) as f32 / rect.width().max(1.0) as f32,
            (event.client_y() as f64 - rect.top()) as f32 / rect.height().max(1.0) as f32,
        );
        (fraction.cmpge(Vec2::ZERO).all() && fraction.cmple(Vec2::ONE).all())
            .then(|| bounds.point_at(fraction))
    }

    fn mouse_down(&mut self, event: MouseEvent) {
        if self.screen != Screen::Game || self.menu.is_open() {
            return;
        }
        let cursor = Vec2::new(event.offset_x() as f32, event.offset_y() as f32);
        self.input.cursor = Some(cursor);
        self.hovered = self.game.as_ref().and_then(|game| self.hovered_unit(game));
        match event.button() {
            RIGHT_BUTTON => {
                self.input.attack_move = false;
                self.input.casting = None;
                if self
                    .game
                    .as_ref()
                    .and_then(Match::you)
                    .is_none_or(|you| you.dead)
                {
                    return;
                }
                self.input.right_held = true;
                self.input.repeat = 0.0;
                match self.hovered {
                    Some(unit) => {
                        self.send(Command::Attack(unit));
                        self.voice("attack");
                        if let Some(game) = self.game.as_mut()
                            && let Some(target) =
                                game.units.iter().find(|candidate| candidate.id == unit)
                        {
                            game.click("AttackClick", Vec2::from(target.position));
                        }
                    }
                    None => {
                        if let Some(point) = self.ground_under(cursor) {
                            self.send(Command::Move(point.to_array()));
                            self.voice("move");
                            if let Some(game) = self.game.as_mut() {
                                game.click("MoveClick", point);
                            }
                        }
                    }
                }
            }
            LEFT_BUTTON if event.alt_key() => {
                if let Some(point) = self.ground_under(cursor) {
                    self.connection.send(&ClientMessage::Ping {
                        position: point.to_array(),
                        kind: if self.hovered.is_some() {
                            PingKind::Danger
                        } else {
                            PingKind::Generic
                        },
                    });
                }
            }
            LEFT_BUTTON if self.input.casting.is_some() => {
                if let Some(slot) = self.input.casting.take() {
                    self.input.charge = None;
                    let target = self.target();
                    self.cast(slot, target);
                }
            }
            LEFT_BUTTON if self.input.attack_move => {
                self.input.attack_move = false;
                match self.hovered {
                    Some(unit) => self.send(Command::Attack(unit)),
                    None => {
                        if let Some(point) = self.ground_under(cursor) {
                            self.send(Command::AttackMove(point.to_array()));
                            if let Some(game) = self.game.as_mut() {
                                game.click("AttackClick", point);
                            }
                        }
                    }
                }
            }
            LEFT_BUTTON => {
                let picked = self
                    .game
                    .as_ref()
                    .and_then(|game| self.unit_at_cursor(game, |unit| !unit.dead));
                if let Some(game) = self.game.as_mut() {
                    game.selected = picked;
                }
            }
            MIDDLE_BUTTON => {
                event.prevent_default();
                self.input.middle_drag = Some(cursor);
            }
            _ => {}
        }
    }

    fn minimap_down(&mut self, event: MouseEvent) {
        let on_minimap = self.screen == Screen::Game
            && self.game.as_ref().is_some_and(|game| {
                game.loading.is_none()
                    && event
                        .target()
                        .and_then(|target| target.dyn_into::<web_sys::Node>().ok())
                        .is_some_and(|target| game.overlay.minimap.contains(Some(&target)))
            });
        let Some(point) = self.minimap_point(&event).filter(|_| on_minimap) else {
            return;
        };
        event.prevent_default();
        match event.button() {
            LEFT_BUTTON if event.alt_key() => self.connection.send(&ClientMessage::Ping {
                position: point.to_array(),
                kind: PingKind::Generic,
            }),
            RIGHT_BUTTON => self.send(Command::Move(point.to_array())),
            LEFT_BUTTON => {
                self.camera.focus = point;
                self.input.minimap_drag = true;
            }
            _ => {}
        }
    }

    fn mouse_up(&mut self, event: MouseEvent) {
        match event.button() {
            RIGHT_BUTTON => self.input.right_held = false,
            MIDDLE_BUTTON => self.input.middle_drag = None,
            LEFT_BUTTON => self.input.minimap_drag = false,
            _ => {}
        }
    }

    fn mouse_move(&mut self, event: MouseEvent) {
        let rect = self.canvas.get_bounding_client_rect();
        let cursor = Vec2::new(
            (event.client_x() as f64 - rect.left()) as f32,
            (event.client_y() as f64 - rect.top()) as f32,
        );
        self.input.cursor = Some(cursor);
        if self.input.minimap_drag {
            if let Some(point) = self.minimap_point(&event) {
                self.camera.focus = point;
            }
            return;
        }
        if let Some(previous) = self.input.middle_drag
            && let (Some(from), Some(to)) = (self.ground_under(previous), self.ground_under(cursor))
        {
            self.input.middle_drag = Some(cursor);
            self.camera.locked = false;
            self.settings.borrow_mut().follow = false;
            self.settings.borrow().save();
            self.camera.focus += from - to;
        }
    }

    fn wheel(&mut self, event: WheelEvent) {
        event.prevent_default();
        self.camera.zoom(event.delta_y().signum() as f32);
    }

    fn my_team(&self) -> Option<Team> {
        self.game.as_ref().map(|game| game.team).or_else(|| {
            self.lobby
                .as_ref()?
                .players
                .iter()
                .find(|player| player.name == self.name)
                .map(|player| player.team)
        })
    }

    fn key_down(&mut self, event: KeyboardEvent) {
        if self.menu.is_open() {
            if event.code() == "Escape" {
                event.prevent_default();
                self.menu.set_open(false);
            }
            return;
        }
        if ui::is_typing(&event) {
            return;
        }
        let code = self.settings.borrow().code(&event.code());
        if code == "Enter" && matches!(self.screen, Screen::Lobby | Screen::Select | Screen::Game) {
            event.prevent_default();
            self.chat.open(event.shift_key());
            return;
        }
        if self.screen != Screen::Game {
            return;
        }
        if event.ctrl_key()
            && let Some(emote) = emote_for(&code)
        {
            event.prevent_default();
            self.send(Command::Emote(emote));
            return;
        }
        if let Some(slot) = item_slot_for(&code) {
            if event.repeat() {
                return;
            }
            let target = self.target();
            self.send(Command::UseItem(slot, target));
            return;
        }
        if let Some(slot) = slot_for(&code) {
            event.prevent_default();
            if event.repeat() {
                return;
            }
            let ready = self
                .game
                .as_ref()
                .is_none_or(|game| ability_ready(&self.data, game, slot));
            if event.ctrl_key() {
                if self.can_level(slot) {
                    self.send(Command::LevelUp(slot));
                    self.sound("spell_level_up");
                }
            } else if !ready {
                return;
            } else if self.game.as_ref().is_some_and(|game| {
                self.charges_up(game, slot)
                    && !game.ability(slot).is_some_and(|ability| ability.recast)
            }) {
                let target = self.target();
                self.cast(slot, target);
                self.input.charge = Some((slot, false));
                self.input.casting = Some(slot);
                self.input.attack_move = false;
            } else if self
                .game
                .as_ref()
                .is_some_and(|game| self.cast_shape(game, slot).is_none())
            {
                self.hovered = self.game.as_ref().and_then(|game| self.hovered_unit(game));
                let target = self.target();
                self.cast(slot, target);
            } else {
                self.input.casting = Some(slot);
                self.input.attack_move = false;
            }
            return;
        }
        match code.as_str() {
            "KeyA" => self.input.attack_move = true,
            "KeyS" | "KeyH" => self.send(Command::Stop),
            "KeyP" if !event.repeat() => {
                if let Some(game) = self.game.as_mut() {
                    game.hud.toggle_shop();
                    game.hud.hide_tooltip();
                }
            }
            "Escape" => {
                self.input.attack_move = false;
                match self.game.as_mut() {
                    Some(game) if game.hud.shop_open() => {
                        game.hud.toggle_shop();
                        game.hud.hide_tooltip();
                    }
                    _ => {
                        self.blur();
                        self.menu.set_open(true);
                    }
                }
            }
            "Tab" => {
                event.prevent_default();
                if let Some(game) = self.game.as_mut()
                    && game.scoreboard.is_none()
                {
                    game.scoreboard = Some(SCOREBOARD_REFRESH);
                    game.hud.show_scoreboard(true);
                }
            }
            "KeyY" if !event.repeat() => self.toggle_camera(),
            "KeyC" => {
                if let Some(game) = &self.game {
                    game.hud.advanced_stats(true);
                }
            }
            "Space" => {
                event.prevent_default();
                self.input.center_held = true;
            }
            code if arrow(code) != Vec2::ZERO
                && !self.input.arrows.iter().any(|held| held == code) =>
            {
                self.input.arrows.push(code.to_owned());
            }
            _ => {}
        }
    }

    fn toggle_camera(&mut self) {
        self.camera.locked = !self.camera.locked;
        self.settings.borrow_mut().follow = self.camera.locked;
        self.settings.borrow().save();
    }

    fn key_up(&mut self, event: KeyboardEvent) {
        if self.menu.is_open() {
            return;
        }
        let code = self.settings.borrow().code(&event.code());
        self.input.arrows.retain(|held| *held != code);
        let charge = slot_for(&code)
            .and_then(|slot| self.input.charge.take_if(|(charging, _)| *charging == slot));
        if let Some(slot) = slot_for(&code)
            && (self.input.casting == Some(slot) || charge.is_some())
        {
            self.input.casting = self.input.casting.filter(|casting| *casting != slot);
            let unanswered = charge.is_some_and(|(_, seen)| !seen);
            let ready = self
                .game
                .as_ref()
                .is_some_and(|game| ability_ready(&self.data, game, slot));
            if ui::is_typing(&event) || !(ready || unanswered) {
                return;
            }
            self.hovered = self.game.as_ref().and_then(|game| self.hovered_unit(game));
            let target = self.target();
            self.cast(slot, target);
            return;
        }
        match code.as_str() {
            "Space" => self.input.center_held = false,
            "KeyC" => {
                if let Some(game) = &self.game {
                    game.hud.advanced_stats(false);
                }
            }
            "Tab" => {
                if let Some(game) = self.game.as_mut().filter(|game| game.winner.is_none()) {
                    game.scoreboard = None;
                    game.hud.show_scoreboard(false);
                }
            }
            _ => {}
        }
    }

    fn edge_pan(&self) -> Vec2 {
        if self.menu.is_open() {
            return Vec2::ZERO;
        }
        let Some(cursor) = self.input.cursor else {
            return Vec2::ZERO;
        };
        let size = self.size();
        let axis = |position: f32, extent: f32| {
            if position < EDGE_MARGIN {
                -1.0
            } else if position > extent - EDGE_MARGIN {
                1.0
            } else {
                0.0
            }
        };
        Vec2::new(axis(cursor.x, size.x), -axis(cursor.y, size.y))
    }

    fn frame(&mut self, now: f64) {
        let dt = self
            .last_frame
            .map_or(0.0, |last| ((now - last) / 1000.0).min(0.1) as f32);
        self.last_frame = Some(now);
        self.handle_network(dt);
        self.handle_ui();
        self.camera.locked = self.settings.borrow().follow;
        if let Some(audio) = &self.audio {
            audio.volume(self.settings.borrow().volume);
        }
        self.chat.show(
            self.settings.borrow().chat && self.lobby.is_some() && self.screen != Screen::Login,
        );
        self.notice_time = (self.notice_time - dt).max(0.0);
        if self.vote_notice_time > 0.0 {
            self.vote_notice_time = (self.vote_notice_time - dt).max(0.0);
            if self.vote_notice_time == 0.0 {
                self.menu.vote(None);
            }
        }
        dom::show(&self.notice, self.notice_time > 0.0);
        dom::show(
            &self.loading_error,
            self.screen == Screen::Game
                && self.pending.get() == 0
                && !self.assets_ready()
                && !self.audio.as_ref().is_some_and(audio::Audio::loading),
        );
        self.receive_assets();
        self.report_loading();
        self.chat.resize(self.canvas.client_height() as f32);
        self.chat.update(dt);
        if self.screen == Screen::Select {
            self.select_view.tick(dt);
        }
        if self.screen == Screen::Game {
            self.update_game(dt);
        }
        self.render();
    }

    fn update_game(&mut self, dt: f32) {
        let Some(mut game) = self.game.take() else {
            return;
        };
        self.advance(&mut game, dt);
        if let Some(audio) = self.audio.as_mut() {
            audio.set_listener(self.camera.focus);
            for (key, position) in game.sounds.drain(..) {
                match position {
                    Some(position) => audio.play_at(&key, position),
                    None => audio.play(&key),
                }
            }
        } else {
            game.sounds.clear();
        }
        self.game = Some(game);
    }

    fn sound(&mut self, key: &str) {
        if let Some(audio) = self.audio.as_mut() {
            audio.play(key);
        }
    }

    fn cast(&mut self, slot: Slot, target: Target) {
        self.send(Command::Cast(slot, target));
        let index = SLOTS.iter().position(|candidate| *candidate == slot);
        let ready = self
            .game
            .as_ref()
            .and_then(|game| game.timeline.latest())
            .and_then(|snapshot| snapshot.you.as_ref())
            .zip(index)
            .is_some_and(|(you, index)| {
                you.abilities[index].rank > 0 && you.abilities[index].cooldown <= 0.0
            });
        let line = match slot {
            Slot::Q => "q",
            Slot::W => "w",
            Slot::E => "e",
            Slot::R => "r",
            Slot::D | Slot::F => return,
        };
        if ready {
            self.spell_voice(line, slot == Slot::R);
        }
    }

    fn spell_voice(&mut self, line: &str, always: bool) {
        let now = self.now();
        let Some(champion) = self
            .game
            .as_ref()
            .and_then(|game| game.you())
            .and_then(|you| champion_key(&self.data, you.kind))
        else {
            return;
        };
        let Some(audio) = self.audio.as_mut() else {
            return;
        };
        let Some(key) = [
            format!("{champion}.vo.{line}"),
            format!("{champion}.vo.spell"),
        ]
        .into_iter()
        .find(|key| audio.has(key)) else {
            return;
        };
        let chance = if always { 1.0 } else { SPELL_VOICE_CHANCE };
        if now - self.last_voice > SPELL_VOICE_COOLDOWN && js_sys::Math::random() < chance {
            self.last_voice = now;
            audio.play(&key);
        }
    }

    fn voice(&mut self, line: &str) {
        let now = self.now();
        let Some(champion) = self
            .game
            .as_ref()
            .and_then(|game| game.you())
            .and_then(|you| champion_key(&self.data, you.kind))
        else {
            return;
        };
        if now - self.last_voice > VOICE_COOLDOWN && js_sys::Math::random() < VOICE_CHANCE {
            self.last_voice = now;
            self.sound(&format!("{champion}.vo.{line}"));
        }
    }

    fn advance(&mut self, game: &mut Match, dt: f32) {
        if self.map_bounds.is_some() {
            game.bounds = self.map_bounds;
        }
        if let Some(frame) = game.timeline.frame(self.now()) {
            let previous = std::mem::take(&mut game.units);
            for unit in previous {
                if game.killed.contains(&unit.id)
                    && !matches!(unit.kind, UnitKind::Champion(_))
                    && !frame.units.iter().any(|current| current.id == unit.id)
                    && let Some(mut puppet) = game.puppets.remove(&unit.id)
                {
                    if let Some(model) = self.library.model(unit.kind, unit.team) {
                        puppet.die(&model.character);
                    }
                    game.corpses.push(Corpse {
                        unit,
                        puppet,
                        age: 0.0,
                    });
                }
            }
            game.killed
                .retain(|id| frame.units.iter().any(|unit| unit.id == *id));
            game.units = frame.units;
            game.missiles = frame.missiles;
            game.clock = (frame.tick / f64::from(TICK_RATE)) as f32;
        }
        {
            let library = &self.library;
            let interactables = game
                .timeline
                .latest()
                .map(|snapshot| snapshot.interactables.clone())
                .unwrap_or_default();
            game.clickers.retain(|id, _| interactables.contains(id));
            if let Some(model) = library.clicker.as_ref() {
                for unit in game
                    .units
                    .iter()
                    .filter(|unit| interactables.contains(&unit.id))
                {
                    let standing = UnitState {
                        dead: false,
                        ..unit.clone()
                    };
                    game.clickers
                        .entry(unit.id)
                        .or_insert_with(|| Puppet::new(&model.character, unit.id))
                        .update(&model.character, &standing, dt);
                }
            }
            let tick = game.timeline.latest().map_or(0, |snapshot| snapshot.tick);
            for unit in &game.units {
                if let Some(model) = library.model(unit.kind, unit.team) {
                    let started = game
                        .puppets
                        .entry(unit.id)
                        .or_insert_with(|| {
                            Puppet::new(&model.character, unit.id).joined(
                                &model.character,
                                unit,
                                tick,
                            )
                        })
                        .update(&model.character, unit, dt);
                    if let Some(clip) = started {
                        if let Some(key) = clip_sound(&self.data, unit.kind, clip) {
                            game.sounds.push((key, Some(Vec2::from(unit.position))));
                        }
                        if let Some(champion) = effect_key(&self.data, unit.kind) {
                            game.clips.push((
                                champion,
                                clip,
                                unit.id,
                                self.ground(Vec2::from(unit.position)),
                                unit.facing,
                            ));
                        }
                    }
                }
            }
            let units = &game.units;
            game.puppets
                .retain(|id, _| units.iter().any(|unit| unit.id == *id));
            for corpse in &mut game.corpses {
                corpse.age += dt;
                if let Some(model) = library.model(corpse.unit.kind, corpse.unit.team) {
                    let unit = UnitState {
                        dead: true,
                        moving: false,
                        ..corpse.unit.clone()
                    };
                    corpse.puppet.update(&model.character, &unit, dt);
                }
            }
        }
        game.corpses.retain(|corpse| corpse.age < CORPSE_TIME);
        if let Some(particles) = self.particles.as_mut() {
            for effect in game.effects.drain(..) {
                let position = Vec3::new(
                    effect.position.x,
                    self.heights.as_ref().map_or(0.0, |heights| {
                        heights.height_at(effect.position.x, effect.position.y)
                    }),
                    effect.position.y,
                );
                particles.effect(
                    effect.name,
                    position,
                    effect.facing,
                    effect.unit,
                    effect.hostile,
                );
            }
            for (champion, clip, unit, position, facing) in game.clips.drain(..) {
                particles.clip(&champion, clip, unit, position, facing);
            }
            for id in game.landed.drain(..) {
                particles.land(id);
            }
            game.idle
                .retain(|id| game.units.iter().any(|unit| unit.id == *id));
            for unit in &game.units {
                let Some(champion) = effect_key(&self.data, unit.kind) else {
                    continue;
                };
                let shown = game.idle.contains(&unit.id);
                if !unit.dead && !shown {
                    let [x, z] = unit.position;
                    let height = self
                        .heights
                        .as_ref()
                        .map_or(0.0, |heights| heights.height_at(x, z));
                    particles.idle(&champion, unit.id, Vec3::new(x, height, z), unit.facing);
                    game.idle.push(unit.id);
                } else if unit.dead && shown {
                    particles.end_unit(unit.id);
                    game.idle.retain(|id| *id != unit.id);
                }
            }
            if let Some(snapshot) = game.timeline.latest() {
                game.relic_glows.resize(snapshot.relics.len(), Vec::new());
                for (relic, glow) in snapshot.relics.iter().zip(&mut game.relic_glows) {
                    let ready = relic.ready_in <= 0.0;
                    if ready && glow.is_empty() {
                        let [x, z] = relic.position;
                        let height = self
                            .heights
                            .as_ref()
                            .map_or(0.0, |heights| heights.height_at(x, z));
                        *glow = particles.place(
                            clip_id(RELIC_GLOW),
                            Vec3::new(x, height, z),
                            0.0,
                            false,
                        );
                    } else if !ready && !glow.is_empty() {
                        particles.stop(glow);
                        glow.clear();
                    }
                }
                game.props.retain(|id, placed| {
                    let kept = snapshot.props.iter().any(|prop| prop.id == *id);
                    if !kept {
                        particles.stop(placed);
                    }
                    kept
                });
                for prop in &snapshot.props {
                    game.props.entry(prop.id).or_insert_with(|| {
                        let [x, z] = prop.position;
                        let height = self
                            .heights
                            .as_ref()
                            .map_or(0.0, |heights| heights.height_at(x, z));
                        particles.place(
                            prop.effect,
                            Vec3::new(x, height, z),
                            prop.facing,
                            prop.team != game.team,
                        )
                    });
                }
            }
            let heights = self.heights.as_ref();
            let units = &game.units;
            let puppets = &game.puppets;
            let library = &self.library;
            let ground = |position: Vec2| {
                Vec3::new(
                    position.x,
                    heights.map_or(0.0, |heights| heights.height_at(position.x, position.y)),
                    position.y,
                )
            };
            let bone_at = |id: UnitId, bone: Option<u32>| {
                let unit = units.iter().find(|unit| unit.id == id)?;
                let base = ground(Vec2::from(unit.position));
                let facing = Quat::from_rotation_y(unit.facing);
                let joint = bone.and_then(|bone| {
                    let model = library.model(unit.kind, unit.team)?;
                    let puppet = puppets.get(&id)?;
                    let joint = puppet.joint(&model.character, bone)?;
                    let (_, rotation, _) = joint.to_scale_rotation_translation();
                    let position = (Mat4::from_translation(base)
                        * Mat4::from_quat(facing)
                        * Mat4::from_scale(Vec3::splat(puppet.scale(&model.character))))
                    .transform_point3(joint.transform_point3(Vec3::ZERO));
                    Some((position, facing * rotation))
                });
                Some(joint.unwrap_or((base, facing)))
            };
            game.flights
                .retain(|id, _| game.missiles.iter().any(|missile| missile.id == *id));
            for missile in &game.missiles {
                let position = Vec2::from(missile.position);
                let [start_bone, target_bone] = particles.flight(missile.kind);
                let flight = game.flights.entry(missile.id).or_insert_with(|| {
                    let launch = start_bone
                        .and_then(|bone| bone_at(missile.owner, Some(bone)))
                        .map(|(point, _)| point)
                        .unwrap_or_else(|| ground(position) + Vec3::Y * missile.height);
                    Flight {
                        origin: position,
                        offset: launch - ground(position),
                    }
                });
                let end = missile.target.and_then(|target| {
                    let goal = units.iter().find(|unit| unit.id == target)?;
                    let height = target_bone
                        .and_then(|bone| bone_at(target, Some(bone)))
                        .map_or(missile.height, |(point, _)| {
                            point.y - ground(Vec2::from(goal.position)).y
                        });
                    Some((Vec2::from(goal.position), height))
                });
                let traveled = position.distance(flight.origin);
                let (progress, height) = match end {
                    Some((goal, height)) => {
                        let total = traveled + position.distance(goal);
                        ((traveled / total.max(1.0)).min(1.0), height)
                    }
                    None => ((traveled / LAUNCH_SETTLE).min(1.0), flight.offset.y),
                };
                let offset = Vec3::new(flight.offset.x, 0.0, flight.offset.z) * (1.0 - progress)
                    + Vec3::Y * (flight.offset.y + (height - flight.offset.y) * progress);
                particles.missile(
                    missile.id,
                    missile.kind,
                    ground(position) + offset,
                    missile.facing,
                );
            }
            particles.update(dt, bone_at);
        } else {
            game.effects.clear();
            game.clips.clear();
            game.landed.clear();
        }
        for ping in &mut game.pings {
            ping.age += dt;
        }
        game.pings.retain(|ping| ping.age < PING_TIME);
        let now = self.now();
        if game.loading.is_some()
            && self.assets_ready()
            && game.timeline.latest().is_some()
            && now - game.created > LOADING_MINIMUM
            && let Some(loading) = game.loading.take()
        {
            loading.remove();
        }
        let first_wave = game.setup.first_wave as f32;
        let stages = [
            (
                WELCOME_AT,
                "game_announcement_onstartmessage2map12",
                Some("announcer.welcome"),
            ),
            (
                first_wave - 30.0,
                "game_announcement_onstartmessage2",
                Some("announcer.thirty_seconds"),
            ),
            (
                first_wave,
                "game_announcement_onminionsspawn",
                Some("announcer.minions_spawned"),
            ),
        ];
        if let Some((at, text, audio)) = stages.get(usize::from(game.timeline_stage))
            && game.loading.is_none()
            && game.clock >= *at
        {
            game.timeline_stage += 1;
            if game.clock < at + STAGE_GRACE {
                game.hud.announce(
                    banner::Banner {
                        text: (*text).to_owned(),
                        style: announcer::Style::Minor,
                        tone: banner::Tone::Neutral,
                        left: None,
                        right: None,
                    },
                    None,
                    *audio,
                );
            }
        }
        if let Some(audio) = game.hud.update_announcements(dt) {
            game.sounds.push((audio.to_owned(), None));
        }
        if game.winner.is_some() {
            game.over += dt;
            if game.over > END_SCOREBOARD && game.scoreboard.is_none() {
                game.scoreboard = Some(SCOREBOARD_REFRESH);
                game.hud.banner(None);
                game.hud.show_scoreboard(true);
            }
        }
        if let Some(bounds) = &game.bounds {
            let sources: Vec<(Vec2, f32)> = game
                .units
                .iter()
                .filter(|unit| unit.team == game.team && !unit.dead)
                .map(|unit| (Vec2::from(unit.position), sight_range(unit.kind)))
                .collect();
            self.fog.update((bounds.min, bounds.max), &sources, dt);
            game.overlay.update_fog(self.fog.visibility());
        }

        let you = game.you().cloned();
        if let Some(you) = &you
            && !game.centered
        {
            game.centered = true;
            self.camera.focus = Vec2::from(you.position);
        }
        let dead = you.as_ref().is_some_and(|unit| unit.dead);
        if dead {
            self.input.casting = None;
            self.input.charge = None;
            self.input.right_held = false;
        }
        if let Some((slot, seen)) = self.input.charge.as_mut()
            && game.ability(*slot).is_some_and(|ability| ability.recast)
        {
            *seen = true;
        }
        self.set_grayscale(dead && game.winner.is_none());
        self.hovered = self.hovered_unit(game);

        if self.input.right_held {
            self.input.repeat += dt;
            if self.input.repeat >= MOVE_REPEAT {
                self.input.repeat = 0.0;
                if let Some(unit) = self.hovered {
                    self.send(Command::Attack(unit));
                } else if let Some(point) = self
                    .input
                    .cursor
                    .and_then(|cursor| self.ground_under(cursor))
                {
                    self.send(Command::Move(point.to_array()));
                }
            }
        }

        let follow = (self.camera.locked && !self.input.minimap_drag || self.input.center_held)
            .then(|| you.as_ref().map(|unit| Vec2::from(unit.position)))
            .flatten();
        match follow {
            Some(position) => self.camera.focus = position,
            None => {
                let held: Vec2 = self.input.arrows.iter().map(|code| arrow(code)).sum();
                let pan = (self.edge_pan() + held).clamp(Vec2::NEG_ONE, Vec2::ONE);
                self.camera.focus += pan * PAN_SPEED * dt;
            }
        }
        let shrines: Vec<Vec2> = game
            .units
            .iter()
            .filter(|unit| unit.kind == UnitKind::Shrine)
            .map(|unit| Vec2::from(unit.position))
            .collect();
        if let [blue, red] = shrines[..] {
            self.camera.focus = keep_on_lane(self.camera.focus, blue, red, LANE_VIEW);
        }
        let target_height = self.terrain(self.camera.focus);
        self.camera.height +=
            (target_height - self.camera.height) * (1.0 - (-CAMERA_HEIGHT_SMOOTHING * dt).exp());
        self.camera.update(dt);
        dom::set_class(
            self.canvas.unchecked_ref(),
            "attack-cursor",
            self.hovered.is_some() || self.input.attack_move,
        );

        let view_projection = self.camera.view_projection(self.aspect());
        let size = self.size();
        let view = [
            Vec2::ZERO,
            Vec2::new(size.x, 0.0),
            Vec2::new(0.0, size.y),
            size,
        ]
        .map(|corner| self.ground_under(corner).unwrap_or(self.camera.focus));
        let project =
            |position: Vec2, height: f32| self.project(&view_projection, position, height);
        let viewer = game.viewer();
        let names: Vec<(UnitId, String)> = game
            .setup
            .players
            .iter()
            .map(|player| (player.unit, player.name.clone()))
            .collect();
        game.overlay.update_bars(
            &game.units,
            &viewer,
            |id| {
                names
                    .iter()
                    .find(|(unit, _)| *unit == id && self.settings.borrow().names)
                    .map(|(_, name)| name.clone())
            },
            |kind| resource_of(&self.data, kind),
            project,
            game.clock,
        );
        if let Some(bounds) = &game.bounds {
            let data = &self.data;
            game.overlay
                .update_minimap(&game.units, bounds, &viewer, view, |kind| match kind {
                    UnitKind::Champion(index) => data
                        .champions
                        .get(usize::from(index))
                        .map(|champion| champion.icon.clone()),
                    _ => None,
                });
            let pings: Vec<(Vec2, bool, f32)> = game
                .pings
                .iter()
                .map(|ping| {
                    (
                        ping.position,
                        ping.kind == PingKind::Danger,
                        ping.age / PING_TIME,
                    )
                })
                .collect();
            game.overlay.update_pings(bounds, &pings);
        }
        game.hud.update_callouts(dt);
        game.hud.update_floating(dt, |position, height| {
            project(Vec2::from(position), height).map(|point| (point.x, point.y))
        });
        let selected = game
            .selected
            .and_then(|id| game.units.iter().find(|unit| unit.id == id))
            .filter(|unit| !unit.dead || matches!(unit.kind, UnitKind::Champion(_)));
        if selected.is_none() {
            game.selected = None;
        }
        let latest = game.timeline.latest();
        let score = |id: UnitId| {
            latest
                .and_then(|snapshot| snapshot.scores.iter().find(|(unit, _)| *unit == id))
                .map(|(_, score)| *score)
        };
        game.target.update(selected.map(|unit| {
            let score = score(unit.id);
            target::Target {
                unit,
                icon: match unit.kind {
                    UnitKind::Champion(index) => self
                        .data
                        .champions
                        .get(usize::from(index))
                        .map(|champion| champion.icon.clone()),
                    kind => {
                        assets::portrait_file(kind, unit.team).map(|file| format!("{file}.png"))
                    }
                },
                items: score
                    .map(|score| {
                        score
                            .items
                            .iter()
                            .map(|item| {
                                item.and_then(|id| self.data.item(id))
                                    .map(|item| item.icon.as_str())
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                score,
                resource: resource_of(&self.data, unit.kind),
                ally: unit.team == game.team,
            }
        }));
        let height = self.canvas.client_height() as f32;
        let settings = self.settings.borrow();
        game.hud.preferences(&settings);
        if let Some(rect) = game
            .hud
            .resize(self.canvas.client_width() as f32, height, settings.hud)
        {
            game.overlay.place_minimap(rect);
        }
        game.target.resize(height * settings.hud);
        game.hud.camera_locked(self.camera.locked);
        game.overlay.resize(height);
        if let Some(snapshot) = game.timeline.latest() {
            game.hud.update(snapshot, &game.setup, game.clock);
            if let Some(refresh) = game.scoreboard.as_mut() {
                *refresh += dt;
                if *refresh >= SCOREBOARD_REFRESH {
                    *refresh = 0.0;
                    game.hud.update_scoreboard(snapshot, &game.setup);
                }
            }
        }
    }

    fn render(&self) {
        let ratio = self.window.device_pixel_ratio();
        let width = (self.canvas.client_width() as f64 * ratio) as u32;
        let height = (self.canvas.client_height() as f64 * ratio) as u32;
        if self.canvas.width() != width || self.canvas.height() != height {
            self.canvas.set_width(width);
            self.canvas.set_height(height);
        }
        self.renderer.begin(width as i32, height as i32);
        let Some(game) = self.game.as_ref().filter(|_| self.screen == Screen::Game) else {
            return;
        };
        let view_projection = self.camera.view_projection(self.aspect());
        let decal = |decal: Decal, center: Vec2, radius: f32| {
            self.renderer
                .decal(&view_projection, decal, self.ground(center), radius);
        };
        match &self.map {
            Some(map) => {
                map.draw(&view_projection);
                if game.bounds.is_some() {
                    self.fog.draw(&view_projection, self.camera.height);
                }
            }
            None => decal(Decal::Ground, self.camera.focus, GROUND_RADIUS),
        }
        for unit in game.units.iter().filter(|unit| !unit.dead) {
            let position = Vec2::from(unit.position);
            let has = |name: &str| unit.buffs.contains(&clip_id(name));
            for (buff, decal_kind, radius) in [("mark", Decal::Ring(WIND, 0.1), 90.0)] {
                if has(buff) {
                    decal(decal_kind, position, radius);
                }
            }
        }
        if let Some(you) = game.you()
            && !you.dead
            && let UnitKind::Champion(index) = you.kind
            && let Some(champion) = self.data.champions.get(usize::from(index))
            && let Some(turret) = self.data.unit("Turret")
        {
            let reach = turret.stats.attack_range
                + turret.stats.gameplay_radius
                + champion.stats.gameplay_radius;
            let position = Vec2::from(you.position);
            for enemy in game.units.iter().filter(|unit| {
                unit.kind == UnitKind::Turret && unit.team != game.team && !unit.dead
            }) {
                let center = Vec2::from(enemy.position);
                let closeness = 1.0 - (center.distance(position) - reach) / TURRET_WARNING;
                if closeness > 0.0 {
                    decal(
                        Decal::Ring(RED * Vec4::new(1.0, 1.0, 1.0, closeness.min(1.0)), 0.012),
                        center,
                        reach,
                    );
                }
            }
        }
        if let Some(slot) = self.input.casting
            && let Some(you) = game.you()
            && let Some(shape) = self.cast_shape(game, slot)
        {
            let position = Vec2::from(you.position);
            let cursor = self
                .input
                .cursor
                .and_then(|cursor| self.ground_under(cursor))
                .unwrap_or(position);
            let offset = cursor - position;
            let direction = offset.normalize_or(Vec2::Y);
            match shape {
                Shape::Line(range, width) => {
                    decal(Decal::Arrow(INDICATOR, direction, width), position, range);
                }
                Shape::Cone(range, half_angle) => decal(
                    Decal::Cone(INDICATOR, direction, half_angle.to_radians()),
                    position,
                    range,
                ),
                Shape::Circle(_, radius) | Shape::Target(radius) if radius <= 0.0 => {}
                Shape::Circle(range, radius) => {
                    let target = position + offset.clamp_length_max(range);
                    decal(
                        Decal::Glow(INDICATOR * Vec4::new(1.0, 1.0, 1.0, 0.25)),
                        target,
                        radius,
                    );
                    decal(Decal::Ring(INDICATOR, 0.05), target, radius);
                }
                Shape::Target(_) => {}
            }
            if let Shape::Circle(range, _) | Shape::Target(range) = shape {
                decal(Decal::Ring(INDICATOR, 0.015), position, range);
            }
        }
        if self.input.casting.is_none()
            && let Some(you) = game.you()
            && let Some(ability) = game.hud.previewing().and_then(|slot| game.ability(slot))
            && ability.range > 0.0
        {
            decal(
                Decal::Ring(INDICATOR, 0.015),
                Vec2::from(you.position),
                ability.range,
            );
        }
        if self.input.attack_move
            && let Some(you) = game.you()
            && let UnitKind::Champion(index) = you.kind
            && let Some(champion) = self.data.champions.get(usize::from(index))
        {
            decal(
                Decal::Ring(Vec4::new(1.0, 1.0, 1.0, 0.35), 0.01),
                Vec2::from(you.position),
                champion.stats.attack_range + champion.stats.gameplay_radius,
            );
        }
        let library = &self.library;
        let opacity_of = |unit: &UnitState| {
            if unit.buffs.contains(&clip_id("camouflage")) {
                CAMOUFLAGE_OPACITY
            } else if unit.buffs.contains(&clip_id("brush")) {
                BRUSH_OPACITY
            } else {
                1.0
            }
        };
        let mut units: Vec<_> = game
            .units
            .iter()
            .filter_map(|unit| Some((unit, game.puppets.get(&unit.id)?)))
            .chain(
                game.corpses
                    .iter()
                    .map(|corpse| (&corpse.unit, &corpse.puppet)),
            )
            .collect();
        units.sort_by(|(a, _), (b, _)| opacity_of(b).total_cmp(&opacity_of(a)));
        let highlighted = if self.menu.is_open() {
            None
        } else {
            self.unit_at_cursor(game, |unit| !unit.dead)
        };
        for (unit, puppet) in units {
            if unit.dead && matches!(unit.kind, UnitKind::Turret | UnitKind::Shrine) {
                continue;
            }
            let Some(model) = library.seen_by(unit.kind, unit.team, game.team) else {
                continue;
            };
            let position = Vec2::from(unit.position);
            if !unit.dead && structure_radius(unit.kind) == 0.0 {
                decal(
                    Decal::Shadow,
                    position,
                    SHADOW_RADIUS * puppet.scale(&model.character).clamp(0.6, 1.8),
                );
            }
            let model_matrix = Mat4::from_translation(self.ground(position))
                * Mat4::from_rotation_y(unit.facing)
                * Mat4::from_scale(Vec3::splat(puppet.scale(&model.character)));
            let tint = if unit.buffs.contains(&clip_id("stasis")) {
                STASIS * Vec4::new(1.0, 1.0, 1.0, 0.85)
            } else {
                Vec4::ZERO
            };
            let skin = puppet.skin(&model.character);
            if highlighted == Some(unit.id) && !unit.dead {
                let color = if unit.team == game.team {
                    ALLY_OUTLINE
                } else {
                    ENEMY_OUTLINE
                };
                self.renderer
                    .outline(&model.mesh, &view_projection, &model_matrix, &skin, color);
            }
            let opacity = opacity_of(unit);
            self.renderer.draw(
                &model.mesh,
                &view_projection,
                &model_matrix,
                &skin,
                tint,
                opacity,
            );
        }
        if let Some(model) = library.clicker.as_ref() {
            for unit in &game.units {
                let Some(puppet) = game.clickers.get(&unit.id) else {
                    continue;
                };
                let model_matrix = Mat4::from_translation(self.ground(Vec2::from(unit.position)))
                    * Mat4::from_rotation_y(unit.facing)
                    * Mat4::from_scale(Vec3::splat(puppet.scale(&model.character)));
                let tint = if self.hovered == Some(unit.id) {
                    Vec4::new(1.0, 1.0, 1.0, 0.25)
                } else {
                    Vec4::ZERO
                };
                self.renderer.draw(
                    &model.mesh,
                    &view_projection,
                    &model_matrix,
                    &puppet.skin(&model.character),
                    tint,
                    1.0,
                );
            }
        }
        if let Some(particles) = &self.particles {
            let up = Vec3::new(0.0, camera::PITCH.cos(), camera::PITCH.sin());
            particles.draw(&view_projection, Vec3::X, up);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_camera_stays_within_a_band_along_the_lane() {
        let (from, to) = (Vec2::ZERO, Vec2::new(10_000.0, 10_000.0));
        let middle = Vec2::splat(5000.0);
        assert_eq!(keep_on_lane(middle, from, to, 1500.0), middle);
        let off = keep_on_lane(Vec2::new(9000.0, 1000.0), from, to, 1500.0);
        assert!((off - middle).length() <= 1500.0 + 0.01);
        let beyond = keep_on_lane(Vec2::splat(-3000.0), from, to, 1500.0);
        assert!(beyond.distance(from) < 0.01);
    }
}
