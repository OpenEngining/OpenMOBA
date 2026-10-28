use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};

use js_sys::{Array, Function, Promise, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AbortController, Blob, HtmlImageElement, ReadableStream, ReadableStreamDefaultReader,
    RequestInit, Response, Url,
};

const PARALLEL: usize = 6;
const STALL_MS: i32 = 20_000;
const FIRST_RETRY_MS: i32 = 500;
const LONGEST_RETRY_MS: i32 = 8_000;
const ASSETS: &str = "assets/";

thread_local! {
    static MANIFEST: RefCell<HashMap<String, Entry>> = RefCell::new(HashMap::new());
    static PROGRESS: RefCell<Progress> = RefCell::new(Progress::default());
    static ACTIVE: Cell<usize> = const { Cell::new(0) };
    static QUEUE: RefCell<VecDeque<Function>> = const { RefCell::new(VecDeque::new()) };
}

struct Entry {
    size: u64,
    version: String,
}

#[derive(Default)]
struct Progress {
    counted: HashSet<String>,
    expected: u64,
    received: u64,
}

enum Failure {
    Final(JsValue),
    Retry(JsValue),
}

struct Slot;

impl Slot {
    async fn take() -> Self {
        if ACTIVE.get() < PARALLEL {
            ACTIVE.set(ACTIVE.get() + 1);
        } else {
            let turn = Promise::new(&mut |resolve, _| {
                QUEUE.with_borrow_mut(|queue| queue.push_back(resolve));
            });
            let _ = JsFuture::from(turn).await;
        }
        Slot
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        match QUEUE.with_borrow_mut(VecDeque::pop_front) {
            Some(next) => {
                let _ = next.call0(&JsValue::NULL);
            }
            None => ACTIVE.set(ACTIVE.get() - 1),
        }
    }
}

pub async fn load_manifest() {
    match fetch("manifest").await {
        Ok(bytes) => {
            let listed = entries(&String::from_utf8_lossy(&bytes));
            MANIFEST.with_borrow_mut(|manifest| manifest.extend(listed));
        }
        Err(error) => web_sys::console::warn_1(&error),
    }
}

fn entries(manifest: &str) -> Vec<(String, Entry)> {
    manifest
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let name = fields.next()?.to_owned();
            let size = fields.next()?.parse().ok()?;
            let version = fields.next()?.to_owned();
            Some((name, Entry { size, version }))
        })
        .collect()
}

fn versioned(url: &str) -> String {
    url.strip_prefix(ASSETS)
        .and_then(|name| MANIFEST.with_borrow(|manifest| Some(manifest.get(name)?.version.clone())))
        .map_or_else(|| url.to_owned(), |version| format!("{url}?v={version}"))
}

pub fn listed(url: &str) -> bool {
    url.strip_prefix(ASSETS)
        .is_some_and(|name| MANIFEST.with_borrow(|manifest| manifest.contains_key(name)))
}

pub fn expect_folder(folder: &str) {
    let names: Vec<String> = MANIFEST.with_borrow(|manifest| {
        manifest
            .keys()
            .filter(|name| name.starts_with(folder))
            .cloned()
            .collect()
    });
    for name in names {
        expect(&name);
    }
}

pub fn expect_file(url: &str) {
    if let Some(name) = url.strip_prefix(ASSETS) {
        expect(name);
    }
}

fn expect(name: &str) -> bool {
    let Some(size) = MANIFEST.with_borrow(|manifest| Some(manifest.get(name)?.size)) else {
        return false;
    };
    PROGRESS.with_borrow_mut(|progress| {
        if progress.counted.insert(name.to_owned()) {
            progress.expected += size;
        }
    });
    true
}

fn receive(bytes: u64) {
    PROGRESS.with_borrow_mut(|progress| progress.received += bytes);
}

fn forget(bytes: u64) {
    PROGRESS.with_borrow_mut(|progress| progress.received -= bytes);
}

pub fn progress() -> (u64, u64) {
    PROGRESS.with_borrow(|progress| (progress.received, progress.expected))
}

pub async fn fetch(url: &str) -> Result<Vec<u8>, JsValue> {
    let counted = url.strip_prefix(ASSETS).is_some_and(expect);
    let mut wait = FIRST_RETRY_MS;
    loop {
        let slot = Slot::take().await;
        let mut got = 0;
        let attempt = attempt(url, counted, &mut got).await;
        drop(slot);
        if attempt.is_err() {
            forget(got);
        }
        match attempt {
            Ok(bytes) => return Ok(bytes),
            Err(Failure::Final(error)) => return Err(error),
            Err(Failure::Retry(error)) => {
                web_sys::console::warn_2(&JsValue::from_str(&format!("retrying {url}")), &error);
                sleep(wait).await;
                wait = (wait * 2).min(LONGEST_RETRY_MS);
            }
        }
    }
}

