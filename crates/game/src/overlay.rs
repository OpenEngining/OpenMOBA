use std::collections::HashMap;

use asset::data::Resource;
use asset::ui::Layout;
use glam::Vec2;
use protocol::{MinionKind, Team, UnitId, UnitKind, UnitState};
use wasm_bindgen::{Clamped, JsCast};
use web_sys::{CanvasRenderingContext2d, Element, HtmlCanvasElement, HtmlElement, ImageData};

use crate::fade::Fade;
use crate::fog;
use crate::frame::{Instance, Template, View};

use crate::dom::{append, element, set_class, set_style};

const MINIMAP_FOG: f32 = 150.0;

pub fn bar_height(kind: UnitKind) -> f32 {
    match kind {
        UnitKind::Champion(_) => 260.0,
        UnitKind::Minion(MinionKind::Siege | MinionKind::Super) => 150.0,
        UnitKind::Minion(_) => 110.0,
        UnitKind::Turret | UnitKind::Shrine => 660.0,
        UnitKind::Inhibitor => 300.0,
        UnitKind::Nexus => 380.0,
        UnitKind::SunDisc => 760.0,
        UnitKind::Soldier => 0.0,
    }
}

const CHAMPION: &str = "LoLFloatingInfoBars/ChampBars";
const MINION: &str = "LoLFloatingInfoBars/FHBMinionSmall";
const LARGE_MINION: &str = "LoLFloatingInfoBars/FHBMinionMed";
const STRUCTURE: &str = "LoLFloatingInfoBars/FHBStructure";
const CHAMPION_PARTS: [&str; 14] = [
    "Borders/ChampFHB_Border_Default",
    "Borders/ChampFHB_LevelBox_Default_Self",
    "Borders/ChampFHB_LevelBox_Default_Blue",
    "Borders/ChampFHB_LevelBox_Default_Red",
    "Text/ChampFHB_Text_Level",
    "Text/ChampFHB_Text_Name",
    "Meters/ChampHP_BarFade",
    "Meters/ChampHP_BarFadeEnemy",
    "Meters/ChampHP_BarTextureGreen",
    "Meters/ChampHP_BarTextureBlue",
    "Meters/ChampHP_BarTextureRed",
    "Meters/ChampHP_AShieldBarTexture",
    "ParTypeData/ChampPAR_BarTextureBlue",
    "ParTypeData/ChampPAR_BarTextureWhite",
];
const MINION_PARTS: [&str; 2] = ["MinionSmallFHB_Border_Default", "MinionSmallHP_BarTexture"];
const LARGE_MINION_PARTS: [&str; 2] = ["MinionMedFHB_Border_Default", "MinionMedHP_BarTexture"];
const STRUCTURE_PARTS: [&str; 4] = [
    "StructureFHB_Border_Default",
    "StructureHP_BarFade",
    "StructureHP_BarFadeEnemy",
    "StructureHP_BarTexture",
];
pub const ALLY_TINT: [u8; 3] = [85, 226, 255];
pub const ENEMY_TINT: [u8; 3] = [244, 93, 79];
const HEALTH_PER_TICK: f32 = 100.0;
const SOURCE_HEIGHT: f32 = 1200.0;

struct Templates {
    champion: Template,
    minion: Template,
    large_minion: Template,
    structure: Template,
}

struct Bar {
    view: Instance,
    health: &'static str,
    fade: &'static str,
    fades: Fade,
    seen: bool,
}

struct Dot {
    element: HtmlElement,
    seen: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min: Vec2,
    pub max: Vec2,
}

impl Bounds {
    pub fn around(units: &[UnitState]) -> Option<Self> {
        let structures = units.iter().filter(|unit| {
            matches!(
                unit.kind,
                UnitKind::Turret | UnitKind::Shrine | UnitKind::Inhibitor | UnitKind::Nexus
            )
        });
        let (min, max) = structures.fold(None, |bounds: Option<(Vec2, Vec2)>, unit| {
            let position = Vec2::from(unit.position);
            Some(bounds.map_or((position, position), |(min, max)| {
                (min.min(position), max.max(position))
            }))
        })?;
        let center = (min + max) * 0.5;
        let half = (max - min).max_element() * 0.5 + 1200.0;
        Some(Self {
            min: center - Vec2::splat(half),
            max: center + Vec2::splat(half),
        })
    }

