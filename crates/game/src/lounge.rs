use std::cell::RefCell;
use std::rc::Rc;

use asset::data::GameData;
use protocol::{Directory, Lobby, Phase, Profile, Team};
use web_sys::{Element, HtmlElement};

use crate::dom::{
    Listener, append, clear, element, input, listen, on, set_class, set_style, set_text, show, text,
};
use crate::menu::button;
use crate::ui::{Events, UiEvent};

pub struct Lounge {
    root: HtmlElement,
    identity: HtmlElement,
    browse: HtmlElement,
    rooms: HtmlElement,
    room: HtmlElement,
    room_title: HtmlElement,
    room_hint: HtmlElement,
    blue: HtmlElement,
    red: HtmlElement,
    start: HtmlElement,
    rankings: HtmlElement,
    leaders: HtmlElement,
    profile: HtmlElement,
    profile_stats: HtmlElement,
    avatars: Vec<HtmlElement>,
    create: HtmlElement,
    nav: Vec<HtmlElement>,
    current: u8,
    joined: bool,
    events: Events,
    data: Rc<GameData>,
    listeners: RefCell<Vec<Listener>>,
}

fn portrait(data: &GameData, avatar: u8) -> HtmlElement {
    let icon = element("div", "avatar");
    if let Some(champion) = data.champions.get(usize::from(avatar)) {
        set_style(
            &icon,
            "background-image",
            &format!("url(\"assets/icons/{}\")", champion.icon),
        );
    }
    icon
}

fn stats(profile: &Profile) -> String {
    let rate = (u64::from(profile.wins) * 100)
        .checked_div(u64::from(profile.games))
        .unwrap_or(0);
    format!(
        "{} · {} · {}% win rate",
        count(profile.wins, "win"),
        count(profile.games, "game"),
        rate
    )
}

fn count(value: u32, noun: &str) -> String {
    format!("{value} {noun}{}", if value == 1 { "" } else { "s" })
}

