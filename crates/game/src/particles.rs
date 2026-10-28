use std::collections::HashMap;
use std::rc::Rc;

use asset::clip_id;
use asset::vfx::{Attach, Blend, Effect, Emitter, Lookup, Primitive, Spawn, VfxIndex};
use glam::{EulerRot, Mat4, Quat, Vec2, Vec3, Vec4};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, future_to_promise};
use web_sys::{
    HtmlImageElement, WebGl2RenderingContext as Gl, WebGlBuffer, WebGlProgram, WebGlShader,
    WebGlTexture, WebGlUniformLocation, WebGlVertexArrayObject,
};

use crate::download::{self, fetch};

const BASE_URL: &str = "assets/vfx";
const INSTANCE_FLOATS: usize = 28;
const INSTANCE_STRIDE: i32 = (INSTANCE_FLOATS * 4) as i32;
const MAX_PARTICLES: usize = 400;
const MAX_BURST: usize = 200;
const MAX_AGE: f32 = 20.0;
const LANDING_MEMORY: f32 = 2.0;
const IDLE: u32 = u32::MAX;
const CLIP: u32 = 0;
const MAX_DEPTH: u8 = 4;
const ROTATION_RATE: f32 = 60.0;

const VERTEX: &str = r#"#version 300 es
layout(location = 0) in vec3 position;
layout(location = 1) in vec2 uv;
layout(location = 2) in vec3 center;
layout(location = 3) in vec3 axis_x;
layout(location = 4) in vec3 axis_y;
layout(location = 5) in vec3 axis_z;
layout(location = 6) in vec4 tint;
layout(location = 7) in vec4 frame;
layout(location = 8) in vec4 extra;
layout(location = 9) in vec4 mult_frame;
uniform mat4 view_projection;
out vec2 v_uv;
out vec2 v_mult_uv;
out vec2 v_alpha_uv;
out vec2 v_lookup;
out vec4 v_tint;
void main() {
    vec3 world = center + position.x * axis_x + position.y * axis_y + position.z * axis_z;
    gl_Position = view_projection * vec4(world, 1.0);
    vec2 base = uv * frame.xy + frame.zw;
    v_alpha_uv = base;
    v_uv = base + extra.xy;
    v_lookup = extra.zw;
    v_mult_uv = uv * mult_frame.xy + mult_frame.zw;
    v_tint = tint;
}
"#;

const FRAGMENT: &str = r#"#version 300 es
precision mediump float;
in vec2 v_uv;
in vec2 v_alpha_uv;
in vec2 v_lookup;
in vec2 v_mult_uv;
in vec4 v_tint;
uniform sampler2D base;
uniform sampler2D ramp;
uniform sampler2D mult;
uniform bool has_base;
uniform bool has_ramp;
uniform bool has_mult;
uniform bool lock_alpha;
uniform bool premultiply;
uniform float alpha_ref;
out vec4 color;
void main() {
    vec4 texel = has_base ? texture(base, v_uv) : vec4(1.0);
    if (has_base && lock_alpha) texel.a = texture(base, v_alpha_uv).a;
    vec4 lookup = has_ramp ? texture(ramp, v_lookup) : vec4(1.0);
    vec4 mask = has_mult ? texture(mult, v_mult_uv) : vec4(1.0);
    vec4 result = texel * lookup * mask * v_tint;
    if (result.a < alpha_ref) discard;
    color = premultiply ? vec4(result.rgb * result.a, result.a) : result;
}
"#;

#[derive(Debug, Clone, Copy)]
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }
}

