use std::cell::Cell;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use asset::ui::{Button, Element, Kind, Layout, Rect, Sprite, TextStyle};
use wasm_bindgen::JsCast;
use web_sys::HtmlElement;

use crate::dom::{append, element, on, set_style, set_text, show};

const FOLDER: &str = "assets/ui";
const SOURCE_HEIGHT: f32 = 1200.0;

pub struct Node {
    element: HtmlElement,
    rect: Rect,
    sprite: Option<Sprite>,
    frames: Option<asset::ui::Frames>,
    saturation: f32,
}

pub struct Frame {
    root: HtmlElement,
    scenes: HashMap<String, HtmlElement>,
    nodes: HashMap<String, Node>,
    buttons: HashMap<String, Rc<ButtonView>>,
    definitions: HashMap<String, Button>,
    height: f32,
}

type ButtonChange = fn(&ButtonView);

struct ButtonView {
    states: [Vec<HtmlElement>; 7],
    region: Option<HtmlElement>,
    hovered: Cell<bool>,
    pressed: Cell<bool>,
    selected: Cell<bool>,
    enabled: Cell<bool>,
    visible: Cell<bool>,
}

impl ButtonView {
    fn state(&self) -> usize {
        let fallback = |state: usize, other: usize| {
            if self.states[state].is_empty() {
                other
            } else {
                state
            }
        };
        if !self.enabled.get() {
            return fallback(Button::INACTIVE, Button::DEFAULT);
        }
        let base = if self.selected.get() {
            fallback(Button::SELECTED, Button::DEFAULT)
        } else {
            Button::DEFAULT
        };
        match (self.pressed.get(), self.hovered.get(), self.selected.get()) {
            (true, _, true) => fallback(Button::SELECTED_CLICKED, base),
            (true, _, false) => fallback(Button::CLICKED, fallback(Button::HOVER, base)),
            (false, true, true) => fallback(Button::SELECTED_HOVER, base),
            (false, true, false) => fallback(Button::HOVER, base),
            (false, false, _) => base,
        }
    }

    fn show(&self, visible: bool) {
        self.visible.set(visible);
        if let Some(region) = &self.region {
            show(region, visible);
        }
        if visible {
            self.refresh();
        } else {
            self.hovered.set(false);
            self.pressed.set(false);
            for element in self.states.iter().flatten() {
                show(element, false);
            }
        }
    }

    fn refresh(&self) {
        if !self.visible.get() {
            return;
        }
        let current = self.state();
        for (index, elements) in self.states.iter().enumerate() {
            if index != current {
                for element in elements {
                    show(element, false);
                }
            }
        }
        for element in &self.states[current] {
            show(element, true);
        }
    }
}