    pub fn fraction_of(&self, position: Vec2) -> Vec2 {
        let size = self.max - self.min;
        Vec2::new(
            (position.x - self.min.x) / size.x,
            (self.max.y - position.y) / size.y,
        )
    }

    pub fn point_at(&self, fraction: Vec2) -> Vec2 {
        let size = self.max - self.min;
        Vec2::new(
            self.min.x + fraction.x * size.x,
            self.max.y - fraction.y * size.y,
        )
    }
}

pub struct Overlay {
    bars_layer: HtmlElement,
    bars: HashMap<UnitId, Bar>,
    templates: Templates,
    scale: f32,
    pub minimap: HtmlElement,
    dots: HashMap<UnitId, Dot>,
    pings: Vec<HtmlElement>,
    view: HtmlElement,
    fog: Option<CanvasRenderingContext2d>,
}

pub struct Viewer {
    pub you: UnitId,
    pub team: Team,
}

impl Overlay {
    pub fn new(parent: &Element, layout: &Layout) -> Self {
        let templates = Templates {
            champion: Template::new(
                layout,
                CHAMPION,
                "Borders/ChampFHB_AnchorBase",
                &CHAMPION_PARTS,
            ),
            minion: Template::new(layout, MINION, "MinionSmallFHB_AnchorBase", &MINION_PARTS),
            large_minion: Template::new(
                layout,
                LARGE_MINION,
                "MinionMedFHB_AnchorBase",
                &LARGE_MINION_PARTS,
            ),
            structure: Template::new(
                layout,
                STRUCTURE,
                "StructureFHB_AnchorBase",
                &STRUCTURE_PARTS,
            ),
        };
        let bars_layer = element("div", "bars-layer");
        append(parent, &bars_layer);
        let minimap = element("div", "minimap");
        let canvas: HtmlCanvasElement = element("canvas", "minimap-fog").unchecked_into();
        canvas.set_width(fog::SIZE as u32);
        canvas.set_height(fog::SIZE as u32);
        append(&minimap, &canvas);
        let fog = canvas
            .get_context("2d")
            .ok()
            .flatten()
            .and_then(|context| context.dyn_into().ok());
        let view = element("div", "minimap-view");
        append(&minimap, &view);
        append(parent, &minimap);
        Self {
            bars_layer,
            bars: HashMap::new(),
            templates,
            scale: 1.0,
            minimap,
            dots: HashMap::new(),
            pings: Vec::new(),
            view,
            fog,
        }
    }

    pub fn resize(&mut self, height: f32) {
        self.scale = (height / SOURCE_HEIGHT).max(1.0);
    }

    pub fn place_minimap(&self, [x, y, width, height]: [f32; 4]) {
        set_style(&self.minimap, "left", &format!("{x}px"));
        set_style(&self.minimap, "top", &format!("{y}px"));
        set_style(&self.minimap, "width", &format!("{width}px"));
        set_style(&self.minimap, "height", &format!("{height}px"));
    }

    pub fn remove(&self) {
        self.bars_layer.remove();
        self.minimap.remove();
    }

