use std::cell::Cell;
use std::rc::Rc;

use asset::data::GameData;
use protocol::{ChampSelect, Setup, Summoner, Team};
use web_sys::{Element, HtmlElement};

use crate::dom::{append, clear, element, on, set_class, set_style, set_text, show, text};
use crate::ui::{Events, UiEvent};

fn art(file: &str) -> String {
    format!("url(\"assets/icons/{file}\")")
}

pub struct ChampSelectView {
    root: HtmlElement,
    background: HtmlElement,
    timer: HtmlElement,
    title: HtmlElement,
    allies: HtmlElement,
    enemies: HtmlElement,
    tiles: Vec<HtmlElement>,
    lock: HtmlElement,
    spells: [HtmlElement; 2],
    picker: HtmlElement,
    summoners: Rc<Cell<[Summoner; 2]>>,
    remaining: f32,
    picked: Option<u8>,
    banning: bool,
    skip: HtmlElement,
    ban_summary: HtmlElement,
}

fn summoner_art(data: &GameData, summoner: Summoner) -> String {
    data.summoner(summoner.id())
        .map_or_else(|| "none".to_owned(), |spell| art(&spell.icon))
}

impl ChampSelectView {
    pub fn picked(&self) -> Option<u8> {
        self.picked
    }

    pub fn banning(&self) -> bool {
        self.banning
    }

    pub fn choose_ban(&mut self, champion: u8) {
        if self.tiles.get(usize::from(champion)).is_none_or(|tile| {
            tile.class_list().contains("locked") || tile.class_list().contains("taken")
        }) {
            return;
        }
        self.picked = Some(champion);
        for (index, tile) in self.tiles.iter().enumerate() {
            set_class(tile, "selected", index == usize::from(champion));
        }
        set_class(&self.lock, "disabled", false);
    }

