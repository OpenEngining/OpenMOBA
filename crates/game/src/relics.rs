use asset::ui::Layout;
use protocol::{RelicState, Team};
use web_sys::Element;

use crate::frame::{Frame, View};

const SCENE: &str = "LoLHANeutralTimerViewController/NeutralTimers";

pub struct RelicTimers {
    frame: Frame,
}

fn part(team: Team, inner: bool, name: &str) -> String {
    let side = match team {
        Team::Blue => "Order",
        Team::Red => "Chaos",
    };
    let depth = if inner { "Inner" } else { "Outer" };
    format!("{SCENE}/{side}{depth}RelicTimer{name}")
}

fn clock(seconds: f32) -> String {
    let seconds = seconds.ceil() as u32;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

impl RelicTimers {
    pub fn new(parent: &Element, layout: &Layout) -> Self {
        let frame = Frame::new(parent, layout, &[SCENE]);
        frame.show_scene(SCENE, true);
        let timers = Self { frame };
        timers.update(&[]);
        timers
    }

    pub fn resize(&mut self, height: f32) {
        self.frame.resize(height);
    }

    pub fn update(&self, relics: &[RelicState]) {
        let frame = &self.frame;
        for team in [Team::Blue, Team::Red] {
            for inner in [true, false] {
                let relic = relics
                    .iter()
                    .find(|relic| relic.team == team && relic.inner == inner);
                let waiting = relic.map(|relic| relic.ready_in).filter(|left| *left > 0.0);
                frame.show(&part(team, inner, "Frame"), relic.is_some());
                frame.show(
                    &part(team, inner, "IconAlive"),
                    relic.is_some() && waiting.is_none(),
                );
                frame.show(&part(team, inner, "IconDead"), waiting.is_some());
                frame.show(&part(team, inner, "Text"), waiting.is_some());
                frame.text(
                    &part(team, inner, "Text"),
                    &waiting.map(clock).unwrap_or_default(),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relic_parts_follow_the_side_and_depth() {
        assert_eq!(
            part(Team::Red, false, "Text"),
            "LoLHANeutralTimerViewController/NeutralTimers/ChaosOuterRelicTimerText"
        );
    }

    #[test]
    fn timers_count_whole_seconds_as_minutes() {
        assert_eq!(clock(65.2), "1:06");
        assert_eq!(clock(9.0), "0:09");
    }
}
