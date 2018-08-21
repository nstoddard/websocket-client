//! A websocket client library which supports both desktop and webassembly.
//! It's only been tested on Linux and wasm32-unknown-emscripten, but should
//! work on other desktop platforms and other webassembly targets.
//!
//! It uses a polling API for receiving messages, so it's probably most
//! suitable for games, which would poll every frame. It may not be suitable
//! for applications which need to respond to messages within milliseconds of
//! receiving them, since the polling would add a slight delay.
//!
//! This supports both text and binary data. On desktop, it uses `websocket`. On
//! webassembly, it uses JavaScript through `stdweb`.

// Needed for js! macro
#![recursion_limit = "256"]

// TODO: see if there's a way to remove all these cfg's
#[cfg(not(target_arch = "wasm32"))]
extern crate websocket;
#[cfg(target_arch = "wasm32")]
#[macro_use]
extern crate stdweb;
extern crate simple_error;

use simple_error::*;
#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;
use std::error::Error;
#[cfg(not(target_arch = "wasm32"))]
use std::io::ErrorKind;
#[cfg(not(target_arch = "wasm32"))]
use std::marker::PhantomData;
#[cfg(target_arch = "wasm32")]
use std::rc::Rc;
#[cfg(target_arch = "wasm32")]
use stdweb::web;
#[cfg(target_arch = "wasm32")]
use stdweb::Value;
#[cfg(not(target_arch = "wasm32"))]
use websocket::client::sync::Client;
#[cfg(not(target_arch = "wasm32"))]
use websocket::stream::sync::NetworkStream;
#[cfg(not(target_arch = "wasm32"))]
use websocket::*;

#[derive(Debug, Clone)]
pub enum SocketMessage {
    Text(String),
    Binary(Vec<u8>),
}

#[cfg(target_arch = "wasm32")]
struct SocketState {
    // TODO: clean this up. Probably put everything in a single RefCell.
    queued: Rc<RefCell<Vec<SocketMessage>>>,
    received: Rc<RefCell<Vec<SocketMessage>>>,
    disconnected: Rc<RefCell<bool>>,
    error: Rc<RefCell<bool>>,
}

#[cfg(target_arch = "wasm32")]
pub struct Socket {
    js_obj: stdweb::Value,
    state: SocketState,
}

// TODO: see if there's a way to merge these impls so they can't accidentally
// get out of sync

// TODO: use a custom error type instead of Box<Error>
#[cfg(target_arch = "wasm32")]
impl Socket {
    /// Creates a new Socket.
    pub fn new(url: String) -> Result<Socket, Box<Error>> {
        stdweb::initialize();

        let state = SocketState {
            queued: Rc::new(RefCell::new(vec![])),
            received: Rc::new(RefCell::new(vec![])),
            disconnected: Rc::new(RefCell::new(false)),
            error: Rc::new(RefCell::new(false)),
        };

        let queued = state.queued.clone();
        let received = state.received.clone();
        let received2 = state.received.clone();
        let disconnected = state.disconnected.clone();
        let error = state.error.clone();

        let get_queued = move || -> stdweb::Array {
            let mut queued = queued.borrow_mut();
            let queued: Vec<Value> = queued
                .drain(..)
                .map(|x| match x {
                    SocketMessage::Text(data) => Value::String(data),
                    SocketMessage::Binary(data) => {
                        let typed_array = web::TypedArray::from(&data[..]);
                        let data = typed_array.buffer();
                        stdweb::Value::Reference(stdweb::Reference::from(data))
                    }
                }).collect();
            stdweb::Array::from(queued)
        };

        // TODO: these two closures are never freed. Probably not a big deal though
        // since they're used throughout the life of the app.
        let add_received_text = move |msg: String| {
            let mut received = received.borrow_mut();
            received.push(SocketMessage::Text(msg));
        };
        let add_received_binary = move |msg: web::TypedArray<u8>| {
            let mut received = received2.borrow_mut();
            received.push(SocketMessage::Binary(msg.to_vec()));
        };
        let set_disconnected = move || {
            let mut disconnected = disconnected.borrow_mut();
            *disconnected = true;
        };
        let set_error = move || {
            let mut error = error.borrow_mut();
            *error = true;
        };
        let js_obj = js! {
            var socket = new WebSocket(@{url});
            var get_queued = @{get_queued};
            var add_received_text = @{add_received_text};
            var add_received_binary = @{add_received_binary};
            var set_disconnected = @{set_disconnected};
            var set_error = @{set_error};
            if (socket) {
                socket.binaryType = "arraybuffer";
                socket.onopen = function(e) {
                    var queued = get_queued();
                    for (var i = 0; i < queued.length; i++) {
                        socket.send(queued[i]);
                    }
                    get_queued.drop();
                };
                socket.onerror = function(e) {
                    console.log("Socket error: " + e);
                    set_error();
                };
                socket.onclose = function(e) {
                    console.log("Socket closed");
                    set_disconnected();
                };
                socket.onmessage = function(m) {
                    if (m.data instanceof ArrayBuffer) {
                        add_received_binary(new Uint8Array(m.data));
                    } else {
                        add_received_text(m.data);
                    }
                };
                return socket;
            } else {
                console.log("Unable to create socket.");
                return null;
            }
        };
        if js_obj == stdweb::Value::Null {
            Err(Box::new(SimpleError::new(
                "Unable to create js_obj for socket",
            )))
        } else {
            Ok(Socket { js_obj, state })
        }
    }

