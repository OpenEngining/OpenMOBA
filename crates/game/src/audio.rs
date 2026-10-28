use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use asset::audio::AudioIndex;
use glam::Vec2;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AudioBuffer, AudioBufferSourceNode, AudioContext, AudioContextState, AudioScheduledSourceNode,
    GainNode,
};

use crate::download::{self, fetch};

const FOLDER: &str = "assets/audio";
const FULL_VOLUME_DISTANCE: f32 = 600.0;
const SILENT_DISTANCE: f32 = 2500.0;
const PAN_DISTANCE: f32 = 1500.0;
const MAX_PAN: f32 = 0.5;
const LATE_SECONDS: f64 = 0.35;
const LIMITER_THRESHOLD: f32 = -6.0;
const LIMITER_KNEE: f32 = 6.0;
const LIMITER_RATIO: f32 = 8.0;

pub struct Audio {
    shared: Rc<Shared>,
    index: AudioIndex,
    listener: Vec2,
}

struct Shared {
    context: AudioContext,
    output: GainNode,
    clips: RefCell<HashMap<String, Clip>>,
    music: RefCell<Music>,
}

enum Clip {
    Loading(Vec<Request>),
    Ready(AudioBuffer),
    Failed,
}

#[derive(Clone, Copy)]
enum Request {
    Effect { volume: f32, pan: f32, time: f64 },
    Music,
}

#[derive(Default)]
struct Music {
    file: Option<String>,
    source: Option<AudioBufferSourceNode>,
}

impl Audio {
    pub async fn load() -> Result<Self, JsValue> {
        let index = AudioIndex::from_bytes(&fetch(&format!("{FOLDER}/index.bin")).await?)
            .map_err(|error| error.to_string())?;
        let context = AudioContext::new()?;
        let limiter = context.create_dynamics_compressor()?;
        limiter.threshold().set_value(LIMITER_THRESHOLD);
        limiter.knee().set_value(LIMITER_KNEE);
        limiter.ratio().set_value(LIMITER_RATIO);
        limiter.connect_with_audio_node(&context.destination())?;
        let output = context.create_gain()?;
        output.connect_with_audio_node(&limiter)?;
        let shared = Rc::new(Shared {
            context,
            output,
            clips: RefCell::new(HashMap::new()),
            music: RefCell::new(Music::default()),
        });
        resume_on_gesture(&shared)?;
        Ok(Self {
            shared,
            index,
            listener: Vec2::ZERO,
        })
    }

    pub fn has(&self, key: &str) -> bool {
        self.index.sounds.contains_key(key)
    }

    pub fn volume(&self, volume: f32) {
        self.shared.output.gain().set_value(volume);
    }

    pub fn play(&mut self, key: &str) {
        self.effect(key, 1.0, 0.0);
    }

    pub fn play_at(&mut self, key: &str, position: Vec2) {
        let offset = position - self.listener;
        let volume = falloff(offset.length());
        if volume > 0.0 {
            let pan = (offset.x / PAN_DISTANCE).clamp(-1.0, 1.0) * MAX_PAN;
            self.effect(key, volume, pan);
        }
    }

    pub fn set_listener(&mut self, position: Vec2) {
        self.listener = position;
    }

    pub fn loop_music(&mut self, key: &str) {
        let Some(file) = self.index.variants(key).first().cloned() else {
            return;
        };
        if self.shared.music.borrow().file.as_ref() == Some(&file) {
            return;
        }
        self.stop_music();
        self.shared.music.borrow_mut().file = Some(file.clone());
        request(&self.shared, &file, Request::Music);
    }

    pub fn stop_music(&mut self) {
        let mut music = self.shared.music.borrow_mut();
        if let Some(source) = music.source.take() {
            let _ = AudioScheduledSourceNode::stop(&source);
        }
        music.file = None;
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.index.sounds.keys()
    }

    pub fn loading(&self) -> bool {
        self.shared
            .clips
            .borrow()
            .values()
            .any(|clip| matches!(clip, Clip::Loading(_)))
    }

    pub fn preload(&mut self, key: &str) {
        for file in self.index.variants(key) {
            if !self.shared.clips.borrow().contains_key(file) {
                self.shared
                    .clips
                    .borrow_mut()
                    .insert(file.clone(), Clip::Loading(Vec::new()));
                load(self.shared.clone(), file.clone());
            }
        }
    }

