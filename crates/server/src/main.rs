mod accounts;
mod hall;
mod manifest;
mod sessions;
mod vote;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use accounts::Accounts;
use asset::data::GameData;
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use futures::{SinkExt, Stream, StreamExt};
use hall::Hall;
use protocol::{
    AuthError, ChampSelect, ClientMessage, Lobby, LobbyPlayer, Phase, SelectPlayer, ServerMessage,
    Summoner, TICK_RATE, Team, decode, encode,
};
use rand::Rng;
use sim::layout::Layout;
use sim::{Cheat, Entrant, Game};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{DefaultPredicate, NotForContentType, Predicate};
use tower_http::services::{ServeDir, ServeFile};
use vote::Vote;

const RETURN_TO_LOBBY: Duration = Duration::from_secs(12);
const SELECT_TIME: Duration = Duration::from_secs(60);
const LOCKED_DELAY: Duration = Duration::from_secs(3);
const SELECT_BROADCAST: Duration = Duration::from_secs(1);
const CHAT_LIMIT: usize = 200;
const ABANDONED: Duration = Duration::from_secs(30);
const SLOW_DOWNLOAD: Duration = Duration::from_secs(3);
const LOADING_LOG_STEP: u8 = 25;

struct Select {
    deadline: Instant,
    broadcast_at: Instant,
    players: Vec<SelectPlayer>,
    banning: bool,
    bans: Vec<(String, Option<u8>)>,
}

struct Loading {
    started: Instant,
    players: Vec<(String, u8)>,
}

struct Client {
    sender: UnboundedSender<Vec<u8>>,
    name: Option<String>,
}

struct Hub {
    accounts: Option<Accounts>,
    id: u64,
    name: String,
    vote: Option<Vote>,
    vote_cooldown: Option<Instant>,
    finished: Option<Vec<(String, bool)>>,
    sessions: sessions::Sessions,
    clients: HashMap<u64, Client>,
    lobby: Vec<(String, Team)>,
    select: Option<Select>,
    game: Option<Game>,
    game_over_at: Option<Instant>,
    loading: Option<Loading>,
    empty_since: Option<Instant>,
    data: Arc<GameData>,
    layout: Layout,
    summoners: HashMap<String, [Summoner; 2]>,
    next_client: u64,
    practice: bool,
}

type Shared = Arc<Mutex<Hall>>;

fn lock(hub: &Shared) -> MutexGuard<'_, Hall> {
    hub.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Options {
    port: u16,
    web: PathBuf,
    assets: Option<PathBuf>,
    data: PathBuf,
    practice: bool,
}

fn options() -> Options {
    let mut options = Options {
        port: 7777,
        web: PathBuf::from("web"),
        assets: None,
        data: PathBuf::from("data"),
        practice: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => {
                options.port = args
                    .next()
                    .and_then(|port| port.parse().ok())
                    .unwrap_or(options.port)
            }
            "--web" => options.web = args.next().map(PathBuf::from).unwrap_or(options.web),
            "--assets" => options.assets = args.next().map(PathBuf::from),
            "--data" => options.data = args.next().map(PathBuf::from).unwrap_or(options.data),
            "--practice" => options.practice = true,
            _ => {}
        }
    }
    options
}

fn load_layout(assets: &Path) -> Layout {
    sim::map::layout(assets).unwrap_or_else(|error| {
        eprintln!("using placeholder layout: {error}");
        Layout::placeholder()
    })
}

#[tokio::main]
async fn main() {
    let options = options();
    let assets = options
        .assets
        .clone()
        .unwrap_or_else(|| options.web.join("assets"));
    let bytes = std::fs::read(assets.join("data.bin"))
        .expect("data.bin is missing from the assets folder, run `just assets` first");
    let data = Arc::new(GameData::from_bytes(&bytes).expect("data.bin is corrupt"));
    let hub = Arc::new(Mutex::new(Hall::new(
        Accounts::load(options.data.join("accounts.json")),
        data,
        load_layout(&assets),
        options.practice,
    )));

    tokio::spawn(run_ticks(hub.clone()));

    let index = options.web.join("index.html");
    let listed = manifest::manifest(&assets);
    let app = Router::new()
        .route("/ws", get(upgrade))
        .route("/manifest", get(move || async move { listed }))
        .nest_service("/assets", ServeDir::new(&assets))
        .fallback_service(ServeDir::new(&options.web).fallback(ServeFile::new(index)))
        .layer(middleware::from_fn(cache_control))
        .layer(
            CompressionLayer::new()
                .compress_when(DefaultPredicate::new().and(NotForContentType::const_new("audio/"))),
        )
        .layer(middleware::from_fn(log_download))
        .with_state(hub);
    let address = SocketAddr::from(([0, 0, 0, 0], options.port));
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .expect("could not bind the port");
    println!("listening on http://{address}");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .expect("server failed");
}

struct Download {
    from: SocketAddr,
    path: String,
    status: StatusCode,
    started: Instant,
    size: Option<usize>,
    sent: usize,
    finished: bool,
}

struct Logged<S> {
    inner: S,
    download: Download,
}

