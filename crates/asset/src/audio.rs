use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

pub fn interface_sound(event: &str) -> String {
    format!("ui.{}", event.to_lowercase())
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AudioIndex {
    pub sounds: BTreeMap<String, Vec<String>>,
}

impl AudioIndex {
    pub fn to_bytes(&self) -> postcard::Result<Vec<u8>> {
        postcard::to_allocvec(self)
    }

    pub fn from_bytes(bytes: &[u8]) -> postcard::Result<Self> {
        postcard::from_bytes(bytes)
    }

    pub fn variants(&self, key: &str) -> &[String] {
        self.sounds.get(key).map_or(&[], Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::borrow::ToOwned;
    use alloc::vec;

    #[test]
    fn round_trips_and_looks_up_variants() {
        let index = AudioIndex {
            sounds: BTreeMap::from([(
                "ability.q".to_owned(),
                vec!["a.ogg".to_owned(), "b.ogg".to_owned()],
            )]),
        };
        let decoded = AudioIndex::from_bytes(&index.to_bytes().unwrap()).unwrap();
        assert_eq!(decoded, index);
        assert_eq!(decoded.variants("ability.q").len(), 2);
        assert!(decoded.variants("missing").is_empty());
    }
}