    pub fn update_bars(
        &mut self,
        units: &[UnitState],
        viewer: &Viewer,
        name_of: impl Fn(UnitId) -> Option<String>,
        resource_of: impl Fn(UnitKind) -> Resource,
        project: impl Fn(Vec2, f32) -> Option<Vec2>,
        time: f32,
    ) {
        for bar in self.bars.values_mut() {
            bar.seen = false;
        }
        for unit in units {
            if unit.dead || unit.kind == UnitKind::Soldier {
                continue;
            }
            let Some(screen) = project(Vec2::from(unit.position), bar_height(unit.kind)) else {
                continue;
            };
            let templates = &self.templates;
            let layer = &self.bars_layer;
            let bar = self.bars.entry(unit.id).or_insert_with(|| {
                create_bar(
                    templates,
                    layer,
                    unit,
                    viewer,
                    name_of(unit.id).as_deref(),
                    resource_of(unit.kind),
                )
            });
            bar.seen = true;
            let name = name_of(unit.id);
            bar.view.show("Text/ChampFHB_Text_Name", name.is_some());
            if let Some(name) = name {
                bar.view.text("Text/ChampFHB_Text_Name", &name);
            }
            set_style(
                &bar.view.root,
                "transform",
                &format!(
                    "translate({:.0}px, {:.0}px) scale({})",
                    screen.x, screen.y, self.scale
                ),
            );
            let total = (unit.health + unit.shield).max(unit.max_health).max(1.0);
            let health = unit.health / total;
            let shielded = health + unit.shield / total;
            let fade = bar.fades.update(health, time);
            let view = &bar.view;
            view.fill(bar.health, health);
            view.span(bar.fade, shielded, fade);
            view.span("Meters/ChampHP_AShieldBarTexture", health, shielded);
            let mana = (unit.mana / unit.max_mana.max(1.0)).clamp(0.0, 1.0);
            view.fill("ParTypeData/ChampPAR_BarTextureBlue", mana);
            view.fill("ParTypeData/ChampPAR_BarTextureWhite", mana);
            view.text("Text/ChampFHB_Text_Level", &unit.level.to_string());
            if let Some(ticks) = view.get("Ticks") {
                set_style(
                    ticks,
                    "--tick",
                    &format!("{:.3}%", HEALTH_PER_TICK / total * 100.0),
                );
            }
        }
        self.bars.retain(|_, bar| {
            if !bar.seen {
                bar.view.root.remove();
            }
            bar.seen
        });
    }

    pub fn update_minimap(
        &mut self,
        units: &[UnitState],
        bounds: &Bounds,
        viewer: &Viewer,
        view: [Vec2; 4],
        portrait: impl Fn(UnitKind) -> Option<String>,
    ) {
        for dot in self.dots.values_mut() {
            dot.seen = false;
        }
        for unit in units {
            if unit.dead || unit.kind == UnitKind::Soldier {
                continue;
            }
            let dot = self.dots.entry(unit.id).or_insert_with(|| {
                let element = element(
                    "div",
                    &format!("dot {} {}", kind_class(unit.kind), relation(unit, viewer)),
                );
                if let Some(icon) = portrait(unit.kind) {
                    set_style(
                        &element,
                        "background-image",
                        &format!("url(\"assets/icons/{icon}\")"),
                    );
                }
                append(&self.minimap, &element);
                Dot {
                    element,
                    seen: true,
                }
            });
            dot.seen = true;
            let fraction = bounds.fraction_of(Vec2::from(unit.position));
            set_style(&dot.element, "left", &format!("{:.2}%", fraction.x * 100.0));
            set_style(&dot.element, "top", &format!("{:.2}%", fraction.y * 100.0));
        }
        self.dots.retain(|_, dot| {
            if !dot.seen {
                dot.element.remove();
            }
            dot.seen
        });
        let corners = view.map(|corner| bounds.fraction_of(corner));
        let top_left = corners.into_iter().reduce(Vec2::min).unwrap_or_default();
        let bottom_right = corners.into_iter().reduce(Vec2::max).unwrap_or_default();
        set_style(&self.view, "left", &format!("{:.2}%", top_left.x * 100.0));
        set_style(&self.view, "top", &format!("{:.2}%", top_left.y * 100.0));
        set_style(
            &self.view,
            "width",
            &format!("{:.2}%", (bottom_right.x - top_left.x) * 100.0),
        );
        set_style(
            &self.view,
            "height",
            &format!("{:.2}%", (bottom_right.y - top_left.y) * 100.0),
        );
    }

    pub fn update_pings(&mut self, bounds: &Bounds, pings: &[(Vec2, bool, f32)]) {
        while self.pings.len() < pings.len() {
            let element = element("div", "minimap-ping");
            append(&self.minimap, &element);
            self.pings.push(element);
        }
        for element in self.pings.drain(pings.len()..) {
            element.remove();
        }
        for (element, (position, danger, progress)) in self.pings.iter().zip(pings) {
            let fraction = bounds.fraction_of(*position);
            set_class(element, "danger", *danger);
            set_style(element, "left", &format!("{:.2}%", fraction.x * 100.0));
            set_style(element, "top", &format!("{:.2}%", fraction.y * 100.0));
            set_style(
                element,
                "transform",
                &format!("scale({:.2})", 1.6 - (progress * 3.0).fract()),
            );
            set_style(element, "opacity", &format!("{:.2}", 1.0 - progress));
        }
    }