impl Frame {
    pub fn new(parent: &web_sys::Element, layout: &Layout, roots: &[&str]) -> Self {
        let root = element("div", "frame");
        append(&root, &fonts(layout));
        let mounted = |name: &str| {
            roots
                .iter()
                .any(|root| name == *root || name.starts_with(&format!("{root}/")))
        };
        let mut scenes: HashMap<String, HtmlElement> = HashMap::new();
        for scene in layout.scenes.iter().filter(|scene| mounted(&scene.name)) {
            let view = element("div", "frame-scene");
            show(&view, scene.visible);
            scenes.insert(scene.name.clone(), view);
        }
        for scene in layout.scenes.iter().filter(|scene| mounted(&scene.name)) {
            let parent = scene
                .parent
                .as_ref()
                .and_then(|parent| scenes.get(parent))
                .unwrap_or(&root);
            append(parent, &scenes[&scene.name]);
        }
        let layers: HashMap<&str, u32> = layout
            .scenes
            .iter()
            .map(|scene| (scene.name.as_str(), scene.layer))
            .collect();
        let mut anchors: HashMap<(String, [u32; 2], u32), HtmlElement> = HashMap::new();
        let mut nodes = HashMap::new();
        for item in layout.elements.iter().filter(|item| {
            mounted(&item.scene) && !matches!(item.kind, Kind::Group | Kind::Button | Kind::Meter)
        }) {
            let Some(scene) = scenes.get(&item.scene) else {
                continue;
            };
            let anchor = item.rect.anchor.map(f32::to_bits);
            let source = item.rect.source[1];
            let container = anchors
                .entry((item.scene.clone(), anchor, source.to_bits()))
                .or_insert_with(|| {
                    let container = element("div", "frame-anchor");
                    set_style(
                        &container,
                        "left",
                        &format!("{}%", item.rect.anchor[0] * 100.0),
                    );
                    set_style(
                        &container,
                        "top",
                        &format!("{}%", item.rect.anchor[1] * 100.0),
                    );
                    if source != SOURCE_HEIGHT {
                        set_style(
                            &container,
                            "transform",
                            &format!(
                                "scale(calc(var(--ui-scale, 1) * {}))",
                                SOURCE_HEIGHT / source
                            ),
                        );
                    }
                    set_style(
                        &container,
                        "z-index",
                        &layers
                            .get(item.scene.as_str())
                            .copied()
                            .unwrap_or(0)
                            .to_string(),
                    );
                    append(scene, &container);
                    container
                });
            let view = node(item);
            append(container, &view);
            nodes.insert(
                item.name.clone(),
                Node {
                    element: view,
                    rect: item.rect,
                    sprite: item.sprite.clone(),
                    frames: item.frames,
                    saturation: item.saturation,
                },
            );
        }
        let definitions: HashMap<String, Button> = layout
            .elements
            .iter()
            .filter(|item| mounted(&item.scene))
            .filter_map(|item| Some((item.name.clone(), item.button.clone()?)))
            .collect();
        let buttons = layout
            .elements
            .iter()
            .filter_map(|item| {
                Some((
                    item.name.clone(),
                    button_view(definitions.get(&item.name)?, item.visible, &nodes),
                ))
            })
            .collect();
        append(parent, &root);
        Self {
            root,
            scenes,
            nodes,
            buttons,
            definitions,
            height: 0.0,
        }
    }

    pub fn remove(&self) {
        self.root.remove();
    }

    pub fn resize(&mut self, height: f32) {
        if self.height != height {
            self.height = height;
            set_style(
                &self.root,
                "--ui-scale",
                &format!("{}", height / SOURCE_HEIGHT),
            );
        }
    }

    pub fn on_click(&self, button: &str, handler: impl Fn(web_sys::Event) + 'static) {
        if let Some(region) = self
            .buttons
            .get(button)
            .and_then(|view| view.region.as_ref())
        {
            on(region, "click", handler);
        }
    }

    pub fn select(&self, button: &str, selected: bool) {
        if let Some(view) = self.buttons.get(button)
            && view.selected.get() != selected
        {
            view.selected.set(selected);
            view.refresh();
        }
    }

    pub fn sound(&self, button: &str) -> Option<String> {
        let selected = self.buttons.get(button)?.selected.get();
        let [pressed, reselected] = &self.definitions.get(button)?.sounds;
        let event = if selected {
            reselected.as_ref().or(pressed.as_ref())
        } else {
            pressed.as_ref()
        }?;
        Some(asset::audio::interface_sound(event))
    }

    pub fn enable(&self, button: &str, enabled: bool) {
        if let Some(view) = self.buttons.get(button)
            && view.enabled.get() != enabled
        {
            view.enabled.set(enabled);
            view.refresh();
        }
    }

    pub fn show_button(&self, button: &str, visible: bool) {
        if let Some(view) = self.buttons.get(button) {
            view.show(visible);
        }
    }

    pub fn scene(&self, name: &str) -> Option<&HtmlElement> {
        self.scenes.get(name)
    }

    pub fn show_scene(&self, name: &str, visible: bool) {
        if let Some(scene) = self.scenes.get(name) {
            show(scene, visible);
        }
    }