impl<S: Stream<Item = Result<Bytes, axum::Error>> + Unpin> Stream for Logged<S> {
    type Item = S::Item;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<S::Item>> {
        let polled = self.inner.poll_next_unpin(context);
        match &polled {
            Poll::Ready(Some(Ok(bytes))) => {
                let download = &mut self.download;
                download.sent += bytes.len();
                download.finished |= download.size == Some(download.sent);
            }
            Poll::Ready(None) => self.download.finished = true,
            _ => {}
        }
        polled
    }
}

impl Drop for Download {
    fn drop(&mut self) {
        let Self {
            from,
            path,
            status,
            started,
            sent,
            finished,
            ..
        } = self;
        let megabytes = *sent as f64 / 1_000_000.0;
        let took = started.elapsed();
        let seconds = took.as_secs_f64();
        let code = status.as_u16();
        if !*finished {
            println!("{from} {path} {code} stopped after {megabytes:.1} MB in {seconds:.1}s");
        } else if !status.is_success() && *status != StatusCode::NOT_MODIFIED {
            println!("{from} {path} {code}");
        } else if took >= SLOW_DOWNLOAD || path == "/" {
            println!("{from} {path} {code} {megabytes:.1} MB in {seconds:.1}s");
        }
    }
}

async fn cache_control(request: Request, next: Next) -> Response {
    let versioned = request
        .uri()
        .query()
        .is_some_and(|query| query.starts_with("v="));
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(if versioned {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        }),
    );
    response
}

async fn log_download(
    ConnectInfo(from): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path().to_owned();
    if path == "/ws" || request.method() != Method::GET {
        return next.run(request).await;
    }
    let started = Instant::now();
    let response = next.run(request).await;
    let size = response
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|length| length.to_str().ok()?.parse().ok());
    let download = Download {
        from,
        path,
        status: response.status(),
        started,
        size,
        sent: 0,
        finished: size == Some(0) || response.status() == StatusCode::NOT_MODIFIED,
    };
    let (parts, body) = response.into_parts();
    let body = Logged {
        inner: body.into_data_stream(),
        download,
    };
    Response::from_parts(parts, Body::from_stream(body))
}

async fn upgrade(
    ConnectInfo(from): ConnectInfo<SocketAddr>,
    socket: WebSocketUpgrade,
    State(hub): State<Shared>,
) -> Response {
    socket.on_upgrade(move |socket| connect(socket, hub, from))
}

async fn connect(socket: WebSocket, hub: Shared, from: SocketAddr) {
    let (mut sink, mut stream) = socket.split();
    let (sender, mut receiver) = unbounded_channel::<Vec<u8>>();
    let id = lock(&hub).join(sender);
    println!("{from} connected");
    let writer = tokio::spawn(async move {
        while let Some(bytes) = receiver.recv().await {
            if sink.send(Message::Binary(bytes.into())).await.is_err() {
                break;
            }
        }
    });
    while let Some(Ok(message)) = stream.next().await {
        if let Message::Binary(bytes) = message
            && let Some(message) = decode::<ClientMessage>(&bytes)
        {
            let mut hub = lock(&hub);
            let before = hub.name_of(id);
            hub.handle(id, message);
            if before.is_none()
                && let Some(name) = hub.name_of(id)
            {
                println!("{from} logged in as {name}");
            }
        }
    }
    writer.abort();
    let mut hub = lock(&hub);
    let name = hub.name_of(id).unwrap_or_default();
    println!("{from} disconnected {name}");
    hub.leave(id);
}

async fn run_ticks(hub: Shared) {
    let mut interval = tokio::time::interval(Duration::from_secs_f64(1.0 / f64::from(TICK_RATE)));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        let ticked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lock(&hub).tick()));
        if ticked.is_err() {
            eprintln!("tick panicked, continuing");
        }
    }
}

impl Hub {
    fn new(accounts: Accounts, data: Arc<GameData>, layout: Layout, practice: bool) -> Self {
        Self::with_accounts(Some(accounts), data, layout, practice)
    }

    fn with_accounts(
        accounts: Option<Accounts>,
        data: Arc<GameData>,
        layout: Layout,
        practice: bool,
    ) -> Self {
        let sessions = accounts
            .as_ref()
            .map_or_else(sessions::Sessions::default, |accounts| {
                sessions::Sessions::load(accounts.sessions_path())
            });
        Self {
            accounts,
            id: 0,
            name: String::new(),
            vote: None,
            vote_cooldown: None,
            finished: None,
            sessions,
            clients: HashMap::new(),
            lobby: Vec::new(),
            select: None,
            game: None,
            game_over_at: None,
            loading: None,
            empty_since: None,
            data,
            layout,
            summoners: HashMap::new(),
            next_client: 0,
            practice,
        }
    }

    fn join(&mut self, sender: UnboundedSender<Vec<u8>>) -> u64 {
        self.next_client += 1;
        self.clients
            .insert(self.next_client, Client { sender, name: None });
        self.next_client
    }

    fn leave(&mut self, client: u64) {
        self.clients.remove(&client);
        self.prune_lobby();
        self.broadcast_lobby();
    }