struct Loaded {
    effect: Effect,
    textures: Vec<Option<usize>>,
    ramps: Vec<Option<usize>>,
    meshes: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Anchor {
    Fixed,
    Unit(u32),
    Bone(u32, u32),
    Missile(u32),
    Particle(Vec3),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum End {
    Fixed(Vec3),
    Bone(u32, u32),
}

impl Anchor {
    fn unit(self) -> Option<u32> {
        match self {
            Self::Unit(unit) | Self::Bone(unit, _) => Some(unit),
            _ => None,
        }
    }
}

struct Particle {
    age: f32,
    life: f32,
    position: Vec3,
    velocity: Vec3,
    acceleration: Vec3,
    drag: Vec3,
    orientation: Quat,
    scale: [f32; 4],
    color: [f32; 4],
    rotation: Vec3,
    spin: Vec3,
    uv_offset: Vec2,
    uv_scroll: Vec2,
    random: f32,
    trail: f32,
    child: Option<u32>,
}

#[derive(Default)]
struct EmitterState {
    accumulator: f32,
    burst: bool,
    travelled: f32,
    particles: Vec<Particle>,
}

struct Instance {
    id: u32,
    tag: u32,
    effect: Rc<Loaded>,
    system: u16,
    anchor: Anchor,
    position: Vec3,
    previous: Vec3,
    end: Option<End>,
    end_position: Option<Vec3>,
    facing: f32,
    orientation: Option<Quat>,
    turn: Quat,
    age: f32,
    duration: Option<f32>,
    stopped: bool,
    killed: bool,
    depth: u8,
    emitters: Vec<EmitterState>,
}

impl Instance {
    fn kill(&mut self) {
        if !self.killed {
            self.stopped = true;
            self.killed = true;
        }
    }
}

struct Pending {
    at: f32,
    tag: u32,
    spawn: Spawn,
    anchor: Anchor,
    position: Vec3,
    end: Option<End>,
    facing: f32,
}

struct Track {
    kind: u32,
    position: Vec3,
    facing: f32,
    seen: bool,
}

struct Simulation {
    index: VfxIndex,
    effects: HashMap<String, Rc<Loaded>>,
    instances: Vec<Instance>,
    pending: Vec<Pending>,
    tracks: HashMap<u32, Track>,
    landed: HashMap<u32, f32>,
    rng: Rng,
    time: f32,
    next_id: u32,
}

impl Simulation {
    fn new(index: VfxIndex, effects: HashMap<String, Rc<Loaded>>) -> Self {
        Self {
            index,
            effects,
            instances: Vec::new(),
            pending: Vec::new(),
            tracks: HashMap::new(),
            landed: HashMap::new(),
            rng: Rng(0x2545_f491),
            time: 0.0,
            next_id: 1,
        }
    }

    fn clear(&mut self) {
        self.instances.clear();
        self.pending.clear();
        self.tracks.clear();
        self.landed.clear();
    }

    fn land(&mut self, id: u32) {
        self.landed.insert(id, self.time);
    }

    fn effect(&mut self, name: u32, position: Vec3, facing: f32, unit: Option<u32>, hostile: bool) {
        let Some(entry) = self.index.effect(name).cloned() else {
            return;
        };
        self.cancel(&entry.cancels, unit);
        let spawns = if hostile && !entry.enemy.is_empty() {
            entry.enemy
        } else {
            entry.spawns
        };
        for spawn in spawns {
            let anchor = match (spawn.attach, unit, spawn.bone.as_deref()) {
                (Attach::Unit | Attach::Beam, Some(unit), Some(bone)) => {
                    Anchor::Bone(unit, clip_id(bone))
                }
                (Attach::Unit | Attach::Beam, Some(unit), None) => Anchor::Unit(unit),
                _ => Anchor::Fixed,
            };
            self.schedule(name, spawn, anchor, position, facing);
        }
    }

    fn idle(&mut self, character: &str, unit: u32, position: Vec3, facing: f32) {
        for spawn in self.index.idle(character).to_vec() {
            let anchor = spawn
                .bone
                .as_deref()
                .map_or(Anchor::Unit(unit), |bone| Anchor::Bone(unit, clip_id(bone)));
            self.schedule(IDLE, spawn, anchor, position, facing);
        }
    }

    fn end_unit(&mut self, unit: u32) {
        let owned = |tag: u32, anchor: Anchor| tag != CLIP && anchor.unit() == Some(unit);
        self.pending
            .retain(|pending| !owned(pending.tag, pending.anchor));
        for instance in &mut self.instances {
            if owned(instance.tag, instance.anchor) {
                instance.kill();
            }
        }
    }

    fn clip(&mut self, character: &str, clip: u32, unit: u32, position: Vec3, facing: f32) {
        for spawn in self.index.clip(character, clip).to_vec() {
            let anchor = spawn
                .bone
                .as_deref()
                .map_or(Anchor::Unit(unit), |bone| Anchor::Bone(unit, clip_id(bone)));
            self.schedule(CLIP, spawn, anchor, position, facing);
        }
    }

    fn missile(&mut self, id: u32, kind: u32, position: Vec3, facing: f32) {
        if let Some(track) = self.tracks.get_mut(&id) {
            track.position = position;
            track.facing = facing;
            track.seen = true;
            return;
        }
        self.tracks.insert(
            id,
            Track {
                kind,
                position,
                facing,
                seen: true,
            },
        );
        let Some(entry) = self.index.missile(kind).cloned() else {
            return;
        };
        self.cancel(&entry.cancels, None);
        for spawn in entry
            .spawns
            .into_iter()
            .filter(|spawn| spawn.attach == Attach::Missile)
        {
            self.schedule(kind, spawn, Anchor::Missile(id), position, facing);
        }
    }

    fn cancel(&mut self, tags: &[u32], unit: Option<u32>) {
        if tags.is_empty() {
            return;
        }
        let matches = |tag: u32, anchor: Anchor| {
            tags.contains(&tag) && (unit.is_none() || anchor.unit() == unit)
        };
        self.pending
            .retain(|pending| !matches(pending.tag, pending.anchor));
        for instance in &mut self.instances {
            if matches(instance.tag, instance.anchor) {
                instance.kill();
            }
        }
    }

    fn schedule(&mut self, tag: u32, spawn: Spawn, anchor: Anchor, position: Vec3, facing: f32) {
        let end = match (spawn.attach, anchor.unit(), spawn.target.as_deref()) {
            (Attach::Beam, _, _) => Some(End::Fixed(position)),
            (_, Some(unit), Some(target)) => Some(End::Bone(unit, clip_id(target))),
            _ => None,
        };
        let pending = Pending {
            at: self.time + spawn.delay,
            tag,
            spawn,
            anchor,
            position,
            end,
            facing,
        };
        if pending.spawn.delay <= 0.0 {
            self.start(pending);
        } else {
            self.pending.push(pending);
        }
    }

    fn place(&mut self, name: u32, position: Vec3, facing: f32, hostile: bool) -> Vec<u32> {
        let Some(entry) = self.index.effect(name).cloned() else {
            return Vec::new();
        };
        let spawns = if hostile && !entry.enemy.is_empty() {
            entry.enemy
        } else {
            entry.spawns
        };
        spawns
            .into_iter()
            .filter_map(|spawn| {
                self.start(Pending {
                    at: self.time,
                    tag: name,
                    spawn,
                    anchor: Anchor::Fixed,
                    position,
                    end: None,
                    facing,
                })
            })
            .collect()
    }

    fn stop(&mut self, ids: &[u32]) {
        for instance in &mut self.instances {
            if ids.contains(&instance.id) {
                instance.kill();
            }
        }
    }

    fn start(&mut self, pending: Pending) -> Option<u32> {
        let effect = self.effects.get(&pending.spawn.effect).cloned()?;
        if effect.effect.systems.is_empty() {
            return None;
        }
        let id = self.instance(
            effect,
            0,
            pending.anchor,
            pending.position,
            pending.facing,
            0,
        );
        if let Some(instance) = self.instances.iter_mut().find(|instance| instance.id == id) {
            instance.tag = pending.tag;
            instance.duration = pending.spawn.duration;
            instance.end = pending.end;
        }
        Some(id)
    }

    fn instance(
        &mut self,
        effect: Rc<Loaded>,
        system: u16,
        anchor: Anchor,
        position: Vec3,
        facing: f32,
        depth: u8,
    ) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        let count = effect
            .effect
            .systems
            .get(usize::from(system))
            .map_or(0, |system| system.emitters.len());
        self.instances.push(Instance {
            id,
            tag: 0,
            effect,
            system,
            anchor,
            position,
            previous: position,
            end: None,
            end_position: None,
            facing,
            orientation: None,
            turn: Quat::IDENTITY,
            age: 0.0,
            duration: None,
            stopped: false,
            killed: false,
            depth,
            emitters: (0..count).map(|_| EmitterState::default()).collect(),
        });
        id
    }

    fn update(&mut self, dt: f32, unit_pose: impl Fn(u32, Option<u32>) -> Option<(Vec3, Quat)>) {
        self.time += dt;
        let mut ended: Vec<u32> = self
            .tracks
            .iter()
            .filter(|(_, track)| !track.seen)
            .map(|(id, _)| *id)
            .collect();
        ended.sort_unstable();
        let ended: Vec<(u32, Track)> = ended
            .into_iter()
            .filter_map(|id| self.tracks.remove(&id).map(|track| (id, track)))
            .collect();
        for track in self.tracks.values_mut() {
            track.seen = false;
        }
        for (id, track) in ended {
            for instance in &mut self.instances {
                if instance.anchor == Anchor::Missile(id) {
                    instance.stopped = true;
                }
            }
            self.pending
                .retain(|pending| pending.anchor != Anchor::Missile(id));
            if self.landed.remove(&id).is_none() {
                continue;
            }
            if let Some(entry) = self.index.missile(track.kind).cloned() {
                for spawn in entry
                    .spawns
                    .into_iter()
                    .filter(|spawn| spawn.attach == Attach::Hit)
                {
                    self.schedule(
                        track.kind,
                        spawn,
                        Anchor::Fixed,
                        track.position,
                        track.facing,
                    );
                }
            }
        }
        let time = self.time;
        self.landed.retain(|_, at| time - *at < LANDING_MEMORY);
        let (due, waiting): (Vec<Pending>, Vec<Pending>) = std::mem::take(&mut self.pending)
            .into_iter()
            .partition(|pending| pending.at <= time);
        self.pending = waiting;
        for mut pending in due {
            let anchored = match pending.anchor {
                Anchor::Unit(unit) => Some((unit, None)),
                Anchor::Bone(unit, bone) => Some((unit, Some(bone))),
                _ => None,
            };
            if let Some((unit, bone)) = anchored {
                match unit_pose(unit, bone) {
                    Some((position, _)) => pending.position = position,
                    None => continue,
                }
            }
            self.start(pending);
        }

        let mut children = Vec::new();
        let mut moves = Vec::new();
        for instance in &mut self.instances {
            instance.turn = Quat::IDENTITY;
            let target = match instance.anchor {
                Anchor::Fixed => Some(instance.position),
                Anchor::Particle(position) => Some(position),
                Anchor::Unit(unit) => unit_pose(unit, None).map(|(position, _)| position),
                Anchor::Bone(unit, bone) => {
                    unit_pose(unit, Some(bone)).map(|(position, rotation)| {
                        if let Some(previous) = instance.orientation {
                            instance.turn = rotation * previous.inverse();
                        }
                        instance.orientation = Some(rotation);
                        position
                    })
                }
                Anchor::Missile(id) => self.tracks.get(&id).map(|track| {
                    instance.facing = track.facing;
                    track.position
                }),
            };
            instance.previous = instance.position;
            match target {
                Some(target) => instance.position = target,
                None => instance.stopped = true,
            }
            instance.end_position = match instance.end {
                Some(End::Fixed(position)) => {
                    Some(Vec3::new(position.x, instance.position.y, position.z))
                }
                Some(End::Bone(unit, bone)) => {
                    unit_pose(unit, Some(bone)).map(|(position, _)| position)
                }
                None => None,
            };
            step(instance, dt, &mut self.rng, &mut children, &mut moves);
        }
        for (child, position) in moves {
            if let Some(instance) = self
                .instances
                .iter_mut()
                .find(|instance| instance.id == child)
            {
                match position {
                    Some(position) => instance.anchor = Anchor::Particle(position),
                    None => instance.stopped = true,
                }
            }
        }
        for request in children {
            let id = self.instance(
                request.effect,
                request.system,
                if request.bound {
                    Anchor::Particle(request.position)
                } else {
                    Anchor::Fixed
                },
                request.position,
                request.facing,
                request.depth,
            );
            if let Some((parent, emitter, particle)) = request.parent
                && let Some(parent) = self
                    .instances
                    .iter_mut()
                    .find(|instance| instance.id == parent)
                && let Some(particle) = parent
                    .emitters
                    .get_mut(emitter)
                    .and_then(|state| state.particles.get_mut(particle))
            {
                particle.child = Some(id);
            }
        }
        self.instances.retain(|instance| !finished(instance));
    }
}

struct ChildRequest {
    effect: Rc<Loaded>,
    system: u16,
    position: Vec3,
    facing: f32,
    depth: u8,
    bound: bool,
    parent: Option<(u32, usize, usize)>,
}

fn emitters(instance: &Instance) -> &[Emitter] {
    instance
        .effect
        .effect
        .systems
        .get(usize::from(instance.system))
        .map_or(&[], |system| system.emitters.as_slice())
}

fn finished(instance: &Instance) -> bool {
    let emitting = !instance.stopped
        && emitters(instance)
            .iter()
            .zip(&instance.emitters)
            .any(|(emitter, state)| still_emitting(emitter, state, instance.age));
    !emitting
        && instance
            .emitters
            .iter()
            .all(|state| state.particles.is_empty())
}

fn still_emitting(emitter: &Emitter, state: &EmitterState, age: f32) -> bool {
    if emitter.single {
        return !state.burst;
    }
    emitter.end.is_none_or(|end| age < end)
}

fn step(
    instance: &mut Instance,
    dt: f32,
    rng: &mut Rng,
    children: &mut Vec<ChildRequest>,
    moves: &mut Vec<(u32, Option<Vec3>)>,
) {
    instance.age += dt;
    if instance
        .duration
        .is_some_and(|duration| instance.age >= duration)
    {
        instance.kill();
    } else if instance.duration.is_none()
        && !matches!(instance.anchor, Anchor::Particle(_))
        && instance.age > MAX_AGE
    {
        instance.stopped = true;
    }
    let effect = instance.effect.clone();
    let Some(system) = effect.effect.systems.get(usize::from(instance.system)) else {
        return;
    };
    let delta = instance.position - instance.previous;
    let spawn = instance
        .orientation
        .unwrap_or_else(|| Quat::from_rotation_y(instance.facing));
    let landed = instance.stopped && matches!(instance.anchor, Anchor::Missile(_));
    for (index, (emitter, state)) in system
        .emitters
        .iter()
        .zip(&mut instance.emitters)
        .enumerate()
    {
        state.travelled += delta.length();
        let mut dead = Vec::new();
        for (slot, particle) in state.particles.iter_mut().enumerate() {
            particle.age += dt;
            if instance.killed {
                let left = emitter.linger.unwrap_or(0.0);
                let until = particle.age + left;
                particle.life = if particle.life < 0.0 {
                    until
                } else {
                    particle.life.min(until)
                };
            } else if instance.stopped {
                match emitter.linger {
                    Some(linger) if particle.life < 0.0 => particle.life = linger,
                    Some(linger) => particle.life = particle.life.min(linger),
                    None if particle.life < 0.0 || landed => particle.life = 0.0,
                    None => {}
                }
            }
            if particle.life >= 0.0 && particle.age >= particle.life {
                dead.push(slot);
                continue;
            }
            let t = fraction(particle);
            particle.velocity += particle.acceleration * dt;
            particle.velocity *= Vec3::ONE - (particle.drag * dt).min(Vec3::ONE);
            particle.position += particle.velocity * dt;
            let beam = matches!(emitter.primitive, Primitive::Beam { .. });
            let bind = if beam { 1.0 } else { emitter.bind.sample(t)[0] };
            let turn = if emitter.local && !beam {
                Quat::IDENTITY.slerp(instance.turn, bind.clamp(0.0, 1.0))
            } else {
                Quat::IDENTITY
            };
            particle.position =
                instance.previous + turn * (particle.position - instance.previous) + delta * bind;
            particle.velocity = turn * particle.velocity;
            particle.orientation = turn * particle.orientation;
            if let Some(child) = particle.child {
                moves.push((child, Some(world_position(emitter, particle))));
            }
        }
        for slot in dead.into_iter().rev() {
            let particle = state.particles.remove(slot);
            if let Some(child) = particle.child {
                moves.push((child, None));
            }
            if let Some(child) = &emitter.child
                && child.on_death
                && instance.depth < MAX_DEPTH
            {
                children.push(ChildRequest {
                    effect: effect.clone(),
                    system: pick(child, 0.0, rng),
                    position: world_position(emitter, &particle),
                    facing: child_facing(instance.facing, emitter, &particle),
                    depth: instance.depth + 1,
                    bound: false,
                    parent: None,
                });
            }
        }
        if instance.stopped || instance.age < emitter.delay {
            continue;
        }
        let count = if emitter.single {
            if state.burst {
                0
            } else {
                state.burst = true;
                (emitter
                    .rate
                    .sample_random(phase(emitter, instance.age), || rng.next())[0]
                    as usize)
                    .max(1)
            }
        } else if emitter.end.is_none_or(|end| instance.age < end) {
            let rate = emitter
                .rate
                .sample_random(phase(emitter, instance.age), || rng.next())[0]
                .max(0.0);
            let opening = if rate > 0.0 && !state.burst {
                state.burst = true;
                1.0
            } else {
                0.0
            };
            state.accumulator += dt * rate + opening;
            let count = state.accumulator.floor();
            state.accumulator -= count;
            count as usize
        } else {
            0
        };
        let count = count
            .min(MAX_BURST)
            .min(MAX_PARTICLES.saturating_sub(state.particles.len()));
        for spawned in 0..count {
            let blend = (spawned + 1) as f32 / count as f32;
            let origin = instance.previous.lerp(instance.position, blend);
            let particle = birth(
                emitter,
                phase(emitter, instance.age),
                origin,
                spawn,
                state.travelled,
                rng,
            );
            if let Some(child) = &emitter.child
                && !child.on_death
                && instance.depth < MAX_DEPTH
            {
                children.push(ChildRequest {
                    effect: effect.clone(),
                    system: pick(child, phase(emitter, instance.age), rng),
                    position: world_position(emitter, &particle),
                    facing: child_facing(instance.facing, emitter, &particle),
                    depth: instance.depth + 1,
                    bound: true,
                    parent: Some((instance.id, index, state.particles.len())),
                });
            }
            state.particles.push(particle);
        }
    }
}

fn child_facing(facing: f32, emitter: &Emitter, particle: &Particle) -> f32 {
    facing + rotation(emitter, particle).y.to_radians()
}

fn pick(child: &asset::vfx::Child, phase: f32, rng: &mut Rng) -> u16 {
    let choice = child.choice.sample_random(phase, || rng.next())[0].max(0.0) as usize;
    child.systems[choice.min(child.systems.len() - 1)]
}

fn phase(emitter: &Emitter, age: f32) -> f32 {
    match emitter.end {
        Some(end) if end > emitter.delay => (age - emitter.delay) / (end - emitter.delay),
        _ => 0.0,
    }
}

fn fraction(particle: &Particle) -> f32 {
    if particle.life > 0.0 {
        (particle.age / particle.life).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn vec3(value: [f32; 4]) -> Vec3 {
    Vec3::new(value[0], value[1], value[2])
}

fn birth(
    emitter: &Emitter,
    phase: f32,
    origin: Vec3,
    spawn: Quat,
    travelled: f32,
    rng: &mut Rng,
) -> Particle {
    let mut random = || rng.next();
    let turn = emitter.turns.iter().fold(Quat::IDENTITY, |total, turn| {
        let angle = turn.angle.sample_random(phase, &mut random)[0];
        let axis = Vec3::from(turn.axis);
        if angle.abs() <= 1e-6 || axis.length_squared() < 1e-12 {
            total
        } else {
            Quat::from_axis_angle(axis.normalize(), angle.to_radians()) * total
        }
    });
    let spawn = if emitter.local { spawn } else { Quat::IDENTITY };
    let offset = turn * vec3(emitter.offset.sample_random(phase, &mut random));
    let local = vec3(emitter.position.sample_random(phase, &mut random)) + offset;
    let velocity = spawn * (turn * vec3(emitter.velocity.sample_random(phase, &mut random)));
    let acceleration =
        spawn * (turn * vec3(emitter.acceleration.sample_random(phase, &mut random)));
    let shared = random();
    let tiling = match emitter.primitive {
        Primitive::Trail { tiling } if tiling > 0.0 => travelled / tiling,
        _ => 0.0,
    };
    Particle {
        age: 0.0,
        life: emitter.lifetime.sample_random(phase, &mut random)[0],
        position: origin + spawn * local,
        velocity,
        acceleration,
        drag: vec3(emitter.drag.sample_random(phase, &mut random)).abs(),
        orientation: spawn,
        scale: emitter.birth_scale.sample_random(phase, &mut random),
        color: emitter.birth_color.sample_random(phase, &mut random),
        rotation: vec3(emitter.birth_rotation.sample_random(phase, &mut random)),
        spin: vec3(emitter.spin.sample_random(phase, &mut random)),
        uv_offset: Vec2::from_slice(&emitter.uv_offset.sample_random(phase, &mut random)[..2]),
        uv_scroll: Vec2::from_slice(&emitter.uv_scroll.sample_random(phase, &mut random)[..2]),
        random: shared,
        trail: tiling,
        child: None,
    }
}

fn lifetime(particle: &Particle) -> f32 {
    if particle.life > 0.0 {
        particle.life
    } else {
        1.0
    }
}

fn world_position(emitter: &Emitter, particle: &Particle) -> Vec3 {
    let life = lifetime(particle);
    particle.position + vec3(emitter.gravity.sample(fraction(particle))) * life * life
}

fn scale(emitter: &Emitter, particle: &Particle) -> Vec3 {
    let curve = emitter.scale.sample(fraction(particle));
    let scale = Vec3::new(
        curve[0] * particle.scale[0],
        curve[1] * particle.scale[1],
        curve[2] * particle.scale[2],
    );
    if emitter.uniform {
        Vec3::splat(scale.x)
    } else {
        scale
    }
}

fn color(emitter: &Emitter, particle: &Particle) -> Vec4 {
    let curve = emitter.color.sample(fraction(particle));
    let color = Vec4::from(curve) * Vec4::from(particle.color);
    match emitter.blend {
        Blend::Add | Blend::Subtract => (color.truncate() * color.w).extend(1.0),
        _ => color,
    }
}

fn rotation(emitter: &Emitter, particle: &Particle) -> Vec3 {
    let integrated =
        vec3(emitter.rotation.sample(fraction(particle))) * ROTATION_RATE * lifetime(particle);
    particle.rotation + particle.spin * particle.age + integrated
}

fn orientation(particle: &Particle, rotation: Vec3) -> Quat {
    particle.orientation
        * Quat::from_euler(
            EulerRot::YXZ,
            rotation.y.to_radians(),
            rotation.x.to_radians(),
            rotation.z.to_radians(),
        )
}

fn cell(emitter: &Emitter, particle: &Particle) -> [f32; 4] {
    let grid = Vec2::from(emitter.grid).max(Vec2::splat(0.01));
    let size = Vec2::ONE / grid;
    if grid.x < 1.0 || grid.y < 1.0 {
        return [size.x, size.y, 0.0, 0.0];
    }
    let frames = u32::from(emitter.frames.max(1));
    let random = if emitter.random_frame {
        particle.random * frames as f32
    } else {
        0.0
    };
    let index = if emitter.frame_rate > 0.0 {
        (particle.age * emitter.frame_rate + random) as u32 % frames
    } else if frames > 1 && !emitter.random_frame {
        ((fraction(particle) * frames as f32) as u32).min(frames - 1)
    } else {
        random as u32 % frames
    };
    let frame = u32::from(emitter.start_frame) + index;
    let columns = grid.x as u32;
    let (column, row) = (frame % columns, frame / columns);
    [size.x, size.y, column as f32 * size.x, row as f32 * size.y]
}

fn mult_cell(emitter: &Emitter, particle: &Particle) -> [f32; 4] {
    let grid = Vec2::from(emitter.mult_grid).max(Vec2::ONE).floor();
    let size = Vec2::ONE / grid;
    let frames = (grid.x * grid.y) as u32;
    let frame = if emitter.mult_random_frame {
        (particle.random * frames as f32) as u32 % frames
    } else {
        0
    };
    let columns = grid.x as u32;
    let (column, row) = (frame % columns, frame / columns);
    [size.x, size.y, column as f32 * size.x, row as f32 * size.y]
}

fn lookup(emitter: &Emitter, particle: &Particle) -> Vec2 {
    let axis = |index: usize| {
        let (scale, offset) = (emitter.lookup_scale[index], emitter.lookup_offset[index]);
        match emitter.lookup[index] {
            Lookup::Constant => scale,
            Lookup::Lifetime => scale * fraction(particle) + offset,
            Lookup::Velocity => scale * particle.velocity.length() + offset,
            Lookup::Random => scale * particle.random + offset,
        }
    };
    Vec2::new(axis(0), axis(1))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct BatchKey {
    layer: u8,
    pass: i16,
    rank: u8,
    blend: u8,
    shape: usize,
    base: usize,
    ramp: usize,
    mult: usize,
    depth: bool,
    lock_alpha: bool,
    alpha_ref: u32,
}

const NONE: usize = usize::MAX;
const QUAD: usize = usize::MAX;

struct View {
    right: Vec3,
    up: Vec3,
    forward: Vec3,
}

fn instances(simulation: &Simulation, view: &View) -> Vec<(BatchKey, [f32; INSTANCE_FLOATS])> {
    let mut records = Vec::new();
    for instance in &simulation.instances {
        let loaded = &instance.effect;
        for (emitter, state) in emitters(instance).iter().zip(&instance.emitters) {
            let texture = |slots: &[Option<usize>], index: Option<u16>| {
                index
                    .and_then(|index| slots.get(usize::from(index)).copied().flatten())
                    .unwrap_or(NONE)
            };
            let base = texture(&loaded.textures, emitter.texture);
            let ramp = texture(&loaded.ramps, emitter.color_texture);
            let mult = texture(&loaded.textures, emitter.mult_texture);
            if base == NONE && ramp == NONE {
                continue;
            }
            let shape = match emitter.primitive {
                Primitive::Mesh(mesh) => match loaded.meshes.get(usize::from(mesh)) {
                    Some(mesh) => *mesh,
                    None => continue,
                },
                _ => QUAD,
            };
            let key = BatchKey {
                layer: u8::from(!emitter.ground),
                pass: emitter.pass,
                rank: emitter.blend.rank(),
                blend: emitter.blend as u8,
                shape,
                base,
                ramp,
                mult,
                depth: emitter.depth && !emitter.ground,
                lock_alpha: emitter.lock_alpha,
                alpha_ref: emitter.alpha_ref.to_bits(),
            };
            if let Primitive::Trail { .. } = emitter.primitive {
                for pair in state.particles.windows(2) {
                    if let Some(record) = segment(emitter, &pair[0], &pair[1], view) {
                        records.push((key, record));
                    }
                }
                continue;
            }
            if let Primitive::Beam { .. } = emitter.primitive {
                let Some(end) = instance.end_position else {
                    continue;
                };
                for particle in &state.particles {
                    if let Some(record) = beam(emitter, particle, instance.position, end, view) {
                        records.push((key, record));
                    }
                }
                continue;
            }
            for particle in &state.particles {
                if let Some(record) = instance_record(emitter, particle, view) {
                    records.push((key, record));
                }
            }
        }
    }
    records.sort_by_key(|(key, _)| *key);
    records
}

fn record(
    center: Vec3,
    axes: [Vec3; 3],
    color: Vec4,
    frame: [f32; 4],
    scroll: Vec2,
    lookup: Vec2,
    mult: [f32; 4],
) -> [f32; INSTANCE_FLOATS] {
    let mut data = [0.0; INSTANCE_FLOATS];
    data[0..3].copy_from_slice(&center.to_array());
    data[3..6].copy_from_slice(&axes[0].to_array());
    data[6..9].copy_from_slice(&axes[1].to_array());
    data[9..12].copy_from_slice(&axes[2].to_array());
    data[12..16].copy_from_slice(&color.to_array());
    data[16..20].copy_from_slice(&frame);
    data[20..22].copy_from_slice(&scroll.to_array());
    data[22..24].copy_from_slice(&lookup.to_array());
    data[24..28].copy_from_slice(&mult);
    data
}

fn instance_record(
    emitter: &Emitter,
    particle: &Particle,
    view: &View,
) -> Option<[f32; INSTANCE_FLOATS]> {
    let color = color(emitter, particle);
    if color.w <= 0.0 && !matches!(emitter.blend, Blend::Add | Blend::Subtract) {
        return None;
    }
    let size = scale(emitter, particle);
    if size.abs().max_element() <= 0.0 {
        return None;
    }
    let center = world_position(emitter, particle);
    let rotation = rotation(emitter, particle);
    let frame = cell(emitter, particle);
    let scroll =
        (particle.uv_offset + particle.uv_scroll * particle.age) * Vec2::new(frame[0], frame[1]);
    let lookup = lookup(emitter, particle);
    let speed = particle.velocity.length();
    let stretch = if emitter.stretch[0] != 0.0 {
        (speed * emitter.stretch[0]).max(emitter.stretch[1])
    } else {
        1.0
    };
    let axes = match emitter.primitive {
        Primitive::Billboard => {
            let (right, up) = if emitter.directional && speed > 1e-3 {
                let along = particle.velocity - view.forward * particle.velocity.dot(view.forward);
                let up = along.normalize_or(view.up);
                (up.cross(view.forward).normalize_or(view.right), up)
            } else {
                let (sin, cos) = rotation.x.to_radians().sin_cos();
                (
                    view.right * cos + view.up * sin,
                    view.up * cos - view.right * sin,
                )
            };
            [right * size.x, up * size.y * stretch, Vec3::ZERO]
        }
        Primitive::Quad => {
            let orientation = directed(emitter, particle, orientation(particle, rotation));
            [
                orientation * Vec3::Y * size.y,
                orientation * Vec3::X * size.x * stretch,
                Vec3::ZERO,
            ]
        }
        Primitive::Ray => {
            let axis = orientation(particle, rotation) * Vec3::Z;
            let side = axis.cross(view.forward).normalize_or(view.right);
            let center = center + axis * (size.z + size.y * 0.5);
            return Some(record(
                center,
                [side * size.x * 0.5, axis * size.y * 0.5, Vec3::ZERO],
                color,
                frame,
                scroll,
                lookup,
                mult_cell(emitter, particle),
            ));
        }
        Primitive::Mesh(_) => {
            let orientation = directed(emitter, particle, orientation(particle, rotation));
            [
                orientation * Vec3::X * size.x,
                orientation * Vec3::Y * size.y,
                orientation * Vec3::Z * size.z,
            ]
        }
        Primitive::Trail { .. } | Primitive::Beam { .. } => return None,
    };
    Some(record(
        center,
        axes,
        color,
        frame,
        scroll,
        lookup,
        mult_cell(emitter, particle),
    ))
}

fn beam(
    emitter: &Emitter,
    particle: &Particle,
    origin: Vec3,
    end: Vec3,
    view: &View,
) -> Option<[f32; INSTANCE_FLOATS]> {
    let color = color(emitter, particle);
    if color.w <= 0.0 && !matches!(emitter.blend, Blend::Add | Blend::Subtract) {
        return None;
    }
    let start = world_position(emitter, particle);
    let finish = end + (start - origin);
    let along = (finish - start) * 0.5;
    if along.length_squared() < 1e-6 {
        return None;
    }
    let (tiling, width, transposed) = match emitter.primitive {
        Primitive::Beam {
            tiling,
            width,
            transposed,
        } => (tiling, width, transposed),
        _ => (0.0, 1.0, false),
    };
    let side =
        along.cross(view.forward).normalize_or(view.up) * scale(emitter, particle).x * width * 0.5;
    let frame = cell(emitter, particle);
    let repeats = if tiling > 0.0 {
        (finish - start).length() / tiling
    } else {
        1.0
    };
    let scroll =
        (particle.uv_offset + particle.uv_scroll * particle.age) * Vec2::new(frame[0], frame[1]);
    let (axes, frame) = if transposed {
        (
            [along, side, Vec3::ZERO],
            [frame[0] * repeats, frame[1], frame[2], frame[3]],
        )
    } else {
        (
            [side, along, Vec3::ZERO],
            [frame[0], frame[1] * repeats, frame[2], frame[3]],
        )
    };
    Some(record(
        (start + finish) * 0.5,
        axes,
        color,
        frame,
        scroll,
        lookup(emitter, particle),
        mult_cell(emitter, particle),
    ))
}

fn directed(emitter: &Emitter, particle: &Particle, orientation: Quat) -> Quat {
    if emitter.directional && particle.velocity.length_squared() > 1e-6 {
        Quat::from_rotation_arc(Vec3::Z, particle.velocity.normalize()) * orientation
    } else {
        orientation
    }
}

fn segment(
    emitter: &Emitter,
    newer: &Particle,
    older: &Particle,
    view: &View,
) -> Option<[f32; INSTANCE_FLOATS]> {
    let (head, tail) = (
        world_position(emitter, newer),
        world_position(emitter, older),
    );
    let along = (tail - head) * 0.5;
    if along.length_squared() < 1e-6 {
        return None;
    }
    let width = (scale(emitter, newer).x + scale(emitter, older).x) * 0.5;
    let side = along.cross(view.forward).normalize_or(view.up) * width;
    let color = (color(emitter, newer) + color(emitter, older)) * 0.5;
    let (start, end) = match emitter.primitive {
        Primitive::Trail { tiling } if tiling > 0.0 => (newer.trail, older.trail),
        _ => (fraction(newer), fraction(older)),
    };
    let frame = cell(emitter, newer);
    let lookup = (lookup(emitter, newer) + lookup(emitter, older)) * 0.5;
    let mult = mult_cell(emitter, newer);
    let (near, far) = (fraction(newer), fraction(older));
    Some(record(
        (head + tail) * 0.5,
        [along, side, Vec3::ZERO],
        color,
        [
            (end - start) * frame[0],
            frame[1],
            frame[2] + start * frame[0],
            frame[3],
        ],
        newer.uv_scroll * newer.age * Vec2::new(frame[0], frame[1]),
        lookup,
        [
            (far - near) * mult[0],
            mult[1],
            mult[2] + near * mult[0],
            mult[3],
        ],
    ))
}

struct Shape {
    vertex_array: WebGlVertexArrayObject,
    count: i32,
}

struct Uniforms {
    view_projection: Option<WebGlUniformLocation>,
    has_base: Option<WebGlUniformLocation>,
    has_ramp: Option<WebGlUniformLocation>,
    has_mult: Option<WebGlUniformLocation>,
    lock_alpha: Option<WebGlUniformLocation>,
    premultiply: Option<WebGlUniformLocation>,
    alpha_ref: Option<WebGlUniformLocation>,
}

pub struct Particles {
    gl: Gl,
    program: WebGlProgram,
    uniforms: Uniforms,
    instances: WebGlBuffer,
    quad: Shape,
    meshes: Vec<Shape>,
    textures: Vec<WebGlTexture>,
    simulation: Simulation,
}

impl Particles {
    pub async fn load(gl: &Gl) -> Result<Self, JsValue> {
        let index = VfxIndex::from_bytes(&fetch(&format!("{BASE_URL}/index.bin")).await?)
            .map_err(|error| error.to_string())?;
        let keys = effect_files(&index);
        let files = all(keys.iter().map(|key| {
            let url = format!("{BASE_URL}/{key}.bin");
            future_to_promise(async move {
                let bytes = fetch(&url).await?;
                Ok(js_sys::Uint8Array::from(bytes.as_slice()).into())
            })
        }))
        .await?;
        let effects: Vec<(String, Effect)> = keys
            .into_iter()
            .zip(files)
            .filter_map(|(key, file)| {
                let bytes = file.dyn_into::<js_sys::Uint8Array>().ok()?.to_vec();
                Some((key, Effect::from_bytes(&bytes).ok()?))
            })
            .collect();

        let program = program(gl)?;
        let uniform = |name| gl.get_uniform_location(&program, name);
        let uniforms = Uniforms {
            view_projection: uniform("view_projection"),
            has_base: uniform("has_base"),
            has_ramp: uniform("has_ramp"),
            has_mult: uniform("has_mult"),
            lock_alpha: uniform("lock_alpha"),
            premultiply: uniform("premultiply"),
            alpha_ref: uniform("alpha_ref"),
        };
        gl.use_program(Some(&program));
        gl.uniform1i(uniform("base").as_ref(), 0);
        gl.uniform1i(uniform("ramp").as_ref(), 1);
        gl.uniform1i(uniform("mult").as_ref(), 2);
        let instances = gl.create_buffer().ok_or("instance buffer")?;

        let mut roles: Vec<(String, bool)> = effects
            .iter()
            .flat_map(|(_, effect)| {
                effect
                    .systems
                    .iter()
                    .flat_map(|system| &system.emitters)
                    .flat_map(|emitter| {
                        [
                            emitter.texture.map(|index| (index, false)),
                            emitter.color_texture.map(|index| (index, true)),
                            emitter.mult_texture.map(|index| (index, false)),
                        ]
                        .into_iter()
                        .flatten()
                        .filter_map(|(index, clamp)| {
                            Some((effect.textures.get(usize::from(index))?.clone(), clamp))
                        })
                    })
            })
            .collect();
        roles.sort();
        roles.dedup();
        let images = all(roles
            .iter()
            .map(|(name, _)| image(&format!("{BASE_URL}/{name}"))))
        .await?;
        let mut textures = Vec::new();
        let mut texture_ids: HashMap<(String, bool), usize> = HashMap::new();
        for ((name, clamp), image) in roles.into_iter().zip(images) {
            if let Ok(image) = image.dyn_into::<HtmlImageElement>() {
                texture_ids.insert((name, clamp), textures.len());
                textures.push(texture(gl, &image, clamp)?);
            }
        }

        let quad = shape(
            gl,
            &instances,
            &[
                -1.0, -1.0, 0.0, 0.0, 1.0, 1.0, -1.0, 0.0, 1.0, 1.0, -1.0, 1.0, 0.0, 0.0, 0.0, 1.0,
                1.0, 0.0, 1.0, 0.0,
            ],
            &[0, 1, 2, 2, 1, 3],
        )?;
        let mut meshes = Vec::new();
        let mut loaded = HashMap::new();
        for (key, effect) in effects {
            let mut mesh_ids = Vec::new();
            for mesh in &effect.meshes {
                let vertices: Vec<f32> = mesh
                    .positions
                    .iter()
                    .zip(&mesh.uvs)
                    .flat_map(|(position, uv)| position.iter().chain(uv).copied())
                    .collect();
                mesh_ids.push(meshes.len());
                meshes.push(shape(gl, &instances, &vertices, &mesh.indices)?);
            }
            let slots = |clamp: bool| -> Vec<Option<usize>> {
                effect
                    .textures
                    .iter()
                    .map(|name| texture_ids.get(&(name.clone(), clamp)).copied())
                    .collect()
            };
            let (textures, ramps) = (slots(false), slots(true));
            loaded.insert(
                key,
                Rc::new(Loaded {
                    effect,
                    textures,
                    ramps,
                    meshes: mesh_ids,
                }),
            );
        }
        Ok(Self {
            gl: gl.clone(),
            program,
            uniforms,
            instances,
            quad,
            meshes,
            textures,
            simulation: Simulation::new(index, loaded),
        })
    }

    pub fn effect(
        &mut self,
        name: u32,
        position: Vec3,
        facing: f32,
        unit: Option<u32>,
        hostile: bool,
    ) {
        self.simulation
            .effect(name, position, facing, unit, hostile);
    }

    pub fn idle(&mut self, champion: &str, unit: u32, position: Vec3, facing: f32) {
        self.simulation.idle(champion, unit, position, facing);
    }

    pub fn end_unit(&mut self, unit: u32) {
        self.simulation.end_unit(unit);
    }

    pub fn place(&mut self, name: u32, position: Vec3, facing: f32, hostile: bool) -> Vec<u32> {
        self.simulation.place(name, position, facing, hostile)
    }

    pub fn stop(&mut self, ids: &[u32]) {
        self.simulation.stop(ids);
    }

    pub fn clear(&mut self) {
        self.simulation.clear();
    }

    pub fn clip(&mut self, champion: &str, clip: u32, unit: u32, position: Vec3, facing: f32) {
        self.simulation.clip(champion, clip, unit, position, facing);
    }

    pub fn flight(&self, kind: u32) -> [Option<u32>; 2] {
        self.simulation
            .index
            .missile(kind)
            .map_or([None, None], |entry| {
                [&entry.start_bone, &entry.target_bone].map(|bone| bone.as_deref().map(clip_id))
            })
    }

    pub fn missile(&mut self, id: u32, kind: u32, position: Vec3, facing: f32) {
        self.simulation.missile(id, kind, position, facing);
    }

    pub fn land(&mut self, id: u32) {
        self.simulation.land(id);
    }

    pub fn update(
        &mut self,
        dt: f32,
        unit_pose: impl Fn(u32, Option<u32>) -> Option<(Vec3, Quat)>,
    ) {
        self.simulation.update(dt, unit_pose);
    }

    pub fn draw(&self, view_projection: &Mat4, camera_right: Vec3, camera_up: Vec3) {
        let view = View {
            right: camera_right.normalize_or(Vec3::X),
            up: camera_up.normalize_or(Vec3::Y),
            forward: camera_right.cross(camera_up).normalize_or(Vec3::Z),
        };
        let records = instances(&self.simulation, &view);
        if records.is_empty() {
            return;
        }
        let gl = &self.gl;
        let data: Vec<u8> = records
            .iter()
            .flat_map(|(_, record)| record.iter().flat_map(|value| value.to_le_bytes()))
            .collect();
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&self.instances));
        gl.buffer_data_with_u8_array(Gl::ARRAY_BUFFER, &data, Gl::STREAM_DRAW);
        gl.use_program(Some(&self.program));
        gl.uniform_matrix4fv_with_f32_array(
            self.uniforms.view_projection.as_ref(),
            false,
            &view_projection.to_cols_array(),
        );
        gl.enable(Gl::BLEND);
        gl.disable(Gl::CULL_FACE);
        let mut start = 0;
        while start < records.len() {
            let key = records[start].0;
            let end = start
                + records[start..]
                    .iter()
                    .take_while(|(other, _)| *other == key)
                    .count();
            self.batch(&key, start, end - start);
            start = end;
        }
        gl.bind_vertex_array(None);
        for unit in [Gl::TEXTURE1, Gl::TEXTURE2] {
            gl.active_texture(unit);
            gl.bind_texture(Gl::TEXTURE_2D, None);
        }
        gl.active_texture(Gl::TEXTURE0);
        gl.enable(Gl::DEPTH_TEST);
        gl.depth_mask(true);
        gl.disable(Gl::BLEND);
        gl.blend_equation(Gl::FUNC_ADD);
        gl.blend_func(Gl::SRC_ALPHA, Gl::ONE_MINUS_SRC_ALPHA);
    }

    fn batch(&self, key: &BatchKey, first: usize, count: usize) {
        let gl = &self.gl;
        let shape = if key.shape == QUAD {
            &self.quad
        } else {
            &self.meshes[key.shape]
        };
        gl.bind_vertex_array(Some(&shape.vertex_array));
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&self.instances));
        instance_attributes(gl, (first * INSTANCE_FLOATS * 4) as i32);
        let blend = Blend::from_id(u64::from(key.blend));
        let (source, destination, equation) = match blend {
            Blend::Add | Blend::AlphaAdd => (Gl::SRC_ALPHA, Gl::ONE, Gl::FUNC_ADD),
            Blend::Subtract => (Gl::ZERO, Gl::ONE_MINUS_SRC_COLOR, Gl::FUNC_ADD),
            Blend::Premultiplied => (Gl::ONE, Gl::ONE_MINUS_SRC_ALPHA, Gl::FUNC_ADD),
            Blend::Min => (Gl::ONE, Gl::ONE, Gl::MIN),
            Blend::Max => (Gl::ONE, Gl::ONE, Gl::MAX),
            Blend::Alpha | Blend::Opaque => (Gl::SRC_ALPHA, Gl::ONE_MINUS_SRC_ALPHA, Gl::FUNC_ADD),
        };
        if blend == Blend::Opaque {
            gl.disable(Gl::BLEND);
        } else {
            gl.enable(Gl::BLEND);
        }
        gl.blend_equation(equation);
        gl.blend_func(source, destination);
        if key.depth {
            gl.enable(Gl::DEPTH_TEST);
        } else {
            gl.disable(Gl::DEPTH_TEST);
        }
        gl.depth_mask(blend == Blend::Opaque);
        gl.uniform1i(self.uniforms.lock_alpha.as_ref(), i32::from(key.lock_alpha));
        gl.uniform1i(
            self.uniforms.premultiply.as_ref(),
            i32::from(blend == Blend::Subtract),
        );
        gl.uniform1f(
            self.uniforms.alpha_ref.as_ref(),
            f32::from_bits(key.alpha_ref),
        );
        for (unit, texture, flag) in [
            (0, key.base, &self.uniforms.has_base),
            (1, key.ramp, &self.uniforms.has_ramp),
            (2, key.mult, &self.uniforms.has_mult),
        ] {
            gl.active_texture(Gl::TEXTURE0 + unit);
            gl.bind_texture(Gl::TEXTURE_2D, self.textures.get(texture));
            gl.uniform1i(flag.as_ref(), i32::from(texture != NONE));
        }
        gl.draw_elements_instanced_with_i32(
            Gl::TRIANGLES,
            shape.count,
            Gl::UNSIGNED_SHORT,
            0,
            count as i32,
        );
    }
}

