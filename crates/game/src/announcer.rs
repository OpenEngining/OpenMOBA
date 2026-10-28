use std::collections::HashMap;

use protocol::{Score, Team, UnitId};

const MULTI_KILL_WINDOW: f32 = 10.0;
const PENTA_KILL_WINDOW: f32 = 30.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Victim {
    Champion,
    Turret,
    Inhibitor,
    Nexus,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Minor,
    Major,
    Epic(u32),
    Pentakill,
    Ace,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Announcement {
    pub text: &'static str,
    pub audio: &'static str,
    pub style: Style,
    pub killer: Option<UnitId>,
    pub victim: Option<UnitId>,
}

#[derive(Default)]
struct Streak {
    spree: u32,
    multi: u32,
    last_kill: f32,
}

pub struct Kill {
    pub time: f32,
    pub killer: Option<(UnitId, Team)>,
    pub victim: UnitId,
    pub victim_team: Team,
    pub victim_kind: Victim,
    pub victim_team_alive: usize,
}

pub struct Announcer {
    you: UnitId,
    team: Team,
    first_blood: bool,
    synced: bool,
    streaks: HashMap<UnitId, Streak>,
}

fn announcement(
    text: &'static str,
    audio: &'static str,
    killer: Option<UnitId>,
    victim: Option<UnitId>,
) -> Announcement {
    Announcement {
        text,
        audio,
        style: Style::Minor,
        killer,
        victim,
    }
}

fn styled(style: Style, announcement: Announcement) -> Announcement {
    Announcement {
        style,
        ..announcement
    }
}

impl Announcer {
    pub fn new(you: UnitId, team: Team) -> Self {
        Self {
            you,
            team,
            first_blood: false,
            synced: false,
            streaks: HashMap::new(),
        }
    }

    pub fn sync(&mut self, scores: &[(UnitId, Score)]) {
        if self.synced {
            return;
        }
        self.synced = true;
        self.first_blood = scores.iter().any(|(_, score)| score.kills > 0);
        self.streaks = scores
            .iter()
            .filter(|(_, score)| score.spree > 0)
            .map(|(id, score)| {
                (
                    *id,
                    Streak {
                        spree: u32::from(score.spree),
                        ..Streak::default()
                    },
                )
            })
            .collect();
    }

    pub fn spree(&self, id: UnitId) -> u32 {
        self.streaks.get(&id).map_or(0, |streak| streak.spree)
    }

    pub fn kill(&mut self, kill: Kill) -> Vec<Announcement> {
        let ally_victim = kill.victim_team == self.team;
        let killer = kill.killer.map(|(id, _)| id);
        match kill.victim_kind {
            Victim::Turret => {
                return vec![if ally_victim {
                    announcement(
                        "game_announcement_onturretdie_yourteam",
                        "announcer.ally_turret_destroyed",
                        killer,
                        None,
                    )
                } else {
                    announcement(
                        "game_announcement_onturretdie_enemyteam",
                        "announcer.enemy_turret_destroyed",
                        killer,
                        None,
                    )
                }];
            }
            Victim::Inhibitor => {
                return vec![if ally_victim {
                    announcement(
                        "game_announcement_ondampenerdie_yourteam",
                        "announcer.ally_inhibitor_destroyed",
                        killer,
                        None,
                    )
                } else {
                    announcement(
                        "game_announcement_ondampenerdie_enemyteam",
                        "announcer.enemy_inhibitor_destroyed",
                        killer,
                        None,
                    )
                }];
            }
            Victim::Nexus | Victim::Other => return Vec::new(),
            Victim::Champion => {}
        }
        let victim_spree = self
            .streaks
            .remove(&kill.victim)
            .map_or(0, |streak| streak.spree);
        let mut announcements = Vec::new();
        let victim = Some(kill.victim);
        let Some((killer_id, _)) = kill.killer else {
            announcements.push(if kill.victim == self.you {
                announcement(
                    "game_announcement_onchampionkill_you_minion",
                    "announcer.you_slain",
                    None,
                    victim,
                )
            } else if ally_victim {
                announcement(
                    "game_announcement_onchampionkill_ally_minion",
                    "announcer.ally_slain",
                    None,
                    victim,
                )
            } else {
                announcement(
                    "game_announcement_onchampionkill_enemy_minion",
                    "announcer.enemy_slain",
                    None,
                    victim,
                )
            });
            return announcements;
        };
        let streak = self.streaks.entry(killer_id).or_default();
        let window = if streak.multi == 4 {
            PENTA_KILL_WINDOW
        } else {
            MULTI_KILL_WINDOW
        };
        streak.multi = if streak.multi > 0 && kill.time - streak.last_kill <= window {
            streak.multi + 1
        } else {
            1
        };
        streak.last_kill = kill.time;
        streak.spree += 1;
        let (multi, spree) = (streak.multi, streak.spree);
        let killer = Some(killer_id);
        if !self.first_blood {
            self.first_blood = true;
            announcements.push(styled(
                Style::Major,
                announcement(
                    "game_announcement_onfirstblood",
                    "announcer.first_blood",
                    killer,
                    victim,
                ),
            ));
        } else if victim_spree >= 3 {
            announcements.push(styled(
                Style::Major,
                announcement(
                    "game_announcement_onchampionshutdownannouncement",
                    "announcer.shutdown",
                    killer,
                    victim,
                ),
            ));
        } else if multi >= 2 {
            let (text, audio, style) = match multi {
                2 => (
                    "game_announcement_onchampiondoublekillannouncement",
                    "announcer.double_kill",
                    Style::Epic(2),
                ),
                3 => (
                    "game_announcement_onchampiontriplekillannouncement",
                    "announcer.triple_kill",
                    Style::Epic(3),
                ),
                4 => (
                    "game_announcement_onchampionquadrakillannouncement",
                    "announcer.quadra_kill",
                    Style::Epic(4),
                ),
                _ => (
                    "game_announcement_onchampionpentakillannouncement",
                    "announcer.penta_kill",
                    Style::Pentakill,
                ),
            };
            announcements.push(styled(style, announcement(text, audio, killer, victim)));
        } else if spree >= 3 {
            let (text, audio) = match spree {
                3 => (
                    "game_announcement_onkillingspreeset1",
                    "announcer.killing_spree",
                ),
                4 => ("game_announcement_onkillingspreeset2", "announcer.rampage"),
                5 => (
                    "game_announcement_onkillingspreeset3",
                    "announcer.unstoppable",
                ),
                6 => (
                    "game_announcement_onkillingspreeset4",
                    "announcer.dominating",
                ),
                7 => ("game_announcement_onkillingspreeset5", "announcer.godlike"),
                _ => (
                    "game_announcement_onkillingspreeset6",
                    "announcer.legendary",
                ),
            };
            announcements.push(styled(
                Style::Major,
                announcement(text, audio, killer, victim),
            ));
        } else if killer_id == self.you {
            announcements.push(announcement(
                "game_announcement_onchampionkill_enemy_you",
                "announcer.you_killed",
                killer,
                victim,
            ));
        } else if kill.victim == self.you {
            announcements.push(announcement(
                "game_announcement_onchampionkill_you_hero",
                "announcer.you_slain",
                killer,
                victim,
            ));
        } else if ally_victim {
            announcements.push(announcement(
                "game_announcement_onchampionkill_ally_hero",
                "announcer.ally_slain",
                killer,
                victim,
            ));
        } else {
            announcements.push(announcement(
                "game_announcement_onchampionkill_enemy_hero",
                "announcer.enemy_slain",
                killer,
                victim,
            ));
        }
        if kill.victim_team_alive == 0 {
            announcements.push(styled(
                Style::Ace,
                announcement("game_announcement_onace", "announcer.ace", killer, None),
            ));
        }
        announcements
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kill(time: f32, killer: UnitId, victim: UnitId, alive: usize) -> Kill {
        Kill {
            time,
            killer: Some((killer, Team::Blue)),
            victim,
            victim_team: Team::Red,
            victim_kind: Victim::Champion,
            victim_team_alive: alive,
        }
    }

    fn texts(announcements: Vec<Announcement>) -> Vec<&'static str> {
        announcements.into_iter().map(|item| item.text).collect()
    }

    #[test]
    fn a_refreshed_client_picks_up_first_blood_and_sprees_from_the_scores() {
        let mut announcer = Announcer::new(1, Team::Blue);
        let score = |kills: u16, spree: u16| Score {
            kills,
            deaths: 0,
            assists: 0,
            spree,
            minions: 0,
            items: [None; 6],
        };
        announcer.sync(&[(1, score(3, 3)), (2, score(0, 0))]);
        assert_eq!(announcer.spree(1), 3);
        let after = texts(announcer.kill(kill(0.0, 1, 2, 1)));
        assert!(!after.contains(&"game_announcement_onfirstblood"));
        assert_eq!(announcer.spree(1), 4);
    }

    #[test]
    fn first_blood_then_double_kill_then_ace() {
        let mut announcer = Announcer::new(1, Team::Blue);
        assert_eq!(
            texts(announcer.kill(kill(10.0, 1, 5, 2))),
            ["game_announcement_onfirstblood"]
        );
        let double = announcer.kill(kill(15.0, 1, 6, 1));
        assert_eq!(double[0].style, Style::Epic(2));
        assert_eq!(
            texts(double),
            ["game_announcement_onchampiondoublekillannouncement"]
        );
        assert_eq!(
            texts(announcer.kill(kill(40.0, 1, 7, 0))),
            [
                "game_announcement_onkillingspreeset1",
                "game_announcement_onace"
            ]
        );
    }

    #[test]
    fn killing_a_spree_is_a_shutdown() {
        let mut announcer = Announcer::new(1, Team::Blue);
        for (time, victim) in [(0.0, 5), (20.0, 6), (40.0, 7)] {
            announcer.kill(Kill {
                victim_team: Team::Blue,
                ..kill(time, 9, victim, 3)
            });
        }
        let shutdown = announcer.kill(Kill {
            victim_team: Team::Red,
            ..kill(60.0, 1, 9, 3)
        });
        assert_eq!(
            texts(shutdown),
            ["game_announcement_onchampionshutdownannouncement"]
        );
    }

    #[test]
    fn structures_speak_from_your_side() {
        let mut announcer = Announcer::new(1, Team::Blue);
        let lost = announcer.kill(Kill {
            victim_team: Team::Blue,
            victim_kind: Victim::Turret,
            ..kill(0.0, 9, 50, 0)
        });
        assert_eq!(texts(lost), ["game_announcement_onturretdie_yourteam"]);
    }
}
