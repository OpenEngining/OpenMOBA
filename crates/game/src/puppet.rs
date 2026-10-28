use asset::{Character, clip_id};
use glam::Mat4;
use protocol::{Animation, UnitState};

use crate::animator::{Animator, Params};

pub struct Puppet {
    animator: Animator,
    seen: Option<Animation>,
    one_shot: bool,
    size: f32,
}

impl Puppet {
    pub fn new(character: &Character, seed: u32) -> Self {
        Self {
            animator: Animator::new(character, "Idle1", seed),
            seen: None,
            one_shot: false,
            size: 1.0,
        }
    }

    pub fn joined(mut self, character: &Character, unit: &UnitState, tick: u32) -> Self {
        let Some(animation) = unit
            .animation
            .filter(|animation| tick.saturating_sub(animation.started) > FRESH_TICKS)
        else {
            return self;
        };
        self.seen = Some(animation);
        if animation.hold || unit.dead {
            self.one_shot = self.animator.play_id(
                character,
                playable(character, animation.clip),
                Params::default(),
                animation.speed,
                true,
            );
        }
        self
    }

    pub fn update(&mut self, character: &Character, unit: &UnitState, dt: f32) -> Option<u32> {
        let params = Params {
            move_speed: unit.move_speed,
            moving: unit.moving || unit.dashing,
        };
        let started = (unit.animation != self.seen)
            .then_some(unit.animation)
            .flatten()
            .map(|animation| animation.clip);
        if unit.animation != self.seen {
            self.seen = unit.animation;
            self.one_shot = unit.animation.is_some_and(|animation| {
                self.animator.play_id(
                    character,
                    playable(character, animation.clip),
                    params,
                    animation.speed,
                    true,
                )
            });
        }
        let held = unit.animation.filter(|animation| animation.hold);
        if self.one_shot
            && !unit.dead
            && held.is_none()
            && (self.animator.finished() || unit.moving)
        {
            self.one_shot = false;
        }
        if let Some(animation) = held
            && self.animator.finished()
        {
            self.animator
                .play_id(character, animation.clip, params, animation.speed, true);
        }
        if !self.one_shot && !unit.dead {
            let clip = clip_id(if params.moving { "Run" } else { "Idle1" });
            let restart = self.animator.finished();
            self.animator.play_id(character, clip, params, 1.0, restart);
        }
        self.size += (unit.size - self.size) * (1.0 - (-dt * GROWTH_RATE).exp());
        self.animator.update(character, params, dt);
        started
    }

    pub fn die(&mut self, character: &Character) {
        self.one_shot =
            self.animator
                .play_id(character, clip_id("Death"), Params::default(), 1.0, false);
    }

    pub fn joint(&self, character: &Character, name: u32) -> Option<Mat4> {
        let index = character
            .joints
            .iter()
            .position(|joint| clip_id(&joint.name) == name)?;
        let skin = self.animator.skin(character);
        let bind = Mat4::from_cols_array(&character.joints[index].inverse_bind).inverse();
        Some(*skin.get(index)? * bind)
    }

    pub fn scale(&self, character: &Character) -> f32 {
        character.scale * self.size
    }

    pub fn skin(&self, character: &Character) -> Vec<Mat4> {
        self.animator.skin(character)
    }
}

const GROWTH_RATE: f32 = 6.0;
const FRESH_TICKS: u32 = 10;

const FALLBACKS: [(&str, &str); 4] = [
    ("Crit", "Attack1"),
    ("Attack3", "Attack1"),
    ("Attack4", "Attack2"),
    ("Attack2", "Attack1"),
];

fn playable(character: &Character, clip: u32) -> u32 {
    let mut clip = clip;
    for (missing, fallback) in FALLBACKS {
        if clip == clip_id(missing) && character.graph.clip(clip).is_none() {
            clip = clip_id(fallback);
        }
    }
    clip
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    #[test]
    fn joints_are_found_by_hashed_name() {
        let character = crate::animator::tests::character();
        let puppet = Puppet::new(&character, 1);
        let root = puppet
            .joint(&character, clip_id("Root"))
            .expect("root joint");
        assert!(root.transform_point3(Vec3::ZERO).length() < 1e-4);
        assert!(puppet.joint(&character, clip_id("r_hand")).is_none());
    }

    #[test]
    fn units_coming_into_view_only_start_animations_that_just_began() {
        let character = crate::animator::tests::character();
        let mut unit = crate::world::tests::unit([0.0, 0.0]);
        unit.animation = Some(Animation {
            clip: clip_id("Walk"),
            started: 100,
            speed: 1.0,
            hold: false,
        });
        let started = |tick: u32| {
            Puppet::new(&character, 1)
                .joined(&character, &unit, tick)
                .update(&character, &unit, 1.0 / 60.0)
        };
        assert_eq!(started(102), Some(clip_id("Walk")));
        assert_eq!(started(400), None);
    }

    #[test]
    fn walk_and_idle_clips_that_do_not_loop_restart_instead_of_freezing() {
        let mut character = crate::animator::tests::character();
        for clip in &mut character.graph.clips {
            clip.flags &= !asset::LOOP;
        }
        let mut puppet = Puppet::new(&character, 1);
        let mut unit = crate::world::tests::unit([0.0, 0.0]);
        unit.moving = true;
        unit.move_speed = 400.0;
        let mut stopped = 0;
        for _ in 0..240 {
            puppet.update(&character, &unit, 1.0 / 60.0);
            stopped = if puppet.animator.finished() {
                stopped + 1
            } else {
                0
            };
        }
        assert!(stopped < 2, "{stopped}");
    }
}
