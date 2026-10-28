use asset::clip_id;
use protocol::{BuffState, Slot, Summoner};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Passive,
    Ability(Slot),
    Summoner(Summoner),
    Item(u32),
    Origin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shown {
    pub icon: Icon,
    pub positive: bool,
    pub stacks: bool,
}

const fn buff(icon: Icon, positive: bool, stacks: bool) -> Shown {
    Shown {
        icon,
        positive,
        stacks,
    }
}

const BUFFS: [(&str, Shown); 38] = [
    ("phenomenal_evil", buff(Icon::Passive, true, true)),
    (
        "siphoning_strike_stacks",
        buff(Icon::Ability(Slot::Q), true, true),
    ),
    (
        "siphoning_strike",
        buff(Icon::Ability(Slot::Q), true, false),
    ),
    ("wither", buff(Icon::Ability(Slot::W), false, false)),
    ("spirit_fire", buff(Icon::Ability(Slot::E), false, false)),
    ("fury", buff(Icon::Ability(Slot::R), true, false)),
    ("decisive_strike", buff(Icon::Ability(Slot::Q), true, false)),
    ("courage_guard", buff(Icon::Ability(Slot::W), true, false)),
    ("judgment", buff(Icon::Ability(Slot::E), true, false)),
    ("judgment_stacks", buff(Icon::Ability(Slot::E), false, true)),
    ("judgment_shred", buff(Icon::Ability(Slot::E), false, false)),
    ("hemorrhage", buff(Icon::Passive, false, true)),
    ("warlords_might", buff(Icon::Passive, true, false)),
    (
        "crippling_strike",
        buff(Icon::Ability(Slot::W), true, false),
    ),
    ("nether_blade", buff(Icon::Ability(Slot::W), true, false)),
    ("riftwalk", buff(Icon::Ability(Slot::R), true, true)),
    ("gathering_storm", buff(Icon::Ability(Slot::Q), true, true)),
    (
        "sweeping_blade_stacks",
        buff(Icon::Ability(Slot::E), true, true),
    ),
    ("last_breath", buff(Icon::Ability(Slot::R), true, false)),
    ("granite_shield", buff(Icon::Passive, true, false)),
    (
        "thunderclap_ready",
        buff(Icon::Ability(Slot::W), true, false),
    ),
    ("thunderclap", buff(Icon::Ability(Slot::W), true, false)),
    ("seismic_haste", buff(Icon::Ability(Slot::Q), true, false)),
    ("ground_slam", buff(Icon::Ability(Slot::E), false, false)),
    ("pick_a_card", buff(Icon::Ability(Slot::W), true, false)),
    ("stacked_deck", buff(Icon::Ability(Slot::E), true, true)),
    ("destiny", buff(Icon::Ability(Slot::R), true, false)),
    ("ghost", buff(Icon::Summoner(Summoner::Ghost), true, false)),
    (
        "summoner_heal",
        buff(Icon::Summoner(Summoner::Heal), true, false),
    ),
    (
        "exhaust",
        buff(Icon::Summoner(Summoner::Exhaust), false, false),
    ),
    (
        "ignite",
        buff(Icon::Summoner(Summoner::Ignite), false, false),
    ),
    ("mark", buff(Icon::Summoner(Summoner::Mark), false, false)),
    ("potion", buff(Icon::Item(2003), true, true)),
    ("stun", buff(Icon::Origin, false, false)),
    ("slow", buff(Icon::Origin, false, false)),
    ("knockup", buff(Icon::Origin, false, false)),
    ("silence", buff(Icon::Origin, false, false)),
    ("root", buff(Icon::Origin, false, false)),
];

pub fn shown(name: u32) -> Option<Shown> {
    BUFFS
        .iter()
        .find(|(candidate, _)| clip_id(candidate) == name)
        .map(|(_, shown)| *shown)
}

pub fn icon(buff: &BuffState, shown: Shown) -> Option<Icon> {
    match shown.icon {
        Icon::Origin => match buff.slot? {
            Slot::D | Slot::F => None,
            slot => Some(Icon::Ability(slot)),
        },
        icon => Some(icon),
    }
}

pub fn elapsed(buff: &BuffState) -> f32 {
    if buff.duration <= 0.0 || buff.duration > 1.0e6 {
        0.0
    } else {
        (1.0 - buff.remaining / buff.duration).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(name: &str, slot: Option<Slot>) -> BuffState {
        BuffState {
            name: clip_id(name),
            source: Some(7),
            slot,
            remaining: 1.0,
            duration: 4.0,
            stacks: 1,
        }
    }

    #[test]
    fn crowd_control_borrows_the_icon_of_the_ability_that_caused_it() {
        let stun = state("stun", Some(Slot::E));
        let shown = shown(stun.name).expect("stuns are shown");
        assert!(!shown.positive);
        assert_eq!(icon(&stun, shown), Some(Icon::Ability(Slot::E)));
        assert_eq!(icon(&state("slow", None), shown), None);
        assert!(super::shown(clip_id("judgment_tick")).is_none());
        assert!((elapsed(&stun) - 0.75).abs() < 1e-6);
    }
}
