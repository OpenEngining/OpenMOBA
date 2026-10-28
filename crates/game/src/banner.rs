use asset::ui::Layout;
use web_sys::Element;

use crate::announcer::Style;
use crate::dom::{set_class, set_style};
use crate::frame::{Frame, View};

const SCENE: &str = "AnnouncementBanners/ClassicBase/BannerScene";
const COMMON: &str = "AnnouncementBanners/ClassicBase/BannerScene/Common";
const TEXTS: [&str; 5] = [
    "MinorAnnouncementText",
    "MajorAnnouncementText",
    "EpicAnnouncementText",
    "PentakillAnnouncementText",
    "AceAnnouncementText",
];
const MULTI_KILLS: [&str; 4] = ["DoubleKill", "TripleKill", "QuadraKill", "Pentakill"];
const TONES: [Tone; 3] = [Tone::Ally, Tone::Enemy, Tone::Neutral];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Ally,
    Enemy,
    Neutral,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Portrait {
    pub icon: String,
    pub ally: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Banner {
    pub text: String,
    pub style: Style,
    pub tone: Tone,
    pub left: Option<Portrait>,
    pub right: Option<Portrait>,
}

#[derive(Clone, Copy)]
struct Group {
    group: &'static str,
    content: &'static str,
    ring: &'static str,
    ally: &'static str,
    enemy: &'static str,
}

const LEFT: Group = Group {
    group: "AnnBannLeftIconGroup",
    content: "AnnBannerLeftIcon_content",
    ring: "AnnBannerLeftIcon_gold_ring",
    ally: "LeftIconAlliedElements/AnnBannerLeftGlowBlue",
    enemy: "LeftIconEnemyElements/AnnBannerLeftGlowRed",
};
const RIGHT: Group = Group {
    group: "AnnBannRightIconGroup",
    content: "AnnBannerRightIcon_content",
    ring: "AnnBannerRightIcon_gold_ring",
    ally: "RightIconAlliedElements/RightIconGlowBlue",
    enemy: "RightIconEnemyElements/RightIconGlowRed",
};
const TOP: Group = Group {
    group: "AnnBannTopIconGroup",
    content: "AnnBannerTopIcon_content",
    ring: "AnnBannerTopIcon_gold_ring",
    ally: "TopIconAlliedElements/AnnBannerTopIcon_glow_blue",
    enemy: "TopIconEnemyElements/AnnBannerTopIcon_glow_red",
};

impl Group {
    fn parts(self) -> [String; 4] {
        [self.content, self.ring, self.ally, self.enemy]
            .map(|part| format!("{COMMON}/{}/{part}", self.group))
    }
}

fn text(style: Style) -> String {
    let index = match style {
        Style::Minor => 0,
        Style::Major => 1,
        Style::Epic(_) => 2,
        Style::Pentakill => 3,
        Style::Ace => 4,
    };
    format!("{COMMON}/Text/{}", TEXTS[index])
}

fn underline(tone: Tone) -> String {
    let color = match tone {
        Tone::Ally => "Blue",
        Tone::Enemy => "Red",
        Tone::Neutral => "Gold",
    };
    format!("{COMMON}/Underlines/AnnBanner{color}UnderlineGroup/AnnBanner{color}_Underline")
}

fn multi_kill(kill: &str, ally: bool) -> String {
    let (side, color) = if ally {
        ("Allied", "Blue")
    } else {
        ("Enemy", "Red")
    };
    let side = if kill == "Pentakill" && !ally {
        "PentaKillEnemy".to_owned()
    } else {
        format!("{kill}{side}")
    };
    format!("{SCENE}/{kill}/{side}Elements/{kill}{color}BG")
}

pub struct Banners {
    frame: Frame,
}

impl Banners {
    pub fn new(parent: &Element, layout: &Layout) -> Self {
        let frame = Frame::new(parent, layout, &[SCENE]);
        frame.show_scene(SCENE, false);
        for group in [LEFT, RIGHT, TOP] {
            if let Some(content) = frame.get(&group.parts()[0]) {
                set_style(content, "border-radius", "50%");
            }
        }
        let banners = Self { frame };
        banners.clear();
        banners
    }

    pub fn resize(&mut self, height: f32) {
        self.frame.resize(height);
    }

    fn clear(&self) {
        let frame = &self.frame;
        for style in [
            Style::Minor,
            Style::Major,
            Style::Epic(2),
            Style::Pentakill,
            Style::Ace,
        ] {
            frame.show(&text(style), false);
        }
        for tone in TONES {
            frame.show(&underline(tone), false);
        }
        for kill in MULTI_KILLS {
            for ally in [true, false] {
                frame.show(&multi_kill(kill, ally), false);
            }
        }
        for group in [LEFT, RIGHT, TOP] {
            for part in group.parts() {
                frame.show(&part, false);
            }
        }
    }

    pub fn hide(&self) {
        self.frame.show_scene(SCENE, false);
    }

    pub fn show(&self, banner: &Banner) {
        self.clear();
        let frame = &self.frame;
        let name = text(banner.style);
        frame.text(&name, &banner.text);
        frame.show(&name, true);
        frame.show(&underline(banner.tone), true);
        let kill = match banner.style {
            Style::Epic(count) => MULTI_KILLS.get(count.saturating_sub(2) as usize),
            Style::Pentakill => MULTI_KILLS.last(),
            _ => None,
        };
        if let Some(kill) = kill {
            frame.show(&multi_kill(kill, banner.tone != Tone::Enemy), true);
        }
        frame.show_scene(SCENE, true);
        if let Some(scene) = frame.scene(SCENE) {
            set_class(scene, "frame-banner", false);
            let _ = scene.offset_width();
            set_class(scene, "frame-banner", true);
        }
        let Some(rect) = frame.rect(&name) else {
            return;
        };
        let width = frame.get(&name).map_or(rect.size[0], |view| {
            set_style(view, "width", "max-content");
            view.offset_width() as f32
        });
        let center = rect.position[0] + rect.size[0] / 2.0;
        frame.move_to(&name, [center - width / 2.0, rect.position[1]]);
        let placements = match (&banner.left, &banner.right) {
            (Some(left), Some(right)) => vec![
                (LEFT, left, center - width / 2.0 - rect.position[0]),
                (
                    RIGHT,
                    right,
                    center + width / 2.0 - (rect.position[0] + rect.size[0]),
                ),
            ],
            (Some(only), None) | (None, Some(only)) => vec![(TOP, only, 0.0)],
            (None, None) => Vec::new(),
        };
        for (group, portrait, shift) in placements {
            let [content, ring, ally, enemy] = group.parts();
            frame.image(&content, &format!("assets/icons/{}", portrait.icon));
            for (part, shown) in [
                (&content, true),
                (&ring, true),
                (&ally, portrait.ally),
                (&enemy, !portrait.ally),
            ] {
                frame.show(part, shown);
                if let Some(original) = frame.rect(part) {
                    frame.move_to(part, [original.position[0] + shift, original.position[1]]);
                }
            }
        }
    }
}