fn instance_attributes(gl: &Gl, offset: i32) {
    for (location, size, at) in [
        (2, 3, 0),
        (3, 3, 12),
        (4, 3, 24),
        (5, 3, 36),
        (6, 4, 48),
        (7, 4, 64),
        (8, 4, 80),
        (9, 4, 96),
    ] {
        gl.enable_vertex_attrib_array(location);
        gl.vertex_attrib_pointer_with_i32(
            location,
            size,
            Gl::FLOAT,
            false,
            INSTANCE_STRIDE,
            offset + at,
        );
        gl.vertex_attrib_divisor(location, 1);
    }
}

fn shape(
    gl: &Gl,
    instances: &WebGlBuffer,
    vertices: &[f32],
    indices: &[u16],
) -> Result<Shape, JsValue> {
    let vertex_array = gl.create_vertex_array().ok_or("vertex array")?;
    gl.bind_vertex_array(Some(&vertex_array));
    let bytes: Vec<u8> = vertices
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    upload(gl, Gl::ARRAY_BUFFER, &bytes)?;
    gl.enable_vertex_attrib_array(0);
    gl.vertex_attrib_pointer_with_i32(0, 3, Gl::FLOAT, false, 20, 0);
    gl.enable_vertex_attrib_array(1);
    gl.vertex_attrib_pointer_with_i32(1, 2, Gl::FLOAT, false, 20, 12);
    let bytes: Vec<u8> = indices
        .iter()
        .flat_map(|index| index.to_le_bytes())
        .collect();
    upload(gl, Gl::ELEMENT_ARRAY_BUFFER, &bytes)?;
    gl.bind_buffer(Gl::ARRAY_BUFFER, Some(instances));
    instance_attributes(gl, 0);
    gl.bind_vertex_array(None);
    Ok(Shape {
        vertex_array,
        count: indices.len() as i32,
    })
}