    pub fn new(parent: &Element, events: &Events, data: &GameData) -> Self {
        let root = element("div", "champ-select");
        let background = element("div", "select-background");
        append(&root, &background);
        let header = element("div", "select-header");
        let title = text("div", "select-title", "Choose your champion!");
        let timer = text("div", "select-timer", "60");
        append(&header, &title);
        append(&header, &timer);
        append(&root, &header);
        let ban_summary = element("div", "ban-summary");
        append(&root, &ban_summary);
        let leave = crate::menu::button(&root, "Leave lobby", events, UiEvent::LeaveLobby);
        leave.set_class_name("draft-leave button");
        let body = element("div", "select-body");
        let allies = element("div", "select-team allies");
        let enemies = element("div", "select-team enemies");
        let center = element("div", "select-center");
        let grid = element("div", "select-grid");
        let tiles = data
            .champions
            .iter()
            .enumerate()
            .map(|(index, champion)| {
                let tile = element("div", "select-tile");
                let portrait = element("div", "select-portrait");
                set_style(&portrait, "background-image", &art(&champion.icon));
                append(&tile, &portrait);
                append(&tile, &text("div", "select-name", &champion.display_name));
                let events = events.clone();
                on(&tile, "click", move |_| {
                    events.borrow_mut().push(UiEvent::Pick(index as u8))
                });
                append(&grid, &tile);
                tile
            })
            .collect();
        append(&center, &grid);
        let summoners = Rc::new(Cell::new(Summoner::DEFAULT));
        let choosing: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));
        let picker = element("div", "summoner-picker");
        for summoner in Summoner::ALL {
            let Some(spell) = data.summoner(summoner.id()) else {
                continue;
            };
            let tile = element("div", "summoner-choice");
            let icon = element("div", "summoner-icon");
            set_style(&icon, "background-image", &art(&spell.icon));
            append(&tile, &icon);
            append(&tile, &text("div", "summoner-name", &spell.display_name));
            let events = events.clone();
            let (summoners, choosing, shown) =
                (summoners.clone(), choosing.clone(), picker.clone());
            on(&tile, "click", move |_| {
                let mut chosen = summoners.get();
                let slot = choosing.take().unwrap_or_default();
                if chosen[1 - slot] == summoner {
                    chosen.swap(0, 1);
                } else {
                    chosen[slot] = summoner;
                }
                show(&shown, false);
                events.borrow_mut().push(UiEvent::Summoners(chosen));
            });
            append(&picker, &tile);
        }
        show(&picker, false);
        let actions = element("div", "select-actions");
        let spells = [0usize, 1].map(|slot| {
            let button = element("div", "summoner-button");
            let (choosing, shown) = (choosing.clone(), picker.clone());
            on(&button, "click", move |_| {
                let open = choosing.get() != Some(slot);
                choosing.set(open.then_some(slot));
                show(&shown, open);
            });
            append(&actions, &button);
            button
        });
        let lock = text("button", "button primary lock-in", "Lock In");
        let lock_events = events.clone();
        let button = lock.clone();
        on(&lock, "click", move |_| {
            if !button.class_list().contains("disabled") {
                lock_events.borrow_mut().push(UiEvent::LockIn);
            }
        });
        append(&actions, &lock);
        let skip = crate::menu::button(&actions, "Skip ban", events, UiEvent::Ban(None));
        append(&center, &picker);
        append(&center, &actions);
        append(&body, &allies);
        append(&body, &center);
        append(&body, &enemies);
        append(&root, &body);
        show(&root, false);
        append(parent, &root);
        Self {
            root,
            background,
            timer,
            title,
            allies,
            enemies,
            tiles,
            lock,
            spells,
            picker,
            summoners,
            remaining: 0.0,
            picked: None,
            banning: false,
            skip,
            ban_summary,
        }
    }

    pub fn show(&self, visible: bool) {
        show(&self.root, visible);
        if !visible {
            show(&self.picker, false);
        }
    }

    pub fn tick(&mut self, dt: f32) {
        self.remaining = (self.remaining - dt).max(0.0);
        set_text(&self.timer, &format!("{:.0}", self.remaining.ceil()));
    }

    pub fn update(&mut self, select: &ChampSelect, you: &str, data: &GameData) {
        self.remaining = select.remaining;
        let Some(me) = select.players.iter().find(|player| player.name == you) else {
            return;
        };
        let team = me.team;
        if self.banning != select.banning {
            self.picked = None;
        }
        self.banning = select.banning;
        if !select.banning {
            self.picked = me.champion;
        }
        let banned = select.bans.iter().any(|(name, _)| name == you);
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
        let bans: std::collections::HashSet<u8> =
            select.bans.iter().filter_map(|(_, ban)| *ban).collect();
        let limit = bans.len() + largest_team >= data.champions.len();
        let names: Vec<&str> = bans
            .iter()
            .filter_map(|index| data.champions.get(usize::from(*index)))
            .map(|champion| champion.display_name.as_str())
            .collect();
        let mut names = names;
        names.sort_unstable();
        set_text(
            &self.ban_summary,
            &if names.is_empty() {
                "Each player may ban one champion. Bans apply to both teams.".to_owned()
            } else {
                format!(
                    "Banned: {}{}",
                    names.join(" · "),
                    if select.banning && limit {
                        ". Enough champions must remain for both teams. Skip to continue."
                    } else {
                        ""
                    }
                )
            },
        );
        show(&self.skip, select.banning && !banned);
        set_text(&self.lock, if select.banning { "Ban" } else { "Lock In" });
        self.summoners.set(me.summoners);
        for (button, summoner) in self.spells.iter().zip(me.summoners) {
            set_style(button, "background-image", &summoner_art(data, summoner));
        }
        let taken = |champion: u8| {
            bans.contains(&champion)
                || select.players.iter().any(|player| {
                    player.name != you
                        && player.team == team
                        && player.locked
                        && player.champion == Some(champion)
                })
        };
        for (index, tile) in self.tiles.iter().enumerate() {
            let champion = index as u8;
            set_class(tile, "selected", self.picked == Some(champion));
            set_class(tile, "taken", taken(champion) || select.banning && limit);
            set_class(tile, "locked", me.locked || select.banning && banned);
        }
        let hovered = me
            .champion
            .and_then(|champion| data.champions.get(usize::from(champion)));
        set_style(
            &self.background,
            "background-image",
            &hovered.map_or_else(|| "none".to_owned(), |champion| art(&champion.splash)),
        );
        set_class(
            &self.lock,
            "disabled",
            if select.banning {
                banned || limit || self.picked.is_none_or(taken)
            } else {
                me.locked || me.champion.is_none_or(taken)
            },
        );
        set_text(
            &self.title,
            if select.banning && banned {
                "Waiting for bans"
            } else if select.banning {
                "Ban a champion"
            } else if me.locked {
                "Waiting for other players"
            } else {
                "Choose your champion!"
            },
        );
        for (list, ally) in [(&self.allies, true), (&self.enemies, false)] {
            clear(list);
            for player in select
                .players
                .iter()
                .filter(|player| (player.team == team) == ally)
            {
                let row = element("div", "select-player");
                set_class(&row, "you", player.name == you);
                set_class(&row, "locked", player.locked);
                let portrait = element("div", "select-player-portrait");
                let champion = ally
                    .then_some(player.champion)
                    .flatten()
                    .and_then(|champion| data.champions.get(usize::from(champion)));
                if let Some(champion) = champion {
                    set_style(&portrait, "background-image", &art(&champion.icon));
                }
                append(&row, &portrait);
                let info = element("div", "select-player-info");
                append(&info, &text("div", "select-player-name", &player.name));
                let status = if select.banning {
                    if select.bans.iter().any(|(name, _)| *name == player.name) {
                        "Ready".to_owned()
                    } else {
                        "Banning".to_owned()
                    }
                } else {
                    match (ally, champion, player.locked) {
                        (false, _, _) => String::new(),
                        (true, Some(champion), true) => champion.display_name.clone(),
                        (true, Some(champion), false) => {
                            format!("Picking {}", champion.display_name)
                        }
                        (true, None, _) => "Picking".to_owned(),
                    }
                };
                append(&info, &text("div", "select-player-champion", &status));
                append(&row, &info);
                if ally {
                    let spells = element("div", "select-player-spells");
                    for summoner in player.summoners {
                        let icon = element("div", "select-player-spell");
                        set_style(&icon, "background-image", &summoner_art(data, summoner));
                        append(&spells, &icon);
                    }
                    append(&row, &spells);
                }
                append(list, &row);
            }
        }
    }
}

