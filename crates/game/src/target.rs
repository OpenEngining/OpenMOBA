use asset::data::Resource;
use asset::ui::Layout;
use protocol::{Score, UnitKind, UnitState};
use web_sys::Element;

use crate::frame::{Frame, View};
use crate::ui;

const SCENE: &str = "TargetFrame/TargetFrame";
const HEALTH: &str = "TargetFrame/TargetFrame/TargetHealthBar";
const MANA: &str = "TargetFrame/TargetFrame/TargetPARBar";
const RESOURCE_BARS: [(Resource, &str); 2] = [
    (
        Resource::Mana,
        "TargetFrame/TargetFrame/TargetPARBarSkins/TargetPar_BarTextureBlue",
    ),
    (
        Resource::Flow,
        "TargetFrame/TargetFrame/TargetPARBar/TargetPar_BarTextureWhite",
    ),
];
const STATS: &str = "TargetFrame/TargetFrame/TargetStats";
const HIDDEN: [&str; 6] = [
    "TargetFrame/TargetFrame/TargetStats/TargetCDR_Icon",
    "TargetFrame/TargetFrame/TargetHealthBar/TargetHP_BarFade",
    "TargetFrame/TargetFrame/TargetStats/Target_MFDRMS_Icon",
    "TargetFrame/TargetFrame/TargetStats/Target_MFDRMS_Text",
    "TargetFrame/TargetFrame/TargetStats/Target_MPIMDS_Icon",
    "TargetFrame/TargetFrame/TargetStats/Target_MPIMDS_Text",
];
const SCORE_PARTS: [&str; 4] = ["CS_Icon", "CS_Text", "KDA_Icon", "KDA_Text"];

pub struct Target<'a> {
    pub unit: &'a UnitState,
    pub icon: Option<String>,
    pub items: Vec<Option<&'a str>>,
    pub score: Option<Score>,
    pub resource: Resource,
    pub ally: bool,
}

pub struct TargetFrame {
    frame: Frame,
}

impl TargetFrame {
    pub fn new(parent: &Element, layout: &Layout) -> Self {
        let frame = Frame::new(parent, layout, &[SCENE]);
        for name in HIDDEN {
            frame.show(name, false);
        }
        Self { frame }
    }

    pub fn remove(&self) {
        self.frame.remove();
    }

    pub fn resize(&mut self, height: f32) {
        self.frame.resize(height);
    }

    pub fn update(&self, target: Option<Target>) {
        let frame = &self.frame;
        frame.show_scene(SCENE, target.is_some());
        let Some(target) = target else {
            return;
        };
        let unit = target.unit;
        let portrait = format!("{SCENE}/TargetPortrait");
        frame.show(&portrait, target.icon.is_some());
        if let Some(icon) = &target.icon {
            frame.image(&portrait, &format!("assets/icons/{icon}"));
        }
        frame.desaturate(&portrait, unit.dead);
        frame.text(&format!("{SCENE}/TargetLevel"), &unit.level.to_string());
        let health = (unit.health / unit.max_health.max(1.0)).clamp(0.0, 1.0);
        let green = format!("{HEALTH}/TargetHP_BarTextureGreen");
        let red = format!("{HEALTH}/TargetHP_BarTextureRed");
        frame.show(&green, target.ally);
        frame.show(&red, !target.ally);
        frame.fill(&green, health);
        frame.fill(&red, health);
        frame.follow(&format!("{HEALTH}/TargetHP_BarTip"), &green, health);
        frame.text(
            &format!("{HEALTH}/TargetHP_ValueText"),
            &format!("{:.0} / {:.0}", unit.health.max(0.0), unit.max_health),
        );
        let mana = unit.max_mana > 0.0 && matches!(unit.kind, UnitKind::Champion(_));
        let mana_fraction = (unit.mana / unit.max_mana.max(1.0)).clamp(0.0, 1.0);
        for (kind, bar) in RESOURCE_BARS {
            frame.show(bar, mana && kind == target.resource);
            frame.fill(bar, mana_fraction);
        }
        frame.follow(
            &format!("{MANA}/TargetPar_BarTip"),
            RESOURCE_BARS[0].1,
            if mana { mana_fraction } else { 0.0 },
        );
        frame.text(
            &format!("{MANA}/TargetPar_ValueText"),
            &if mana {
                format!("{:.0} / {:.0}", unit.mana, unit.max_mana)
            } else {
                String::new()
            },
        );
        for name in ui::STAT_NAMES {
            frame.show(&format!("{STATS}/Target{name}_Icon"), unit.stats.is_some());
            frame.show(&format!("{STATS}/Target{name}_Text"), unit.stats.is_some());
        }
        if let Some(stats) = &unit.stats {
            for (name, value) in ui::STAT_NAMES.iter().zip(ui::stat_texts(stats)) {
                frame.text(&format!("{STATS}/Target{name}_Text"), &value);
            }
        }
        for part in SCORE_PARTS {
            frame.show(&format!("{STATS}/Target_{part}"), target.score.is_some());
        }
        if let Some(score) = target.score {
            frame.text(
                &format!("{STATS}/Target_KDA_Text"),
                &format!("{}/{}/{}", score.kills, score.deaths, score.assists),
            );
            frame.text(
                &format!("{STATS}/Target_CS_Text"),
                &score.minions.to_string(),
            );
        }
        for slot in 0..7 {
            let icon = target.items.get(slot).copied().flatten();
            let name = format!("{SCENE}/TargetInventory/TargetItem{slot}_");
            frame.show(&name, icon.is_some());
            frame.show(
                &format!("{SCENE}/TargetInventory/TargetItem{slot}_Backdrop"),
                target.score.is_some(),
            );
            if let Some(icon) = icon {
                frame.image(&name, &format!("assets/icons/{icon}"));
            }
        }
    }
}
