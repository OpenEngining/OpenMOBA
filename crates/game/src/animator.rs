use asset::{Character, ClipKind, Transform, Updater, clip_id};
use glam::{Mat4, Quat, Vec3};

pub const DEFAULT_BLEND: f32 = 0.1;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Params {
    pub move_speed: f32,
    pub moving: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Local {
    rotation: Quat,
    translation: Vec3,
    scale: Vec3,
}

impl Local {
    fn from_transform(transform: &Transform) -> Self {
        Self {
            rotation: Quat::from_array(transform.rotation),
            translation: Vec3::from_array(transform.translation),
            scale: Vec3::from_array(transform.scale),
        }
    }

    fn blend(self, other: Self, t: f32) -> Self {
        Self {
            rotation: self.rotation.slerp(other.rotation, t),
            translation: self.translation.lerp(other.translation, t),
            scale: self.scale.lerp(other.scale, t),
        }
    }

    fn matrix(self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Node {
    Atomic {
        clip: u32,
        animation: usize,
        time: f32,
        rate: f32,
        loops: bool,
        track: u16,
        mask: Option<u16>,
    },
    Selector {
        clip: u32,
        loops: bool,
        child: Box<Node>,
    },
    Sequencer {
        clip: u32,
        loops: bool,
        index: usize,
        child: Box<Node>,
    },
    Parallel {
        children: Vec<Node>,
    },
    Condition {
        clip: u32,
        choice: u32,
        child: Box<Node>,
    },
    Empty,
}

struct Layer {
    animation: usize,
    time: f32,
    track: u16,
    mask: Option<u16>,
}

struct Context<'a> {
    character: &'a Character,
    params: Params,
    random: &'a mut Random,
}

impl Node {
    fn new(context: &mut Context, id: u32) -> Self {
        let Some(clip) = context.character.graph.clip(id) else {
            return Node::Empty;
        };
        let loops = clip.loops();
        match &clip.kind {
            ClipKind::Atomic {
                animation,
                track,
                mask,
                tick,
            } => {
                let fps = context.character.animations[*animation as usize].fps;
                Node::Atomic {
                    clip: id,
                    animation: *animation as usize,
                    time: 0.0,
                    rate: tick.map_or(1.0, |tick| 1.0 / (tick * fps)),
                    loops,
                    track: *track,
                    mask: *mask,
                }
            }
            ClipKind::Selector { options } => {
                let choice = context.random.weighted(options);
                Node::Selector {
                    clip: id,
                    loops,
                    child: Box::new(Node::new(context, choice)),
                }
            }
            ClipKind::Sequencer { clips } => Node::Sequencer {
                clip: id,
                loops,
                index: 0,
                child: Box::new(
                    clips
                        .first()
                        .map_or(Node::Empty, |first| Node::new(context, *first)),
                ),
            },
            ClipKind::Parallel { clips } => Node::Parallel {
                children: clips
                    .iter()
                    .map(|child| Node::new(context, *child))
                    .collect(),
            },
            ClipKind::ConditionFloat { .. }
            | ClipKind::ConditionBool { .. }
            | ClipKind::Parametric { .. } => {
                let choice = condition(clip.kind.clone(), context.params);
                Node::Condition {
                    clip: id,
                    choice,
                    child: Box::new(Node::new(context, choice)),
                }
            }
        }
    }

    fn loops(&self, character: &Character) -> bool {
        match self {
            Node::Atomic { loops, .. } | Node::Selector { loops, .. } => *loops,
            Node::Sequencer { loops, child, .. } => *loops || child.loops(character),
            Node::Parallel { children } => {
                children.first().is_some_and(|child| child.loops(character))
            }
            Node::Condition { clip, child, .. } => {
                character.graph.clip(*clip).is_some_and(|clip| clip.loops())
                    || child.loops(character)
            }
            Node::Empty => false,
        }
    }

    fn leaf(&self) -> u32 {
        match self {
            Node::Atomic { clip, .. } => *clip,
            Node::Selector { child, .. }
            | Node::Sequencer { child, .. }
            | Node::Condition { child, .. } => child.leaf(),
            Node::Parallel { children } => children.first().map_or(0, Node::leaf),
            Node::Empty => 0,
        }
    }

    fn update(&mut self, context: &mut Context, dt: f32) -> bool {
        match self {
            Node::Atomic {
                animation,
                time,
                rate,
                loops,
                ..
            } => {
                let animation = &context.character.animations[*animation];
                let duration = animation.frames.len() as f32 / animation.fps;
                *time += dt * *rate;
                if *time < duration {
                    return false;
                }
                *time = if *loops {
                    (*time - duration) % duration.max(f32::EPSILON)
                } else {
                    duration
                };
                true
            }
            Node::Selector { clip, loops, child } => {
                let cycled = child.update(context, dt);
                if cycled
                    && *loops
                    && let Some(ClipKind::Selector { options }) =
                        context.character.graph.clip(*clip).map(|clip| &clip.kind)
                {
                    let choice = context.random.weighted(options);
                    **child = Node::new(context, choice);
                }
                cycled
            }
            Node::Sequencer {
                clip,
                loops,
                index,
                child,
            } => {
                let character = context.character;
                let Some(ClipKind::Sequencer { clips }) =
                    character.graph.clip(*clip).map(|clip| &clip.kind)
                else {
                    return false;
                };
                if !child.update(context, dt) {
                    return false;
                }
                if *index + 1 < clips.len() {
                    *index += 1;
                    **child = Node::new(context, clips[*index]);
                    false
                } else if child.loops(character) {
                    true
                } else {
                    if *loops {
                        *index = 0;
                        **child = Node::new(context, clips[0]);
                    }
                    true
                }
            }
            Node::Parallel { children } => children
                .iter_mut()
                .map(|child| child.update(context, dt))
                .fold(None, |first, cycled| first.or(Some(cycled)))
                .unwrap_or(false),
            Node::Condition {
                clip,
                choice,
                child,
            } => {
                if let Some(kind) = context
                    .character
                    .graph
                    .clip(*clip)
                    .map(|clip| clip.kind.clone())
                {
                    let change_mid_play = matches!(
                        kind,
                        ClipKind::ConditionFloat {
                            change_mid_play: true,
                            ..
                        } | ClipKind::ConditionBool {
                            change_mid_play: true,
                            ..
                        }
                    );
                    let next = condition(kind, context.params);
                    if change_mid_play && next != *choice {
                        *choice = next;
                        **child = Node::new(context, next);
                    }
                }
                let cycled = child.update(context, dt);
                let looping = context
                    .character
                    .graph
                    .clip(*clip)
                    .is_some_and(|clip| clip.loops());
                if cycled && looping && !child.loops(context.character) {
                    let next = context
                        .character
                        .graph
                        .clip(*clip)
                        .map_or(*choice, |clip| condition(clip.kind.clone(), context.params));
                    *choice = next;
                    **child = Node::new(context, next);
                }
                cycled
            }
            Node::Empty => false,
        }
    }

    fn layers(&self, out: &mut Vec<Layer>) {
        match self {
            Node::Atomic {
                animation,
                time,
                track,
                mask,
                ..
            } => out.push(Layer {
                animation: *animation,
                time: *time,
                track: *track,
                mask: *mask,
            }),
            Node::Selector { child, .. }
            | Node::Sequencer { child, .. }
            | Node::Condition { child, .. } => child.layers(out),
            Node::Parallel { children } => children.iter().for_each(|child| child.layers(out)),
            Node::Empty => {}
        }
    }
}

fn condition(kind: ClipKind, params: Params) -> u32 {
    let value = |updater: Updater| match updater {
        Updater::MoveSpeed => params.move_speed,
        Updater::IsMoving => f32::from(u8::from(params.moving)),
        Updater::TurnAngle | Updater::SpellTargetAngle | Updater::Unknown => 0.0,
    };
    match kind {
        ClipKind::ConditionFloat { updater, pairs, .. } => {
            let value = value(updater);
            pairs
                .iter()
                .find(|(_, threshold)| value <= *threshold)
                .or(pairs.last())
                .map_or(0, |(clip, _)| *clip)
        }
        ClipKind::ConditionBool {
            updater,
            when_true,
            when_false,
            ..
        } => {
            if value(updater) > 0.0 {
                when_true
            } else {
                when_false
            }
        }
        ClipKind::Parametric { updater, pairs } => {
            let value = value(updater);
            pairs
                .iter()
                .min_by(|a, b| (a.1 - value).abs().total_cmp(&(b.1 - value).abs()))
                .map_or(0, |(clip, _)| *clip)
        }
        _ => 0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Random(u32);

impl Random {
    pub fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }

    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / (1 << 24) as f32
    }

    fn weighted(&mut self, options: &[(u32, f32)]) -> u32 {
        let total: f32 = options.iter().map(|(_, weight)| weight).sum();
        let mut roll = self.next() * total;
        for (clip, weight) in options {
            if roll < *weight {
                return *clip;
            }
            roll -= weight;
        }
        options.last().map_or(0, |(clip, _)| *clip)
    }
}

pub struct Animator {
    request: u32,
    node: Node,
    random: Random,
    pose: Vec<Local>,
    fade: Option<(Vec<Local>, f32, f32)>,
    speed: f32,
    finished: bool,
}

impl Animator {
    pub fn new(character: &Character, name: &str, seed: u32) -> Self {
        let mut random = Random::new(seed);
        let request = clip_id(name);
        let node = Node::new(
            &mut Context {
                character,
                params: Params::default(),
                random: &mut random,
            },
            request,
        );
        let mut animator = Self {
            request,
            node,
            random,
            pose: Vec::new(),
            fade: None,
            speed: 1.0,
            finished: false,
        };
        animator.pose = animator.sample(character);
        animator
    }

    pub fn finished(&self) -> bool {
        self.finished
    }

    pub fn play_id(
        &mut self,
        character: &Character,
        request: u32,
        params: Params,
        speed: f32,
        restart: bool,
    ) -> bool {
        if (request == self.request && !restart) || character.graph.clip(request).is_none() {
            return false;
        }
        self.speed = speed.max(0.05);
        self.finished = false;
        let from_leaf = self.node.leaf();
        let from_request = self.request;
        self.request = request;
        self.node = Node::new(
            &mut Context {
                character,
                params,
                random: &mut self.random,
            },
            request,
        );
        let blend = character
            .graph
            .blend(from_leaf, self.node.leaf())
            .or_else(|| character.graph.blend(from_request, request))
            .unwrap_or(DEFAULT_BLEND);
        self.fade_from_current(blend);
        true
    }

    pub fn update(&mut self, character: &Character, params: Params, dt: f32) {
        let leaf = self.node.leaf();
        let dt = dt * self.speed;
        let cycled = self.node.update(
            &mut Context {
                character,
                params,
                random: &mut self.random,
            },
            dt,
        );
        if cycled && !self.node.loops(character) {
            self.finished = true;
        }
        let next_leaf = self.node.leaf();
        if next_leaf != leaf {
            let blend = character
                .graph
                .blend(leaf, next_leaf)
                .unwrap_or(DEFAULT_BLEND);
            self.fade_from_current(blend);
        }
        self.fade = self.fade.take().and_then(|(from, elapsed, duration)| {
            (elapsed + dt < duration).then_some((from, elapsed + dt, duration))
        });
        self.pose = self.sample(character);
    }

    pub fn skin(&self, character: &Character) -> Vec<Mat4> {
        let mut globals: Vec<Mat4> = Vec::with_capacity(self.pose.len());
        for (joint, local) in character.joints.iter().zip(&self.pose) {
            let parent = joint
                .parent
                .and_then(|parent| globals.get(usize::from(parent)).copied())
                .unwrap_or(Mat4::IDENTITY);
            globals.push(parent * local.matrix());
        }
        globals
            .iter()
            .zip(&character.joints)
            .map(|(global, joint)| *global * Mat4::from_cols_array(&joint.inverse_bind))
            .collect()
    }

    fn fade_from_current(&mut self, duration: f32) {
        self.fade = (duration > 0.0).then(|| (self.pose.clone(), 0.0, duration));
    }

    fn sample(&self, character: &Character) -> Vec<Local> {
        let mut pose: Vec<Local> = character
            .joints
            .iter()
            .map(|joint| Local::from_transform(&joint.bind))
            .collect();
        let mut layers = Vec::new();
        self.node.layers(&mut layers);
        let priority = |layer: &Layer| {
            character
                .graph
                .tracks
                .get(usize::from(layer.track))
                .map_or(0, |track| track.priority)
        };
        layers.retain(|layer| {
            !character
                .graph
                .tracks
                .get(usize::from(layer.track))
                .is_some_and(|track| track.additive)
        });
        layers.sort_by_key(|layer| std::cmp::Reverse(priority(layer)));
        for layer in &layers {
            let frames = sample_animation(&character.animations[layer.animation], layer.time);
            let mask = layer
                .mask
                .and_then(|mask| character.graph.masks.get(usize::from(mask)));
            for (index, (joint, frame)) in pose.iter_mut().zip(frames).enumerate() {
                let weight =
                    mask.map_or(1.0, |mask| mask.weights.get(index).copied().unwrap_or(0.0));
                if weight > 0.0 {
                    *joint = joint.blend(frame, weight);
                }
            }
        }
        match &self.fade {
            Some((from, elapsed, duration)) if from.len() == pose.len() => {
                let t = elapsed / duration;
                from.iter()
                    .zip(pose)
                    .map(|(from, to)| from.blend(to, t))
                    .collect()
            }
            _ => pose,
        }
    }
}

fn sample_animation(animation: &asset::Animation, time: f32) -> impl Iterator<Item = Local> + '_ {
    let count = animation.frames.len();
    let position = (time * animation.fps).clamp(0.0, (count - 1) as f32);
    let index = position as usize;
    let next = (index + 1).min(count - 1);
    let t = position - index as f32;
    animation.frames[index]
        .iter()
        .zip(&animation.frames[next])
        .map(move |(from, to)| Local::from_transform(from).blend(Local::from_transform(to), t))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use asset::{Animation, Blend, Clip, Graph, Joint, Track};

    fn transform(x: f32) -> Transform {
        Transform {
            rotation: Quat::IDENTITY.to_array(),
            translation: [x, 0.0, 0.0],
            scale: [1.0; 3],
        }
    }

    fn atomic(name: &str, animation: u32, flags: u32) -> Clip {
        Clip {
            id: clip_id(name),
            name: Some(name.into()),
            flags,
            kind: ClipKind::Atomic {
                animation,
                track: 0,
                mask: None,
                tick: None,
            },
        }
    }

    pub fn character() -> Character {
        let animation = |x: f32| Animation {
            fps: 10.0,
            frames: vec![vec![transform(x)]; 10],
        };
        let mut clips = vec![
            atomic("Idle_In", 0, 0),
            atomic("Idle_Base", 1, 0),
            atomic("Idle_A", 2, 0),
            atomic("Idle_B", 3, 0),
            atomic("Walk", 4, asset::LOOP),
            atomic("Run_Clip", 5, asset::LOOP),
            Clip {
                id: clip_id("Idle_Selector"),
                name: None,
                flags: 6,
                kind: ClipKind::Selector {
                    options: vec![(clip_id("Idle_A"), 1.0), (clip_id("Idle_B"), 1.0)],
                },
            },
            Clip {
                id: clip_id("Idle1"),
                name: None,
                flags: asset::LOOP,
                kind: ClipKind::Sequencer {
                    clips: vec![
                        clip_id("Idle_In"),
                        clip_id("Idle_Base"),
                        clip_id("Idle_Selector"),
                    ],
                },
            },
            Clip {
                id: clip_id("Run"),
                name: None,
                flags: 0,
                kind: ClipKind::ConditionFloat {
                    updater: Updater::MoveSpeed,
                    pairs: vec![(clip_id("Walk"), 300.0), (clip_id("Run_Clip"), 350.0)],
                    change_mid_play: true,
                },
            },
        ];
        clips.sort_by_key(|clip| clip.id);
        let mut blends = vec![Blend {
            from: clip_id("Idle_In"),
            to: clip_id("Run_Clip"),
            time: 0.0,
        }];
        blends.sort_by_key(|blend| (blend.from, blend.to));
        Character {
            name: "Test".into(),
            stats: asset::Stats::default(),
            scale: 1.0,
            self_illumination: 0.0,
            vertices: Vec::new(),
            indices: Vec::new(),
            submeshes: Vec::new(),
            textures: Vec::new(),
            joints: vec![Joint {
                name: "root".into(),
                parent: None,
                bind: transform(-1.0),
                inverse_bind: Mat4::IDENTITY.to_cols_array(),
            }],
            animations: (0..6).map(|index| animation(index as f32)).collect(),
            graph: Graph {
                clips,
                tracks: vec![Track {
                    name: "Default".into(),
                    priority: 0,
                    additive: false,
                }],
                masks: Vec::new(),
                blends,
            },
        }
    }

    fn root_x(animator: &Animator, character: &Character) -> f32 {
        animator.skin(character)[0].transform_point3(Vec3::ZERO).x
    }

    #[test]
    fn idle_plays_its_lead_in_then_base_then_loops_variations() {
        let character = character();
        let mut animator = Animator::new(&character, "Idle1", 7);
        let params = Params::default();
        assert_eq!(root_x(&animator, &character), 0.0);
        animator.update(&character, params, 1.0);
        animator.update(&character, params, DEFAULT_BLEND);
        assert_eq!(root_x(&animator, &character), 1.0);
        let mut seen = Vec::new();
        for _ in 0..40 {
            animator.update(&character, params, 1.0);
            animator.update(&character, params, DEFAULT_BLEND);
            seen.push(root_x(&animator, &character));
        }
        assert!(seen.iter().all(|x| *x == 2.0 || *x == 3.0));
        assert!(seen.contains(&2.0) && seen.contains(&3.0));
    }

    #[test]
    fn run_picks_walk_when_slow_and_switches_mid_play() {
        let character = character();
        let mut animator = Animator::new(&character, "Idle1", 7);
        let slow = Params {
            move_speed: 250.0,
            moving: true,
        };
        animator.play_id(&character, clip_id("Run"), slow, 1.0, false);
        animator.update(&character, slow, 0.5);
        assert_eq!(root_x(&animator, &character), 4.0);
        let fast = Params {
            move_speed: 330.0,
            moving: true,
        };
        animator.update(&character, fast, 0.5);
        assert_eq!(root_x(&animator, &character), 5.0);
    }

    #[test]
    fn blend_table_entries_override_the_default() {
        let character = character();
        let mut animator = Animator::new(&character, "Idle1", 7);
        let params = Params {
            move_speed: 330.0,
            moving: true,
        };
        animator.play_id(&character, clip_id("Run"), params, 1.0, false);
        animator.update(&character, params, 0.01);
        assert_eq!(root_x(&animator, &character), 5.0);

        let mut animator = Animator::new(&character, "Idle1", 7);
        animator.update(&character, params, 1.5);
        animator.play_id(&character, clip_id("Run"), params, 1.0, false);
        animator.update(&character, params, DEFAULT_BLEND / 2.0);
        let x = root_x(&animator, &character);
        assert!(x > 1.0 && x < 5.0);
    }

    #[test]
    fn looping_conditions_restart_a_choice_that_plays_once() {
        let mut character = character();
        for clip in &mut character.graph.clips {
            if clip.id == clip_id("Run") {
                clip.flags = asset::LOOP;
            }
            if clip.id == clip_id("Run_Clip") {
                clip.flags = 0;
            }
        }
        let params = Params {
            move_speed: 330.0,
            moving: true,
        };
        let mut animator = Animator::new(&character, "Run", 7);
        animator.update(&character, params, 1.5);
        let mut layers = Vec::new();
        animator.node.layers(&mut layers);
        assert!(layers[0].time < 0.9, "{}", layers[0].time);
        assert!(!animator.finished());
    }
}
