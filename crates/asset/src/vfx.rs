use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

pub const INTEGRAL_SAMPLES: usize = 16;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Random {
    pub times: Vec<f32>,
    pub values: Vec<f32>,
}

impl Random {
    pub fn factor(&self, u: f32) -> f32 {
        if self.times.is_empty() {
            return self.values.first().copied().unwrap_or(1.0);
        }
        if self.times.len() != self.values.len() {
            return 0.0;
        }
        lookup(&self.times, &self.values, u, |a, b, t| a + (b - a) * t)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Value {
    pub times: Vec<f32>,
    pub values: Vec<[f32; 4]>,
    pub random: Vec<Option<Random>>,
}

impl Default for Value {
    fn default() -> Self {
        Self::constant([0.0; 4])
    }
}

impl Value {
    pub fn constant(value: [f32; 4]) -> Self {
        Self {
            times: Vec::new(),
            values: vec![value],
            random: Vec::new(),
        }
    }

    pub fn scalar(value: f32) -> Self {
        Self::constant([value; 4])
    }

    pub fn curve(keys: &[(f32, [f32; 4])]) -> Self {
        Self {
            times: keys.iter().map(|(time, _)| *time).collect(),
            values: keys.iter().map(|(_, value)| *value).collect(),
            random: Vec::new(),
        }
    }

    pub fn is_constant(&self) -> bool {
        self.values.len() <= 1 && self.random.iter().all(Option::is_none)
    }

    pub fn sample(&self, t: f32) -> [f32; 4] {
        match self.values.len() {
            0 => [0.0; 4],
            1 => self.values[0],
            _ => lookup(&self.times, &self.values, t, |a, b, t| {
                core::array::from_fn(|channel| a[channel] + (b[channel] - a[channel]) * t)
            }),
        }
    }

    pub fn sample_random(&self, t: f32, mut random: impl FnMut() -> f32) -> [f32; 4] {
        let mut value = self.sample(t);
        for (channel, table) in value.iter_mut().zip(&self.random) {
            if let Some(table) = table {
                *channel *= table.factor(random());
            }
        }
        value
    }

    pub fn integrated(&self) -> Self {
        self.integrate(1)
    }

    pub fn integrated_twice(&self) -> Self {
        self.integrate(2)
    }

    fn integrate(&self, times: usize) -> Self {
        if self.values.len() < 2 {
            return Self::constant(self.sample(0.0));
        }
        let step = 1.0 / (INTEGRAL_SAMPLES - 1) as f32;
        let mut keys: Vec<(f32, [f32; 4])> = (0..INTEGRAL_SAMPLES)
            .map(|index| {
                let time = index as f32 * step;
                (time, self.sample(time))
            })
            .collect();
        for _ in 0..times {
            let mut total = [0.0; 4];
            let mut previous = keys[0].1;
            keys[0].1 = total;
            for key in keys.iter_mut().skip(1) {
                let current = key.1;
                for channel in 0..4 {
                    total[channel] += (previous[channel] + current[channel]) * 0.5 * step;
                }
                previous = current;
                key.1 = total;
            }
        }
        Self::curve(&keys)
    }
}

fn lookup<T: Copy>(times: &[f32], values: &[T], t: f32, lerp: impl Fn(T, T, f32) -> T) -> T {
    let last = values.len().min(times.len()) - 1;
    if t <= times[0] {
        return values[0];
    }
    if t >= times[last] {
        return values[last];
    }
    let next = times[..=last].partition_point(|time| *time <= t);
    let (start, end) = (times[next - 1], times[next]);
    let span = end - start;
    let blend = if span > 0.0 { (t - start) / span } else { 1.0 };
    lerp(values[next - 1], values[next], blend)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Blend {
    Add,
    #[default]
    Alpha,
    Subtract,
    Opaque,
    AlphaAdd,
    Premultiplied,
    Min,
    Max,
}

impl Blend {
    pub fn from_id(id: u64) -> Self {
        match id {
            0 => Self::Add,
            2 => Self::Subtract,
            3 => Self::Opaque,
            4 => Self::AlphaAdd,
            5 => Self::Premultiplied,
            6 => Self::Min,
            7 => Self::Max,
            _ => Self::Alpha,
        }
    }

    pub fn rank(self) -> u8 {
        match self {
            Self::Opaque => 0,
            Self::Add | Self::Subtract => 1,
            _ => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub enum Primitive {
    #[default]
    Billboard,
    Quad,
    Ray,
    Mesh(u16),
    Trail {
        tiling: f32,
    },
    Beam {
        tiling: f32,
        width: f32,
        transposed: bool,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lookup {
    #[default]
    Constant,
    Lifetime,
    Velocity,
    Random,
}

impl Lookup {
    pub fn from_id(id: u64) -> Self {
        match id {
            1 => Self::Lifetime,
            2 => Self::Velocity,
            3 => Self::Random,
            _ => Self::Constant,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    pub axis: [f32; 3],
    pub angle: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Child {
    pub systems: Vec<u16>,
    pub choice: Value,
    pub on_death: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Emitter {
    pub name: String,
    pub delay: f32,
    pub end: Option<f32>,
    pub single: bool,
    pub rate: Value,
    pub lifetime: Value,
    pub linger: Option<f32>,
    pub position: Value,
    pub offset: Value,
    pub turns: Vec<Turn>,
    pub velocity: Value,
    pub acceleration: Value,
    pub drag: Value,
    pub gravity: Value,
    pub bind: Value,
    pub local: bool,
    pub directional: bool,
    pub stretch: [f32; 2],
    pub primitive: Primitive,
    pub blend: Blend,
    pub pass: i16,
    pub depth: bool,
    pub ground: bool,
    pub alpha_ref: f32,
    pub texture: Option<u16>,
    pub grid: [f32; 2],
    pub frames: u16,
    pub frame_rate: f32,
    pub start_frame: u16,
    pub random_frame: bool,
    pub color_texture: Option<u16>,
    pub mult_texture: Option<u16>,
    pub mult_grid: [f32; 2],
    pub mult_random_frame: bool,
    pub lookup: [Lookup; 2],
    pub lookup_scale: [f32; 2],
    pub lookup_offset: [f32; 2],
    pub birth_color: Value,
    pub color: Value,
    pub birth_scale: Value,
    pub scale: Value,
    pub uniform: bool,
    pub birth_rotation: Value,
    pub spin: Value,
    pub rotation: Value,
    pub uv_offset: Value,
    pub uv_scroll: Value,
    pub lock_alpha: bool,
    pub child: Option<Child>,
}

impl Default for Emitter {
    fn default() -> Self {
        Self {
            name: String::new(),
            delay: 0.0,
            end: None,
            single: false,
            rate: Value::scalar(0.0),
            lifetime: Value::scalar(-1.0),
            linger: None,
            position: Value::default(),
            offset: Value::default(),
            turns: Vec::new(),
            velocity: Value::default(),
            acceleration: Value::default(),
            drag: Value::default(),
            gravity: Value::default(),
            bind: Value::default(),
            local: false,
            directional: false,
            stretch: [0.0; 2],
            primitive: Primitive::Billboard,
            blend: Blend::Alpha,
            pass: 0,
            depth: true,
            ground: false,
            alpha_ref: 0.0,
            texture: None,
            grid: [1.0; 2],
            frames: 1,
            frame_rate: 0.0,
            start_frame: 0,
            random_frame: false,
            color_texture: None,
            mult_texture: None,
            mult_grid: [1.0; 2],
            mult_random_frame: false,
            lookup: [Lookup::Lifetime, Lookup::Constant],
            lookup_scale: [1.0; 2],
            lookup_offset: [0.0; 2],
            birth_color: Value::scalar(1.0),
            color: Value::scalar(1.0),
            birth_scale: Value::scalar(1.0),
            scale: Value::scalar(1.0),
            uniform: false,
            birth_rotation: Value::default(),
            spin: Value::default(),
            rotation: Value::default(),
            uv_offset: Value::default(),
            uv_scroll: Value::default(),
            lock_alpha: false,
            child: None,
        }
    }
}

impl Emitter {
    pub fn duration(&self) -> Option<f32> {
        if self.single {
            return Some(self.delay);
        }
        self.end
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct System {
    pub name: String,
    pub emitters: Vec<Emitter>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colors: Vec<[u8; 4]>,
    pub indices: Vec<u16>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Effect {
    pub systems: Vec<System>,
    pub textures: Vec<String>,
    pub meshes: Vec<Mesh>,
}

impl Effect {
    pub fn to_bytes(&self) -> postcard::Result<Vec<u8>> {
        postcard::to_allocvec(self)
    }

    pub fn from_bytes(bytes: &[u8]) -> postcard::Result<Self> {
        postcard::from_bytes(bytes)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Attach {
    Position,
    Unit,
    Missile,
    Hit,
    Beam,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spawn {
    pub effect: String,
    pub attach: Attach,
    pub delay: f32,
    pub duration: Option<f32>,
    pub bone: Option<String>,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EffectEntry {
    pub name: u32,
    pub spawns: Vec<Spawn>,
    pub enemy: Vec<Spawn>,
    pub cancels: Vec<u32>,
    pub start_bone: Option<String>,
    pub target_bone: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ClipEffects {
    pub character: String,
    pub clip: u32,
    pub spawns: Vec<Spawn>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VfxIndex {
    pub effects: Vec<EffectEntry>,
    pub missiles: Vec<EffectEntry>,
    pub clips: Vec<ClipEffects>,
    pub idle: Vec<ClipEffects>,
}

impl VfxIndex {
    pub fn to_bytes(&self) -> postcard::Result<Vec<u8>> {
        postcard::to_allocvec(self)
    }

    pub fn from_bytes(bytes: &[u8]) -> postcard::Result<Self> {
        postcard::from_bytes(bytes)
    }

    pub fn effect(&self, name: u32) -> Option<&EffectEntry> {
        find(&self.effects, name)
    }

    pub fn missile(&self, kind: u32) -> Option<&EffectEntry> {
        find(&self.missiles, kind)
    }

    pub fn idle(&self, character: &str) -> &[Spawn] {
        self.idle
            .iter()
            .find(|entry| entry.character.eq_ignore_ascii_case(character))
            .map_or(&[], |entry| entry.spawns.as_slice())
    }

    pub fn clip(&self, character: &str, clip: u32) -> &[Spawn] {
        self.clips
            .iter()
            .find(|entry| entry.clip == clip && entry.character.eq_ignore_ascii_case(character))
            .map_or(&[], |entry| entry.spawns.as_slice())
    }
}

fn find(entries: &[EffectEntry], name: u32) -> Option<&EffectEntry> {
    entries
        .binary_search_by_key(&name, |entry| entry.name)
        .ok()
        .map(|index| &entries[index])
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::borrow::ToOwned;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn curves_hold_ends_and_interpolate_between_keys() {
        let value = Value::curve(&[(0.2, [0.0; 4]), (0.6, [4.0; 4]), (1.0, [2.0; 4])]);
        assert_eq!(value.sample(0.0)[0], 0.0);
        assert!(close(value.sample(0.4)[0], 2.0));
        assert!(close(value.sample(0.8)[0], 3.0));
        assert_eq!(value.sample(5.0)[0], 2.0);
        assert_eq!(Value::scalar(3.0).sample(0.7), [3.0; 4]);
    }

    #[test]
    fn probability_tables_scale_each_channel() {
        let mut value = Value::constant([10.0, 10.0, 10.0, 10.0]);
        value.random = vec![
            Some(Random {
                times: vec![0.0, 1.0],
                values: vec![0.5, 1.5],
            }),
            None,
            Some(Random {
                times: Vec::new(),
                values: vec![2.0],
            }),
        ];
        let sampled = value.sample_random(0.0, || 0.5);
        assert!(close(sampled[0], 10.0));
        assert_eq!(sampled[1], 10.0);
        assert_eq!(sampled[2], 20.0);
        assert_eq!(sampled[3], 10.0);
        let low = value.sample_random(0.0, || 0.0);
        assert!(close(low[0], 5.0));
    }

    #[test]
    fn mismatched_tables_zero_the_channel() {
        let table = Random {
            times: vec![0.0, 1.0],
            values: vec![1.0],
        };
        assert_eq!(table.factor(0.3), 0.0);
    }

    #[test]
    fn integration_accumulates_rates() {
        let rate = Value::curve(&[(0.0, [2.0; 4]), (1.0, [2.0; 4])]);
        let once = rate.integrated();
        assert!(close(once.sample(0.5)[0], 1.0));
        assert!(close(once.sample(1.0)[0], 2.0));
        let twice = rate.integrated_twice();
        assert!(close(twice.sample(1.0)[0], 1.0));
        assert!((twice.sample(0.5)[0] - 0.25).abs() < 0.01);
        assert_eq!(Value::scalar(4.0).integrated(), Value::scalar(4.0));
    }

    #[test]
    fn index_round_trips_and_finds_entries() {
        let spawn = Spawn {
            effect: "slash_effect".to_owned(),
            attach: Attach::Unit,
            delay: 0.1,
            duration: Some(2.0),
            bone: None,
            target: None,
        };
        let entry = EffectEntry {
            name: 3,
            spawns: vec![spawn.clone()],
            enemy: Vec::new(),
            cancels: vec![9],
            start_bone: Some("Spine2".into()),
            target_bone: None,
        };
        let index = VfxIndex {
            effects: vec![
                entry.clone(),
                EffectEntry {
                    name: 9,
                    ..EffectEntry::default()
                },
            ],
            missiles: vec![entry.clone()],
            clips: vec![ClipEffects {
                character: "Wanderer".to_owned(),
                clip: 7,
                spawns: vec![spawn.clone()],
            }],
            idle: vec![ClipEffects {
                character: "Emperor".to_owned(),
                clip: 0,
                spawns: vec![spawn.clone()],
            }],
        };
        let decoded = VfxIndex::from_bytes(&index.to_bytes().unwrap()).unwrap();
        assert_eq!(decoded, index);
        assert_eq!(decoded.effect(3), Some(&entry));
        assert!(decoded.effect(4).is_none());
        assert_eq!(decoded.missile(3), Some(&entry));
        assert!(decoded.missile(9).is_none());
        assert_eq!(decoded.idle("emperor"), core::slice::from_ref(&spawn));
        assert_eq!(decoded.clip("wanderer", 7), &[spawn]);
        assert!(decoded.clip("Guardian", 7).is_empty());
        assert!(decoded.idle("Wanderer").is_empty());
    }

    #[test]
    fn effects_round_trip() {
        let effect = Effect {
            systems: vec![System {
                name: "test".to_owned(),
                emitters: vec![Emitter::default()],
            }],
            textures: vec!["a.png".to_owned()],
            meshes: vec![Mesh {
                positions: vec![[0.0; 3]],
                uvs: vec![[0.0; 2]],
                colors: vec![[255; 4]],
                indices: vec![0, 0, 0],
            }],
        };
        assert_eq!(
            Effect::from_bytes(&effect.to_bytes().unwrap()).unwrap(),
            effect
        );
    }
}
