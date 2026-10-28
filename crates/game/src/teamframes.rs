use asset::data::Resource;
use asset::ui::Layout;
use protocol::UnitState;
use web_sys::Element;

use crate::dom::set_style;
use crate::frame::{Frame, View};

const SCENE: &str = "TeamFrames/TeamFrames";
const TEMPLATE: &str = "TeamFrames/TeamFrames/TeamFrameGroup";
const FRAMES: usize = 4;
const PITCH: f32 = 67.0;
const ICON: &str = "TeamFrame0_Icon_Data";
const HEALTH: &str = "TeamFrame0_HPBarTextureGreen";
const LEVEL: &str = "TeamFrame0_Level";
const TIMER: &str = "TeamFrame0_Timer";
const ULT: &str = "TeamFrame0_Ult";
const ULT_BACKGROUND: &str = "TeamFrame0_UltBg";
const ULT_COOLDOWN: &str = "TeamFrame0_UltCooldownFX";
const BASICS: [&str; 6] = [
    ICON,
    "TeamFrame0_HPBackdrop",
    HEALTH,
    "TeamFrame0_HPFrame",
    LEVEL,
    "TeamFrame0_Icon",
];
const BORDERS: [(bool, &str); 4] = [
    (true, "TeamFrame_FrameGroup/TeamFrame0_Frame"),
    (true, "TeamFrame_FrameGroup/TeamFrame0_FrameUnderlay"),
    (false, "TeamFrame_FrameGroupDead/TeamFrame0_FrameDead"),
    (
        false,
        "TeamFrame_FrameGroupDead/TeamFrame0_FrameDeadUnderlay",
    ),
];
const RESOURCE_BARS: [(Resource, &str); 2] = [
    (Resource::Mana, "TeamFrame0_PARBarTextureBlue"),
    (Resource::Flow, "TeamFrame0_PARBarTextureWhite"),
];
const HIDDEN: [&str; 10] = [
    "TeamFrame0_PARBarTextureYellow",
    "TeamFrame0_PARBarTextureLgtBlue",
    "TeamFrame0_PARBarTextureOrange",
    "TeamFrame0_PARBarTextureRed",
    "TeamFrame0_ReciprocityOverlay",
    "TeamFrame0_ReciprocityOverlayMyBad",
    "TeamFrame0_ObjPlanAgainstVote",
    "TeamFrame0_ObjPlanForVote",
    "TeamFrame0_VoiceChatHalo",
    "TeamFrame0_UltSheenFX",
];

pub struct Ally<'a> {
    pub icon: &'a str,
    pub resource: Resource,
    pub unit: &'a UnitState,
}

pub struct TeamFrames {
    frame: Frame,
}

fn group(index: usize) -> String {
    if index == 0 {
        TEMPLATE.to_owned()
    } else {
        format!("{TEMPLATE}{index}")
    }
}

fn part(index: usize, name: &str) -> String {
    format!("{}/{name}", group(index))
}

fn timer(seconds: f32) -> String {
    format!("{:.0}", seconds.ceil())
}

impl TeamFrames {
    pub fn new(parent: &Element, layout: &Layout) -> Self {
        let mut frame = Frame::new(parent, layout, &[SCENE]);
        frame.show_scene(SCENE, true);
        for index in 1..FRAMES {
            frame.instance(TEMPLATE, &group(index), [-PITCH * index as f32, 0.0]);
        }
        for index in 0..FRAMES {
            for name in HIDDEN {
                frame.show(&part(index, name), false);
            }
            if let Some(icon) = frame.get(&part(index, ICON)) {
                set_style(icon, "border-radius", "50%");
            }
        }
        let frames = Self { frame };
        frames.update(&[]);
        frames
    }

    pub fn resize(&mut self, height: f32) {
        self.frame.resize(height);
    }

    pub fn update(&self, allies: &[Ally]) {
        let frame = &self.frame;
        for index in 0..FRAMES {
            let ally = allies.get(index);
            for name in BASICS {
                frame.show(&part(index, name), ally.is_some());
            }
            for (alive, name) in BORDERS {
                frame.show(
                    &part(index, name),
                    ally.is_some_and(|ally| ally.unit.dead != alive),
                );
            }
            let ultimate = ally.and_then(|ally| ally.unit.ultimate);
            frame.show(&part(index, ULT_BACKGROUND), ultimate.is_some());
            frame.show(
                &part(index, ULT),
                ultimate.is_some_and(|remaining| remaining <= 0.0),
            );
            frame.show(
                &part(index, ULT_COOLDOWN),
                ultimate.is_some_and(|remaining| remaining > 0.0),
            );
            frame.radial(&part(index, ULT_COOLDOWN), ultimate.unwrap_or(0.0));
            let respawn = ally.and_then(|ally| ally.unit.respawn);
            frame.show(&part(index, TIMER), respawn.is_some());
            frame.text(&part(index, TIMER), &respawn.map(timer).unwrap_or_default());
            for (kind, bar) in RESOURCE_BARS {
                frame.show(
                    &part(index, bar),
                    ally.is_some_and(|ally| ally.resource == kind && ally.unit.max_mana > 0.0),
                );
            }
            let Some(Ally {
                icon,
                resource,
                unit,
            }) = ally
            else {
                continue;
            };
            let portrait = part(index, ICON);
            frame.image(&portrait, &format!("assets/icons/{icon}"));
            if let Some(view) = frame.get(&portrait) {
                set_style(view, "filter", if unit.dead { "grayscale(1)" } else { "" });
            }
            frame.fill(&part(index, HEALTH), unit.health / unit.max_health.max(1.0));
            if let Some((_, bar)) = RESOURCE_BARS.iter().find(|(kind, _)| kind == resource) {
                frame.fill(&part(index, bar), unit.mana / unit.max_mana.max(1.0));
            }
            frame.text(&part(index, LEVEL), &unit.level.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instances_extend_the_template_group_name() {
        assert_eq!(
            part(0, LEVEL),
            "TeamFrames/TeamFrames/TeamFrameGroup/TeamFrame0_Level"
        );
        assert_eq!(
            part(2, BORDERS[2].1),
            "TeamFrames/TeamFrames/TeamFrameGroup2/TeamFrame_FrameGroupDead/TeamFrame0_FrameDead"
        );
    }

    #[test]
    fn respawn_timers_round_up_to_whole_seconds() {
        assert_eq!(timer(4.2), "5");
        assert_eq!(timer(0.0), "0");
    }
}