    pub fn instance(&mut self, template: &str, name: &str, offset: [f32; 2]) {
        let prefix = format!("{template}/");
        let copies: Vec<(String, Node)> = self
            .nodes
            .iter()
            .filter(|(source, _)| source.starts_with(&prefix))
            .filter_map(|(source, node)| {
                let element: HtmlElement = node.element.clone_node().ok()?.dyn_into().ok()?;
                let rect = Rect {
                    position: [
                        node.rect.position[0] + offset[0],
                        node.rect.position[1] + offset[1],
                    ],
                    ..node.rect
                };
                let [x, y] = self::offset(&rect);
                set_style(&element, "left", &format!("{x}px"));
                set_style(&element, "top", &format!("{y}px"));
                let parent = node.element.parent_element()?;
                let _ = parent.append_child(&element);
                Some((
                    source.replacen(template, name, 1),
                    Node {
                        element,
                        rect,
                        sprite: node.sprite.clone(),
                        frames: node.frames,
                        saturation: node.saturation,
                    },
                ))
            })
            .collect();
        self.nodes.extend(copies);
        let copies: Vec<(String, Button)> = self
            .definitions
            .iter()
            .filter(|(source, _)| *source == template || source.starts_with(&prefix))
            .map(|(source, button)| {
                let rename = |part: &String| part.replacen(template, name, 1);
                let copy = Button {
                    sounds: button.sounds.clone(),
                    region: button.region.as_ref().map(rename),
                    states: button
                        .states
                        .each_ref()
                        .map(|names| names.iter().map(rename).collect()),
                };
                (rename(source), copy)
            })
            .collect();
        for (copy, button) in copies {
            self.buttons
                .insert(copy.clone(), button_view(&button, false, &self.nodes));
            self.definitions.insert(copy, button);
        }
    }

    pub fn add(&mut self, name: &str, from: &str, class: &str) {
        add_node(&mut self.nodes, name, from, class);
    }
}

impl View for Frame {
    fn node(&self, name: &str) -> Option<&Node> {
        self.nodes.get(name)
    }
}

pub struct Template {
    items: Vec<Element>,
    scene: String,
}

impl Template {
    pub fn new(layout: &Layout, scene: &str, anchor: &str, parts: &[&str]) -> Self {
        let origin = layout
            .element(&format!("{scene}/{anchor}"))
            .map_or([0.0; 2], |anchor| anchor.rect.position);
        let items = parts
            .iter()
            .filter_map(|part| layout.element(&format!("{scene}/{part}")))
            .map(|item| {
                let mut item = item.clone();
                item.rect.position = [
                    item.rect.position[0] - origin[0],
                    item.rect.position[1] - origin[1],
                ];
                item
            })
            .collect();
        Self {
            items,
            scene: scene.to_owned(),
        }
    }

    pub fn instance(&self, parent: &HtmlElement) -> Instance {
        let root = element("div", "frame-instance");
        let prefix = format!("{}/", self.scene);
        let nodes = self
            .items
            .iter()
            .map(|item| {
                let view = node(item);
                append(&root, &view);
                (
                    item.name.trim_start_matches(&prefix).to_owned(),
                    Node {
                        element: view,
                        rect: item.rect,
                        sprite: item.sprite.clone(),
                        frames: item.frames,
                        saturation: item.saturation,
                    },
                )
            })
            .collect();
        append(parent, &root);
        Instance { root, nodes }
    }
}

pub struct Instance {
    pub root: HtmlElement,
    nodes: HashMap<String, Node>,
}

impl Instance {
    pub fn add(&mut self, name: &str, from: &str, class: &str) {
        add_node(&mut self.nodes, name, from, class);
    }
}

impl View for Instance {
    fn node(&self, name: &str) -> Option<&Node> {
        self.nodes.get(name)
    }
}

pub trait View {
    fn node(&self, name: &str) -> Option<&Node>;

    fn get(&self, name: &str) -> Option<&HtmlElement> {
        self.node(name).map(|node| &node.element)
    }

    fn place_scaled(&self, name: &str, width: f32, height: f32, scale: f32) -> Option<[f32; 4]> {
        self.node(name).map(|node| {
            let rect = node.rect;
            let base = rect.place(width, height);
            [
                rect.anchor[0] * width + (base[0] - rect.anchor[0] * width) * scale,
                rect.anchor[1] * height + (base[1] - rect.anchor[1] * height) * scale,
                base[2] * scale,
                base[3] * scale,
            ]
        })
    }

    fn show(&self, name: &str, visible: bool) {
        if let Some(view) = self.get(name) {
            show(view, visible);
        }
    }

    fn text(&self, name: &str, value: &str) {
        if let Some(view) = self.get(name) {
            set_text(view, value);
        }
    }

