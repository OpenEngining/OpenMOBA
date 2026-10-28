use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use protocol::{ClientMessage, ServerMessage, decode, encode};
use wasm_bindgen::prelude::*;
use web_sys::{BinaryType, MessageEvent, WebSocket};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Connecting,
    Open,
    Closed,
}

pub struct Connection {
    socket: WebSocket,
    inbox: Rc<RefCell<VecDeque<ServerMessage>>>,
    status: Rc<RefCell<Status>>,
    _message: Closure<dyn Fn(MessageEvent)>,
    _open: Closure<dyn Fn()>,
    _close: Closure<dyn Fn()>,
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.socket.set_onmessage(None);
        self.socket.set_onopen(None);
        self.socket.set_onclose(None);
        let _ = self.socket.close();
    }
}

impl Connection {
    pub fn open() -> Result<Self, JsValue> {
        let location = web_sys::window().ok_or("no window")?.location();
        let scheme = if location.protocol()? == "https:" {
            "wss"
        } else {
            "ws"
        };
        let socket = WebSocket::new(&format!("{scheme}://{}/ws", location.host()?))?;
        socket.set_binary_type(BinaryType::Arraybuffer);
        let inbox = Rc::new(RefCell::new(VecDeque::new()));
        let status = Rc::new(RefCell::new(Status::Connecting));

        let received = inbox.clone();
        let on_message = Closure::<dyn Fn(MessageEvent)>::new(move |event: MessageEvent| {
            if let Ok(buffer) = event.data().dyn_into::<js_sys::ArrayBuffer>() {
                let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
                if let Some(message) = decode::<ServerMessage>(&bytes) {
                    received.borrow_mut().push_back(message);
                }
            }
        });
        socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

        let opened = status.clone();
        let on_open = Closure::<dyn Fn()>::new(move || *opened.borrow_mut() = Status::Open);
        socket.set_onopen(Some(on_open.as_ref().unchecked_ref()));

        let closed = status.clone();
        let on_close = Closure::<dyn Fn()>::new(move || *closed.borrow_mut() = Status::Closed);
        socket.set_onclose(Some(on_close.as_ref().unchecked_ref()));

        Ok(Self {
            socket,
            inbox,
            status,
            _message: on_message,
            _open: on_open,
            _close: on_close,
        })
    }

    pub fn status(&self) -> Status {
        *self.status.borrow()
    }

    pub fn send(&self, message: &ClientMessage) {
        if self.status() == Status::Open {
            let _ = self.socket.send_with_u8_array(&encode(message));
        }
    }

    pub fn drain(&self) -> Vec<ServerMessage> {
        self.inbox.borrow_mut().drain(..).collect()
    }
}