    fn prune_lobby(&mut self) {
        if self.phase() != Phase::Lobby {
            return;
        }
        let online: Vec<String> = self
            .clients
            .values()
            .filter_map(|client| client.name.clone())
            .collect();
        self.lobby.retain(|(name, _)| online.contains(name));
    }

    fn send(&self, client: u64, message: &ServerMessage) {
        if let Some(client) = self.clients.get(&client) {
            let _ = client.sender.send(encode(message));
        }
    }

    fn online(&self, name: &str) -> bool {
        self.clients
            .values()
            .any(|client| client.name.as_deref() == Some(name))
    }

    fn lobby(&self) -> Lobby {
        Lobby {
            id: self.id,
            name: self.name.clone(),
            host: self.host(),
            players: self
                .lobby
                .iter()
                .map(|(name, team)| LobbyPlayer {
                    name: name.clone(),
                    team: *team,
                    online: self.online(name),
                })
                .collect(),
            phase: self.phase(),
        }
    }

    fn broadcast_lobby(&self) {
        let message = encode(&ServerMessage::Lobby(self.lobby()));
        for client in self.clients.values().filter(|client| client.name.is_some()) {
            let _ = client.sender.send(message.clone());
        }
    }

    fn handle(&mut self, client: u64, message: ClientMessage) {
        match message {
            ClientMessage::SignUp { name, password } => {
                let Some(accounts) = self.accounts.as_mut() else {
                    return;
                };
                let result = accounts.sign_up(&name, &password);
                self.authenticated(client, result.map(|()| name));
            }
            ClientMessage::LogIn { name, password } => {
                let Some(accounts) = self.accounts.as_ref() else {
                    return;
                };
                let result = accounts.log_in(&name, &password);
                let name = self.canonical_name(&name);
                self.authenticated(client, result.map(|()| name));
            }
            ClientMessage::Resume { token } => {
                let result = self
                    .sessions
                    .take(&token)
                    .ok_or(AuthError::WrongNameOrPassword);
                self.authenticated(client, result);
            }
            ClientMessage::JoinTeam(team) => {
                if self.phase() == Phase::Lobby
                    && self.lobby.iter().filter(|(_, side)| *side == team).count() < 5
                    && let Some(name) = self.name_of(client)
                    && let Some(entry) = self.lobby.iter_mut().find(|(entry, _)| *entry == name)
                {
                    entry.1 = team;
                    self.broadcast_lobby();
                }
            }
            ClientMessage::Start => {
                if self.name_of(client).is_some_and(|name| name == self.host()) {
                    self.begin_select();
                }
            }
            ClientMessage::Pick(champion) => {
                if let Some(name) = self.name_of(client) {
                    self.pick(&name, champion);
                }
            }
            ClientMessage::LockIn => {
                if let Some(name) = self.name_of(client) {
                    self.lock_in(&name);
                }
            }
            ClientMessage::Summoners(summoners) => {
                if let Some(name) = self.name_of(client) {
                    self.choose_summoners(&name, summoners);
                }
            }
            ClientMessage::Chat { text, all } => {
                let Some(name) = self.name_of(client) else {
                    return;
                };
                match Cheat::parse(&text).filter(|_| self.practice) {
                    Some(cheat) => {
                        if let Some(game) = self.game.as_mut()
                            && let Some(seat) = game.seats.iter().find(|seat| seat.name == name)
                        {
                            let unit = seat.unit;
                            game.cheat(unit, cheat);
                        }
                    }
                    None => self.chat(name, text, all),
                }
            }
            ClientMessage::Ping { position, kind } => {
                if let Some(name) = self.name_of(client)
                    && let Some(team) = self.team_of(&name)
                {
                    let message = encode(&ServerMessage::Ping {
                        from: name,
                        position,
                        kind,
                    });
                    self.send_team(team, &message);
                }
            }
            ClientMessage::Loading(percent) => {
                if let Some(name) = self.name_of(client) {
                    self.loaded(&name, percent.min(100));
                }
            }
            ClientMessage::EndGame => {
                self.start_vote(client, protocol::VoteKind::EndGame);
            }
            ClientMessage::StartVote(kind) => self.start_vote(client, kind),
            ClientMessage::Vote(yes) => self.cast_vote(client, yes),
            ClientMessage::Ban(champion) => {
                if let Some(name) = self.name_of(client) {
                    self.ban(&name, champion);
                }
            }
            ClientMessage::Command(command) => {
                if let Some(name) = self.name_of(client)
                    && let Some(game) = self.game.as_mut()
                    && let Some(seat) = game.seats.iter().find(|seat| seat.name == name)
                {
                    let unit = seat.unit;
                    game.command(unit, command);
                }
            }
            _ => {}
        }
    }

    fn canonical_name(&self, name: &str) -> String {
        if let Some(display) = self
            .accounts
            .as_ref()
            .and_then(|accounts| accounts.display_name(name))
        {
            return display;
        }
        self.lobby
            .iter()
            .find(|(entry, _)| entry.eq_ignore_ascii_case(name))
            .map_or_else(|| name.to_owned(), |(entry, _)| entry.clone())
    }

    fn name_of(&self, client: u64) -> Option<String> {
        self.clients
            .get(&client)
            .and_then(|client| client.name.clone())
    }