    fn image(&self, name: &str, url: &str) {
        if let Some(view) = self.get(name) {
            set_style(view, "background-image", &format!("url(\"{url}\")"));
        }
    }

    fn desaturate(&self, name: &str, desaturated: bool) {
        if let Some(node) = self.node(name) {
            set_style(
                &node.element,
                "filter",
                &if desaturated {
                    format!("grayscale({:.2})", 1.0 - node.saturation)
                } else {
                    String::new()
                },
            );
        }
    }

    fn follow(&self, tip: &str, bar: &str, fraction: f32) {
        let (Some(tip), Some(bar)) = (self.node(tip), self.node(bar)) else {
            return;
        };
        set_style(
            &tip.element,
            "transform",
            &format!(
                "translateX({:.2}px)",
                -(1.0 - fraction.clamp(0.0, 1.0)) * bar.rect.size[0]
            ),
        );
    }

    fn sweep(&self, name: &str, elapsed: f32, color: &str) {
        if let Some(view) = self.get(name) {
            set_style(
                view,
                "background",
                &format!(
                    "conic-gradient({color} {:.2}%, transparent 0)",
                    elapsed.clamp(0.0, 1.0) * 100.0
                ),
            );
        }
    }

    fn fill(&self, name: &str, fraction: f32) {
        if let Some(view) = self.get(name) {
            set_style(
                view,
                "clip-path",
                &format!(
                    "inset(0 {:.2}% 0 0)",
                    (1.0 - fraction.clamp(0.0, 1.0)) * 100.0
                ),
            );
        }
    }

    fn fill_up(&self, name: &str, fraction: f32) {
        if let Some(view) = self.get(name) {
            set_style(
                view,
                "clip-path",
                &format!(
                    "inset({:.2}% 0 0 0)",
                    (1.0 - fraction.clamp(0.0, 1.0)) * 100.0
                ),
            );
        }
    }

    fn span(&self, name: &str, from: f32, to: f32) {
        if let Some(view) = self.get(name) {
            set_style(
                view,
                "clip-path",
                &format!(
                    "inset(0 {:.2}% 0 {:.2}%)",
                    (1.0 - to.clamp(0.0, 1.0)) * 100.0,
                    from.clamp(0.0, 1.0) * 100.0
                ),
            );
        }
    }

    fn radial(&self, name: &str, remaining: f32) {
        if let Some(view) = self.get(name) {
            let done = (1.0 - remaining.clamp(0.0, 1.0)) * 100.0;
            let mask = format!("conic-gradient(transparent {done:.2}%, #000 {done:.2}%)");
            set_style(view, "mask-image", &mask);
            set_style(view, "-webkit-mask-image", &mask);
        }
    }

    fn animate(&self, name: &str, elapsed: f32) {
        let Some(node) = self.node(name) else {
            return;
        };
        let (Some(frames), Some(sprite)) = (node.frames, &node.sprite) else {
            return;
        };
        let index = ((elapsed * frames.fps) as u16).min(frames.count.saturating_sub(1));
        let per_row = frames.per_row.max(1);
        let [u0, v0, u1, v1] = sprite.uv;
        let column = f32::from(index % per_row);
        let row = f32::from(index / per_row);
        let uv = [
            u0 + column * (u1 - u0),
            v0 + row * (v1 - v0),
            u1 + column * (u1 - u0),
            v1 + row * (v1 - v0),
        ];
        set_style(
            &node.element,
            "background-position",
            &background_position(uv, node.rect.size),
        );
    }

    fn duration(&self, name: &str) -> f32 {
        self.node(name)
            .and_then(|node| node.frames)
            .map_or(0.0, |frames| f32::from(frames.count) / frames.fps.max(1.0))
    }

    fn interactive(&self, name: &str) -> Option<HtmlElement> {
        let view = self.get(name)?;
        set_style(view, "pointer-events", "auto");
        show(view, true);
        Some(view.clone())
    }

    fn tint(&self, name: &str, color: [u8; 3]) {
        if let Some(node) = self.node(name)
            && let Some(sprite) = &node.sprite
        {
            multiply(&node.element, &sprite.texture, color);
        }
    }

    fn rect(&self, name: &str) -> Option<Rect> {
        self.node(name).map(|node| node.rect)
    }

