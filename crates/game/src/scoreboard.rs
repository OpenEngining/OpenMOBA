use asset::data::GameData;
use asset::ui::Layout;
use protocol::{Setup, Snapshot, Team};
use web_sys::Element;

use crate::frame::{Frame, View};

const SCENE: &str = "Scoreboard/Scoreboard";
const SCORES: &str = "Scoreboard/Scoreboard/Scoreboard_TeamScores";
const SLOT_HEIGHT: &str = "Scoreboard/Scoreboard/SB_PlayerSlotHeightRef";
const ROWS: usize = 5;
const ITEMS: usize = 7;
const TEAMS: [Team; 2] = [Team::Blue, Team::Red];
const SIDES: [&str; 2] = ["Left", "Right"];
const ROW_PARTS: [&str; 7] = [
    "Icon_Data",
    "Frame",
    "Level",
    "Spell0_",
    "Spell1_",
    "KDA_Text",
    "CS_Text",
];

pub struct Scoreboard {
    frame: Frame,
}

fn row(team: usize, index: usize) -> String {
    format!("{SCENE}/SB_T{}P{index}", team + 1)
}

fn part(team: usize, index: usize, part: &str) -> String {
    format!("{}/SB_T{}P0_{part}", row(team, index), team + 1)
}

impl Scoreboard {
    pub fn new(parent: &Element, layout: &Layout) -> Self {
        let mut frame = Frame::new(parent, layout, &[SCENE]);
        frame.show_scene(SCENE, false);
        let pitch = frame.rect(SLOT_HEIGHT).map_or(84.0, |rect| rect.size[1]);
        for team in 0..TEAMS.len() {
            for index in 1..ROWS {
                frame.instance(
                    &row(team, 0),
                    &row(team, index),
                    [0.0, pitch * index as f32],
                );
            }
            for index in 0..ROWS {
                frame.text(&part(team, index, "VS_Text"), "");
            }
            frame.show(&format!("{SCORES}/SB_TeamKills_T{}Score", team + 1), true);
        }
        frame.show(&format!("{SCORES}/SB_TeamKills_Icon1"), true);
        Self { frame }
    }

    pub fn resize(&mut self, height: f32) {
        self.frame.resize(height);
    }

    pub fn show(&self, visible: bool) {
        self.frame.show_scene(SCENE, visible);
    }

    pub fn update(&self, snapshot: &Snapshot, setup: &Setup, data: &GameData) {
        let frame = &self.frame;
        let you = setup.players.iter().find(|player| player.unit == setup.you);
        let ally = you.map_or(Team::Blue, |player| player.team);
        for (team_index, team) in TEAMS.iter().enumerate() {
            let side = SIDES[team_index];
            frame.show(&format!("{SCENE}/SB_AllySide_{side}"), *team == ally);
            frame.show(&format!("{SCENE}/SB_EnemySide_{side}"), *team != ally);
            let players: Vec<_> = setup
                .players
                .iter()
                .filter(|player| player.team == *team)
                .collect();
            let kills: u32 = players
                .iter()
                .filter_map(|player| {
                    snapshot
                        .scores
                        .iter()
                        .find(|(unit, _)| *unit == player.unit)
                })
                .map(|(_, score)| u32::from(score.kills))
                .sum();
            frame.text(
                &format!("{SCORES}/SB_TeamKills_T{}Score", team_index + 1),
                &kills.to_string(),
            );
            for index in 0..ROWS {
                let player = players.get(index);
                let name = |name: &str| part(team_index, index, name);
                for shown in ROW_PARTS.iter().chain(&["SummonerName"]) {
                    frame.show(&name(shown), player.is_some());
                }
                for slot in 0..ITEMS {
                    frame.show(&name(&format!("Item{slot}_Backdrop")), player.is_some());
                }
                let Some(player) = player else {
                    for overlay in [
                        "FrameSelf",
                        "PlayerSlot_Self",
                        "FrameDead",
                        "PlayerSlot_Dead",
                        "PlayerSlot_FoW",
                    ] {
                        frame.show(&name(overlay), false);
                    }
                    for slot in 0..ITEMS {
                        frame.show(&name(&format!("Item{slot}_")), false);
                    }
                    continue;
                };
                let unit = snapshot.units.iter().find(|unit| unit.id == player.unit);
                let dead = unit.is_some_and(|unit| unit.dead);
                let mine = player.unit == setup.you;
                frame.show(&name("FrameSelf"), mine);
                frame.show(&name("PlayerSlot_Self"), mine);
                frame.show(&name("FrameDead"), dead);
                frame.show(&name("PlayerSlot_Dead"), dead);
                frame.show(&name("PlayerSlot_FoW"), unit.is_none());
                frame.show(&name("Frame"), !mine && !dead);
                if let Some(champion) = data.champions.get(usize::from(player.champion)) {
                    frame.image(
                        &name("Icon_Data"),
                        &format!("assets/icons/{}", champion.icon),
                    );
                }
                frame.desaturate(&name("Icon_Data"), dead);
                if let Some(unit) = unit {
                    frame.text(&name("Level"), &unit.level.to_string());
                }
                for (slot, summoner) in player.summoners.iter().enumerate() {
                    if let Some(summoner) = data.summoner(summoner.id()) {
                        frame.image(
                            &name(&format!("Spell{slot}_")),
                            &format!("assets/icons/{}", summoner.icon),
                        );
                    }
                }
                frame.text(&name("SummonerName"), &player.name);
                let score = snapshot
                    .scores
                    .iter()
                    .find(|(unit, _)| *unit == player.unit)
                    .map(|(_, score)| *score);
                if let Some(score) = score {
                    frame.text(
                        &name("KDA_Text"),
                        &format!("{}/{}/{}", score.kills, score.deaths, score.assists),
                    );
                    frame.text(&name("CS_Text"), &score.minions.to_string());
                }
                for slot in 0..ITEMS {
                    let icon = score
                        .and_then(|score| score.items.get(slot).copied().flatten())
                        .and_then(|item| data.item(item));
                    let cell = name(&format!("Item{slot}_"));
                    frame.show(&cell, icon.is_some());
                    if let Some(icon) = icon {
                        frame.image(&cell, &format!("assets/icons/{}", icon.icon));
                    }
                }
            }
        }
    }
}
