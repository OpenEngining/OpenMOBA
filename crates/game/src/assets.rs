use std::collections::HashMap;

use asset::Character;
use protocol::{MinionKind, Team, UnitKind};
use wasm_bindgen::prelude::*;

use crate::download::{fetch, image};
use crate::render::{CharacterMesh, Renderer};

const HOSTILE_SOLDIER_TEXTURE: &str = "soldier_enemy.png";

pub struct Model {
    pub character: Character,
    pub mesh: CharacterMesh,
}

#[derive(Default)]
pub struct Library {
    pub champions: HashMap<u8, Model>,
    pub minions: Vec<(Team, MinionKind, Model)>,
    pub soldier: Option<Model>,
    pub hostile_soldier: Option<Model>,
    pub sun_disc: Option<Model>,
    pub clicker: Option<Model>,
    pub structures: Vec<(UnitKind, Team, Model)>,
}

impl Library {
    pub fn seen_by(&self, kind: UnitKind, team: Team, viewer: Team) -> Option<&Model> {
        match kind {
            UnitKind::Soldier if team != viewer => {
                self.hostile_soldier.as_ref().or(self.soldier.as_ref())
            }
            _ => self.model(kind, team),
        }
    }

    pub fn model(&self, kind: UnitKind, team: Team) -> Option<&Model> {
        match kind {
            UnitKind::Champion(index) => self.champions.get(&index),
            UnitKind::Minion(minion) => self
                .minions
                .iter()
                .find(|(owner, kind, _)| *owner == team && *kind == minion)
                .map(|(_, _, model)| model),
            UnitKind::Soldier => self.soldier.as_ref(),
            UnitKind::SunDisc => self.sun_disc.as_ref(),
            structure => self
                .structures
                .iter()
                .find(|(kind, owner, _)| *kind == structure && *owner == team)
                .map(|(_, _, model)| model),
        }
    }
}

pub fn minion_file(team: Team, kind: MinionKind) -> String {
    let side = match team {
        Team::Blue => "order",
        Team::Red => "chaos",
    };
    let kind = match kind {
        MinionKind::Melee => "melee",
        MinionKind::Caster => "ranged",
        MinionKind::Siege => "siege",
        MinionKind::Super => "super",
    };
    format!("ha_{side}minion{kind}")
}

pub fn portrait_file(kind: UnitKind, team: Team) -> Option<String> {
    match kind {
        UnitKind::Minion(minion) => Some(minion_file(team, minion)),
        UnitKind::Turret | UnitKind::Shrine => Some(structure_file(kind, team)),
        _ => None,
    }
}

fn structure_file(kind: UnitKind, team: Team) -> String {
    let name = match kind {
        UnitKind::Shrine => "shrine",
        UnitKind::Inhibitor => "inhibitor",
        UnitKind::Nexus => "nexus",
        _ => "turret",
    };
    let side = match team {
        Team::Blue => "order",
        Team::Red => "chaos",
    };
    format!("{name}_{side}")
}

pub async fn load_units(renderer: &Renderer) -> Result<Library, JsValue> {
    let mut library = Library::default();
    for team in [Team::Blue, Team::Red] {
        for kind in [
            MinionKind::Melee,
            MinionKind::Caster,
            MinionKind::Siege,
            MinionKind::Super,
        ] {
            if let Ok(model) = load_model(renderer, "units", &minion_file(team, kind)).await {
                library.minions.push((team, kind, model));
            }
        }
    }
    library.soldier = load_model(renderer, "units", "soldier").await.ok();
    if crate::download::listed(&format!("assets/units/{HOSTILE_SOLDIER_TEXTURE}")) {
        library.hostile_soldier =
            load_dressed(renderer, "units", "soldier", Some(HOSTILE_SOLDIER_TEXTURE))
                .await
                .map_err(|error| web_sys::console::warn_1(&error))
                .ok();
    }
    library.sun_disc = load_model(renderer, "units", "sun_disc").await.ok();
    library.clicker = load_model(renderer, "units", "sun_disc_clicker").await.ok();
    for kind in [
        UnitKind::Turret,
        UnitKind::Shrine,
        UnitKind::Inhibitor,
        UnitKind::Nexus,
    ] {
        for team in [Team::Blue, Team::Red] {
            if let Ok(model) = load_model(renderer, "units", &structure_file(kind, team)).await {
                library.structures.push((kind, team, model));
            }
        }
    }
    Ok(library)
}

pub async fn load_model(renderer: &Renderer, folder: &str, name: &str) -> Result<Model, JsValue> {
    load_dressed(renderer, folder, name, None).await
}

async fn load_dressed(
    renderer: &Renderer,
    folder: &str,
    name: &str,
    outfit: Option<&str>,
) -> Result<Model, JsValue> {
    let bytes = fetch(&format!("assets/{folder}/{name}.bin")).await?;
    let character = Character::from_bytes(&bytes).map_err(|error| error.to_string())?;
    let mut images = Vec::new();
    for texture in &character.textures {
        let texture = outfit.unwrap_or(texture);
        images.push(image(&format!("assets/{folder}/{texture}")).await?);
    }
    let mesh = renderer.character(&character, &images)?;
    Ok(Model { character, mesh })
}