fn effect_files(index: &VfxIndex) -> Vec<String> {
    let mut keys: Vec<String> = index
        .effects
        .iter()
        .chain(&index.missiles)
        .flat_map(|entry| entry.spawns.iter().chain(&entry.enemy))
        .chain(
            index
                .clips
                .iter()
                .chain(&index.idle)
                .flat_map(|entry| &entry.spawns),
        )
        .map(|spawn| spawn.effect.clone())
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

async fn all(promises: impl Iterator<Item = js_sys::Promise>) -> Result<Vec<JsValue>, JsValue> {
    let array: js_sys::Array = promises.collect();
    let results: js_sys::Array = JsFuture::from(js_sys::Promise::all(&array))
        .await?
        .dyn_into()?;
    Ok(results.iter().collect())
}

fn image(source: &str) -> js_sys::Promise {
    let source = source.to_owned();
    future_to_promise(async move {
        Ok(download::image(&source)
            .await
            .map_or(JsValue::NULL, JsValue::from))
    })
}

fn program(gl: &Gl) -> Result<WebGlProgram, JsValue> {
    let program = gl.create_program().ok_or("program")?;
    gl.attach_shader(&program, &shader(gl, Gl::VERTEX_SHADER, VERTEX)?);
    gl.attach_shader(&program, &shader(gl, Gl::FRAGMENT_SHADER, FRAGMENT)?);
    gl.link_program(&program);
    if gl
        .get_program_parameter(&program, Gl::LINK_STATUS)
        .is_truthy()
    {
        Ok(program)
    } else {
        Err(gl.get_program_info_log(&program).unwrap_or_default().into())
    }
}

fn shader(gl: &Gl, kind: u32, source: &str) -> Result<WebGlShader, JsValue> {
    let shader = gl.create_shader(kind).ok_or("shader")?;
    gl.shader_source(&shader, source);
    gl.compile_shader(&shader);
    if gl
        .get_shader_parameter(&shader, Gl::COMPILE_STATUS)
        .is_truthy()
    {
        Ok(shader)
    } else {
        Err(gl.get_shader_info_log(&shader).unwrap_or_default().into())
    }
}

fn upload(gl: &Gl, target: u32, bytes: &[u8]) -> Result<(), JsValue> {
    let buffer = gl.create_buffer().ok_or("buffer")?;
    gl.bind_buffer(target, Some(&buffer));
    gl.buffer_data_with_u8_array(target, bytes, Gl::STATIC_DRAW);
    Ok(())
}

fn texture(gl: &Gl, image: &HtmlImageElement, clamp: bool) -> Result<WebGlTexture, JsValue> {
    let texture = gl.create_texture().ok_or("texture")?;
    gl.bind_texture(Gl::TEXTURE_2D, Some(&texture));
    gl.tex_image_2d_with_u32_and_u32_and_html_image_element(
        Gl::TEXTURE_2D,
        0,
        Gl::RGBA as i32,
        Gl::RGBA,
        Gl::UNSIGNED_BYTE,
        image,
    )?;
    gl.generate_mipmap(Gl::TEXTURE_2D);
    gl.tex_parameteri(
        Gl::TEXTURE_2D,
        Gl::TEXTURE_MIN_FILTER,
        Gl::LINEAR_MIPMAP_LINEAR as i32,
    );
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_MAG_FILTER, Gl::LINEAR as i32);
    let wrap = if clamp { Gl::CLAMP_TO_EDGE } else { Gl::REPEAT } as i32;
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_S, wrap);
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_T, wrap);
    Ok(texture)
}

