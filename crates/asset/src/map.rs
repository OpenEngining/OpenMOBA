use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

pub const ORDER: u16 = 100;
pub const CHAOS: u16 = 200;

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct MapVertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub lightmap_uv: [f32; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Blending {
    Opaque,
    AlphaTest(f32),
    Blend(f32),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapMaterial {
    pub name: String,
    pub texture: Option<u16>,
    pub tint: [f32; 4],
    pub blending: Blending,
    pub double_sided: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MapDraw {
    pub material: u16,
    pub lightmap: Option<u16>,
    pub start: u32,
    pub count: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MapGeometry {
    pub textures: Vec<String>,
    pub materials: Vec<MapMaterial>,
    pub vertices: Vec<MapVertex>,
    pub indices: Vec<u32>,
    pub draws: Vec<MapDraw>,
    pub lightmap_scale: f32,
}

impl MapGeometry {
    pub fn to_bytes(&self) -> postcard::Result<Vec<u8>> {
        postcard::to_allocvec(self)
    }

    pub fn from_bytes(bytes: &[u8]) -> postcard::Result<Self> {
        postcard::from_bytes(bytes)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HeightField {
    pub min: [f32; 2],
    pub spacing: [f32; 2],
    pub width: u32,
    pub height: u32,
    pub samples: Vec<f32>,
}

impl HeightField {
    pub fn height_at(&self, x: f32, z: f32) -> f32 {
        if self.width == 0 || self.height == 0 {
            return 0.0;
        }
        let max_x = (self.width - 1) as f32;
        let max_z = (self.height - 1) as f32;
        let u = ((x - self.min[0]) / self.spacing[0]).clamp(0.0, max_x);
        let v = ((z - self.min[1]) / self.spacing[1]).clamp(0.0, max_z);
        let (x0, z0) = (u as u32, v as u32);
        let (x1, z1) = ((x0 + 1).min(self.width - 1), (z0 + 1).min(self.height - 1));
        let (tx, tz) = (u - x0 as f32, v - z0 as f32);
        let sample = |x: u32, z: u32| self.samples[(z * self.width + x) as usize];
        let top = sample(x0, z0) + (sample(x1, z0) - sample(x0, z0)) * tx;
        let bottom = sample(x0, z1) + (sample(x1, z1) - sample(x0, z1)) * tx;
        top + (bottom - top) * tz
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NavGrid {
    pub min: [f32; 2],
    pub cell_size: f32,
    pub width: u32,
    pub height: u32,
    pub flags: Vec<u16>,
    pub heights: HeightField,
}

impl NavGrid {
    pub const BRUSH: u16 = 1;
    pub const WALL: u16 = 2;

    pub fn cell_at(&self, x: f32, z: f32) -> Option<(u32, u32)> {
        let u = (x - self.min[0]) / self.cell_size;
        let v = (z - self.min[1]) / self.cell_size;
        (u >= 0.0 && v >= 0.0 && u < self.width as f32 && v < self.height as f32)
            .then_some((u as u32, v as u32))
    }

    pub fn cell_center(&self, cell: (u32, u32)) -> [f32; 2] {
        [
            self.min[0] + (cell.0 as f32 + 0.5) * self.cell_size,
            self.min[1] + (cell.1 as f32 + 0.5) * self.cell_size,
        ]
    }

    pub fn flags_at(&self, x: f32, z: f32) -> u16 {
        self.cell_at(x, z).map_or(Self::WALL, |(cx, cz)| {
            self.flags[(cz * self.width + cx) as usize]
        })
    }

    pub fn is_walkable(&self, x: f32, z: f32) -> bool {
        self.flags_at(x, z) & Self::WALL == 0
    }

    pub fn is_brush(&self, x: f32, z: f32) -> bool {
        self.flags_at(x, z) & Self::BRUSH != 0
    }

    pub fn height_at(&self, x: f32, z: f32) -> f32 {
        self.heights.height_at(x, z)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectKind {
    Turret,
    FountainTurret,
    Inhibitor,
    Nexus,
    HealthRelic,
    Shopkeeper,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapObject {
    pub kind: ObjectKind,
    pub team: u16,
    pub position: [f32; 3],
    pub facing: f32,
    pub record: String,
    pub skin: String,
    pub lane: Option<u8>,
    pub active_at_start: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MinionKind {
    Melee,
    Ranged,
    Siege,
    Super,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SpawnRule {
    Constant(u32),
    PerInhibitorDown(Vec<u32>),
    Rotating { start: f32, counts: Vec<u32> },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct MinionUpgrade {
    pub health: f32,
    pub health_growth: f32,
    pub health_late: f32,
    pub health_max_bonus: f32,
    pub damage: f32,
    pub damage_max: f32,
    pub armor: f32,
    pub gold_max: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BarracksUnit {
    pub kind: MinionKind,
    pub record: String,
    pub spawn: SpawnRule,
    pub upgrade: MinionUpgrade,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Barracks {
    pub team: u16,
    pub position: [f32; 3],
    pub initial_spawn: f32,
    pub wave_interval: f32,
    pub minion_interval: f32,
    pub upgrade_interval: f32,
    pub upgrades_before_late_scaling: u32,
    pub move_speed_increase_delay: f32,
    pub move_speed_increase_interval: f32,
    pub move_speed_increase: f32,
    pub move_speed_increase_times: u32,
    pub experience_radius: f32,
    pub gold_radius: f32,
    pub units: Vec<BarracksUnit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Fountain {
    pub team: u16,
    pub position: [f32; 3],
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MapData {
    pub bounds_min: [f32; 2],
    pub bounds_max: [f32; 2],
    pub nav: NavGrid,
    pub objects: Vec<MapObject>,
    pub fountains: Vec<Fountain>,
    pub barracks: Vec<Barracks>,
    pub lane: Vec<[f32; 2]>,
}

impl MapData {
    pub fn to_bytes(&self) -> postcard::Result<Vec<u8>> {
        postcard::to_allocvec(self)
    }

    pub fn from_bytes(bytes: &[u8]) -> postcard::Result<Self> {
        postcard::from_bytes(bytes)
    }

    pub fn fountain(&self, team: u16) -> Option<[f32; 3]> {
        self.fountains
            .iter()
            .find(|fountain| fountain.team == team)
            .map(|fountain| fountain.position)
    }

    pub fn lane_from(&self, team: u16) -> Vec<[f32; 2]> {
        let mut lane = self.lane.clone();
        if team == CHAOS {
            lane.reverse();
        }
        lane
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn grid() -> NavGrid {
        NavGrid {
            min: [100.0, 200.0],
            cell_size: 50.0,
            width: 3,
            height: 2,
            flags: vec![0, NavGrid::BRUSH, NavGrid::WALL, 0, 0, NavGrid::WALL | 64],
            heights: HeightField {
                min: [100.0, 200.0],
                spacing: [10.0, 10.0],
                width: 2,
                height: 2,
                samples: vec![0.0, 10.0, 20.0, 30.0],
            },
        }
    }

    #[test]
    fn finds_cells_and_rejects_outside_points() {
        let grid = grid();
        assert_eq!(grid.cell_at(100.0, 200.0), Some((0, 0)));
        assert_eq!(grid.cell_at(249.9, 299.9), Some((2, 1)));
        assert_eq!(grid.cell_at(99.9, 250.0), None);
        assert_eq!(grid.cell_at(250.0, 250.0), None);
        assert_eq!(grid.cell_center((1, 1)), [175.0, 275.0]);
    }

    #[test]
    fn walls_and_outside_are_not_walkable() {
        let grid = grid();
        assert!(grid.is_walkable(120.0, 220.0));
        assert!(grid.is_walkable(170.0, 220.0));
        assert!(grid.is_brush(170.0, 220.0));
        assert!(!grid.is_walkable(220.0, 220.0));
        assert!(!grid.is_walkable(220.0, 270.0));
        assert!(!grid.is_walkable(0.0, 0.0));
    }

    #[test]
    fn heights_interpolate_bilinearly_and_clamp() {
        let grid = grid();
        assert_eq!(grid.height_at(100.0, 200.0), 0.0);
        assert_eq!(grid.height_at(105.0, 200.0), 5.0);
        assert_eq!(grid.height_at(100.0, 205.0), 10.0);
        assert_eq!(grid.height_at(105.0, 205.0), 15.0);
        assert_eq!(grid.height_at(-500.0, 9000.0), 20.0);
        assert_eq!(grid.height_at(9000.0, 9000.0), 30.0);
    }

    #[test]
    fn lane_runs_from_each_teams_side() {
        let data = MapData {
            lane: vec![[0.0, 0.0], [1.0, 1.0], [2.0, 2.0]],
            ..MapData::default()
        };
        assert_eq!(data.lane_from(ORDER)[0], [0.0, 0.0]);
        assert_eq!(data.lane_from(CHAOS)[0], [2.0, 2.0]);
    }
}
