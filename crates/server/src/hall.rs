use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use asset::data::GameData;
use protocol::{ClientMessage, Directory, Phase, RoomInfo, ServerMessage};
use sim::layout::Layout;
use tokio::sync::mpsc::UnboundedSender;

use crate::{Accounts, Client, Hub};

pub struct Hall {
    auth: Hub,
    rooms: HashMap<u64, Hub>,
    membership: HashMap<String, u64>,
    next_room: u64,
    refreshed: Instant,
    sent: HashMap<u64, Directory>,
}

impl Hall {
    pub fn new(accounts: Accounts, data: Arc<GameData>, layout: Layout, practice: bool) -> Self {
        Self {
            auth: Hub::new(accounts, data, layout, practice),
            rooms: HashMap::new(),
            membership: HashMap::new(),
            next_room: 0,
            refreshed: Instant::now(),
            sent: HashMap::new(),
        }
    }

    pub fn join(&mut self, sender: UnboundedSender<Vec<u8>>) -> u64 {
        self.auth.join(sender)
    }

    pub fn name_of(&self, client: u64) -> Option<String> {
        self.auth.name_of(client)
    }

    pub fn leave(&mut self, client: u64) {
        if let Some(name) = self.name_of(client)
            && let Some(room) = self
                .membership
                .get(&name)
                .and_then(|id| self.rooms.get_mut(id))
        {
            room.leave(client);
            if room.phase() == Phase::Select {
                room.back_to_lobby();
            }
        }
        self.auth.leave(client);
        self.sent.remove(&client);
        self.refresh();
    }

    fn notice(&self, client: u64, message: &str) {
        self.auth
            .send(client, &ServerMessage::Notice(message.to_owned()));
    }

    pub fn handle(&mut self, client: u64, message: ClientMessage) {
        if matches!(
            message,
            ClientMessage::SignUp { .. }
                | ClientMessage::LogIn { .. }
                | ClientMessage::Resume { .. }
        ) {
            if self.name_of(client).is_some() {
                return;
            }
            self.auth.handle(client, message);
            if let Some(name) = self.name_of(client) {
                let previous: Vec<u64> = self
                    .auth
                    .clients
                    .iter()
                    .filter(|(id, entry)| **id != client && entry.name.as_deref() == Some(&name))
                    .map(|(id, _)| *id)
                    .collect();
                for id in previous {
                    self.auth.send(
                        id,
                        &ServerMessage::AuthFailed(protocol::AuthError::WrongNameOrPassword),
                    );
                    self.leave(id);
                    self.auth.clients.remove(&id);
                }
                if let Some(room) = self.membership.get(&name).copied() {
                    self.enter(client, room);
                }
            }
            self.refresh();
            return;
        }
        let Some(name) = self.name_of(client) else {
            return;
        };
        match message {
            ClientMessage::CreateLobby(title) => {
                if self.membership.contains_key(&name) || self.rooms.len() >= 32 {
                    return;
                }
                let title = title.trim();
                if title.is_empty()
                    || title.chars().count() > 32
                    || title.chars().any(char::is_control)
                {
                    self.notice(client, "Use a lobby name between 1 and 32 characters.");
                    return;
                }
                self.next_room += 1;
                let id = self.next_room;
                let mut room = Hub::with_accounts(
                    None,
                    self.auth.data.clone(),
                    self.auth.layout.clone(),
                    self.auth.practice,
                );
                room.id = id;
                room.name = title.to_owned();
                self.rooms.insert(id, room);
                self.enter(client, id);
            }
            ClientMessage::JoinLobby(id) => {
                if self.membership.contains_key(&name) {
                    return;
                }
                self.enter(client, id);
            }
            ClientMessage::LeaveLobby => {
                if let Some(id) = self.membership.get(&name).copied()
                    && let Some(room) = self.rooms.get_mut(&id)
                {
                    if room.phase() == Phase::Game {
                        self.notice(client, "Use a vote to finish the current match.");
                        return;
                    }
                    if room.phase() == Phase::Select {
                        room.back_to_lobby();
                    }
                    room.leave(client);
                    room.lobby.retain(|(player, _)| *player != name);
                    room.broadcast_lobby();
                    self.membership.remove(&name);
                }
            }
            ClientMessage::SetAvatar(avatar) => {
                if usize::from(avatar) < self.auth.data.champions.len()
                    && let Some(accounts) = &mut self.auth.accounts
                    && accounts.avatar(&name, avatar).is_err()
                {
                    self.notice(client, "Could not save your profile. Please try again.");
                }
            }
            ClientMessage::LogOut => {
                if self.auth.sessions.revoke(&name).is_err() {
                    self.notice(
                        client,
                        "Could not clear remembered login on the server. Please try again.",
                    );
                }
                if let Some(room) = self
                    .membership
                    .get(&name)
                    .and_then(|id| self.rooms.get_mut(id))
                {
                    room.leave(client);
                    if room.phase() == Phase::Select {
                        room.back_to_lobby();
                    }
                }
                if let Some(entry) = self.auth.clients.get_mut(&client) {
                    entry.name = None;
                }
                self.auth.prune_lobby();
                self.sent.remove(&client);
            }
            message => {
                if let Some(room) = self
                    .membership
                    .get(&name)
                    .and_then(|id| self.rooms.get_mut(id))
                {
                    room.handle(client, message);
                }
            }
        }
        self.refresh();
    }