#[cfg(test)]
mod tests {
    use super::*;
    use asset::clip_id;
    use asset::vfx::{EffectEntry, System, Value};

    fn loaded(emitter: Emitter) -> Rc<Loaded> {
        Rc::new(Loaded {
            effect: Effect {
                systems: vec![System {
                    name: "system".to_owned(),
                    emitters: vec![emitter],
                }],
                ..Effect::default()
            },
            textures: Vec::new(),
            ramps: Vec::new(),
            meshes: Vec::new(),
        })
    }

    fn stream() -> Emitter {
        Emitter {
            rate: Value::scalar(20.0),
            lifetime: Value::scalar(0.5),
            ..Emitter::default()
        }
    }

    fn burst() -> Emitter {
        Emitter {
            single: true,
            rate: Value::scalar(3.0),
            lifetime: Value::scalar(0.2),
            ..Emitter::default()
        }
    }

    fn spawn(effect: &str, attach: Attach, delay: f32) -> Spawn {
        Spawn {
            effect: effect.to_owned(),
            attach,
            delay,
            duration: None,
            bone: None,
            target: None,
        }
    }

    fn entry(name: &str, spawns: Vec<Spawn>, cancels: Vec<u32>) -> EffectEntry {
        EffectEntry {
            name: clip_id(name),
            spawns,
            cancels,
            ..EffectEntry::default()
        }
    }

