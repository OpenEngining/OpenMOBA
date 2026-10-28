use std::path::Path;

use asset::map::{CHAOS, MapData, ORDER, ObjectKind};
use glam::Vec2;
use protocol::Team;

use crate::layout::{Layout, Placement, Structure};
use crate::unit::Tier;

pub fn layout(assets: &Path) -> Result<Layout, String> {
    let path = assets.join("map/data.bin");
    let bytes = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let data =
        MapData::from_bytes(&bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    from_map(data)
}

fn team(id: u16) -> Option<Team> {
    match id {
        ORDER => Some(Team::Blue),
        CHAOS => Some(Team::Red),
        _ => None,
    }
}

fn turret_tier(record: &str) -> Tier {
    let lower = record.to_lowercase();
    if lower.ends_with("ha_outer") {
        Tier::Outer
    } else if lower.ends_with("ha_inhibitor") {
        Tier::Inner
    } else {
        Tier::Nexus
    }
}

pub fn from_map(data: MapData) -> Result<Layout, String> {
    let fountain = |id: u16| {
        data.fountain(id)
            .map(|position| Vec2::new(position[0], position[2]))
            .ok_or(format!("the map has no fountain for team {id}"))
    };
    let structures = data
        .objects
        .iter()
        .filter_map(|object| {
            let structure = match object.kind {
                ObjectKind::Turret => Structure::Turret(turret_tier(&object.record)),
                ObjectKind::FountainTurret => Structure::Turret(Tier::Fountain),
                ObjectKind::Inhibitor => Structure::Inhibitor,
                ObjectKind::Nexus => Structure::Nexus,
                _ => return None,
            };
            Some(Placement {
                team: team(object.team)?,
                structure,
                position: Vec2::new(object.position[0], object.position[2]),
                facing: object.facing,
            })
        })
        .collect();
    let relics = data
        .objects
        .iter()
        .filter(|object| object.kind == ObjectKind::HealthRelic)
        .map(|object| Vec2::new(object.position[0], object.position[2]))
        .collect();
    let barracks = data.barracks.iter().find(|barracks| barracks.team == ORDER);
    let lane: Vec<Vec2> = data.lane.iter().copied().map(Vec2::from).collect();
    let barracks_at = |id: u16, fallback: Option<&Vec2>| {
        data.barracks
            .iter()
            .find(|barracks| barracks.team == id)
            .map(|barracks| Vec2::new(barracks.position[0], barracks.position[2]))
            .or(fallback.copied())
            .ok_or(format!("the map has no barracks for team {id}"))
    };
    Ok(Layout {
        fountains: [fountain(ORDER)?, fountain(CHAOS)?],
        barracks: [
            barracks_at(ORDER, lane.first())?,
            barracks_at(CHAOS, lane.last())?,
        ],
        minion_interval: barracks
            .map_or(crate::MINION_INTERVAL, |barracks| barracks.minion_interval),
        lane,
        structures,
        relics,
        fountain_radius: 900.0,
        first_wave: barracks.map_or(crate::FIRST_WAVE, |barracks| barracks.initial_spawn),
        wave_interval: barracks.map_or(crate::WAVE_INTERVAL, |barracks| barracks.wave_interval),
        nav: Some(data.nav),
    })
}
