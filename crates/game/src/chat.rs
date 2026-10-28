use asset::ui::Layout;
use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlElement, HtmlInputElement, KeyboardEvent};

use crate::dom::{append, element, input, on, set_class, show, text};
use crate::frame::{Frame, View};
use crate::ui::{Events, UiEvent};

const VISIBLE_TIME: f32 = 10.0;
const HISTORY: usize = 40;
const ALL_PREFIX: &str = "/all ";
const SCENE: &str = "Chat/ChatFrame";
const INPUT_BOX: &str = "Chat/ChatFrame/ChatFrame_InputBox";
const BACKGROUND: &str = "Chat/ChatFrame/ChatFrame_Background";
const LOG: &str = "Chat/ChatFrame/ChatFrameTextArea/ChatFrame_Text";
const TEXT_EDIT: &str = "Chat/ChatFrame/ChatFrame_TextEdit";
const UNUSED: [&str; 3] = [
    "Chat/ChatFrame/ChatSlider/ChatFrame_Slider_Slider",
    "Chat/ChatFrame/ChatFrame_ChannelSelect/ChannelBox_Btn/ChannelBox_Btn_HeaderText",
    "Chat/ChatFrame/ChatFrame_ChatRestrictionText",
];

pub struct Chat {
    root: HtmlElement,
    frame: Frame,
    log: HtmlElement,
    input: HtmlInputElement,
    lines: Vec<(HtmlElement, f32)>,
    open: bool,
}

pub struct Line<'a> {
    pub all: bool,
    pub ally: bool,
    pub from: &'a str,
    pub champion: Option<&'a str>,
    pub text: &'a str,
}

impl Chat {
    pub fn new(parent: &Element, layout: &Layout, events: &Events) -> Self {
        let root = element("div", "chat");
        let frame = Frame::new(&root, layout, &[SCENE]);
        frame.show_scene(SCENE, true);
        let log = element("div", "chat-log");
        let input = input("text", "");
        input.set_class_name("chat-input");
        let _ = input.set_attribute("maxlength", "200");
        if let Some(area) = frame.get(LOG) {
            append(area, &log);
        }
        if let Some(edit) = frame.get(TEXT_EDIT) {
            append(edit, &input);
        }
        for part in UNUSED {
            frame.show(part, false);
        }
        show(input.unchecked_ref(), false);
        let sender = input.clone();
        let events = events.clone();
        on(&input, "keydown", move |event| {
            let event: KeyboardEvent = event.unchecked_into();
            event.stop_propagation();
            match event.key().as_str() {
                "Enter" => {
                    event.prevent_default();
                    let value = sender.value();
                    let (text, all) = match value.strip_prefix(ALL_PREFIX) {
                        Some(rest) => (rest.to_owned(), true),
                        None => (value, sender.class_list().contains("all")),
                    };
                    events.borrow_mut().push(UiEvent::Chat { text, all });
                }
                "Escape" => {
                    event.prevent_default();
                    events.borrow_mut().push(UiEvent::Chat {
                        text: String::new(),
                        all: false,
                    });
                }
                _ => {}
            }
        });
        append(parent, &root);
        Self {
            root,
            frame,
            log,
            input,
            lines: Vec::new(),
            open: false,
        }
    }

    pub fn show(&self, visible: bool) {
        show(&self.root, visible);
    }

    pub fn resize(&mut self, height: f32) {
        self.frame.resize(height);
    }

    pub fn open(&mut self, all: bool) {
        self.open = true;
        set_class(self.input.unchecked_ref(), "all", all);
        self.input
            .set_placeholder(if all { "[All]" } else { "[Team]" });
        show(self.input.unchecked_ref(), true);
        self.typing(true);
        set_class(&self.root, "open", true);
        let _ = self.input.focus();
    }

    pub fn close(&mut self) {
        self.open = false;
        self.input.set_value("");
        let _ = self.input.blur();
        show(self.input.unchecked_ref(), false);
        self.typing(false);
        set_class(&self.root, "open", false);
    }

    fn typing(&self, active: bool) {
        self.frame.show_scene(INPUT_BOX, active);
        self.frame.show(BACKGROUND, active);
    }

    pub fn push(&mut self, line: Line) {
        let row = element("div", "chat-line");
        set_class(&row, if line.ally { "ally" } else { "enemy" }, true);
        if line.all {
            append(&row, &text("span", "chat-channel", "[All] "));
        }
        let sender = match line.champion {
            Some(champion) => format!("{} ({champion}): ", line.from),
            None => format!("{}: ", line.from),
        };
        append(&row, &text("span", "chat-from", &sender));
        append(&row, &text("span", "chat-text", line.text));
        append(&self.log, &row);
        self.lines.push((row, 0.0));
        if self.lines.len() > HISTORY {
            let (old, _) = self.lines.remove(0);
            old.remove();
        }
        self.log.set_scroll_top(self.log.scroll_height());
    }

    pub fn update(&mut self, dt: f32) {
        for (row, age) in &mut self.lines {
            *age += dt;
            set_class(row, "faded", *age > VISIBLE_TIME && !self.open);
        }
    }
}