impl Lounge {
    pub fn new(parent: &Element, events: &Events, data: Rc<GameData>) -> Self {
        let root = element("main", "lounge");
        let header = element("header", "lounge-header");
        let brand = element("div", "brand");
        append(&brand, &text("span", "brand-mark", "O"));
        append(&brand, &text("span", "", "OpenMOBA"));
        append(&header, &brand);
        let navigation = element("nav", "lounge-nav");
        let nav = ["Lobbies", "Leaderboard", "Profile"]
            .iter()
            .enumerate()
            .map(|(index, title)| {
                let item = button(&navigation, title, events, UiEvent::MenuTab(index as u8));
                item.set_class_name("nav-link");
                item
            })
            .collect();
        append(&header, &navigation);
        let identity = element("div", "identity");
        append(&header, &identity);
        let settings = button(&header, "Settings", events, UiEvent::OpenMenu);
        settings.set_class_name("nav-link");
        append(&root, &header);
        let content = element("div", "lounge-content");
        let browse = element("section", "browse");
        let hero = element("div", "lounge-hero");
        let atmosphere = element("div", "hero-atmosphere");
        let _ = atmosphere.set_attribute("aria-hidden", "true");
        append(&hero, &atmosphere);
        append(&hero, &text("div", "eyebrow", "GATHER YOUR PARTY"));
        append(
            &hero,
            &text("h1", "hero-title", "One bridge.\nAll your friends."),
        );
        append(
            &hero,
            &text(
                "p",
                "hero-copy",
                "Pick your side. Ban your nemesis. Make it a quick one.",
            ),
        );
        let create_button = text("button", "button primary", "Create lobby");
        append(&hero, &create_button);
        append(
            &hero,
            &text("span", "hero-caption", "THE BRIDGE / UP TO 5 VS 5"),
        );
        append(&browse, &hero);
        let heading = element("div", "section-heading");
        append(&heading, &text("h2", "", "Lobbies"));
        append(&heading, &text("span", "muted", "Live rooms · Pick a seat"));
        append(&browse, &heading);
        let rooms = element("div", "room-list");
        append(&browse, &rooms);
        append(&content, &browse);
        let room = element("section", "room-detail");
        let room_title = text("h1", "", "");
        let room_hint = text("p", "muted", "");
        append(&room, &text("div", "eyebrow", "YOUR PARTY"));
        append(&room, &room_title);
        append(&room, &room_hint);
        let teams = element("div", "teams");
        let column = |team, title, class| {
            let column = element("div", class);
            let header = button(&column, title, events, UiEvent::JoinTeam(team));
            header.set_class_name("team-header");
            let players = element("ul", "players");
            append(&column, &players);
            append(&teams, &column);
            players
        };
        let blue = column(Team::Blue, "Join Blue", "team blue");
        let red = column(Team::Red, "Join Red", "team red");
        append(&room, &teams);
        let actions = element("div", "room-actions");
        button(&actions, "Leave lobby", events, UiEvent::LeaveLobby);
        let start = button(&actions, "Start match", events, UiEvent::Start);
        start.set_class_name("button primary");
        append(&room, &actions);
        append(
            &room,
            &text(
                "p",
                "muted room-rules",
                "One ban per player, then champion picks. You can start alone to practice. Practice and cancelled games do not count toward the leaderboard.",
            ),
        );
        append(&content, &room);
        let rankings = element("section", "rankings");
        append(&rankings, &text("div", "eyebrow", "BRAGGING RIGHTS"));
        append(&rankings, &text("h1", "", "Leaderboard"));
        append(
            &rankings,
            &text("p", "muted", "Good games add up. Ranked by wins."),
        );
        let leaders = element("div", "leader-list");
        append(&rankings, &leaders);
        append(&content, &rankings);
        let profile = element("section", "profile-page");
        append(
            &profile,
            &text("div", "eyebrow", "YOUR CORNER OF THE ABYSS"),
        );
        append(&profile, &text("h1", "", "Profile"));
        let profile_stats = element("div", "profile-stats");
        append(&profile, &profile_stats);
        append(&profile, &text("h2", "", "Choose your avatar"));
        append(
            &profile,
            &text("p", "muted", "Saved automatically to your account."),
        );
        let choices = element("div", "avatar-choices");
        let avatars = data
            .champions
            .iter()
            .enumerate()
            .map(|(index, champion)| {
                let choice = element("button", "avatar-choice");
                let _ = choice.set_attribute("aria-label", &champion.display_name);
                let _ = choice.set_attribute("title", &champion.display_name);
                append(&choice, &portrait(&data, index as u8));
                let events = events.clone();
                on(&choice, "click", move |_| {
                    events.borrow_mut().push(UiEvent::SetAvatar(index as u8))
                });
                append(&choices, &choice);
                choice
            })
            .collect();
        append(&profile, &choices);
        button(&profile, "Log out", events, UiEvent::LogOut);
        append(&content, &profile);
        append(&root, &content);
        let footer = element("footer", "lounge-footer");
        append(&footer, &text("span", "", "OPENMOBA"));
        append(&footer, &text("span", "", "Private games. Familiar faces."));
        append(&root, &footer);
        let create = element("div", "create-overlay");
        let form = element("form", "create-panel");
        append(&form, &text("div", "eyebrow", "MAKE ROOM FOR YOUR FRIENDS"));
        append(&form, &text("h2", "", "Create lobby"));
        let label = text("label", "", "Lobby name");
        let name = input("text", "Tonight's Match");
        name.set_max_length(32);
        name.set_required(true);
        append(&label, &name);
        append(&form, &label);
        let actions = element("div", "row");
        let cancel = text("button", "button", "Cancel");
        let _ = cancel.set_attribute("type", "button");
        append(&actions, &cancel);
        append(&actions, &text("button", "button primary", "Create"));
        append(&form, &actions);
        append(&create, &form);
        append(&root, &create);
        on(&create_button, "click", {
            let create = create.clone();
            let name = name.clone();
            move |_| {
                show(&create, true);
                let _ = name.focus();
            }
        });
        on(&cancel, "click", {
            let create = create.clone();
            move |_| show(&create, false)
        });
        on(&form, "submit", {
            let events = events.clone();
            move |event| {
                event.prevent_default();
                events.borrow_mut().push(UiEvent::CreateLobby(name.value()));
            }
        });
        show(&create, false);
        show(&root, false);
        append(parent, &root);
        let mut view = Self {
            root,
            identity,
            browse,
            rooms,
            room,
            room_title,
            room_hint,
            blue,
            red,
            start,
            rankings,
            leaders,
            profile,
            profile_stats,
            avatars,
            create,
            nav,
            current: 0,
            joined: false,
            events: events.clone(),
            data,
            listeners: RefCell::default(),
        };
        view.tab(0);
        view
    }

    pub fn show(&self, visible: bool) {
        show(&self.root, visible);
    }

    pub fn tab(&mut self, tab: u8) {
        self.current = tab;
        show(&self.browse, tab == 0 && !self.joined);
        show(&self.room, tab == 0 && self.joined);
        show(&self.rankings, tab == 1);
        show(&self.profile, tab == 2);
        for (index, link) in self.nav.iter().enumerate() {
            set_class(link, "active", index == usize::from(tab));
        }
    }