    fn effect(&mut self, key: &str, volume: f32, pan: f32) {
        if self.shared.context.state() != AudioContextState::Running {
            return;
        }
        let variants = self.index.variants(key);
        if variants.is_empty() {
            return;
        }
        let pick = (js_sys::Math::random() * variants.len() as f64) as usize;
        let file = variants[pick.min(variants.len() - 1)].clone();
        let time = self.shared.context.current_time();
        request(&self.shared, &file, Request::Effect { volume, pan, time });
    }
}

fn falloff(distance: f32) -> f32 {
    let t = ((distance - FULL_VOLUME_DISTANCE) / (SILENT_DISTANCE - FULL_VOLUME_DISTANCE))
        .clamp(0.0, 1.0);
    (1.0 - t) * (1.0 - t)
}

fn request(shared: &Rc<Shared>, file: &str, request: Request) {
    let mut clips = shared.clips.borrow_mut();
    match clips.get_mut(file) {
        Some(Clip::Ready(buffer)) => {
            let buffer = buffer.clone();
            drop(clips);
            start(shared, file, &buffer, request);
        }
        Some(Clip::Loading(pending)) => pending.push(request),
        Some(Clip::Failed) => {}
        None => {
            clips.insert(file.to_owned(), Clip::Loading(vec![request]));
            drop(clips);
            load(shared.clone(), file.to_owned());
        }
    }
}

fn load(shared: Rc<Shared>, file: String) {
    download::expect_file(&format!("{FOLDER}/{file}"));
    wasm_bindgen_futures::spawn_local(async move {
        let decoded = decode(&shared.context, &file).await;
        let clip = match &decoded {
            Ok(buffer) => Clip::Ready(buffer.clone()),
            Err(error) => {
                web_sys::console::warn_2(&JsValue::from_str(&file), error);
                Clip::Failed
            }
        };
        let previous = shared.clips.borrow_mut().insert(file.clone(), clip);
        if let (Ok(buffer), Some(Clip::Loading(pending))) = (decoded, previous) {
            for request in pending {
                start(&shared, &file, &buffer, request);
            }
        }
    });
}

async fn decode(context: &AudioContext, file: &str) -> Result<AudioBuffer, JsValue> {
    let bytes = fetch(&format!("{FOLDER}/{file}")).await?;
    let data = js_sys::Uint8Array::from(bytes.as_slice()).buffer();
    JsFuture::from(context.decode_audio_data(&data)?)
        .await?
        .dyn_into()
}

fn start(shared: &Shared, file: &str, buffer: &AudioBuffer, request: Request) {
    let started = match request {
        Request::Effect { volume, pan, time } => {
            if shared.context.current_time() - time > LATE_SECONDS {
                return;
            }
            source(shared, buffer, volume, pan, false).map(drop)
        }
        Request::Music => {
            let mut music = shared.music.borrow_mut();
            if music.file.as_deref() != Some(file) || music.source.is_some() {
                return;
            }
            source(shared, buffer, 1.0, 0.0, true).map(|node| music.source = Some(node))
        }
    };
    if let Err(error) = started {
        web_sys::console::warn_1(&error);
    }
}

fn source(
    shared: &Shared,
    buffer: &AudioBuffer,
    volume: f32,
    pan: f32,
    looping: bool,
) -> Result<AudioBufferSourceNode, JsValue> {
    let context = &shared.context;
    let source = context.create_buffer_source()?;
    source.set_buffer(Some(buffer));
    source.set_loop(looping);
    let gain = context.create_gain()?;
    gain.gain().set_value(volume);
    let panner = context.create_stereo_panner()?;
    panner.pan().set_value(pan);
    source.connect_with_audio_node(&gain)?;
    gain.connect_with_audio_node(&panner)?;
    panner.connect_with_audio_node(&shared.output)?;
    source.start()?;
    Ok(source)
}

fn resume_on_gesture(shared: &Rc<Shared>) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or("no window")?;
    for event in ["pointerdown", "keydown"] {
        let shared = shared.clone();
        let closure = Closure::<dyn Fn()>::new(move || {
            if shared.context.state() == AudioContextState::Suspended {
                let _ = shared.context.resume();
            }
        });
        window.add_event_listener_with_callback(event, closure.as_ref().unchecked_ref())?;
        closure.forget();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_off_with_distance() {
        assert_eq!(falloff(0.0), 1.0);
        assert_eq!(falloff(FULL_VOLUME_DISTANCE), 1.0);
        assert!(falloff(1500.0) > 0.0 && falloff(1500.0) < 1.0);
        assert_eq!(falloff(SILENT_DISTANCE), 0.0);
        assert_eq!(falloff(5000.0), 0.0);
    }
}
