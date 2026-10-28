#![no_std]

extern crate alloc;

pub mod audio;

pub mod map;

pub mod data;

pub mod vfx;

pub mod ui;

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

pub const LOOP: u32 = 2;

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub joints: [u8; 4],
    pub weights: [f32; 4],
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub rotation: [f32; 4],
    pub translation: [f32; 3],
    pub scale: [f32; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Joint {
    pub name: String,
    pub parent: Option<u16>,
    pub bind: Transform,
    pub inverse_bind: [f32; 16],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Submesh {
    pub name: String,
    pub start: u32,
    pub count: u32,
    pub texture: u16,
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Animation {
    pub fps: f32,
    pub frames: Vec<Vec<Transform>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Updater {
    MoveSpeed,
    IsMoving,
    TurnAngle,
    SpellTargetAngle,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClipKind {
    Atomic {
        animation: u32,
        track: u16,
        mask: Option<u16>,
        tick: Option<f32>,
    },
    Selector {
        options: Vec<(u32, f32)>,
    },
    Sequencer {
        clips: Vec<u32>,
    },
    Parallel {
        clips: Vec<u32>,
    },
    ConditionFloat {
        updater: Updater,
        pairs: Vec<(u32, f32)>,
        change_mid_play: bool,
    },
    ConditionBool {
        updater: Updater,
        when_true: u32,
        when_false: u32,
        change_mid_play: bool,
    },
    Parametric {
        updater: Updater,
        pairs: Vec<(u32, f32)>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub id: u32,
    pub name: Option<String>,
    pub flags: u32,
    pub kind: ClipKind,
}

impl Clip {
    pub fn loops(&self) -> bool {
        self.flags & LOOP != 0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub name: String,
    pub priority: u8,
    pub additive: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mask {
    pub name: String,
    pub weights: Vec<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Blend {
    pub from: u32,
    pub to: u32,
    pub time: f32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    pub clips: Vec<Clip>,
    pub tracks: Vec<Track>,
    pub masks: Vec<Mask>,
    pub blends: Vec<Blend>,
}

impl Graph {
    pub fn clip(&self, id: u32) -> Option<&Clip> {
        self.clips
            .binary_search_by_key(&id, |clip| clip.id)
            .ok()
            .map(|index| &self.clips[index])
    }

    pub fn blend(&self, from: u32, to: u32) -> Option<f32> {
        self.blends
            .binary_search_by_key(&(from, to), |blend| (blend.from, blend.to))
            .ok()
            .map(|index| self.blends[index].time)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub move_speed: f32,
    pub attack_range: f32,
    pub selection_radius: f32,
    pub pathing_radius: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Character {
    pub name: String,
    pub stats: Stats,
    pub scale: f32,
    pub self_illumination: f32,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u16>,
    pub submeshes: Vec<Submesh>,
    pub textures: Vec<String>,
    pub joints: Vec<Joint>,
    pub animations: Vec<Animation>,
    pub graph: Graph,
}

impl Character {
    pub fn to_bytes(&self) -> postcard::Result<Vec<u8>> {
        postcard::to_allocvec(self)
    }

    pub fn from_bytes(bytes: &[u8]) -> postcard::Result<Self> {
        postcard::from_bytes(bytes)
    }
}

pub fn clip_id(name: &str) -> u32 {
    if let Some(hex) = name
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        && let Ok(id) = u32::from_str_radix(hex, 16)
    {
        return id;
    }
    name.bytes().fold(0x811c_9dc5, |hash: u32, byte| {
        (hash ^ u32::from(byte.to_ascii_lowercase())).wrapping_mul(0x0100_0193)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_ids_match_expected_hashes() {
        assert_eq!(clip_id("Run_LeadIn"), 0x67e5_1852);
        assert_eq!(clip_id("Idle4_BASE"), 0xabc7_5b53);
        assert_eq!(clip_id("{41472d6f}"), 0x4147_2d6f);
    }
}
