use asset::map::NavGrid;
use glam::Vec2;
use protocol::Team;

use crate::unit::Tier;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Structure {
    Turret(Tier),
    Inhibitor,
    Nexus,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub team: Team,
    pub structure: Structure,
    pub position: Vec2,
    pub facing: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub fountains: [Vec2; 2],
    pub lane: Vec<Vec2>,
    pub structures: Vec<Placement>,
    pub relics: Vec<Vec2>,
    pub fountain_radius: f32,
    pub first_wave: f32,
    pub wave_interval: f32,
    pub barracks: [Vec2; 2],
    pub minion_interval: f32,
    pub nav: Option<NavGrid>,
}

pub fn team_index(team: Team) -> usize {
    match team {
        Team::Blue => 0,
        Team::Red => 1,
    }
}

impl Layout {
    pub fn fountain(&self, team: Team) -> Vec2 {
        self.fountains[team_index(team)]
    }

    pub fn lane_for(&self, team: Team) -> Vec<Vec2> {
        match team {
            Team::Blue => self.lane.clone(),
            Team::Red => self.lane.iter().rev().copied().collect(),
        }
    }

    pub fn placeholder() -> Self {
        let blue = Vec2::new(1000.0, 1000.0);
        let red = Vec2::new(12000.0, 12000.0);
        let along = |t: f32| blue.lerp(red, t);
        let structures = [Team::Blue, Team::Red]
            .into_iter()
            .flat_map(|team| {
                let at = |t: f32| match team {
                    Team::Blue => along(t),
                    Team::Red => along(1.0 - t),
                };
                let side = |offset: f32| Vec2::new(-offset, offset);
                [
                    (Structure::Turret(Tier::Outer), at(0.38)),
                    (Structure::Turret(Tier::Inner), at(0.28)),
                    (Structure::Inhibitor, at(0.22)),
                    (Structure::Turret(Tier::Nexus), at(0.14) + side(250.0)),
                    (Structure::Turret(Tier::Nexus), at(0.14) - side(250.0)),
                    (Structure::Nexus, at(0.1)),
                    (Structure::Turret(Tier::Fountain), at(0.0)),
                ]
                .into_iter()
                .map(move |(structure, position)| Placement {
                    team,
                    structure,
                    position,
                    facing: 0.0,
                })
            })
            .collect();
        Self {
            fountains: [blue, red],
            lane: (0..=10)
                .map(|step| along(0.05 + 0.09 * step as f32))
                .collect(),
            structures,
            relics: vec![along(0.42), along(0.58)],
            fountain_radius: 900.0,
            first_wave: crate::FIRST_WAVE,
            wave_interval: crate::WAVE_INTERVAL,
            barracks: [along(0.02), along(0.98)],
            minion_interval: crate::MINION_INTERVAL,
            nav: None,
        }
    }
}
