use serde::{Deserialize, Serialize};

pub const CONTROLS: [(&str, &str); 11] = [
    ("Ability Q", "KeyQ"),
    ("Ability W", "KeyW"),
    ("Ability E", "KeyE"),
    ("Ability R", "KeyR"),
    ("Summoner D", "KeyD"),
    ("Summoner F", "KeyF"),
    ("Attack move", "KeyA"),
    ("Stop", "KeyS"),
    ("Shop", "KeyP"),
    ("Toggle camera", "KeyY"),
    ("Center camera", "Space"),
];

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub follow: bool,
    pub hud: f32,
    pub chat: bool,
    pub damage: bool,
    pub names: bool,
    pub volume: f32,
    pub keys: [String; 11],
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            follow: false,
            hud: 1.0,
            chat: true,
            damage: true,
            names: true,
            volume: 1.0,
            keys: CONTROLS.map(|(_, key)| key.to_owned()),
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let mut settings: Self = web_sys::window()
            .and_then(|window| window.local_storage().ok().flatten())
            .and_then(|storage| storage.get_item("lll-settings-v1").ok().flatten())
            .and_then(|value| serde_json::from_str(&value).ok())
            .unwrap_or_default();
        settings.hud = settings.hud.clamp(0.7, 1.2);
        settings.volume = settings.volume.clamp(0.0, 1.0);
        if settings.keys.iter().any(|key| !allowed(key))
            || settings
                .keys
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                != CONTROLS.len()
        {
            settings.keys = Self::default().keys;
        }
        settings
    }

    pub fn save(&self) {
        if let Some(storage) =
            web_sys::window().and_then(|window| window.local_storage().ok().flatten())
            && let Ok(value) = serde_json::to_string(self)
        {
            let _ = storage.set_item("lll-settings-v1", &value);
        }
    }

    pub fn bind(&mut self, index: usize, code: &str) -> bool {
        if index >= self.keys.len() || !allowed(code) {
            return false;
        }
        if let Some(other) = self.keys.iter().position(|key| key == code) {
            self.keys.swap(index, other);
        } else {
            self.keys[index] = code.to_owned();
        }
        true
    }

    pub fn code(&self, code: &str) -> String {
        if let Some(index) = self.keys.iter().position(|key| key == code) {
            CONTROLS[index].1.to_owned()
        } else if CONTROLS.iter().any(|(_, key)| *key == code) {
            String::new()
        } else {
            code.to_owned()
        }
    }
}

pub fn label(code: &str) -> &str {
    code.strip_prefix("Key").unwrap_or(code)
}

fn allowed(code: &str) -> bool {
    code == "Space"
        || code.strip_prefix("Key").is_some_and(|key| {
            key.len() == 1 && key.as_bytes()[0].is_ascii_uppercase() && key != "C" && key != "H"
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebinding_swaps_conflicts_and_disables_old_keys() {
        let mut settings = Settings::default();
        assert!(settings.bind(0, "KeyW"));
        assert_eq!(settings.code("KeyW"), "KeyQ");
        assert_eq!(settings.code("KeyQ"), "KeyW");
        assert!(settings.bind(0, "KeyZ"));
        assert_eq!(settings.code("KeyW"), "");
        assert_eq!(settings.code("KeyZ"), "KeyQ");
        assert!(!settings.bind(0, "Escape"));
        assert!(!settings.bind(0, "Digit1"));
        assert_eq!(settings.code("Tab"), "Tab");
    }
}