pub struct LoadingView {
    root: HtmlElement,
    you: String,
    bars: Vec<(String, HtmlElement, HtmlElement)>,
    size: Option<HtmlElement>,
}

impl LoadingView {
    pub fn new(parent: &Element, setup: &Setup, data: &GameData) -> Self {
        let root = element("div", "loading");
        let mut bars = Vec::new();
        let mut size = None;
        let you = setup
            .players
            .iter()
            .find(|player| player.unit == setup.you)
            .map(|player| player.name.clone())
            .unwrap_or_default();
        for team in [Team::Blue, Team::Red] {
            let row = element("div", "loading-team");
            for player in setup.players.iter().filter(|player| player.team == team) {
                let card = element(
                    "div",
                    if team == Team::Blue {
                        "loading-card blue"
                    } else {
                        "loading-card red"
                    },
                );
                if let Some(champion) = data.champions.get(usize::from(player.champion)) {
                    set_style(&card, "background-image", &art(&champion.loading));
                    let label = element("div", "loading-label");
                    append(
                        &label,
                        &text("div", "loading-champion", &champion.display_name),
                    );
                    append(&label, &text("div", "loading-name", &player.name));
                    let bar = element("div", "loading-bar");
                    let fill = element("div", "loading-fill");
                    append(&bar, &fill);
                    append(&label, &bar);
                    let percent = text("div", "loading-percent", "0%");
                    append(&label, &percent);
                    append(&card, &label);
                    bars.push((player.name.clone(), fill, percent));
                }
                if player.unit == setup.you {
                    let mine = element("div", "loading-size");
                    append(&card, &mine);
                    size = Some(mine);
                }
                append(&row, &card);
            }
            append(&root, &row);
        }
        append(parent, &root);
        Self {
            root,
            you,
            bars,
            size,
        }
    }

    pub fn update(&self, players: &[(String, u8)]) {
        for (name, percent) in players.iter().filter(|(name, _)| *name != self.you) {
            self.set(name, *percent);
        }
    }

    pub fn show_own(&self, percent: u8, received: u64, expected: u64) {
        self.set(&self.you, percent);
        if let Some(size) = &self.size {
            let megabytes = |bytes: u64| (bytes + 500_000) / 1_000_000;
            set_text(
                size,
                &format!("{} / {} MB", megabytes(received), megabytes(expected)),
            );
        }
    }

    fn set(&self, name: &str, percent: u8) {
        for (_, fill, label) in self.bars.iter().filter(|(player, ..)| player == name) {
            set_style(fill, "width", &format!("{percent}%"));
            set_text(label, &format!("{percent}%"));
        }
    }

    pub fn remove(&self) {
        self.root.remove();
    }
}
