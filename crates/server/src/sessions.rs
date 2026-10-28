use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const LIFETIME: u64 = 30 * 24 * 60 * 60;

#[derive(Clone, Serialize, Deserialize)]
struct Session {
    name: String,
    expires: u64,
}

#[derive(Default)]
pub struct Sessions {
    path: Option<PathBuf>,
    entries: HashMap<String, Session>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn key(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

impl Sessions {
    pub fn load(path: PathBuf) -> Self {
        let entries = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Err(_) => HashMap::new(),
        };
        let mut sessions = Self {
            path: Some(path),
            entries,
        };
        sessions
            .entries
            .retain(|_, session| session.expires > now());
        sessions
    }

    pub fn issue(&mut self, name: &str) -> std::io::Result<String> {
        let token: String = rand::rng()
            .sample_iter(rand::distr::Alphanumeric)
            .take(48)
            .map(char::from)
            .collect();
        let previous = self.entries.clone();
        self.entries.retain(|_, session| session.expires > now());
        let mut owned: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, session)| session.name == name)
            .map(|(key, session)| (key.clone(), session.expires))
            .collect();
        owned.sort_by_key(|(_, expires)| *expires);
        for (key, _) in owned.iter().take(owned.len().saturating_sub(7)) {
            self.entries.remove(key);
        }
        self.entries.insert(
            key(&token),
            Session {
                name: name.to_owned(),
                expires: now() + LIFETIME,
            },
        );
        if let Err(error) = self.save() {
            self.entries = previous;
            return Err(error);
        }
        Ok(token)
    }

    pub fn take(&mut self, token: &str) -> Option<String> {
        if token.len() > 128 {
            return None;
        }
        let key = key(token);
        let session = self.entries.remove(&key)?;
        if self.save().is_err() {
            self.entries.insert(key, session);
            return None;
        }
        (session.expires > now()).then_some(session.name)
    }

    pub fn revoke(&mut self, name: &str) -> std::io::Result<()> {
        self.entries.retain(|_, session| session.name != name);
        self.save()
    }

    fn save(&self) -> std::io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, serde_json::to_vec(&self.entries)?)?;
        fs::rename(temporary, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembered_sessions_survive_restart_are_hashed_and_can_be_revoked() {
        let path =
            std::env::temp_dir().join(format!("lll-sessions-{}.json", rand::random::<u64>()));
        let mut sessions = Sessions::load(path.clone());
        let token = sessions.issue("Yurii").unwrap();
        assert!(!fs::read_to_string(&path).unwrap().contains(&token));
        let mut restarted = Sessions::load(path.clone());
        assert_eq!(restarted.take(&token).as_deref(), Some("Yurii"));
        assert!(restarted.take(&token).is_none());
        let token = restarted.issue("Yurii").unwrap();
        restarted.revoke("Yurii").unwrap();
        assert!(Sessions::load(path.clone()).take(&token).is_none());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn expired_sessions_cannot_resume() {
        let mut sessions = Sessions::default();
        let token = sessions.issue("a").unwrap();
        sessions.entries.get_mut(&key(&token)).unwrap().expires = now() - 1;
        assert!(sessions.take(&token).is_none());
    }
}