    fn move_to(&self, name: &str, position: [f32; 2]) {
        if let Some(node) = self.node(name) {
            let [x, y] = offset(&Rect {
                position,
                ..node.rect
            });
            set_style(&node.element, "left", &format!("{x}px"));
            set_style(&node.element, "top", &format!("{y}px"));
        }
    }
}

fn button_view(button: &Button, visible: bool, nodes: &HashMap<String, Node>) -> Rc<ButtonView> {
    let view = Rc::new(ButtonView {
        states: button.states.each_ref().map(|names| {
            names
                .iter()
                .filter_map(|name| nodes.get(name))
                .map(|node| node.element.clone())
                .collect()
        }),
        region: button
            .region
            .as_ref()
            .and_then(|name| nodes.get(name))
            .map(|node| node.element.clone()),
        hovered: Cell::new(false),
        pressed: Cell::new(false),
        selected: Cell::new(false),
        enabled: Cell::new(true),
        visible: Cell::new(visible),
    });
    view.show(visible);
    if let Some(region) = &view.region {
        set_style(region, "pointer-events", "auto");
        let events: [(&str, ButtonChange); 4] = [
            ("mouseenter", |view| view.hovered.set(true)),
            ("mouseleave", |view| {
                view.hovered.set(false);
                view.pressed.set(false);
            }),
            ("mousedown", |view| view.pressed.set(true)),
            ("mouseup", |view| view.pressed.set(false)),
        ];
        for (event, change) in events {
            let view = view.clone();
            on(region, event, move |_| {
                change(&view);
                view.refresh();
            });
        }
    }
    view
}

fn add_node(nodes: &mut HashMap<String, Node>, name: &str, from: &str, class: &str) {
    let Some(source) = nodes.get(from) else {
        return;
    };
    let view = element("div", &format!("frame-node {class}"));
    let [x, y] = offset(&source.rect);
    set_style(&view, "left", &format!("{x}px"));
    set_style(&view, "top", &format!("{y}px"));
    set_style(&view, "width", &format!("{}px", source.rect.size[0]));
    set_style(&view, "height", &format!("{}px", source.rect.size[1]));
    set_style(
        &view,
        "z-index",
        &source
            .element
            .style()
            .get_property_value("z-index")
            .unwrap_or_default(),
    );
    if let Some(parent) = source.element.parent_element() {
        let _ = parent.insert_before(&view, source.element.next_sibling().as_ref());
    }
    nodes.insert(
        name.to_owned(),
        Node {
            element: view,
            rect: source.rect,
            sprite: None,
            frames: None,
            saturation: 0.0,
        },
    );
}

fn offset(rect: &Rect) -> [f32; 2] {
    [
        rect.position[0] - rect.anchor[0] * rect.source[0],
        rect.position[1] - rect.anchor[1] * rect.source[1],
    ]
}

fn node(item: &Element) -> HtmlElement {
    let view = element(
        "div",
        match item.kind {
            Kind::Text => "frame-node frame-text",
            Kind::Desaturate | Kind::Icon if item.sprite.is_none() => "frame-node frame-image",
            _ => "frame-node",
        },
    );
    let [x, y] = offset(&item.rect);
    let [width, height] = item.rect.size;
    set_style(&view, "left", &format!("{x}px"));
    set_style(&view, "top", &format!("{y}px"));
    set_style(&view, "width", &format!("{width}px"));
    set_style(&view, "height", &format!("{height}px"));
    set_style(&view, "z-index", &depth(item).to_string());
    if item.round {
        set_style(&view, "border-radius", "50%");
    }
    if let Some(sprite) = &item.sprite {
        set_style(
            &view,
            "background-image",
            &format!("url(\"{FOLDER}/{}\")", sprite.texture),
        );
        let [u0, v0, u1, v1] = sprite.uv;
        let scale = [
            width / (u1 - u0).abs().max(1.0),
            height / (v1 - v0).abs().max(1.0),
        ];
        set_style(
            &view,
            "background-size",
            &format!(
                "{}px {}px",
                sprite.size[0] * scale[0],
                sprite.size[1] * scale[1]
            ),
        );
        set_style(
            &view,
            "background-position",
            &background_position(sprite.uv, item.rect.size),
        );
        let flip = [u1 < u0, v1 < v0];
        if flip.contains(&true) {
            set_style(
                &view,
                "transform",
                &format!(
                    "scale({}, {})",
                    if flip[0] { -1 } else { 1 },
                    if flip[1] { -1 } else { 1 }
                ),
            );
        }
    }
    if let Some(text) = &item.text {
        style_text(&view, text, item.rect.source[1] / SOURCE_HEIGHT);
        set_text(&view, &text.label);
    }
    tint(&view, item);
    show(&view, item.visible);
    view
}