    // TODO: the 'send' functions should probably pass borrowed data
    /// Sends a textual message.
    pub fn send(&mut self, data: String) -> Result<(), Box<Error>> {
        let queued = self.state.queued.clone();
        let ready = match js! {
        var socket = @{&self.js_obj};
        if (socket.readyState == 2 || socket.readyState == 3) {
            console.log("Error: socket already closed!");
            // TODO: return error
        }
        return socket.readyState === 1;
        } {
            Value::Bool(bool) => bool,
            _ => panic!("invalid type"),
        };
        if ready {
            js! {
                var data = @{data};
                var socket = @{&self.js_obj};
                socket.send(data);
            };
        } else {
            let mut queued = queued.borrow_mut();
            queued.push(SocketMessage::Text(data));
        }
        Ok(())
    }

    /// Sends a binary message.
    pub fn send_binary(&mut self, data: Vec<u8>) -> Result<(), Box<Error>> {
        let queued = self.state.queued.clone();
        let ready = match js! {
            var socket = @{&self.js_obj};
            if (socket.readyState == 2 || socket.readyState == 3) {
                console.log("Error: socket already closed!");
                // TODO: return error
            }
            return socket.readyState === 1;
        } {
            Value::Bool(bool) => bool,
            _ => panic!("invalid type"),
        };
        if ready {
            let typed_array = web::TypedArray::from(&data[..]);
            let data = typed_array.buffer();
            js! {
                var data = @{data};
                var socket = @{&self.js_obj};
                socket.send(data);
            };
        } else {
            let mut queued = queued.borrow_mut();
            queued.push(SocketMessage::Binary(data));
        }

        Ok(())
    }

    /// Returns all messages that have been received since the last call to
    /// this function.
    ///
    /// Returns an `Err` if there's be an error or the Socket has been
    /// disconnected, or `Some(vec![])` if no messages have been received.
    /// If this returns `Err`, this `Socket` should no longer be used.
    pub fn recv_all(&mut self) -> Result<Vec<SocketMessage>, Box<Error>> {
        let disconnected = self.state.disconnected.borrow();
        let error = self.state.error.borrow();
        if *disconnected || *error {
            Err(Box::new(SimpleError::new(
                "Socket disconnected or there's been an error",
            )))
        } else {
            let mut received = self.state.received.borrow_mut();
            let res = received.drain(..).collect();
            Ok(res)
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl Drop for Socket {
    fn drop(&mut self) {
        js! {
            var socket = @{&self.js_obj};
            socket.close();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub struct Socket {
    client: Client<Box<NetworkStream + Send>>,
    // This is used to mark this type as !Send, to match the wasm version of this
    // struct which can't implement `Send`.
    not_send: PhantomData<*const ()>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Socket {
    /// Creates a new Socket.
    pub fn new(url: String) -> Result<Socket, Box<Error>> {
        let client = ClientBuilder::new(&url)?.connect(None)?;
        // In theory, NetworkStream *should* imply AsTcpStream, but that doesn't seem
        // to work in practice. Possibly a bug in `websocket`.
        client.stream_ref().as_tcp().set_nodelay(true)?;
        client.stream_ref().as_tcp().set_nonblocking(true)?;

        Ok(Socket {
            client,
            not_send: PhantomData,
        })
    }

    /// Sends a textual message.
    pub fn send(&mut self, data: String) -> Result<(), Box<Error>> {
        self.client
            .send_message(&message::OwnedMessage::Text(data))?;
        Ok(())
    }

    /// Sends a binary message.
    pub fn send_binary(&mut self, data: Vec<u8>) -> Result<(), Box<Error>> {
        self.client
            .send_message(&message::OwnedMessage::Binary(data))?;
        Ok(())
    }

    /// Returns all messages that have been received since the last call to
    /// this function.
    ///
    /// Returns an `Err` if there's be an error or the Socket has been
    /// disconnected, or `Some(vec![])` if no messages have been received.
    /// If this returns `Err`, this `Socket` should no longer be used.
    pub fn recv_all(&mut self) -> Result<Vec<SocketMessage>, Box<Error>> {
        let mut res = vec![];
        loop {
            match self.client.recv_message() {
                Ok(message) => {
                    match message {
                        message::OwnedMessage::Text(msg) => res.push(SocketMessage::Text(msg)),
                        message::OwnedMessage::Binary(msg) => res.push(SocketMessage::Binary(msg)),
                        message::OwnedMessage::Ping(data) => {
                            self.client
                                .send_message(&message::OwnedMessage::Pong(data))
                                .unwrap();
                        }
                        message::OwnedMessage::Close(_) => {
                            return Err(Box::new(SimpleError::new(
                                "Socket disconnected or there's been an error",
                            )));
                        }
                        other => panic!("Unsupported message type: {:?}", other),
                    };
                }
                Err(err) => match err {
                    WebSocketError::IoError(ref err) if err.kind() == ErrorKind::WouldBlock => {
                        break;
                    }
                    _ => {
                        return Err(Box::new(SimpleError::new(
                            "Socket disconnected or there's been an error",
                        )));
                    }
                },
            }
        }
        Ok(res)
    }
}
