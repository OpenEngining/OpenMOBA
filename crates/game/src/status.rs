use asset::clip_id;
use protocol::{UnitId, UnitState};

const STATUSES: [(&str, &str, &str); 2] = [
    ("stun", "Stunned", "StunEnded"),
    ("silence", "Silenced", "SilenceEnded"),
];

pub fn is_status(name: u32) -> bool {
    STATUSES
        .iter()
        .any(|(_, started, ended)| name == clip_id(started) || name == clip_id(ended))
}

pub fn changes(before: &[UnitState], after: &[UnitState]) -> Vec<(UnitId, &'static str)> {
    let has = |units: &[UnitState], id: UnitId, buff: u32| {
        units
            .iter()
            .find(|unit| unit.id == id)
            .is_some_and(|unit| !unit.dead && unit.buffs.contains(&buff))
    };
    let ids = before.iter().chain(after).map(|unit| unit.id);
    let mut seen = Vec::new();
    let mut changes = Vec::new();
    for id in ids {
        if seen.contains(&id) {
            continue;
        }
        seen.push(id);
        for (buff, started, ended) in STATUSES {
            let buff = clip_id(buff);
            match (has(before, id, buff), has(after, id, buff)) {
                (false, true) => changes.push((id, started)),
                (true, false) => changes.push((id, ended)),
                _ => {}
            }
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use protocol::{Team, UnitKind};

    use super::*;

    fn unit(id: UnitId, buffs: &[&str]) -> UnitState {
        UnitState {
            id,
            kind: UnitKind::Turret,
            team: Team::Blue,
            position: [0.0, 0.0],
            facing: 0.0,
            health: 100.0,
            max_health: 100.0,
            shield: 0.0,
            mana: 0.0,
            max_mana: 0.0,
            level: 1,
            move_speed: 0.0,
            moving: false,
            dashing: false,
            dead: false,
            targetable: true,
            size: 1.0,
            respawn: None,
            ultimate: None,
            animation: None,
            stats: None,
            buffs: buffs.iter().map(|name| clip_id(name)).collect(),
        }
    }

    #[test]
    fn stuns_start_and_end_with_the_buff() {
        let calm = [unit(1, &[]), unit(2, &["silence"])];
        let stunned = [unit(1, &["stun"]), unit(2, &["silence"])];
        assert_eq!(changes(&calm, &stunned), [(1, "Stunned")]);
        assert_eq!(changes(&stunned, &calm), [(1, "StunEnded")]);
        assert_eq!(
            changes(&stunned, &[]),
            [(1, "StunEnded"), (2, "SilenceEnded")]
        );
    }

    #[test]
    fn only_stun_and_silence_icons_count_as_statuses() {
        assert!(is_status(clip_id("StunEnded")));
        assert!(is_status(clip_id("Silenced")));
        assert!(!is_status(clip_id("Flash")));
    }
}
