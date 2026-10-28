use std::collections::HashMap;
use std::time::{Duration, Instant};

use protocol::{ServerMessage, VoteKind, VoteStatus};

use crate::Hub;

pub struct Vote {
    kind: VoteKind,
    players: Vec<String>,
    ballots: HashMap<String, bool>,
    deadline: Instant,
    broadcast: Instant,
}

impl Vote {
    fn needed(&self) -> usize {
        match self.kind {
            VoteKind::Surrender => self.players.len() / 2 + 1,
            VoteKind::EndGame => self.players.len(),
        }
    }

    fn yes(&self) -> usize {
        self.ballots.values().filter(|yes| **yes).count()
    }

    fn result(&self) -> Option<bool> {
        if self.yes() >= self.needed() {
            Some(true)
        } else if Instant::now() >= self.deadline
            || self.yes() + self.players.len() - self.ballots.len() < self.needed()
        {
            Some(false)
        } else {
            None
        }
    }
}

impl Hub {
    pub fn start_vote(&mut self, client: u64, kind: VoteKind) {
        let Some(name) = self.name_of(client) else {
            return;
        };
        let Some(game) = &self.game else {
            return;
        };
        if game.winner.is_some() || !self.playing(&name) {
            return;
        }
        let notice = if self.vote.is_some() {
            Some("A vote is already running.")
        } else if self
            .vote_cooldown
            .is_some_and(|at| at.elapsed() < Duration::from_secs(60))
        {
            Some("Wait before starting another vote.")
        } else {
            None
        };
        if let Some(notice) = notice {
            self.send(client, &ServerMessage::Notice(notice.into()));
            return;
        }
        let Some(team) = self.team_of(&name) else {
            return;
        };
        let players = game
            .seats
            .iter()
            .filter(|seat| kind == VoteKind::EndGame || seat.team == team)
            .map(|seat| seat.name.clone())
            .collect();
        self.vote = Some(Vote {
            kind,
            players,
            ballots: HashMap::from([(name, true)]),
            deadline: Instant::now() + Duration::from_secs(30),
            broadcast: Instant::now(),
        });
        self.vote_cooldown = Some(Instant::now());
        self.broadcast_vote();
        self.update_vote();
    }

    pub fn cast_vote(&mut self, client: u64, yes: bool) {
        let Some(name) = self.name_of(client) else {
            return;
        };
        if let Some(vote) = &mut self.vote
            && vote.players.contains(&name)
            && !vote.ballots.contains_key(&name)
        {
            vote.ballots.insert(name, yes);
            self.broadcast_vote();
            self.update_vote();
        }
    }

    pub fn broadcast_vote(&self) {
        let Some(vote) = &self.vote else {
            return;
        };
        for (id, client) in &self.clients {
            let Some(name) = &client.name else {
                continue;
            };
            if vote.players.contains(name) {
                self.send(
                    *id,
                    &ServerMessage::Vote(Some(VoteStatus {
                        kind: vote.kind,
                        yes: vote.yes(),
                        needed: vote.needed(),
                        remaining: vote
                            .deadline
                            .saturating_duration_since(Instant::now())
                            .as_secs_f32(),
                        voted: vote.ballots.contains_key(name),
                        result: vote.result(),
                    })),
                );
            }
        }
    }

    pub fn update_vote(&mut self) {
        let Some(vote) = &self.vote else {
            return;
        };
        if let Some(passed) = vote.result() {
            let kind = vote.kind;
            let team = vote.players.first().and_then(|name| self.team_of(name));
            self.broadcast_vote();
            self.vote = None;
            if passed {
                match kind {
                    VoteKind::EndGame => self.back_to_lobby(),
                    VoteKind::Surrender => {
                        if let Some(game) = &mut self.game {
                            game.winner = team.map(|team| team.enemy());
                        }
                        self.loading = None;
                    }
                }
            }
        } else if vote.broadcast.elapsed() >= Duration::from_secs(1) {
            self.broadcast_vote();
            if let Some(vote) = &mut self.vote {
                vote.broadcast = Instant::now();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vote(kind: VoteKind) -> Vote {
        Vote {
            kind,
            players: vec!["a".into(), "b".into(), "c".into()],
            ballots: HashMap::new(),
            deadline: Instant::now() + Duration::from_secs(30),
            broadcast: Instant::now(),
        }
    }

    #[test]
    fn surrender_needs_a_majority_but_ending_needs_everyone() {
        let mut surrender = vote(VoteKind::Surrender);
        surrender
            .ballots
            .extend([("a".into(), true), ("b".into(), true)]);
        assert_eq!(surrender.result(), Some(true));
        let mut end = vote(VoteKind::EndGame);
        end.ballots = surrender.ballots;
        assert_eq!(end.result(), None);
        end.ballots.insert("c".into(), false);
        assert_eq!(end.result(), Some(false));
    }

    #[test]
    fn unanswered_votes_expire() {
        let mut vote = vote(VoteKind::EndGame);
        vote.deadline = Instant::now() - Duration::from_secs(1);
        assert_eq!(vote.result(), Some(false));
    }
}
