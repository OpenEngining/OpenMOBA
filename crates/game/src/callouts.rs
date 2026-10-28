use asset::ui::Layout;
use web_sys::Element;

use crate::banner::Portrait;
use crate::dom::set_style;
use crate::frame::{Frame, View};
use crate::overlay::{ALLY_TINT, ENEMY_TINT};

const GROUP: &str = "KillCallouts";
const TEMPLATE: &str = "KillCallouts/KillCallout0";
const SLOT_AREA: &str = "KillCallouts/KillCallout0/KillCallout0_SlotArea";
const SLOTS: usize = 5;
const ASSISTERS: usize = 4;
const TIME: f32 = 5.0;
const FADE: f32 = 0.25;
const SPREE: u32 = 3;
const PARTS: [&str; 5] = [
    "Background",
    "KillerFrame",
    "KillerPortrait",
    "TargetFrame",
    "TargetPortrait",
];
const MARKS: [Mark; 3] = [Mark::Normal, Mark::Streak, Mark::Shutdown];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Normal,
    Streak,
    Shutdown,
}

impl Mark {
    pub fn of(killer_spree: u32, victim_spree: u32) -> Self {
        if victim_spree >= SPREE {
            Self::Shutdown
        } else if killer_spree >= SPREE {
            Self::Streak
        } else {
            Self::Normal
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Normal => "KillIconNormal",
            Self::Streak => "KillIconStreak",
            Self::Shutdown => "KillIconShutdown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Callout {
    pub killer: Option<Portrait>,
    pub victim: Portrait,
    pub assists: Vec<Portrait>,
    pub mark: Mark,
}

pub struct Callouts {
    frame: Frame,
    entries: Vec<(Callout, f32)>,
}

fn slot(index: usize) -> String {
    format!("{GROUP}/KillCallout{index}")
}

fn part(index: usize, part: &str) -> String {
    format!("{}/KillCallout0_{part}", slot(index))
}

fn assister(index: usize, assister: usize, part: &str) -> String {
    self::part(index, &format!("Assister{assister}{part}"))
}

fn tint(ally: bool) -> [u8; 3] {
    if ally { ALLY_TINT } else { ENEMY_TINT }
}

fn opacity(age: f32) -> f32 {
    (age / FADE).min((TIME - age) / FADE).clamp(0.0, 1.0)
}

impl Callouts {
    pub fn new(parent: &Element, layout: &Layout) -> Self {
        let mut frame = Frame::new(parent, layout, &[TEMPLATE]);
        frame.show_scene(TEMPLATE, true);
        let pitch = frame.rect(SLOT_AREA).map_or(70.0, |rect| rect.size[1]);
        for index in 1..SLOTS {
            frame.instance(TEMPLATE, &slot(index), [0.0, pitch * index as f32]);
        }
        let callouts = Self {
            frame,
            entries: Vec::new(),
        };
        callouts.render();
        callouts
    }

    pub fn resize(&mut self, height: f32) {
        self.frame.resize(height);
    }

    pub fn push(&mut self, callout: Callout) {
        self.entries.push((callout, 0.0));
        if self.entries.len() > SLOTS {
            self.entries.remove(0);
        }
        self.render();
    }

    pub fn update(&mut self, dt: f32) {
        for (_, age) in &mut self.entries {
            *age += dt;
        }
        let count = self.entries.len();
        self.entries.retain(|(_, age)| *age < TIME);
        if self.entries.len() != count {
            self.render();
        }
        for (index, (_, age)) in self.entries.iter().enumerate() {
            let alpha = format!("{:.3}", opacity(*age));
            let names =
                PARTS
                    .iter()
                    .copied()
                    .chain(MARKS.iter().map(|mark| mark.icon()))
                    .map(|name| part(index, name))
                    .chain((0..ASSISTERS).flat_map(|n| {
                        [assister(index, n, "Frame"), assister(index, n, "Portrait")]
                    }));
            for name in names {
                if let Some(view) = self.frame.get(&name) {
                    set_style(view, "opacity", &alpha);
                }
            }
        }
    }

    fn render(&self) {
        let frame = &self.frame;
        for index in 0..SLOTS {
            let callout = self.entries.get(index).map(|(callout, _)| callout);
            for name in PARTS {
                frame.show(&part(index, name), callout.is_some());
            }
            for mark in MARKS {
                frame.show(
                    &part(index, mark.icon()),
                    callout.is_some_and(|callout| callout.mark == mark),
                );
            }
            for n in 0..ASSISTERS {
                let shown = callout.and_then(|callout| callout.assists.get(n));
                for name in ["Frame", "Portrait"] {
                    frame.show(&assister(index, n, name), shown.is_some());
                }
                if let Some(portrait) = shown {
                    frame.image(
                        &assister(index, n, "Portrait"),
                        &format!("assets/icons/{}", portrait.icon),
                    );
                    frame.tint(&assister(index, n, "Frame"), tint(portrait.ally));
                }
            }
            let Some(callout) = callout else {
                continue;
            };
            frame.tint(&part(index, "Background"), tint(!callout.victim.ally));
            frame.tint(&part(index, "TargetFrame"), tint(callout.victim.ally));
            frame.image(
                &part(index, "TargetPortrait"),
                &format!("assets/icons/{}", callout.victim.icon),
            );
            let killer = callout.killer.as_ref();
            frame.show(&part(index, "KillerFrame"), killer.is_some());
            frame.show(&part(index, "KillerPortrait"), killer.is_some());
            if let Some(killer) = killer {
                frame.tint(&part(index, "KillerFrame"), tint(killer.ally));
                frame.image(
                    &part(index, "KillerPortrait"),
                    &format!("assets/icons/{}", killer.icon),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callouts_fade_in_and_out() {
        assert_eq!(opacity(0.0), 0.0);
        assert_eq!(opacity(FADE), 1.0);
        assert_eq!(opacity(TIME / 2.0), 1.0);
        assert_eq!(opacity(TIME), 0.0);
    }

    #[test]
    fn shutdowns_outrank_streaks() {
        assert_eq!(Mark::of(0, 0), Mark::Normal);
        assert_eq!(Mark::of(3, 0), Mark::Streak);
        assert_eq!(Mark::of(5, 4), Mark::Shutdown);
    }

    #[test]
    fn instanced_slots_rename_the_template() {
        assert_eq!(
            part(2, "Background"),
            "KillCallouts/KillCallout2/KillCallout0_Background"
        );
        assert_eq!(
            assister(0, 3, "Frame"),
            "KillCallouts/KillCallout0/KillCallout0_Assister3Frame"
        );
    }
}
