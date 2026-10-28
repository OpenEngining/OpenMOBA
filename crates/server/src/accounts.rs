use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use protocol::{AuthError, Profile};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
struct Account {
    hash: String,
    name: String,
    #[serde(default)]
    avatar: u8,
    #[serde(default)]
    wins: u32,
    #[serde(default)]
    games: u32,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StoredAccount {
    Legacy(String),
    Current(Account),
}

pub struct Accounts {
    path: PathBuf,
    accounts: HashMap<String, Account>,
}

pub fn valid_name(name: &str) -> bool {
    (1..=16).contains(&name.chars().count())
        && name
            .chars()
            .all(|character| character.is_alphanumeric() || character == '_' || character == ' ')
        && name.trim() == name
}

impl Accounts {
    pub fn sessions_path(&self) -> PathBuf {
        self.path.with_extension("sessions.json")
    }

    pub fn load(path: PathBuf) -> Self {
        let stored: HashMap<String, StoredAccount> = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).expect("invalid accounts file"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(error) => panic!("could not read accounts: {error}"),
        };
        let accounts = stored
            .into_iter()
            .map(|(key, stored)| {
                let account = match stored {
                    StoredAccount::Legacy(hash) => Account {
                        hash,
                        name: key.clone(),
                        avatar: 0,
                        wins: 0,
                        games: 0,
                    },
                    StoredAccount::Current(account) => account,
                };
                (key, account)
            })
            .collect();
        Self { path, accounts }
    }

    fn key(name: &str) -> String {
        name.to_lowercase()
    }

    pub fn display_name(&self, name: &str) -> Option<String> {
        self.accounts
            .get(&Self::key(name))
            .map(|account| account.name.clone())
    }

    pub fn sign_up(&mut self, name: &str, password: &str) -> Result<(), AuthError> {
        if !valid_name(name) || password.is_empty() || password.len() > 256 {
            return Err(AuthError::InvalidName);
        }
        if self.accounts.contains_key(&Self::key(name)) {
            return Err(AuthError::NameTaken);
        }
        let salt_bytes: [u8; 16] = rand::random();
        let salt = SaltString::encode_b64(&salt_bytes).map_err(|_| AuthError::InvalidName)?;
        let hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|_| AuthError::InvalidName)?
            .to_string();
        self.accounts.insert(
            Self::key(name),
            Account {
                hash,
                name: name.to_owned(),
                avatar: 0,
                wins: 0,
                games: 0,
            },
        );
        if self.save().is_err() {
            self.accounts.remove(&Self::key(name));
            return Err(AuthError::Unavailable);
        }
        Ok(())
    }

    pub fn log_in(&self, name: &str, password: &str) -> Result<(), AuthError> {
        if password.len() > 256 {
            return Err(AuthError::WrongNameOrPassword);
        }
        let account = self
            .accounts
            .get(&Self::key(name))
            .ok_or(AuthError::WrongNameOrPassword)?;
        let parsed =
            PasswordHash::new(&account.hash).map_err(|_| AuthError::WrongNameOrPassword)?;
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .map_err(|_| AuthError::WrongNameOrPassword)
    }

    pub fn profile(&self, name: &str) -> Option<Profile> {
        self.accounts.get(&Self::key(name)).map(|account| Profile {
            name: account.name.clone(),
            avatar: account.avatar,
            wins: account.wins,
            games: account.games,
        })
    }

    pub fn leaderboard(&self) -> Vec<Profile> {
        let mut profiles: Vec<_> = self
            .accounts
            .values()
            .filter_map(|account| self.profile(&account.name))
            .collect();
        profiles.sort_by(|a, b| {
            b.wins
                .cmp(&a.wins)
                .then_with(|| a.games.cmp(&b.games))
                .then_with(|| a.name.cmp(&b.name))
        });
        profiles.truncate(100);
        profiles
    }

    pub fn avatar(&mut self, name: &str, avatar: u8) -> std::io::Result<()> {
        let Some(account) = self.accounts.get_mut(&Self::key(name)) else {
            return Ok(());
        };
        let previous = account.avatar;
        account.avatar = avatar;
        if let Err(error) = self.save() {
            self.accounts
                .get_mut(&Self::key(name))
                .expect("account exists")
                .avatar = previous;
            return Err(error);
        }
        Ok(())
    }

    pub fn record(&mut self, players: &[(String, bool)]) -> std::io::Result<()> {
        let previous = self.accounts.clone();
        for (name, won) in players {
            if let Some(account) = self.accounts.get_mut(&Self::key(name)) {
                account.games = account.games.saturating_add(1);
                account.wins = account.wins.saturating_add(u32::from(*won));
            }
        }
        if let Err(error) = self.save() {
            self.accounts = previous;
            return Err(error);
        }
        Ok(())
    }

    fn save(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_vec_pretty(&self.accounts)?;
        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, json)?;
        fs::rename(temporary, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_short_and_simple() {
        assert!(valid_name("Yurii"));
        assert!(valid_name("player_two 2"));
        assert!(!valid_name(""));
        assert!(!valid_name(" padded"));
        assert!(!valid_name("way_too_long_for_a_name"));
        assert!(!valid_name("<script>"));
    }

    #[test]
    fn sign_up_then_log_in() {
        let path = std::env::temp_dir().join(format!("lll-accounts-{}.json", std::process::id()));
        let mut accounts = Accounts::load(path.clone());
        assert_eq!(accounts.sign_up("PlayerOne", "secretpass"), Ok(()));
        assert_eq!(
            accounts.sign_up("playerone", "other"),
            Err(AuthError::NameTaken)
        );
        assert_eq!(accounts.log_in("PLAYERONE", "secretpass"), Ok(()));
        assert_eq!(
            accounts.log_in("PlayerOne", "wrong"),
            Err(AuthError::WrongNameOrPassword)
        );
        let reloaded = Accounts::load(path.clone());
        assert_eq!(reloaded.log_in("PlayerOne", "secretpass"), Ok(()));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn legacy_passwords_survive_profile_migration_and_results_persist() {
        let path = std::env::temp_dir().join(format!("lll-profile-{}.json", rand::random::<u64>()));
        let mut accounts = Accounts::load(path.clone());
        accounts.sign_up("Hero", "passphrase").unwrap();
        let hash = accounts.accounts["hero"].hash.clone();
        fs::write(
            &path,
            serde_json::to_vec(&HashMap::from([("hero", hash)])).unwrap(),
        )
        .unwrap();
        let mut accounts = Accounts::load(path.clone());
        assert_eq!(accounts.log_in("HERO", "passphrase"), Ok(()));
        accounts.avatar("hero", 3).unwrap();
        accounts.record(&[("hero".into(), true)]).unwrap();
        accounts.record(&[("hero".into(), false)]).unwrap();
        let reloaded = Accounts::load(path.clone());
        assert_eq!(reloaded.log_in("HERO", "passphrase"), Ok(()));
        assert_eq!(
            reloaded.profile("hero").unwrap(),
            Profile {
                name: "hero".into(),
                avatar: 3,
                wins: 1,
                games: 2
            }
        );
        assert_eq!(reloaded.leaderboard().len(), 1);
        fs::remove_file(path).unwrap();
    }
}