    fn simulation(mut effects: Vec<EffectEntry>, mut missiles: Vec<EffectEntry>) -> Simulation {
        effects.sort_by_key(|entry| entry.name);
        missiles.sort_by_key(|entry| entry.name);
        let index = VfxIndex {
            effects,
            missiles,
            clips: Vec::new(),
            idle: Vec::new(),
        };
        let loaded = HashMap::from([
            ("trail".to_owned(), loaded(stream())),
            ("boom".to_owned(), loaded(burst())),
        ]);
        Simulation::new(index, loaded)
    }

    fn anchored(simulation: &Simulation, anchor: Anchor) -> Vec<&Instance> {
        simulation
            .instances
            .iter()
            .filter(|instance| instance.anchor == anchor)
            .collect()
    }

    #[test]
    fn missiles_follow_their_track_and_burst_on_hit() {
        let mut simulation = simulation(
            Vec::new(),
            vec![entry(
                "Bolt",
                vec![
                    spawn("trail", Attach::Missile, 0.0),
                    spawn("boom", Attach::Hit, 0.0),
                ],
                Vec::new(),
            )],
        );
        let kind = clip_id("Bolt");
        simulation.missile(7, kind, Vec3::ZERO, 0.0);
        simulation.update(0.1, |_, _| None);
        assert_eq!(anchored(&simulation, Anchor::Missile(7)).len(), 1);
        assert!(anchored(&simulation, Anchor::Fixed).is_empty());

        let target = Vec3::new(100.0, 50.0, 0.0);
        simulation.missile(7, kind, target, 1.0);
        simulation.update(0.1, |_, _| None);
        let trail = anchored(&simulation, Anchor::Missile(7));
        assert_eq!(trail[0].position, target);
        assert_eq!(trail[0].facing, 1.0);
        assert!(!trail[0].emitters[0].particles.is_empty());

        simulation.land(7);
        simulation.update(0.1, |_, _| None);
        assert!(anchored(&simulation, Anchor::Missile(7)).is_empty());
        let hit = anchored(&simulation, Anchor::Fixed);
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].position, target);
        assert_eq!(hit[0].emitters[0].particles.len(), 3);

        for _ in 0..10 {
            simulation.update(0.1, |_, _| None);
        }
        assert!(simulation.instances.is_empty());
        assert!(simulation.tracks.is_empty());
    }

    #[test]
    fn missiles_that_fly_out_of_sight_vanish_without_bursting() {
        let mut simulation = simulation(
            Vec::new(),
            vec![entry(
                "Bolt",
                vec![
                    spawn("trail", Attach::Missile, 0.0),
                    spawn("boom", Attach::Hit, 0.0),
                ],
                Vec::new(),
            )],
        );
        simulation.missile(7, clip_id("Bolt"), Vec3::ZERO, 0.0);
        simulation.update(0.1, |_, _| None);
        simulation.update(0.1, |_, _| None);
        assert!(anchored(&simulation, Anchor::Fixed).is_empty());
        assert!(simulation.tracks.is_empty());
    }

    #[test]
    fn landed_missiles_drop_particles_that_do_not_linger() {
        let mut simulation = simulation(
            Vec::new(),
            vec![entry(
                "Bolt",
                vec![
                    spawn("glow", Attach::Missile, 0.0),
                    spawn("trail", Attach::Missile, 0.0),
                ],
                Vec::new(),
            )],
        );
        simulation.effects.insert(
            "glow".to_owned(),
            loaded(Emitter {
                single: true,
                rate: Value::scalar(1.0),
                lifetime: Value::scalar(2.0),
                ..Emitter::default()
            }),
        );
        simulation.effects.insert(
            "trail".to_owned(),
            loaded(Emitter {
                single: true,
                rate: Value::scalar(1.0),
                lifetime: Value::scalar(2.0),
                linger: Some(0.5),
                ..Emitter::default()
            }),
        );
        let kind = clip_id("Bolt");
        let alive = |simulation: &Simulation| {
            simulation
                .instances
                .iter()
                .filter(|instance| {
                    instance
                        .emitters
                        .iter()
                        .any(|state| !state.particles.is_empty())
                })
                .count()
        };
        simulation.missile(7, kind, Vec3::ZERO, 0.0);
        simulation.update(0.1, |_, _| None);
        assert_eq!(alive(&simulation), 2);
        simulation.update(0.1, |_, _| None);
        assert_eq!(alive(&simulation), 1);
    }

    #[test]
    fn clearing_drops_every_effect_and_missile_track() {
        let mut simulation = simulation(
            Vec::new(),
            vec![entry(
                "Bolt",
                vec![spawn("trail", Attach::Missile, 0.0)],
                Vec::new(),
            )],
        );
        simulation.missile(7, clip_id("Bolt"), Vec3::ZERO, 0.0);
        simulation.update(0.1, |_, _| None);
        assert!(!simulation.instances.is_empty());
        simulation.clear();
        assert!(simulation.instances.is_empty());
        assert!(simulation.pending.is_empty());
        assert!(simulation.tracks.is_empty());
    }

    #[test]
    fn delayed_spawns_wait_and_can_be_cancelled() {
        let mut simulation = simulation(
            vec![
                entry(
                    "Charge",
                    vec![spawn("trail", Attach::Position, 0.5)],
                    Vec::new(),
                ),
                entry(
                    "Release",
                    vec![spawn("boom", Attach::Position, 0.0)],
                    vec![clip_id("Charge")],
                ),
            ],
            Vec::new(),
        );
        simulation.effect(clip_id("Charge"), Vec3::ZERO, 0.0, None, false);
        simulation.update(0.25, |_, _| None);
        assert!(simulation.instances.is_empty());
        simulation.update(0.25, |_, _| None);
        assert_eq!(simulation.instances.len(), 1);

        simulation.effect(clip_id("Charge"), Vec3::ZERO, 0.0, None, false);
        simulation.effect(clip_id("Release"), Vec3::ZERO, 0.0, None, false);
        assert!(simulation.pending.is_empty());
        assert!(simulation.instances[0].stopped);
        assert_eq!(simulation.instances.len(), 2);
    }

    #[test]
    fn removed_effects_drop_their_particles_except_those_that_linger() {
        let lasting = Emitter {
            single: true,
            rate: Value::scalar(1.0),
            lifetime: Value::scalar(25000.0),
            ..Emitter::default()
        };
        let lingering = Emitter {
            linger: Some(0.5),
            ..lasting.clone()
        };
        let glow = entry(
            "Glow",
            vec![
                spawn("lasting", Attach::Position, 0.0),
                spawn("lingering", Attach::Position, 0.0),
            ],
            Vec::new(),
        );
        let mut simulation = Simulation::new(
            VfxIndex {
                effects: vec![glow],
                ..VfxIndex::default()
            },
            HashMap::from([
                ("lasting".to_owned(), loaded(lasting)),
                ("lingering".to_owned(), loaded(lingering)),
            ]),
        );
        let ids = simulation.place(clip_id("Glow"), Vec3::ZERO, 0.0, false);
        simulation.update(0.1, |_, _| None);
        let alive = |simulation: &Simulation| {
            simulation
                .instances
                .iter()
                .filter(|instance| {
                    instance
                        .emitters
                        .iter()
                        .any(|state| !state.particles.is_empty())
                })
                .count()
        };
        assert_eq!(alive(&simulation), 2);
        simulation.stop(&ids);
        simulation.update(0.1, |_, _| None);
        assert_eq!(alive(&simulation), 1);
        simulation.update(0.5, |_, _| None);
        assert_eq!(alive(&simulation), 0);
    }

    #[test]
    fn enemy_variants_are_loaded_with_the_rest() {
        let mut gate = entry(
            "Gate",
            vec![spawn("trail", Attach::Position, 0.0)],
            Vec::new(),
        );
        gate.enemy = vec![spawn("boom", Attach::Position, 0.0)];
        let index = VfxIndex {
            effects: vec![gate],
            ..VfxIndex::default()
        };
        assert_eq!(effect_files(&index), ["boom", "trail"]);
    }

    #[test]
    fn hostile_effects_use_the_enemy_variant() {
        let mut gate = entry(
            "Gate",
            vec![spawn("trail", Attach::Position, 0.0)],
            Vec::new(),
        );
        gate.enemy = vec![spawn("boom", Attach::Position, 0.0)];
        let mut simulation = simulation(vec![gate], Vec::new());
        let effect = |simulation: &Simulation| simulation.instances[0].effect.clone();
        simulation.effect(clip_id("Gate"), Vec3::ZERO, 0.0, None, false);
        simulation.update(0.0, |_, _| None);
        let friendly = effect(&simulation);
        simulation.instances.clear();
        simulation.effect(clip_id("Gate"), Vec3::ZERO, 0.0, None, true);
        simulation.update(0.0, |_, _| None);
        assert!(!Rc::ptr_eq(&friendly, &effect(&simulation)));
    }

    #[test]
    fn unit_spawns_follow_the_unit_and_skip_missing_units() {
        let mut simulation = simulation(
            vec![
                entry("Aura", vec![spawn("trail", Attach::Unit, 0.0)], Vec::new()),
                entry("Late", vec![spawn("trail", Attach::Unit, 0.1)], Vec::new()),
            ],
            Vec::new(),
        );
        simulation.effect(clip_id("Aura"), Vec3::ZERO, 0.0, Some(3), false);
        simulation.effect(clip_id("Late"), Vec3::ZERO, 0.0, Some(4), false);
        let position = Vec3::new(10.0, 0.0, 20.0);
        simulation.update(0.1, |unit, _| {
            (unit == 3).then_some((position, Quat::IDENTITY))
        });
        assert_eq!(simulation.instances.len(), 1);
        assert_eq!(simulation.instances[0].anchor, Anchor::Unit(3));
        assert_eq!(simulation.instances[0].position, position);
        simulation.update(0.1, |_, _| None);
        assert!(simulation.instances[0].stopped);
    }

    #[test]
    fn bone_spawns_emit_along_the_bone_and_turn_with_it() {
        let mut simulation = simulation(
            vec![entry(
                "Glow",
                vec![Spawn {
                    bone: Some("Weapon".to_owned()),
                    ..spawn("blade", Attach::Unit, 0.0)
                }],
                Vec::new(),
            )],
            Vec::new(),
        );
        let blade = Emitter {
            single: true,
            rate: Value::scalar(1.0),
            lifetime: Value::scalar(10.0),
            velocity: Value::constant([0.0, 100.0, 0.0, 0.0]),
            bind: Value::scalar(1.0),
            local: true,
            ..Emitter::default()
        };
        simulation.effects.insert("blade".to_owned(), loaded(blade));
        simulation.effect(clip_id("Glow"), Vec3::ZERO, 0.0, Some(5), false);
        let swing = Quat::from_rotation_z(-std::f32::consts::FRAC_PI_2);
        simulation.update(0.0, |_, _| Some((Vec3::ZERO, swing)));
        let motion = |simulation: &Simulation| {
            let particle = &simulation.instances[0].emitters[0].particles[0];
            (particle.velocity, particle.orientation)
        };
        assert!(motion(&simulation).0.distance(Vec3::new(100.0, 0.0, 0.0)) < 1e-3);
        simulation.update(0.0, |_, _| Some((Vec3::ZERO, Quat::IDENTITY)));
        let (velocity, orientation) = motion(&simulation);
        assert!(velocity.distance(Vec3::new(0.0, 100.0, 0.0)) < 1e-3);
        assert!(orientation.angle_between(Quat::IDENTITY) < 1e-3);
    }

    #[test]
    fn placed_effects_stop_only_when_asked() {
        let mut simulation = simulation(
            vec![entry(
                "Glow",
                vec![spawn("trail", Attach::Position, 0.0)],
                Vec::new(),
            )],
            Vec::new(),
        );
        let first = simulation.place(clip_id("Glow"), Vec3::ZERO, 0.0, false);
        let second = simulation.place(clip_id("Glow"), Vec3::X, 0.0, false);
        assert_eq!(first.len(), 1);
        simulation.stop(&first);
        let stopped = |ids: &[u32]| {
            simulation
                .instances
                .iter()
                .filter(|instance| ids.contains(&instance.id))
                .all(|instance| instance.stopped)
        };
        assert!(stopped(&first));
        assert!(!stopped(&second));
    }

    #[test]
    fn cancels_only_touch_the_same_unit() {
        let mut simulation = simulation(
            vec![
                entry("Spin", vec![spawn("trail", Attach::Unit, 0.0)], Vec::new()),
                entry(
                    "Stop",
                    vec![spawn("boom", Attach::Unit, 0.0)],
                    vec![clip_id("Spin")],
                ),
            ],
            Vec::new(),
        );
        simulation.effect(clip_id("Spin"), Vec3::ZERO, 0.0, Some(1), false);
        simulation.effect(clip_id("Spin"), Vec3::ZERO, 0.0, Some(2), false);
        simulation.effect(clip_id("Stop"), Vec3::ZERO, 0.0, Some(1), false);
        let stopped = |unit| {
            simulation
                .instances
                .iter()
                .find(|instance| {
                    instance.anchor == Anchor::Unit(unit) && instance.tag == clip_id("Spin")
                })
                .is_some_and(|instance| instance.stopped)
        };
        assert!(stopped(1));
        assert!(!stopped(2));
    }

    #[test]
    fn stopped_endless_particles_linger() {
        let mut simulation = simulation(Vec::new(), Vec::new());
        let effect = loaded(Emitter {
            single: true,
            rate: Value::scalar(2.0),
            linger: Some(0.3),
            ..Emitter::default()
        });
        let id = simulation.instance(effect, 0, Anchor::Fixed, Vec3::ZERO, 0.0, 0);
        simulation.update(1.0, |_, _| None);
        assert_eq!(simulation.instances[0].emitters[0].particles.len(), 2);
        simulation.instances[0].stopped = true;
        simulation.update(0.1, |_, _| None);
        assert!(
            simulation.instances[0].emitters[0]
                .particles
                .iter()
                .all(|particle| particle.life == 0.3)
        );
        simulation.update(0.5, |_, _| None);
        assert!(
            simulation
                .instances
                .iter()
                .all(|instance| instance.id != id)
        );
    }

    #[test]
    fn emitters_spawn_their_first_particle_as_soon_as_they_start() {
        let mut simulation = simulation(Vec::new(), Vec::new());
        let blur = loaded(Emitter {
            rate: Value::scalar(2.0),
            delay: 0.75,
            end: Some(0.85),
            lifetime: Value::scalar(0.5),
            ..Emitter::default()
        });
        let silent = loaded(Emitter {
            rate: Value::scalar(0.0),
            lifetime: Value::scalar(0.5),
            ..Emitter::default()
        });
        simulation.instance(blur, 0, Anchor::Fixed, Vec3::ZERO, 0.0, 0);
        simulation.instance(silent, 0, Anchor::Fixed, Vec3::ZERO, 0.0, 0);
        let counts = |simulation: &Simulation| {
            simulation
                .instances
                .iter()
                .map(|instance| instance.emitters[0].particles.len())
                .collect::<Vec<_>>()
        };
        simulation.update(0.5, |_, _| None);
        assert_eq!(counts(&simulation), [0, 0]);
        simulation.update(0.3, |_, _| None);
        assert_eq!(counts(&simulation), [1, 0]);
        simulation.update(0.04, |_, _| None);
        assert_eq!(counts(&simulation), [1, 0]);
    }

    #[test]
    fn effects_spawned_by_a_moving_particle_carry_their_particles_along() {
        let mut simulation = simulation(Vec::new(), Vec::new());
        let held = Emitter {
            single: true,
            rate: Value::scalar(1.0),
            lifetime: Value::scalar(-1.0),
            bind: Value::scalar(1.0),
            ..Emitter::default()
        };
        let parent = Emitter {
            child: Some(asset::vfx::Child {
                systems: vec![1],
                choice: Value::scalar(0.0),
                on_death: false,
            }),
            ..held.clone()
        };
        let card = Rc::new(Loaded {
            effect: Effect {
                systems: vec![
                    System {
                        name: "missile".to_owned(),
                        emitters: vec![parent],
                    },
                    System {
                        name: "card".to_owned(),
                        emitters: vec![held],
                    },
                ],
                ..Effect::default()
            },
            textures: Vec::new(),
            ramps: Vec::new(),
            meshes: Vec::new(),
        });
        simulation.instance(card, 0, Anchor::Unit(7), Vec3::ZERO, 0.0, 0);
        for step in 1..=5 {
            let at = Vec3::new(step as f32 * 100.0, 0.0, 0.0);
            simulation.update(0.1, |_, _| Some((at, Quat::IDENTITY)));
        }
        let card = simulation
            .instances
            .iter()
            .find(|instance| instance.depth == 1)
            .unwrap();
        let particle = &card.emitters[0].particles[0];
        assert!(particle.position.x > 300.0);
    }

    #[test]
    fn only_effects_without_a_set_duration_stop_at_the_age_limit() {
        let mut simulation = simulation(Vec::new(), Vec::new());
        let glow = Emitter {
            rate: Value::scalar(1.0),
            lifetime: Value::scalar(0.5),
            ..Emitter::default()
        };
        let endless =
            simulation.instance(loaded(glow.clone()), 0, Anchor::Fixed, Vec3::ZERO, 0.0, 0);
        let lasting =
            simulation.instance(loaded(glow.clone()), 0, Anchor::Fixed, Vec3::ZERO, 0.0, 0);
        let bound = simulation.instance(
            loaded(glow),
            0,
            Anchor::Particle(Vec3::ZERO),
            Vec3::ZERO,
            0.0,
            1,
        );
        if let Some(instance) = simulation
            .instances
            .iter_mut()
            .find(|instance| instance.id == lasting)
        {
            instance.duration = Some(f32::MAX);
        }
        for _ in 0..25 {
            simulation.update(1.0, |_, _| None);
        }
        let stopped = |id: u32| {
            simulation
                .instances
                .iter()
                .find(|instance| instance.id == id)
                .is_none_or(|instance| instance.stopped)
        };
        assert!(stopped(endless));
        assert!(!stopped(lasting));
        assert!(!stopped(bound));
    }

    fn particle(emitter: &Emitter, age: f32, life: f32) -> Particle {
        let mut particle = birth(emitter, 0.0, Vec3::ZERO, Quat::IDENTITY, 0.0, &mut Rng(1));
        particle.age = age;
        particle.life = life;
        particle
    }

    #[test]
    fn beams_reach_from_their_unit_to_the_effect_position_at_the_same_height() {
        let mut simulation = simulation(
            vec![entry(
                "Pulse",
                vec![spawn("beam", Attach::Beam, 0.0)],
                Vec::new(),
            )],
            Vec::new(),
        );
        let beam = Emitter {
            primitive: Primitive::Beam {
                tiling: 0.0,
                width: 1.0,
                transposed: false,
            },
            ..burst()
        };
        simulation.effects.insert("beam".to_owned(), loaded(beam));
        simulation.effect(
            clip_id("Pulse"),
            Vec3::new(500.0, 0.0, 0.0),
            0.0,
            Some(7),
            false,
        );
        simulation.update(0.016, |_, _| {
            Some((Vec3::new(0.0, 100.0, 0.0), Quat::IDENTITY))
        });
        let instances = anchored(&simulation, Anchor::Unit(7));
        assert_eq!(instances.len(), 1);
        assert_eq!(
            instances[0].end_position,
            Some(Vec3::new(500.0, 100.0, 0.0))
        );
    }

    #[test]
    fn beams_stretch_their_texture_along_the_beam_and_tile_by_length() {
        let emitter = Emitter {
            primitive: Primitive::Beam {
                tiling: 100.0,
                width: 1.0,
                transposed: false,
            },
            birth_scale: Value::scalar(40.0),
            scale: Value::scalar(1.0),
            ..Emitter::default()
        };
        let particle = particle(&emitter, 0.1, 1.0);
        let view = View {
            right: Vec3::X,
            up: Vec3::Y,
            forward: Vec3::NEG_Z,
        };
        let end = Vec3::new(0.0, 300.0, 0.0);
        let record = beam(&emitter, &particle, Vec3::ZERO, end, &view).unwrap();
        assert_eq!(&record[0..3], &[0.0, 150.0, 0.0]);
        assert_eq!(Vec3::from_slice(&record[3..6]).length(), 20.0);
        assert_eq!(&record[6..9], &[0.0, 150.0, 0.0]);
        assert_eq!(record[17], 3.0);
    }

    #[test]
    fn flipbooks_play_over_lifetime_or_at_a_frame_rate() {
        let emitter = Emitter {
            grid: [4.0, 2.0],
            frames: 8,
            ..Emitter::default()
        };
        assert_eq!(
            cell(&emitter, &particle(&emitter, 0.5, 1.0)),
            [0.25, 0.5, 0.0, 0.5]
        );
        assert_eq!(
            cell(&emitter, &particle(&emitter, 1.0, 1.0)),
            [0.25, 0.5, 0.75, 0.5]
        );
        let timed = Emitter {
            frame_rate: 10.0,
            start_frame: 1,
            ..emitter.clone()
        };
        assert_eq!(
            cell(&timed, &particle(&timed, 0.25, 1.0)),
            [0.25, 0.5, 0.75, 0.0]
        );
        let single = Emitter::default();
        assert_eq!(
            cell(&single, &particle(&single, 0.5, 1.0)),
            [1.0, 1.0, 0.0, 0.0]
        );
    }

    #[test]
    fn masks_hold_one_frame_of_their_own_grid() {
        let fixed = Emitter {
            mult_grid: [4.0, 1.0],
            ..Emitter::default()
        };
        let mut sample = particle(&fixed, 0.5, 1.0);
        assert_eq!(mult_cell(&fixed, &sample), [0.25, 1.0, 0.0, 0.0]);
        let random = Emitter {
            mult_random_frame: true,
            ..fixed
        };
        sample.random = 0.6;
        assert_eq!(mult_cell(&random, &sample), [0.25, 1.0, 0.5, 0.0]);
        sample.age = 0.9;
        assert_eq!(mult_cell(&random, &sample), [0.25, 1.0, 0.5, 0.0]);
        assert_eq!(
            mult_cell(&Emitter::default(), &sample),
            [1.0, 1.0, 0.0, 0.0]
        );
    }

    #[test]
    fn additive_colors_are_premultiplied() {
        let emitter = Emitter {
            blend: Blend::Add,
            birth_color: Value::constant([1.0, 0.5, 0.0, 0.5]),
            ..Emitter::default()
        };
        assert_eq!(
            color(&emitter, &particle(&emitter, 0.0, 1.0)),
            Vec4::new(0.5, 0.25, 0.0, 1.0)
        );
    }

    #[test]
    fn random_numbers_stay_in_unit_range() {
        let mut rng = Rng(0x2545_f491);
        let values: Vec<f32> = (0..10_000).map(|_| rng.next()).collect();
        assert!(values.iter().all(|value| (0.0..1.0).contains(value)));
        let mean = values.iter().sum::<f32>() / values.len() as f32;
        assert!((mean - 0.5).abs() < 0.02);
    }
}
