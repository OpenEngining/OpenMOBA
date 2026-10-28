use std::collections::HashSet;

use asset::map::NavGrid;
use glam::Vec2;
use protocol::{Team, UnitId, UnitKind, sight_range};

use crate::unit::Unit;

pub const BRUSH_REACH: f32 = 450.0;

pub fn sight(unit: &Unit) -> f32 {
    sight_range(unit.kind)
}

fn in_brush(nav: Option<&NavGrid>, position: Vec2) -> bool {
    nav.is_some_and(|nav| nav.is_brush(position.x, position.y))
}

fn sees(nav: Option<&NavGrid>, viewer: &Unit, target: &Unit, time: f32) -> bool {
    let distance = viewer.position.distance(target.position);
    if distance > sight(viewer) + target.radius() {
        return false;
    }
    if in_brush(nav, target.position)
        && target.revealed_until < time
        && !(in_brush(nav, viewer.position) && distance <= BRUSH_REACH)
    {
        return false;
    }
    true
}

pub fn detects(units: &[Unit], team: Team, target: &Unit) -> bool {
    let Some(range) = target.camouflage() else {
        return true;
    };
    target.team == team
        || units.iter().any(|viewer| {
            let distance = viewer.position.distance(target.position) - target.radius();
            viewer.team == team
                && viewer.alive()
                && match viewer.kind {
                    UnitKind::Champion(_) => distance <= range,
                    UnitKind::Turret | UnitKind::Shrine => {
                        distance <= viewer.stats.attack_range + viewer.radius()
                    }
                    _ => false,
                }
        })
}

pub fn visible_to(units: &[Unit], nav: Option<&NavGrid>, team: Team, time: f32) -> HashSet<UnitId> {
    let viewers: Vec<&Unit> = units
        .iter()
        .filter(|unit| unit.team == team && unit.alive())
        .collect();
    units
        .iter()
        .filter(|target| {
            target.team == team
                || target.is_structure()
                || (viewers.iter().any(|viewer| sees(nav, viewer, target, time))
                    && detects(units, team, target))
        })
        .map(|unit| unit.id)
        .collect()
}

pub fn hidden_in_brush(
    units: &[Unit],
    nav: Option<&NavGrid>,
    team: Team,
    time: f32,
) -> HashSet<UnitId> {
    let seen = visible_to(units, nav, team.enemy(), time);
    units
        .iter()
        .filter(|unit| {
            unit.team == team
                && unit.alive()
                && in_brush(nav, unit.position)
                && !seen.contains(&unit.id)
        })
        .map(|unit| unit.id)
        .collect()
}

pub fn point_visible(units: &[Unit], nav: Option<&NavGrid>, team: Team, point: Vec2) -> bool {
    units.iter().any(|viewer| {
        viewer.team == team
            && viewer.alive()
            && viewer.position.distance(point) <= sight(viewer)
            && (!in_brush(nav, point) || in_brush(nav, viewer.position))
    })
}