    fn enter(&mut self, client: u64, id: u64) {
        let Some(name) = self.name_of(client) else {
            return;
        };
        let Some(room) = self.rooms.get_mut(&id) else {
            self.membership.remove(&name);
            return;
        };
        let returning = room.lobby.iter().any(|(player, _)| *player == name);
        if !returning && (room.phase() != Phase::Lobby || room.lobby.len() >= 10) {
            self.notice(client, "Lobby full or unavailable.");
            return;
        }
        let Some(entry) = self.auth.clients.get(&client) else {
            return;
        };
        room.clients.insert(
            client,
            Client {
                sender: entry.sender.clone(),
                name: None,
            },
        );
        self.membership.insert(name.clone(), id);
        room.attach(client, name);
    }

    fn refresh(&mut self) {
        self.rooms
            .retain(|_, room| !room.clients.is_empty() || room.phase() == Phase::Game);
        self.membership.retain(|name, id| {
            self.rooms
                .get(id)
                .is_some_and(|room| room.lobby.iter().any(|(player, _)| player == name))
        });
        let mut rooms: Vec<_> = self
            .rooms
            .values()
            .map(|room| RoomInfo {
                id: room.id,
                name: room.name.clone(),
                host: room.host(),
                players: room.lobby.len(),
                phase: room.phase(),
            })
            .collect();
        rooms.sort_by_key(|room| room.id);
        let Some(accounts) = &self.auth.accounts else {
            return;
        };
        let leaderboard = accounts.leaderboard();
        for (id, client) in &self.auth.clients {
            let Some(name) = client.name.as_ref() else {
                continue;
            };
            let Some(profile) = accounts.profile(name) else {
                continue;
            };
            let directory = Directory {
                rooms: rooms.clone(),
                profile,
                leaderboard: leaderboard.clone(),
                room: self.membership.get(name).copied(),
            };
            if self.sent.get(id) != Some(&directory) {
                self.auth
                    .send(*id, &ServerMessage::Directory(directory.clone()));
                self.sent.insert(*id, directory);
            }
        }
    }

