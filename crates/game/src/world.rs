use std::collections::VecDeque;

use protocol::{MissileState, Snapshot, TICK_RATE, UnitState};

const DELAY_TICKS: f64 = 2.0;
const KEEP: usize = 32;
const SNAP_SPEED: f32 = 3000.0;
const CLOCK_CATCH_UP: f64 = 0.05;

#[derive(Default)]
pub struct Timeline {
    snapshots: VecDeque<Snapshot>,
    offset: Option<f64>,
}

pub struct Frame {
    pub tick: f64,
    pub units: Vec<UnitState>,
    pub missiles: Vec<MissileState>,
}

impl Timeline {
    pub fn push(&mut self, snapshot: Snapshot, now: f64) {
        let offset = f64::from(snapshot.tick) - now * f64::from(TICK_RATE);
        self.offset = Some(match self.offset {
            Some(current) if offset <= current => current + (offset - current) * CLOCK_CATCH_UP,
            _ => offset,
        });
        if self
            .snapshots
            .back()
            .is_some_and(|last| snapshot.tick <= last.tick)
        {
            self.snapshots.clear();
        }
        self.snapshots.push_back(snapshot);
        while self.snapshots.len() > KEEP {
            self.snapshots.pop_front();
        }
    }

    pub fn latest(&self) -> Option<&Snapshot> {
        self.snapshots.back()
    }

    pub fn frame(&self, now: f64) -> Option<Frame> {
        let latest = self.snapshots.back()?;
        let tick =
            (now * f64::from(TICK_RATE) + self.offset? - DELAY_TICKS).min(f64::from(latest.tick));
        let after = self
            .snapshots
            .iter()
            .position(|snapshot| f64::from(snapshot.tick) >= tick)
            .unwrap_or(self.snapshots.len() - 1);
        let to = &self.snapshots[after];
        let Some(from) = after.checked_sub(1).map(|index| &self.snapshots[index]) else {
            return Some(Frame {
                tick,
                units: to.units.clone(),
                missiles: to.missiles.clone(),
            });
        };
        let span = (to.tick - from.tick).max(1) as f64;
        let t = ((tick - f64::from(from.tick)) / span).clamp(0.0, 1.0) as f32;
        let units = to
            .units
            .iter()
            .map(|unit| {
                from.units
                    .iter()
                    .find(|previous| previous.id == unit.id)
                    .map_or_else(
                        || unit.clone(),
                        |previous| blend_unit(previous, unit, t, span as f32),
                    )
            })
            .collect();
        let missiles = to
            .missiles
            .iter()
            .map(|missile| {
                from.missiles
                    .iter()
                    .find(|previous| previous.id == missile.id)
                    .map_or_else(
                        || missile.clone(),
                        |previous| MissileState {
                            position: lerp2(previous.position, missile.position, t),
                            height: previous.height + (missile.height - previous.height) * t,
                            ..missile.clone()
                        },
                    )
            })
            .collect();
        Some(Frame {
            tick,
            units,
            missiles,
        })
    }
}

fn lerp2(a: [f32; 2], b: [f32; 2], t: f32) -> [f32; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    a + ((b - a + PI).rem_euclid(TAU) - PI) * t
}

fn blend_unit(from: &UnitState, to: &UnitState, t: f32, ticks: f32) -> UnitState {
    let distance = (to.position[0] - from.position[0]).hypot(to.position[1] - from.position[1]);
    let reach = SNAP_SPEED * ticks / TICK_RATE as f32;
    let position = if distance > reach || from.dead != to.dead {
        to.position
    } else {
        lerp2(from.position, to.position, t)
    };
    let casting = to.animation.is_some() && to.animation != from.animation;
    UnitState {
        position,
        facing: if casting {
            to.facing
        } else {
            lerp_angle(from.facing, to.facing, t)
        },
        health: from.health + (to.health - from.health) * t,
        shield: from.shield + (to.shield - from.shield) * t,
        mana: from.mana + (to.mana - from.mana) * t,
        ..to.clone()
    }
}

#[cfg(test)]
pub mod tests {
    use protocol::{Animation, Team, UnitKind};

    use super::*;

    pub fn unit(position: [f32; 2]) -> UnitState {
        UnitState {
            id: 1,
            kind: UnitKind::Turret,
            team: Team::Blue,
            position,
            facing: 0.0,
            health: 100.0,
            max_health: 100.0,
            shield: 0.0,
            mana: 0.0,
            max_mana: 0.0,
            level: 1,
            move_speed: 0.0,
            moving: false,
            dashing: false,
            dead: false,
            targetable: true,
            size: 1.0,
            respawn: None,
            ultimate: None,
            animation: None,
            stats: None,
            buffs: Vec::new(),
        }
    }

    fn snapshot(tick: u32, x: f32) -> Snapshot {
        Snapshot {
            tick,
            units: vec![unit([x, 0.0])],
            missiles: Vec::new(),
            relics: Vec::new(),
            props: Vec::new(),
            interactables: Vec::new(),
            scores: Vec::new(),
            you: None,
            events: Vec::new(),
        }
    }

    #[test]
    fn plays_back_slightly_behind_the_server() {
        let mut timeline = Timeline::default();
        let step = 1.0 / f64::from(TICK_RATE);
        for tick in 0..4 {
            timeline.push(snapshot(tick, tick as f32 * 30.0), f64::from(tick) * step);
        }
        let frame = timeline.frame(3.5 * step).expect("frame");
        assert!((frame.tick - 1.5).abs() < 1e-6);
        assert!((frame.units[0].position[0] - 45.0).abs() < 1e-3);
    }

    #[test]
    fn teleports_are_not_smoothed() {
        let mut timeline = Timeline::default();
        let step = 1.0 / f64::from(TICK_RATE);
        timeline.push(snapshot(0, 0.0), 0.0);
        timeline.push(snapshot(1, 1000.0), step);
        let frame = timeline.frame(2.5 * step).expect("frame");
        assert_eq!(frame.units[0].position, [1000.0, 0.0]);
    }

    #[test]
    fn flashes_snap_but_dashes_still_glide() {
        let step = 1.0 / f64::from(TICK_RATE);
        let shown = |to: f32| {
            let mut timeline = Timeline::default();
            timeline.push(snapshot(0, 0.0), 0.0);
            timeline.push(snapshot(1, to), step);
            timeline.frame(2.5 * step).expect("frame").units[0].position[0]
        };
        assert_eq!(shown(400.0), 400.0);
        assert!(shown(70.0) < 70.0);
    }

    #[test]
    fn units_face_where_a_new_animation_points_at_once() {
        let step = 1.0 / f64::from(TICK_RATE);
        let shown = |animation: Option<Animation>| {
            let mut timeline = Timeline::default();
            timeline.push(snapshot(0, 0.0), 0.0);
            let mut turned = snapshot(1, 0.0);
            turned.units[0].facing = 3.0;
            turned.units[0].animation = animation;
            timeline.push(turned, step);
            timeline.frame(2.2 * step).expect("frame").units[0].facing
        };
        let cast = Animation {
            clip: 7,
            started: 1,
            speed: 1.0,
            hold: false,
        };
        assert_eq!(shown(Some(cast)), 3.0);
        assert!(shown(None) < 3.0);
    }

    #[test]
    fn angles_take_the_short_way_round() {
        let angle = lerp_angle(3.0, -3.0, 0.5);
        assert!(angle.abs() > 3.0);
    }
}
