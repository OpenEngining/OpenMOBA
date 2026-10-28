use crate::dom::{Listener, append, element, input, listen, on, set_text, show, text};
use crate::settings::{CONTROLS, Settings, label};
use crate::ui::{Events, UiEvent};
use protocol::{VoteKind, VoteStatus};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlElement, KeyboardEvent};

pub type Preferences = Rc<RefCell<Settings>>;

pub struct Menu {
    root: HtmlElement,
    opener: HtmlElement,
    controls: HtmlElement,
    match_actions: HtmlElement,
    vote: HtmlElement,
    vote_title: HtmlElement,
    vote_count: HtmlElement,
    vote_buttons: HtmlElement,
    settings: Preferences,
    listeners: RefCell<Vec<Listener>>,
    open: bool,
}

pub fn button(parent: &Element, label: &str, events: &Events, event: UiEvent) -> HtmlElement {
    let button = text("button", "button", label);
    let events = events.clone();
    on(&button, "click", move |_| {
        events.borrow_mut().push(event.clone())
    });
    append(parent, &button);
    button
}

impl Menu {
    pub fn new(parent: &Element, events: &Events, settings: Preferences) -> Self {
        let opener = button(parent, "Settings", events, UiEvent::OpenMenu);
        opener.set_class_name("game-menu-button");
        show(&opener, false);
        let root = element("div", "menu");
        let _ = root.set_attribute("role", "dialog");
        let _ = root.set_attribute("aria-modal", "true");
        let _ = root.set_attribute("aria-label", "Settings");
        let panel = element("div", "settings-panel");
        append(&panel, &text("div", "eyebrow", "MAKE IT YOURS"));
        append(&panel, &text("h1", "", "Settings"));
        let controls = element("div", "settings-grid");
        append(&panel, &controls);
        let actions = element("div", "settings-actions");
        button(&actions, "Reset defaults", events, UiEvent::ResetSettings);
        button(&actions, "Resume", events, UiEvent::CloseMenu);
        append(&panel, &actions);
        let match_actions = element("div", "match-actions");
        let surrender = element("div", "");
        button(
            &surrender,
            "Surrender",
            events,
            UiEvent::StartVote(VoteKind::Surrender),
        );
        append(
            &surrender,
            &text(
                "p",
                "muted",
                "Your team needs a majority to surrender. The other team wins.",
            ),
        );
        append(&match_actions, &surrender);
        let end = element("div", "");
        button(
            &end,
            "End game",
            events,
            UiEvent::StartVote(VoteKind::EndGame),
        );
        append(
            &end,
            &text(
                "p",
                "muted",
                "Everyone must agree to end the game. No result is recorded.",
            ),
        );
        append(&match_actions, &end);
        append(&panel, &match_actions);
        append(&root, &panel);
        show(&root, false);
        append(parent, &root);
        let vote = element("div", "vote-panel");
        let vote_title = text("h3", "", "");
        let vote_count = text("p", "muted", "");
        let vote_buttons = element("div", "row");
        button(&vote_buttons, "Yes", events, UiEvent::Vote(true));
        button(&vote_buttons, "No", events, UiEvent::Vote(false));
        append(&vote, &vote_title);
        append(&vote, &vote_count);
        append(&vote, &vote_buttons);
        show(&vote, false);
        append(parent, &vote);
        Self {
            root,
            opener,
            controls,
            match_actions,
            vote,
            vote_title,
            vote_count,
            vote_buttons,
            settings,
            listeners: RefCell::default(),
            open: false,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn set_open(&mut self, open: bool) {
        self.open = open;
        if open {
            self.populate();
        }
        show(&self.root, open);
    }

    pub fn in_game(&self, playing: bool) {
        show(&self.match_actions, playing);
        show(&self.opener, playing);
    }

    fn populate(&self) {
        self.listeners.borrow_mut().clear();
        crate::dom::clear(&self.controls);
        let mut listeners = self.listeners.borrow_mut();
        let general = element("section", "settings-section");
        append(&general, &text("h2", "", "Camera & interface"));
        for (title, get, set) in [
            (
                "Follow champion",
                (|s: &Settings| s.follow) as fn(&Settings) -> bool,
                (|s: &mut Settings, v| s.follow = v) as fn(&mut Settings, bool),
            ),
            (
                "Show chat",
                |s: &Settings| s.chat,
                |s: &mut Settings, v| s.chat = v,
            ),
            (
                "Show damage numbers",
                |s: &Settings| s.damage,
                |s: &mut Settings, v| s.damage = v,
            ),
            (
                "Show player names",
                |s: &Settings| s.names,
                |s: &mut Settings, v| s.names = v,
            ),
        ] {
            let row = text("label", "setting-row", title);
            let checkbox = input("checkbox", "");
            checkbox.set_checked(get(&self.settings.borrow()));
            let settings = self.settings.clone();
            let value = checkbox.clone();
            listeners.push(listen(&checkbox, "change", move |_| {
                let mut settings = settings.borrow_mut();
                set(&mut settings, value.checked());
                settings.save();
            }));
            append(&row, &checkbox);
            append(&general, &row);
        }
        for (title, min, max, get, set) in [
            (
                "HUD scale",
                "70",
                "120",
                (|s: &Settings| s.hud) as fn(&Settings) -> f32,
                (|s: &mut Settings, v| s.hud = v) as fn(&mut Settings, f32),
            ),
            (
                "Volume",
                "0",
                "100",
                |s: &Settings| s.volume,
                |s: &mut Settings, v| s.volume = v,
            ),
        ] {
            let row = text("label", "setting-row range-row", title);
            let range = input("range", "");
            range.set_min(min);
            range.set_max(max);
            range.set_value(&(get(&self.settings.borrow()) * 100.0).round().to_string());
            let output = text("span", "setting-value", &format!("{}%", range.value()));
            let settings = self.settings.clone();
            let value = range.clone();
            let shown = output.clone();
            listeners.push(listen(&range, "input", move |_| {
                if let Ok(value) = value.value().parse::<f32>() {
                    let mut settings = settings.borrow_mut();
                    set(&mut settings, value / 100.0);
                    settings.save();
                    set_text(&shown, &format!("{value:.0}%"));
                }
            }));
            append(&row, &range);
            append(&row, &output);
            append(&general, &row);
        }
        append(&self.controls, &general);
        let keys = element("section", "settings-section");
        append(&keys, &text("h2", "", "Controls"));
        append(
            &keys,
            &text(
                "p",
                "muted",
                "Select a key, then press its replacement. Escape cancels. Conflicting bindings swap.",
            ),
        );
        let buttons: Vec<_> = CONTROLS
            .iter()
            .enumerate()
            .map(|(index, (title, _))| {
                let row = text("div", "binding-row", title);
                let key = text(
                    "button",
                    "key-binding",
                    label(&self.settings.borrow().keys[index]),
                );
                let _ = key.set_attribute("aria-label", title);
                append(&row, &key);
                append(&keys, &row);
                key
            })
            .collect();
        for (index, key) in buttons.iter().enumerate() {
            let settings = self.settings.clone();
            let shown = buttons.clone();
            listeners.push(listen(key, "click", {
                let key = key.clone();
                move |_| set_text(&key, "Press a key")
            }));
            listeners.push(listen(key, "keydown", move |event| {
                let event: KeyboardEvent = event.unchecked_into();
                event.stop_propagation();
                if event.code() == "Tab" {
                    return;
                }
                event.prevent_default();
                let mut settings = settings.borrow_mut();
                if event.code() == "Escape"
                    || (!event.ctrl_key()
                        && !event.alt_key()
                        && !event.meta_key()
                        && settings.bind(index, &event.code()))
                {
                    settings.save();
                    for (button, code) in shown.iter().zip(&settings.keys) {
                        set_text(button, label(code));
                    }
                    let _ = shown[index].blur();
                }
            }));
        }
        append(&self.controls, &keys);
    }

    pub fn vote(&self, vote: Option<&VoteStatus>) {
        show(&self.vote, vote.is_some());
        let Some(vote) = vote else {
            return;
        };
        set_text(
            &self.vote_title,
            match vote.result {
                Some(true) => "Vote passed",
                Some(false) => "Vote failed",
                None if vote.kind == VoteKind::Surrender => "Surrender vote",
                None => "End game vote",
            },
        );
        set_text(
            &self.vote_count,
            &format!(
                "{} / {} votes · {:.0}s remaining",
                vote.yes,
                vote.needed,
                vote.remaining.ceil()
            ),
        );
        show(&self.vote_buttons, !vote.voted && vote.result.is_none());
    }
}