    fn authenticated(&mut self, client: u64, result: Result<String, AuthError>) {
        let name = match result {
            Ok(name) => name,
            Err(error) => {
                self.send(client, &ServerMessage::AuthFailed(error));
                return;
            }
        };
        let token = match self.sessions.issue(&name) {
            Ok(token) => token,
            Err(_) => {
                self.send(client, &ServerMessage::AuthFailed(AuthError::Unavailable));
                return;
            }
        };
        self.send(
            client,
            &ServerMessage::Session {
                token,
                name: name.clone(),
            },
        );
        self.attach(client, name);
    }

    fn attach(&mut self, client: u64, name: String) {
        if let Some(entry) = self.clients.get_mut(&client) {
            entry.name = Some(name.clone());
        }
        if !self.lobby.iter().any(|(entry, _)| *entry == name) {
            let count = |side: Team| {
                self.lobby
                    .iter()
                    .filter(|(entry, team)| *team == side && self.online(entry))
                    .count()
            };
            let (blue, red) = (count(Team::Blue), count(Team::Red));
            let team = if blue <= red { Team::Blue } else { Team::Red };
            self.lobby.push((name.clone(), team));
        }
        self.broadcast_lobby();
        if let Some(game) = self.game.as_ref()
            && let Some(setup) = game.setup_for(&name)
        {
            let winner = game.winner;
            self.send(client, &ServerMessage::GameStarted(setup));
            let loaded = match &self.loading {
                Some(loading) => loading.players.clone(),
                None => game
                    .seats
                    .iter()
                    .map(|seat| (seat.name.clone(), 100))
                    .collect(),
            };
            self.send(client, &ServerMessage::Loading(loaded));
            if let Some(winner) = winner {
                self.send(client, &ServerMessage::GameOver { winner });
            }
        }
        if let Some(select) = self.champ_select() {
            self.send(client, &ServerMessage::ChampSelect(select));
        }
        self.broadcast_vote();
    }

    fn playing(&self, name: &str) -> bool {
        self.game
            .as_ref()
            .is_some_and(|game| game.seats.iter().any(|seat| seat.name == name))
            || self
                .select
                .as_ref()
                .is_some_and(|select| select.players.iter().any(|player| player.name == name))
    }

    fn host(&self) -> String {
        self.lobby
            .iter()
            .find(|(name, _)| self.online(name))
            .map(|(name, _)| name.clone())
            .unwrap_or_default()
    }

    fn phase(&self) -> Phase {
        if self.game.is_some() {
            Phase::Game
        } else if self.select.is_some() {
            Phase::Select
        } else {
            Phase::Lobby
        }
    }

    fn team_of(&self, name: &str) -> Option<Team> {
        self.game
            .as_ref()
            .and_then(|game| game.seats.iter().find(|seat| seat.name == name))
            .map(|seat| seat.team)
            .or_else(|| {
                self.lobby
                    .iter()
                    .find(|(entry, _)| entry == name)
                    .map(|(_, team)| *team)
            })
    }

    fn send_team(&self, team: Team, message: &[u8]) {
        for client in self.clients.values() {
            if client
                .name
                .as_deref()
                .is_some_and(|name| self.team_of(name) == Some(team))
            {
                let _ = client.sender.send(message.to_vec());
            }
        }
    }

    fn send_named(&self, message: &[u8]) {
        for client in self.clients.values().filter(|client| client.name.is_some()) {
            let _ = client.sender.send(message.to_vec());
        }
    }

    fn chat(&self, from: String, text: String, all: bool) {
        let text: String = text.trim().chars().take(CHAT_LIMIT).collect();
        let Some(team) = self.team_of(&from) else {
            return;
        };
        if text.is_empty() {
            return;
        }
        let message = encode(&ServerMessage::Chat {
            from,
            team,
            text,
            all,
        });
        if all || self.phase() == Phase::Lobby {
            self.send_named(&message);
        } else {
            self.send_team(team, &message);
        }
    }

    fn champ_select(&self) -> Option<ChampSelect> {
        self.select.as_ref().map(|select| ChampSelect {
            players: select.players.clone(),
            banning: select.banning,
            bans: select.bans.clone(),
            remaining: select
                .deadline
                .saturating_duration_since(Instant::now())
                .as_secs_f32(),
        })
    }

    fn broadcast_select(&mut self) {
        if let Some(select) = self.champ_select() {
            self.send_named(&encode(&ServerMessage::ChampSelect(select)));
        }
        if let Some(select) = self.select.as_mut() {
            select.broadcast_at = Instant::now() + SELECT_BROADCAST;
        }
    }

    fn begin_select(&mut self) {
        if self.game.is_some() || self.select.is_some() {
            return;
        }
        let players: Vec<SelectPlayer> = self
            .lobby
            .iter()
            .filter(|(name, _)| self.online(name))
            .map(|(name, team)| SelectPlayer {
                name: name.clone(),
                team: *team,
                champion: None,
                locked: false,
                summoners: self
                    .summoners
                    .get(name)
                    .copied()
                    .unwrap_or(Summoner::DEFAULT),
            })
            .collect();
        if players.is_empty() {
            return;
        }
        let now = Instant::now();
        self.select = Some(Select {
            deadline: now + Duration::from_secs(20),
            broadcast_at: now,
            players,
            banning: true,
            bans: Vec::new(),
        });
        self.broadcast_lobby();
        self.broadcast_select();
    }