fn depth(item: &Element) -> u32 {
    item.layer * 2 + u32::from(item.kind == Kind::Text)
}

fn tint(view: &HtmlElement, item: &Element) {
    let [red, green, blue, alpha] = item.color;
    if alpha < 255 {
        set_style(view, "opacity", &format!("{:.3}", f32::from(alpha) / 255.0));
    }
    let Some(sprite) = &item.sprite else {
        return;
    };
    if [red, green, blue] != [255; 3] {
        multiply(view, &sprite.texture, [red, green, blue]);
    }
}

fn multiply(view: &HtmlElement, texture: &str, [red, green, blue]: [u8; 3]) {
    set_style(
        view,
        "background-color",
        &format!("rgb({red}, {green}, {blue})"),
    );
    set_style(view, "background-blend-mode", "multiply");
    let style = view.style();
    for prefix in ["", "-webkit-"] {
        let image = format!("url(\"{FOLDER}/{texture}\")");
        let _ = style.set_property(&format!("{prefix}mask-image"), &image);
        for property in ["size", "position"] {
            let value = style
                .get_property_value(&format!("background-{property}"))
                .unwrap_or_default();
            let _ = style.set_property(&format!("{prefix}mask-{property}"), &value);
        }
        let _ = style.set_property(&format!("{prefix}mask-repeat"), "no-repeat");
    }
}

fn background_position(uv: [f32; 4], size: [f32; 2]) -> String {
    let [u0, v0, u1, v1] = uv;
    let scale = [
        size[0] / (u1 - u0).abs().max(1.0),
        size[1] / (v1 - v0).abs().max(1.0),
    ];
    format!("{}px {}px", -u0.min(u1) * scale[0], -v0.min(v1) * scale[1])
}

fn style_text(view: &HtmlElement, text: &TextStyle, scale: f32) {
    let [red, green, blue, alpha] = text.color;
    set_style(view, "font-family", &format!("\"{}\"", family(&text.font)));
    set_style(view, "font-size", &format!("{}px", text.size * scale));
    set_style(
        view,
        "color",
        &format!(
            "rgba({red}, {green}, {blue}, {:.3})",
            f32::from(alpha) / 255.0
        ),
    );
    set_style(
        view,
        "justify-content",
        match text.align {
            1 => "center",
            2 => "flex-end",
            _ => "flex-start",
        },
    );
    let mut shadows = Vec::new();
    if text.outline > 0.0 {
        let size = text.outline;
        for (x, y) in [
            (-1, -1),
            (0, -1),
            (1, -1),
            (-1, 0),
            (1, 0),
            (-1, 1),
            (0, 1),
            (1, 1),
        ] {
            shadows.push(format!(
                "{}px {}px 0 #000",
                x as f32 * size,
                y as f32 * size
            ));
        }
    }
    if text.shadow != [0.0, 0.0] {
        shadows.push(format!("{}px {}px 0 #000", text.shadow[0], text.shadow[1]));
    }
    if !shadows.is_empty() {
        set_style(view, "text-shadow", &shadows.join(", "));
    }
}

fn family(file: &str) -> &str {
    file.rsplit_once('.').map_or(file, |(stem, _)| stem)
}

fn fonts(layout: &Layout) -> HtmlElement {
    let files: BTreeSet<&str> = layout
        .elements
        .iter()
        .filter_map(|item| item.text.as_ref())
        .map(|text| text.font.as_str())
        .collect();
    let style = element("style", "");
    style.set_text_content(Some(
        &files
            .iter()
            .map(|file| {
                format!(
                    "@font-face {{ font-family: \"{}\"; src: url(\"{FOLDER}/{file}\"); }}",
                    family(file)
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
    ));
    style
}