pub async fn image(url: &str) -> Result<HtmlImageElement, JsValue> {
    let bytes = fetch(url).await?;
    let parts = Array::of1(&Uint8Array::from(bytes.as_slice()));
    let source = Url::create_object_url_with_blob(&Blob::new_with_u8_array_sequence(&parts)?)?;
    let image = HtmlImageElement::new()?;
    let loaded = Promise::new(&mut |resolve, reject| {
        image.set_onload(Some(&resolve));
        image.set_onerror(Some(&reject));
    });
    image.set_src(&source);
    let decoded = JsFuture::from(loaded).await;
    let _ = Url::revoke_object_url(&source);
    decoded.map_err(|_| JsValue::from_str(&format!("{url}: not an image")))?;
    Ok(image)
}

async fn attempt(url: &str, counted: bool, got: &mut u64) -> Result<Vec<u8>, Failure> {
    let controller = AbortController::new().map_err(Failure::Final)?;
    let init = RequestInit::new();
    init.set_signal(Some(&controller.signal()));
    let window = web_sys::window().ok_or_else(|| Failure::Final("no window".into()))?;
    let request = window.fetch_with_str_and_init(&versioned(url), &init);
    let response: Response = within(request, &controller).await?.unchecked_into();
    let status = response.status();
    if !response.ok() {
        let error = JsValue::from_str(&format!("{url}: {status}"));
        return Err(if status == 408 || status == 429 || status >= 500 {
            Failure::Retry(error)
        } else {
            Failure::Final(error)
        });
    }
    let Some(body) = response.body() else {
        return Ok(Vec::new());
    };
    let reader: ReadableStreamDefaultReader = ReadableStream::get_reader(&body).unchecked_into();
    let mut bytes = Vec::new();
    loop {
        let chunk = within(reader.read(), &controller).await?;
        if Reflect::get(&chunk, &"done".into()).is_ok_and(|done| done.is_truthy()) {
            return Ok(bytes);
        }
        let value: Uint8Array = Reflect::get(&chunk, &"value".into())
            .map_err(Failure::Retry)?
            .unchecked_into();
        let start = bytes.len();
        bytes.resize(start + value.length() as usize, 0);
        value.copy_to(&mut bytes[start..]);
        if counted {
            *got += u64::from(value.length());
            receive(u64::from(value.length()));
        }
    }
}

async fn within(promise: Promise, controller: &AbortController) -> Result<JsValue, Failure> {
    let raced = Promise::race(&Array::of2(&promise, &timer(STALL_MS)));
    match JsFuture::from(raced).await {
        Ok(value) if value.is_undefined() => {
            controller.abort();
            Err(Failure::Retry("stalled".into()))
        }
        Ok(value) => Ok(value),
        Err(error) => Err(Failure::Retry(error)),
    }
}

fn timer(ms: i32) -> Promise {
    Promise::new(&mut |resolve, _| {
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
        }
    })
}

async fn sleep(ms: i32) {
    let _ = JsFuture::from(timer(ms)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_urls_carry_the_version_from_the_manifest() {
        MANIFEST.with_borrow_mut(|known| {
            known.extend(entries(
                "champions/emperor.bin\t10\t65a1\ndata.bin\t3\t77\n",
            ))
        });
        assert_eq!(
            versioned("assets/champions/emperor.bin"),
            "assets/champions/emperor.bin?v=65a1"
        );
        assert_eq!(versioned("assets/data.bin"), "assets/data.bin?v=77");
        assert_eq!(versioned("assets/missing.bin"), "assets/missing.bin");
        assert_eq!(versioned("manifest"), "manifest");
    }

    #[test]
    fn progress_counts_each_listed_file_once() {
        MANIFEST.with_borrow_mut(|known| {
            known.extend(entries(
                "vfx/a.bin\t100\t1\nvfx/b.png\t50\t1\nunits/c.bin\t7\t1\n",
            ))
        });
        expect_folder("vfx/");
        assert!(expect("vfx/a.bin"));
        assert!(!expect("vfx/missing.bin"));
        receive(30);
        assert_eq!(progress(), (30, 150));
        forget(30);
        assert!(expect("units/c.bin"));
        assert_eq!(progress(), (0, 157));
    }
}