    fn choose_summoners(&mut self, name: &str, summoners: [Summoner; 2]) {
        if summoners[0] == summoners[1] {
            return;
        }
        let Some(player) = self
            .select
            .as_mut()
            .and_then(|select| select.players.iter_mut().find(|player| player.name == name))
        else {
            return;
        };
        player.summoners = summoners;
        self.summoners.insert(name.to_owned(), summoners);
        self.broadcast_select();
    }

    fn pick(&mut self, name: &str, champion: u8) {
        let Some(select) = self.select.as_mut() else {
            return;
        };
        if select.banning
            || select.bans.iter().any(|(_, ban)| *ban == Some(champion))
            || usize::from(champion) >= self.data.champions.len()
            || !available(&select.players, name, champion)
        {
            return;
        }
        if let Some(player) = select
            .players
            .iter_mut()
            .find(|player| player.name == name && !player.locked)
        {
            player.champion = Some(champion);
            self.broadcast_select();
        }
    }

    fn lock_in(&mut self, name: &str) {
        let Some(select) = self.select.as_mut() else {
            return;
        };
        let Some(champion) = select
            .players
            .iter()
            .find(|player| player.name == name)
            .and_then(|player| player.champion)
        else {
            return;
        };
        if select.banning
            || select.bans.iter().any(|(_, ban)| *ban == Some(champion))
            || !available(&select.players, name, champion)
        {
            return;
        }
        if let Some(player) = select.players.iter_mut().find(|player| player.name == name) {
            player.locked = true;
        }
        if select.players.iter().all(|player| player.locked) {
            select.deadline = select.deadline.min(Instant::now() + LOCKED_DELAY);
        }
        self.broadcast_select();
    }

    fn update_select(&mut self) {
        let Some(select) = self.select.as_ref() else {
            return;
        };
        let now = Instant::now();
        if now >= select.deadline {
            if select.banning {
                self.begin_picks();
            } else {
                self.start();
            }
        } else if now >= select.broadcast_at {
            self.broadcast_select();
        }
    }

    fn ban(&mut self, name: &str, champion: Option<u8>) {
        let Some(select) = self.select.as_mut() else {
            return;
        };
        if !select.banning
            || !select.players.iter().any(|player| player.name == name)
            || select.bans.iter().any(|(player, _)| player == name)
            || champion.is_some_and(|champion| usize::from(champion) >= self.data.champions.len())
        {
            return;
        }
        let largest_team = [Team::Blue, Team::Red]
            .into_iter()
            .map(|team| {
                select
                    .players
                    .iter()
                    .filter(|player| player.team == team)
                    .count()
            })
            .max()
            .unwrap_or(1);
        let distinct: std::collections::HashSet<u8> =
            select.bans.iter().filter_map(|(_, ban)| *ban).collect();
        if champion.is_some_and(|champion| {
            !distinct.contains(&champion)
                && distinct.len() + largest_team >= self.data.champions.len()
        }) {
            return;
        }
        select.bans.push((name.to_owned(), champion));
        if select.bans.len() == select.players.len() {
            self.begin_picks();
        } else {
            self.broadcast_select();
        }
    }

    fn begin_picks(&mut self) {
        if let Some(select) = &mut self.select {
            select.banning = false;
            select.deadline = Instant::now() + SELECT_TIME;
        }
        self.broadcast_select();
    }

    fn start(&mut self) {
        let Some(select) = self.select.take() else {
            return;
        };
        let order: Vec<String> = select
            .players
            .iter()
            .map(|player| player.name.clone())
            .collect();
        let mut picks: Vec<SelectPlayer> = Vec::new();
        let (locked, hovering): (Vec<SelectPlayer>, Vec<SelectPlayer>) =
            select.players.into_iter().partition(|player| player.locked);
        for player in locked.into_iter().chain(hovering) {
            let free = |champion: &u8| {
                !select.bans.iter().any(|(_, ban)| *ban == Some(*champion))
                    && picks
                        .iter()
                        .all(|other| other.team != player.team || other.champion != Some(*champion))
            };
            let champion = player.champion.filter(free).or_else(|| {
                let free: Vec<u8> = (0..self.data.champions.len() as u8).filter(free).collect();
                (!free.is_empty()).then(|| free[rand::rng().random_range(0..free.len())])
            });
            picks.push(SelectPlayer {
                champion,
                locked: true,
                ..player
            });
        }
        picks.sort_by_key(|player| order.iter().position(|name| *name == player.name));
        let players: Vec<Entrant> = picks
            .into_iter()
            .map(|player| Entrant {
                name: player.name,
                team: player.team,
                champion: player.champion,
                summoners: player.summoners,
            })
            .collect();
        let seed = rand::rng().random();
        let game = Game::new(self.data.clone(), self.layout.clone(), &players, seed);
        for (id, client) in &self.clients {
            if let Some(setup) = client.name.as_deref().and_then(|name| game.setup_for(name)) {
                self.send(*id, &ServerMessage::GameStarted(setup));
            }
        }
        self.loading = Some(Loading {
            started: Instant::now(),
            players: game
                .seats
                .iter()
                .map(|seat| (seat.name.clone(), 0))
                .collect(),
        });
        self.game = Some(game);
        self.game_over_at = None;
        self.broadcast_lobby();
    }

