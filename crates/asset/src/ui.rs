use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub scenes: Vec<Scene>,
    pub elements: Vec<Element>,
    pub strings: BTreeMap<String, String>,
    pub filters: Vec<StatFilter>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StatFilter {
    pub button: String,
    pub categories: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub name: String,
    pub parent: Option<String>,
    pub visible: bool,
    pub layer: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Icon,
    Text,
    Region,
    Radial,
    Fill,
    Animation,
    Group,
    Meter,
    Button,
    Desaturate,
    Ammo,
    Cooldown,
    Scissor,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Button {
    pub region: Option<String>,
    pub states: [Vec<String>; 7],
    pub sounds: [Option<String>; 2],
}

impl Button {
    pub const DEFAULT: usize = 0;
    pub const HOVER: usize = 1;
    pub const CLICKED: usize = 2;
    pub const INACTIVE: usize = 3;
    pub const SELECTED: usize = 4;
    pub const SELECTED_HOVER: usize = 5;
    pub const SELECTED_CLICKED: usize = 6;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Element {
    pub name: String,
    pub scene: String,
    pub kind: Kind,
    pub visible: bool,
    pub layer: u32,
    pub rect: Rect,
    pub sprite: Option<Sprite>,
    pub text: Option<TextStyle>,
    pub frames: Option<Frames>,
    pub color: [u8; 4],
    pub saturation: f32,
    pub round: bool,
    pub button: Option<Button>,
    pub children: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub anchor: [f32; 2],
    pub source: [f32; 2],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sprite {
    pub texture: String,
    pub uv: [f32; 4],
    pub size: [f32; 2],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    pub font: String,
    pub size: f32,
    pub color: [u8; 4],
    pub outline: f32,
    pub shadow: [f32; 2],
    pub align: u8,
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Frames {
    pub fps: f32,
    pub count: u16,
    pub per_row: u16,
}

impl Layout {
    pub fn to_bytes(&self) -> postcard::Result<Vec<u8>> {
        postcard::to_allocvec(self)
    }

    pub fn from_bytes(bytes: &[u8]) -> postcard::Result<Self> {
        postcard::from_bytes(bytes)
    }

    pub fn element(&self, name: &str) -> Option<&Element> {
        self.elements.iter().find(|element| element.name == name)
    }

    pub fn string(&self, key: &str) -> &str {
        self.strings.get(key).map_or("", String::as_str)
    }
}

impl Rect {
    pub fn place(&self, width: f32, height: f32) -> [f32; 4] {
        let scale = height / self.source[1];
        let [anchor_x, anchor_y] = self.anchor;
        [
            anchor_x * width + (self.position[0] - anchor_x * self.source[0]) * scale,
            anchor_y * height + (self.position[1] - anchor_y * self.source[1]) * scale,
            self.size[0] * scale,
            self.size[1] * scale,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bottom_centered_rects_stay_centered_on_wide_screens() {
        let rect = Rect {
            position: [768.0, 1054.0],
            size: [64.0, 64.0],
            anchor: [0.5, 1.0],
            source: [1600.0, 1200.0],
        };
        let [x, y, width, height] = rect.place(1920.0, 1080.0);
        assert!((x - (960.0 - 32.0 * 0.9)).abs() < 0.01);
        assert!((y - (1080.0 - 146.0 * 0.9)).abs() < 0.01);
        assert!((width - 57.6).abs() < 0.01 && (height - 57.6).abs() < 0.01);
    }
}