    pub fn update_fog(&self, visibility: &[f32]) {
        let Some(context) = &self.fog else {
            return;
        };
        let mut pixels = vec![0_u8; fog::SIZE * fog::SIZE * 4];
        for z in 0..fog::SIZE {
            for x in 0..fog::SIZE {
                let seen = visibility[z * fog::SIZE + x];
                let index = ((fog::SIZE - 1 - z) * fog::SIZE + x) * 4;
                pixels[index + 3] = ((1.0 - seen.clamp(0.0, 1.0)) * MINIMAP_FOG) as u8;
            }
        }
        if let Ok(image) = ImageData::new_with_u8_clamped_array_and_sh(
            Clamped(&pixels),
            fog::SIZE as u32,
            fog::SIZE as u32,
        ) {
            let _ = context.put_image_data(&image, 0.0, 0.0);
        }
    }
}

fn kind_class(kind: UnitKind) -> &'static str {
    match kind {
        UnitKind::Champion(_) => "champion",
        UnitKind::Minion(_) | UnitKind::Soldier => "minion",
        UnitKind::Turret | UnitKind::Shrine | UnitKind::SunDisc => "turret",
        UnitKind::Inhibitor => "inhibitor",
        UnitKind::Nexus => "nexus",
    }
}

fn relation(unit: &UnitState, viewer: &Viewer) -> &'static str {
    if unit.id == viewer.you {
        "self"
    } else if unit.team == viewer.team {
        "ally"
    } else {
        "enemy"
    }
}

fn create_bar(
    templates: &Templates,
    layer: &HtmlElement,
    unit: &UnitState,
    viewer: &Viewer,
    name: Option<&str>,
    resource: Resource,
) -> Bar {
    let relation = relation(unit, viewer);
    let enemy = relation == "enemy";
    let (template, health, fade) = match unit.kind {
        UnitKind::Champion(_) => (
            &templates.champion,
            match relation {
                "self" => "Meters/ChampHP_BarTextureGreen",
                "ally" => "Meters/ChampHP_BarTextureBlue",
                _ => "Meters/ChampHP_BarTextureRed",
            },
            if enemy {
                "Meters/ChampHP_BarFadeEnemy"
            } else {
                "Meters/ChampHP_BarFade"
            },
        ),
        UnitKind::Minion(MinionKind::Siege | MinionKind::Super) => {
            (&templates.large_minion, "MinionMedHP_BarTexture", "")
        }
        UnitKind::Minion(_) | UnitKind::Soldier => {
            (&templates.minion, "MinionSmallHP_BarTexture", "")
        }
        _ => (
            &templates.structure,
            "StructureHP_BarTexture",
            if enemy {
                "StructureHP_BarFadeEnemy"
            } else {
                "StructureHP_BarFade"
            },
        ),
    };
    let mut view = template.instance(layer);
    for part in [health, fade, "Meters/ChampHP_AShieldBarTexture"] {
        view.show(part, true);
    }
    for border in [
        "Borders/ChampFHB_Border_Default",
        "MinionSmallFHB_Border_Default",
        "MinionMedFHB_Border_Default",
        "StructureFHB_Border_Default",
    ] {
        view.show(border, true);
    }
    view.show(
        match relation {
            "self" => "Borders/ChampFHB_LevelBox_Default_Self",
            "ally" => "Borders/ChampFHB_LevelBox_Default_Blue",
            _ => "Borders/ChampFHB_LevelBox_Default_Red",
        },
        true,
    );
    view.show(
        match resource {
            Resource::Mana if unit.max_mana > 0.0 => "ParTypeData/ChampPAR_BarTextureBlue",
            Resource::Flow => "ParTypeData/ChampPAR_BarTextureWhite",
            _ => "",
        },
        true,
    );
    if !matches!(unit.kind, UnitKind::Champion(_)) {
        view.tint(health, if enemy { ENEMY_TINT } else { ALLY_TINT });
    }
    if let Some(name) = name {
        view.text("Text/ChampFHB_Text_Name", name);
    }
    view.show("Text/ChampFHB_Text_Name", name.is_some());
    if matches!(unit.kind, UnitKind::Champion(_)) {
        view.add("Ticks", health, "frame-ticks");
    }
    Bar {
        view,
        health,
        fade,
        fades: Fade::default(),
        seen: true,
    }
}
