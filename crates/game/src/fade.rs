const FADE_DELAY: f32 = 0.5;
const FADE_SPEED: f32 = 0.5;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Fade {
    health: f32,
    from: f32,
    start: f32,
}

impl Fade {
    fn shown(&self, time: f32) -> f32 {
        let fading = (time - self.start - FADE_DELAY).max(0.0) * FADE_SPEED;
        (self.from - fading).max(self.health)
    }

    pub fn update(&mut self, health: f32, time: f32) -> f32 {
        let shown = self.shown(time);
        if health < self.health {
            self.from = shown;
            self.start = time;
        }
        if health >= shown {
            self.from = health;
        }
        self.health = health;
        self.shown(time)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damage_leaves_a_fading_trail_that_waits_then_shrinks() {
        let mut fade = Fade::default();
        assert_eq!(fade.update(1.0, 0.0), 1.0);
        assert_eq!(fade.update(0.6, 1.0), 1.0);
        assert_eq!(fade.update(0.6, 1.0 + FADE_DELAY), 1.0);
        assert!((fade.update(0.6, 1.5 + FADE_DELAY) - 0.75).abs() < 1e-5);
        assert_eq!(fade.update(0.6, 10.0), 0.6);
        assert_eq!(fade.update(0.9, 11.0), 0.9);
    }
}