    pub fn directory(&mut self, directory: &Directory) {
        let joined = directory.room.is_some();
        if self.joined != joined {
            self.current = 0;
        }
        self.joined = joined;
        if joined {
            show(&self.create, false);
        }
        self.tab(self.current);
        clear(&self.identity);
        append(
            &self.identity,
            &portrait(&self.data, directory.profile.avatar),
        );
        let identity = element("div", "identity-copy");
        append(&identity, &text("strong", "", &directory.profile.name));
        append(
            &identity,
            &text("span", "muted", &count(directory.profile.wins, "win")),
        );
        append(&self.identity, &identity);
        clear(&self.profile_stats);
        append(
            &self.profile_stats,
            &portrait(&self.data, directory.profile.avatar),
        );
        let details = element("div", "");
        append(&details, &text("h2", "", &directory.profile.name));
        append(&details, &text("p", "muted", &stats(&directory.profile)));
        append(&self.profile_stats, &details);
        for (index, avatar) in self.avatars.iter().enumerate() {
            set_class(
                avatar,
                "selected",
                index == usize::from(directory.profile.avatar),
            );
            let _ = avatar.set_attribute(
                "aria-pressed",
                if index == usize::from(directory.profile.avatar) {
                    "true"
                } else {
                    "false"
                },
            );
        }
        clear(&self.rooms);
        let mut listeners = self.listeners.borrow_mut();
        listeners.clear();
        if directory.rooms.is_empty() {
            let empty = element("div", "empty-rooms");
            append(&empty, &text("span", "empty-symbol", "◇"));
            append(&empty, &text("h3", "", "The bridge is yours."));
            append(
                &empty,
                &text("p", "muted", "No open lobbies. Create one to get started."),
            );
            append(&self.rooms, &empty);
        }
        for room in &directory.rooms {
            let row = element("div", "room-row");
            append(&row, &text("span", "room-symbol", "◇"));
            let info = element("div", "room-info");
            append(&info, &text("h3", "", &room.name));
            append(
                &info,
                &text("span", "muted", &format!("Hosted by {}", room.host)),
            );
            append(&row, &info);
            append(
                &row,
                &text("span", "room-count", &format!("{} / 10", room.players)),
            );
            append(
                &row,
                &text(
                    "span",
                    "phase-pill",
                    match room.phase {
                        Phase::Lobby => "Waiting",
                        Phase::Select => "Drafting",
                        Phase::Game => "In game",
                    },
                ),
            );
            let join = text("button", "button", "Join");
            if room.phase != Phase::Lobby || room.players >= 10 || joined {
                let _ = join.set_attribute("disabled", "");
            }
            let events = self.events.clone();
            let id = room.id;
            listeners.push(listen(&join, "click", move |_| {
                events.borrow_mut().push(UiEvent::JoinLobby(id))
            }));
            append(&row, &join);
            append(&self.rooms, &row);
        }
        clear(&self.leaders);
        for (rank, player) in directory.leaderboard.iter().enumerate() {
            let row = element("div", "leader-row");
            set_class(&row, "you", player.name == directory.profile.name);
            append(
                &row,
                &text("span", "leader-rank", &format!("{:02}", rank + 1)),
            );
            append(&row, &portrait(&self.data, player.avatar));
            append(&row, &text("strong", "leader-name", &player.name));
            append(&row, &text("span", "muted", &count(player.games, "game")));
            append(
                &row,
                &text("strong", "leader-wins", &count(player.wins, "win")),
            );
            append(&self.leaders, &row);
        }
    }

    pub fn update(&self, lobby: &Lobby, you: &str) {
        set_text(&self.room_title, &lobby.name);
        set_text(
            &self.room_hint,
            &format!(
                "Hosted by {} · {}/10 players · {}",
                lobby.host,
                lobby.players.len(),
                if lobby.host == you {
                    "You're the host. Start when everyone's here."
                } else {
                    "Only the host can start."
                }
            ),
        );
        for (team, list) in [(Team::Blue, &self.blue), (Team::Red, &self.red)] {
            clear(list);
            let players: Vec<_> = lobby
                .players
                .iter()
                .filter(|player| player.team == team)
                .collect();
            for player in &players {
                let item = text(
                    "li",
                    "player",
                    &format!(
                        "{}{}",
                        player.name,
                        if player.name == lobby.host {
                            " · Host"
                        } else {
                            ""
                        }
                    ),
                );
                set_class(&item, "offline", !player.online);
                set_class(&item, "you", player.name == you);
                append(list, &item);
            }
            for _ in players.len()..5 {
                append(list, &text("li", "player empty-seat", "Open seat"));
            }
        }
        if lobby.host == you && lobby.phase == Phase::Lobby {
            let _ = self.start.remove_attribute("disabled");
        } else {
            let _ = self.start.set_attribute("disabled", "");
        }
    }
}