    fn loaded(&mut self, name: &str, percent: u8) {
        let Some(loading) = self.loading.as_mut() else {
            return;
        };
        let Some(entry) = loading
            .players
            .iter_mut()
            .find(|(player, _)| player == name)
        else {
            return;
        };
        if entry.1 == percent {
            return;
        }
        if percent / LOADING_LOG_STEP != entry.1 / LOADING_LOG_STEP {
            println!(
                "{name} loaded {percent}% after {}s",
                loading.started.elapsed().as_secs()
            );
        }
        entry.1 = percent;
        let message = encode(&ServerMessage::Loading(loading.players.clone()));
        if loading.players.iter().all(|(_, percent)| *percent == 100) {
            println!("everyone loaded, the match begins");
            self.loading = None;
        }
        self.send_named(&message);
    }

    fn back_to_lobby(&mut self) {
        self.game = None;
        self.vote = None;
        self.vote_cooldown = None;
        self.send_named(&encode(&ServerMessage::Vote(None)));
        self.loading = None;
        self.select = None;
        self.game_over_at = None;
        self.empty_since = None;
        self.prune_lobby();
        self.broadcast_lobby();
    }

    fn end_if_abandoned(&mut self) {
        let Some(game) = self.game.as_ref() else {
            self.empty_since = None;
            return;
        };
        if game.seats.iter().any(|seat| self.online(&seat.name)) {
            self.empty_since = None;
            return;
        }
        let since = *self.empty_since.get_or_insert_with(Instant::now);
        if since.elapsed() >= ABANDONED {
            self.back_to_lobby();
        }
    }

    fn tick(&mut self) {
        self.update_select();
        self.update_vote();
        self.end_if_abandoned();
        let Some(game) = self.game.as_mut() else {
            return;
        };
        if self.loading.is_some() {
            return;
        }
        game.step();
        let winner = game.winner;
        let game = self.game.as_ref().expect("game exists");
        for client in self.clients.values() {
            let Some(name) = client.name.as_deref() else {
                continue;
            };
            let Some(seat) = game.seats.iter().find(|seat| seat.name == name) else {
                continue;
            };
            let _ = client.sender.send(encode(&ServerMessage::Snapshot(Box::new(
                game.snapshot(Some(seat.unit)),
            ))));
        }
        if let Some(game) = self.game.as_mut() {
            game.clear_events();
        }
        if let Some(winner) = winner {
            match self.game_over_at {
                None => {
                    self.game_over_at = Some(Instant::now());
                    self.vote = None;
                    self.send_named(&encode(&ServerMessage::Vote(None)));
                    if !self.practice
                        && let Some(game) = &self.game
                        && [Team::Blue, Team::Red]
                            .iter()
                            .all(|team| game.seats.iter().any(|seat| seat.team == *team))
                    {
                        self.finished = Some(
                            game.seats
                                .iter()
                                .map(|seat| (seat.name.clone(), seat.team == winner))
                                .collect(),
                        );
                    }
                    let message = encode(&ServerMessage::GameOver { winner });
                    for client in self.clients.values() {
                        let _ = client.sender.send(message.clone());
                    }
                }
                Some(at) if at.elapsed() > RETURN_TO_LOBBY => self.back_to_lobby(),
                Some(_) => {}
            }
        }
    }
}

fn available(players: &[SelectPlayer], name: &str, champion: u8) -> bool {
    let Some(team) = players
        .iter()
        .find(|player| player.name == name)
        .map(|player| player.team)
    else {
        return true;
    };
    !players.iter().any(|player| {
        player.name != name
            && player.team == team
            && player.locked
            && player.champion == Some(champion)
    })
}

#[cfg(test)]
mod tests {
    use asset::data::ChampionData;
    use tokio::sync::mpsc::UnboundedReceiver;

    use super::*;

    pub(super) fn hub() -> Hub {
        let champion = |name: &str| ChampionData {
            name: name.into(),
            ..ChampionData::default()
        };
        let data = GameData {
            champions: ["Emperor", "Swordsman", "Sniper"]
                .into_iter()
                .map(champion)
                .collect(),
            ..GameData::default()
        };
        let accounts = Accounts::load(
            std::env::temp_dir().join(format!("lll-hub-test-{}.json", rand::random::<u64>())),
        );
        Hub::new(accounts, Arc::new(data), Layout::placeholder(), false)
    }

    fn player(hub: &mut Hub, name: &str) -> (u64, UnboundedReceiver<Vec<u8>>) {
        let (sender, receiver) = unbounded_channel();
        let id = hub.join(sender);
        hub.authenticated(id, Ok(name.to_owned()));
        (id, receiver)
    }