    pub fn tick(&mut self) {
        for room in self.rooms.values_mut() {
            room.tick();
            if let Some(result) = &room.finished
                && let Some(accounts) = &mut self.auth.accounts
            {
                match accounts.record(result) {
                    Ok(()) => room.finished = None,
                    Err(error) => eprintln!("could not save match result: {error}"),
                }
            }
        }
        if self.refreshed.elapsed() >= Duration::from_secs(1) {
            self.refreshed = Instant::now();
            self.refresh();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{chats, hub};
    use protocol::{Team, decode};
    use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

    fn hall() -> Hall {
        Hall {
            auth: hub(),
            rooms: HashMap::new(),
            membership: HashMap::new(),
            next_room: 0,
            refreshed: Instant::now(),
            sent: HashMap::new(),
        }
    }

    fn player(hall: &mut Hall, name: &str) -> (u64, UnboundedReceiver<Vec<u8>>) {
        let (sender, receiver) = unbounded_channel();
        let id = hall.join(sender);
        hall.auth.authenticated(id, Ok(name.to_owned()));
        (id, receiver)
    }

    #[test]
    fn rooms_keep_chat_drafts_and_matches_separate() {
        let mut hall = hall();
        let (a, mut first) = player(&mut hall, "a");
        let (b, mut second) = player(&mut hall, "b");
        hall.handle(a, ClientMessage::CreateLobby("First".into()));
        hall.handle(b, ClientMessage::CreateLobby("Second".into()));
        hall.handle(
            a,
            ClientMessage::Chat {
                text: "hello".into(),
                all: true,
            },
        );
        assert_eq!(chats(&mut first), 1);
        assert_eq!(chats(&mut second), 0);
        hall.handle(a, ClientMessage::Start);
        assert_eq!(hall.rooms[&1].phase(), Phase::Select);
        assert_eq!(hall.rooms[&2].phase(), Phase::Lobby);
        hall.rooms.get_mut(&1).unwrap().start();
        assert_eq!(hall.rooms[&1].phase(), Phase::Game);
        assert!(hall.rooms[&2].game.is_none());
        hall.handle(b, ClientMessage::JoinLobby(1));
        assert_eq!(hall.membership["b"], 2);
    }

    #[test]
    fn leaving_transfers_host_and_cancels_an_incomplete_draft() {
        let mut hall = hall();
        let (a, _) = player(&mut hall, "a");
        let (b, _) = player(&mut hall, "b");
        hall.handle(a, ClientMessage::CreateLobby("First".into()));
        hall.handle(b, ClientMessage::JoinLobby(1));
        hall.handle(a, ClientMessage::Start);
        hall.handle(a, ClientMessage::LeaveLobby);
        assert_eq!(hall.rooms[&1].host(), "b");
        assert_eq!(hall.rooms[&1].phase(), Phase::Lobby);
        assert!(!hall.membership.contains_key("a"));
        hall.handle(b, ClientMessage::LeaveLobby);
        assert!(hall.rooms.is_empty());
    }

    #[test]
    fn reconnect_resumes_the_same_match_and_rotates_the_session() {
        let mut hall = hall();
        let (a, mut receiver) = player(&mut hall, "a");
        let (b, _) = player(&mut hall, "b");
        hall.handle(a, ClientMessage::CreateLobby("First".into()));
        hall.handle(b, ClientMessage::JoinLobby(1));
        hall.handle(a, ClientMessage::Start);
        hall.rooms.get_mut(&1).unwrap().start();
        let token = std::iter::from_fn(|| receiver.try_recv().ok())
            .find_map(|bytes| match decode::<ServerMessage>(&bytes) {
                Some(ServerMessage::Session { token, .. }) => Some(token),
                _ => None,
            })
            .unwrap();
        hall.leave(a);
        let (sender, mut receiver) = unbounded_channel();
        let again = hall.join(sender);
        hall.handle(
            again,
            ClientMessage::Resume {
                token: token.clone(),
            },
        );
        assert_eq!(hall.membership["a"], 1);
        assert!(hall.auth.sessions.take(&token).is_none());
        assert_eq!(hall.rooms[&1].team_of("a"), Some(Team::Blue));
        assert!(
            std::iter::from_fn(|| receiver.try_recv().ok()).any(|bytes| matches!(
                decode::<ServerMessage>(&bytes),
                Some(ServerMessage::GameStarted(_))
            ))
        );
    }

    #[test]
    fn anonymous_clients_cannot_create_or_enter_rooms() {
        let mut hall = hall();
        let (sender, _) = unbounded_channel();
        let id = hall.join(sender);
        hall.handle(id, ClientMessage::CreateLobby("Anonymous".into()));
        hall.handle(id, ClientMessage::JoinLobby(1));
        assert!(hall.rooms.is_empty());
        assert!(hall.membership.is_empty());
    }
}