    #[test]
    fn teammates_never_start_with_the_same_champion() {
        let mut hub = hub();
        for name in ["a", "b", "c", "d"] {
            player(&mut hub, name);
        }
        hub.begin_select();
        hub.begin_picks();
        hub.pick("c", 0);
        hub.pick("a", 0);
        hub.lock_in("a");
        hub.start();
        let game = hub.game.as_ref().unwrap();
        let champion = |name: &str| {
            game.seats
                .iter()
                .find(|seat| seat.name == name)
                .map(|seat| seat.champion)
                .unwrap()
        };
        assert_eq!(champion("a"), 0);
        assert_ne!(champion("c"), 0);
    }

    #[test]
    fn a_game_everyone_left_ends_after_a_while() {
        let mut hub = hub();
        let (a, _) = player(&mut hub, "a");
        hub.begin_select();
        hub.start();
        hub.tick();
        assert!(hub.game.is_some());
        hub.clients.remove(&a);
        hub.tick();
        assert!(hub.game.is_some());
        hub.empty_since = Some(Instant::now() - ABANDONED);
        hub.tick();
        assert!(hub.game.is_none());
        assert_eq!(hub.phase(), Phase::Lobby);
    }

    #[test]
    fn players_who_leave_the_lobby_free_their_team_slot() {
        let mut hub = hub();
        player(&mut hub, "a");
        let (b, _) = player(&mut hub, "b");
        hub.leave(b);
        assert_eq!(hub.lobby.len(), 1);
        player(&mut hub, "c");
        let teams: Vec<Team> = hub.lobby.iter().map(|(_, team)| *team).collect();
        assert_eq!(teams, vec![Team::Blue, Team::Red]);
    }

    pub(super) fn chats(receiver: &mut UnboundedReceiver<Vec<u8>>) -> usize {
        std::iter::from_fn(|| receiver.try_recv().ok())
            .filter(|bytes| {
                matches!(
                    decode::<ServerMessage>(bytes),
                    Some(ServerMessage::Chat { .. })
                )
            })
            .count()
    }

    #[test]
    fn team_chat_in_champion_select_stays_on_the_team() {
        let mut hub = hub();
        let (a, mut blue) = player(&mut hub, "a");
        let (_, mut red) = player(&mut hub, "b");
        hub.begin_select();
        hub.handle(
            a,
            ClientMessage::Chat {
                text: "mid".into(),
                all: false,
            },
        );
        assert_eq!(chats(&mut blue), 1);
        assert_eq!(chats(&mut red), 0);
    }

    #[test]
    fn only_logged_in_players_can_start_champion_select() {
        let mut hub = hub();
        player(&mut hub, "a");
        let (sender, _receiver) = unbounded_channel();
        let stranger = hub.join(sender);
        hub.handle(stranger, ClientMessage::Start);
        assert_eq!(hub.phase(), Phase::Lobby);
    }

    #[test]
    fn teams_are_fixed_once_champion_select_starts() {
        let mut hub = hub();
        let (a, _) = player(&mut hub, "a");
        player(&mut hub, "b");
        hub.begin_select();
        hub.handle(a, ClientMessage::JoinTeam(Team::Red));
        assert_eq!(hub.team_of("a"), Some(Team::Blue));
    }

    #[test]
    fn reconnecting_after_the_nexus_falls_still_shows_the_result() {
        let mut hub = hub();
        let (a, _) = player(&mut hub, "a");
        hub.begin_select();
        hub.start();
        hub.game.as_mut().unwrap().winner = Some(Team::Blue);
        hub.leave(a);
        let (_, mut receiver) = player(&mut hub, "a");
        let over = std::iter::from_fn(|| receiver.try_recv().ok()).any(|bytes| {
            matches!(
                decode::<ServerMessage>(&bytes),
                Some(ServerMessage::GameOver { winner: Team::Blue })
            )
        });
        assert!(over);
    }

    #[test]
    fn ending_a_match_requires_every_player_and_rejects_outsiders() {
        let mut hub = hub();
        let (a, _) = player(&mut hub, "a");
        let (b, _) = player(&mut hub, "b");
        hub.begin_select();
        let (late, _) = player(&mut hub, "late");
        hub.handle(late, ClientMessage::EndGame);
        assert_eq!(hub.phase(), Phase::Select);
        hub.handle(a, ClientMessage::EndGame);
        assert_eq!(hub.phase(), Phase::Select);
        hub.start();
        hub.handle(late, ClientMessage::EndGame);
        assert_eq!(hub.phase(), Phase::Game);
        hub.handle(a, ClientMessage::EndGame);
        assert_eq!(hub.phase(), Phase::Game);
        hub.handle(late, ClientMessage::Vote(true));
        hub.handle(a, ClientMessage::Vote(true));
        assert_eq!(hub.phase(), Phase::Game);
        hub.handle(b, ClientMessage::Vote(true));
        assert_eq!(hub.phase(), Phase::Lobby);
        assert!(hub.finished.is_none());
    }

    #[test]
    fn surrender_counts_only_teammates_and_records_one_result() {
        let mut hub = hub();
        let (a, _) = player(&mut hub, "a");
        let (b, _) = player(&mut hub, "b");
        let (c, _) = player(&mut hub, "c");
        hub.begin_select();
        hub.start();
        hub.loading = None;
        hub.handle(a, ClientMessage::StartVote(protocol::VoteKind::Surrender));
        hub.handle(b, ClientMessage::Vote(true));
        assert!(hub.game.as_ref().unwrap().winner.is_none());
        hub.handle(c, ClientMessage::Vote(true));
        assert_eq!(hub.game.as_ref().unwrap().winner, Some(Team::Red));
        hub.tick();
        let result = hub.finished.take().unwrap();
        assert_eq!(
            result
                .iter()
                .filter(|(_, won)| *won)
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            vec!["b"]
        );
        hub.tick();
        assert!(hub.finished.is_none());
    }

    #[test]
    fn bans_apply_to_manual_and_automatic_picks_and_leave_enough_champions() {
        let mut hub = hub();
        player(&mut hub, "a");
        player(&mut hub, "b");
        player(&mut hub, "c");
        hub.begin_select();
        hub.pick("a", 0);
        assert!(hub.select.as_ref().unwrap().players[0].champion.is_none());
        hub.ban("a", Some(0));
        hub.ban("a", Some(1));
        hub.ban("b", Some(1));
        assert_eq!(
            hub.select.as_ref().unwrap().bans,
            vec![("a".into(), Some(0))]
        );
        hub.ban("b", None);
        hub.ban("c", None);
        assert!(!hub.select.as_ref().unwrap().banning);
        hub.pick("a", 0);
        assert!(hub.select.as_ref().unwrap().players[0].champion.is_none());
        hub.start();
        assert!(
            hub.game
                .as_ref()
                .unwrap()
                .seats
                .iter()
                .all(|seat| seat.champion != 0)
        );
    }

    #[test]
    fn only_host_starts_and_team_capacity_is_enforced() {
        let mut hub = hub();
        let (host, _) = player(&mut hub, "host");
        let (guest, _) = player(&mut hub, "guest");
        hub.handle(guest, ClientMessage::Start);
        assert_eq!(hub.phase(), Phase::Lobby);
        for index in 0..8 {
            player(&mut hub, &format!("player{index}"));
        }
        hub.handle(guest, ClientMessage::JoinTeam(Team::Blue));
        assert_eq!(hub.team_of("guest"), Some(Team::Red));
        hub.handle(host, ClientMessage::Start);
        assert_eq!(hub.phase(), Phase::Select);
    }

    #[test]
    fn rejoining_a_running_match_shows_everyone_as_loaded() {
        let mut hub = hub();
        let (a, _) = player(&mut hub, "a");
        player(&mut hub, "b");
        hub.begin_select();
        hub.start();
        hub.loading = None;
        hub.leave(a);
        let (_, mut receiver) = player(&mut hub, "a");
        let loaded =
            std::iter::from_fn(|| receiver.try_recv().ok()).find_map(|bytes| {
                match decode::<ServerMessage>(&bytes) {
                    Some(ServerMessage::Loading(players)) => Some(players),
                    _ => None,
                }
            });
        assert_eq!(
            loaded,
            Some(vec![("a".to_owned(), 100), ("b".to_owned(), 100)])
        );
    }

    #[test]
    fn the_match_waits_until_everyone_has_loaded() {
        let mut hub = hub();
        let (a, _) = player(&mut hub, "a");
        let (b, _) = player(&mut hub, "b");
        hub.begin_select();
        hub.start();
        let ticks = |hub: &Hub| hub.game.as_ref().unwrap().tick;
        hub.tick();
        hub.handle(a, ClientMessage::Loading(100));
        hub.handle(b, ClientMessage::Loading(60));
        hub.tick();
        assert_eq!(ticks(&hub), 0);
        hub.handle(b, ClientMessage::Loading(100));
        hub.tick();
        assert_eq!(ticks(&hub), 1);
        hub.handle(b, ClientMessage::Loading(0));
        hub.tick();
        assert_eq!(ticks(&hub), 2);
    }

    #[test]
    fn resuming_a_session_swaps_its_token_for_a_new_one() {
        let mut hub = hub();
        let (a, mut receiver) = player(&mut hub, "a");
        let token = std::iter::from_fn(|| receiver.try_recv().ok())
            .find_map(|bytes| match decode::<ServerMessage>(&bytes) {
                Some(ServerMessage::Session { token, .. }) => Some(token),
                _ => None,
            })
            .unwrap();
        hub.leave(a);
        let (sender, _receiver) = unbounded_channel();
        let again = hub.join(sender);
        hub.handle(
            again,
            ClientMessage::Resume {
                token: token.clone(),
            },
        );
        assert_eq!(hub.name_of(again).as_deref(), Some("a"));
        assert!(hub.sessions.take(&token).is_none());
    }

    #[test]
    fn players_who_log_in_join_the_lobby_on_alternating_teams() {
        let mut hub = hub();
        player(&mut hub, "a");
        player(&mut hub, "b");
        let teams: Vec<Team> = hub.lobby.iter().map(|(_, team)| *team).collect();
        assert_eq!(teams, vec![Team::Blue, Team::Red]);
    }
}
