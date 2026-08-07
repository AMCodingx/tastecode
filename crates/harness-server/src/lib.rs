//! Native Personal Harness core transport.
//!
//! The server owns persistence and protocol ordering. Provider-specific behavior
//! stays behind adapters as it is ported; this transport never branches on a
//! provider name.

mod access;
mod inbox;
mod push;
mod router;

pub use access::{allowed_origin, assert_safe_bind, has_access};
pub use router::SERVER_VERSION;

use harness_protocol::{Push, Request, Response, channel};
use harness_store::Store;
use push::{PendingPush, PushBus};
use serde_json::Value;
use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use thiserror::Error;
use tungstenite::handshake::server::{Request as HandshakeRequest, Response as HandshakeResponse};
use tungstenite::protocol::CloseFrame;
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::{Error as WebSocketError, Message, WebSocket, accept_hdr};

pub const DEFAULT_PORT: u16 = 4311;
const IO_POLL_INTERVAL: Duration = Duration::from_millis(40);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub address: SocketAddr,
    pub access_token: Option<String>,
    pub store_path: PathBuf,
}

impl ServerConfig {
    pub fn loopback(store_path: impl Into<PathBuf>) -> Self {
        Self {
            address: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), DEFAULT_PORT),
            access_token: None,
            store_path: store_path.into(),
        }
    }

    pub fn from_environment() -> Result<Self, ServerError> {
        let host = std::env::var("HARNESS_HOST").unwrap_or_else(|_| "127.0.0.1".into());
        let host = host
            .parse::<IpAddr>()
            .map_err(|_| ServerError::InvalidHost(host))?;
        let port = std::env::var("HARNESS_PORT")
            .ok()
            .map(|value| {
                value
                    .parse::<u16>()
                    .map_err(|_| ServerError::InvalidPort(value))
            })
            .transpose()?
            .unwrap_or(DEFAULT_PORT);
        Ok(Self {
            address: SocketAddr::new(host, port),
            access_token: std::env::var("HARNESS_ACCESS_TOKEN").ok(),
            store_path: store_location()?,
        })
    }
}

#[derive(Debug, Error)]
pub enum ServerError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Store(#[from] harness_store::StoreError),
    #[error("{0}")]
    UnsafeBind(&'static str),
    #[error("HARNESS_HOST must be a literal IP address: {0}")]
    InvalidHost(String),
    #[error("HARNESS_PORT must be between 0 and 65535: {0}")]
    InvalidPort(String),
    #[error("the operating system did not provide a per-user data directory")]
    MissingDataDirectory,
    #[error("the Harness server thread panicked")]
    ThreadPanicked,
}

pub struct ServerHandle {
    address: SocketAddr,
    state: Arc<ServerState>,
    join: Option<JoinHandle<()>>,
}

impl ServerHandle {
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn close(mut self) -> Result<(), ServerError> {
        self.stop()
    }

    fn stop(&mut self) -> Result<(), ServerError> {
        self.state.shutdown.store(true, Ordering::Release);
        if let Some(join) = self.join.take() {
            join.join().map_err(|_| ServerError::ThreadPanicked)?;
        }
        Ok(())
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

pub fn start(config: ServerConfig) -> Result<ServerHandle, ServerError> {
    assert_safe_bind(config.address.ip(), config.access_token.as_deref())
        .map_err(ServerError::UnsafeBind)?;
    let listener = TcpListener::bind(config.address)?;
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?;
    let store = Store::open(&config.store_path)?;
    recover_worktree_metadata(&store);
    let state = Arc::new(ServerState {
        store: Mutex::new(store),
        inbox: Mutex::new(inbox::InboxProjections::default()),
        push: PushBus::new(),
        shutdown: AtomicBool::new(false),
        access_token: config.access_token,
    });
    let listener_state = Arc::clone(&state);
    let join = thread::Builder::new()
        .name("harness-server".into())
        .spawn(move || run_listener(listener, listener_state))?;
    Ok(ServerHandle {
        address,
        state,
        join: Some(join),
    })
}

fn recover_worktree_metadata(store: &Store) {
    let Ok(worktrees) = store.worktrees() else {
        return;
    };
    let repositories = worktrees
        .iter()
        .map(|worktree| PathBuf::from(&worktree.repo_path))
        .collect::<BTreeSet<_>>();
    for repository in repositories {
        harness_workspace::prune_worktrees(repository);
    }
    for worktree in worktrees {
        if !std::path::Path::new(&worktree.path).exists() {
            let _ = store.forget_worktree(&worktree.thread_id);
        }
    }
}

pub fn store_location() -> Result<PathBuf, ServerError> {
    if let Some(override_path) = std::env::var_os("HARNESS_DATA_DIR") {
        return Ok(PathBuf::from(override_path).join("harness.db"));
    }
    dirs::data_dir()
        .map(|path| path.join("PersonalHarness").join("harness.db"))
        .ok_or(ServerError::MissingDataDirectory)
}

pub(crate) struct ServerState {
    store: Mutex<Store>,
    inbox: Mutex<inbox::InboxProjections>,
    push: PushBus,
    shutdown: AtomicBool,
    access_token: Option<String>,
}

fn run_listener(listener: TcpListener, state: Arc<ServerState>) {
    let mut workers = Vec::new();
    while !state.shutdown.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _peer)) => {
                let worker_state = Arc::clone(&state);
                match thread::Builder::new()
                    .name("harness-connection".into())
                    .spawn(move || handle_connection(stream, worker_state))
                {
                    Ok(worker) => workers.push(worker),
                    Err(error) => eprintln!("[server] failed to start connection: {error}"),
                }
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(IO_POLL_INTERVAL);
            }
            Err(error) => {
                eprintln!("[server] listener failed: {error}");
                break;
            }
        }
        let mut index = 0;
        while index < workers.len() {
            if workers[index].is_finished() {
                let worker = workers.swap_remove(index);
                let _ = worker.join();
            } else {
                index += 1;
            }
        }
    }
    state.shutdown.store(true, Ordering::Release);
    for worker in workers {
        let _ = worker.join();
    }
}

fn handle_connection(stream: TcpStream, state: Arc<ServerState>) {
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT));
    let _ = stream.set_write_timeout(Some(HANDSHAKE_TIMEOUT));

    let mut denial = None;
    let socket = accept_hdr(
        stream,
        |request: &HandshakeRequest, response: HandshakeResponse| {
            let origin_allowed = match request.headers().get("origin") {
                None => true,
                Some(value) => value
                    .to_str()
                    .ok()
                    .is_some_and(|origin| allowed_origin(Some(origin))),
            };
            if !origin_allowed {
                denial = Some("Origin not allowed");
            } else if !has_access(&request.uri().to_string(), state.access_token.as_deref()) {
                denial = Some("Access denied");
            }
            Ok(response)
        },
    );
    let Ok(mut socket) = socket else {
        return;
    };
    if let Some(reason) = denial {
        let _ = socket.close(Some(CloseFrame {
            code: CloseCode::Policy,
            reason: reason.into(),
        }));
        return;
    }
    let _ = socket.get_mut().set_read_timeout(Some(IO_POLL_INTERVAL));

    let (connection_id, pushes) = state.push.add();
    let _ = state
        .push
        .send(connection_id, channel::SERVER_WELCOME, router::welcome());
    run_connection(&mut socket, &state, connection_id, pushes);
    state.push.remove(connection_id);
}

fn run_connection(
    socket: &mut WebSocket<TcpStream>,
    state: &ServerState,
    connection_id: u64,
    pushes: Receiver<PendingPush>,
) {
    let mut sequence = 0_u64;
    loop {
        if state.shutdown.load(Ordering::Acquire) {
            let _ = socket.close(None);
            return;
        }
        if !flush_pushes(socket, &pushes, &mut sequence) {
            return;
        }
        match socket.read() {
            Ok(Message::Text(text)) => {
                if !handle_request(socket, state, connection_id, &pushes, &mut sequence, &text) {
                    return;
                }
            }
            Ok(Message::Binary(bytes)) => {
                if let Ok(text) = std::str::from_utf8(&bytes)
                    && !handle_request(socket, state, connection_id, &pushes, &mut sequence, text)
                {
                    return;
                }
            }
            Ok(Message::Ping(_) | Message::Pong(_)) => {
                if socket.flush().is_err() {
                    return;
                }
            }
            Ok(Message::Close(_) | Message::Frame(_)) => return,
            Err(error) if is_read_timeout(&error) => {}
            Err(_) => return,
        }
    }
}

fn handle_request(
    socket: &mut WebSocket<TcpStream>,
    state: &ServerState,
    connection_id: u64,
    pushes: &Receiver<PendingPush>,
    sequence: &mut u64,
    text: &str,
) -> bool {
    let Ok(request) = serde_json::from_str::<Request<Value>>(text) else {
        return true;
    };
    let response = match router::route(state, connection_id, &request.method, request.params) {
        Ok(result) => Response::Success {
            id: request.id,
            result,
        },
        Err(error) => Response::Failure {
            id: request.id,
            error: error.0,
        },
    };

    // Routes enqueue their pushes synchronously. Flush them first to retain the
    // established push-before-response ordering of the TypeScript server.
    flush_pushes(socket, pushes, sequence) && send_json(socket, &response)
}

fn flush_pushes(
    socket: &mut WebSocket<TcpStream>,
    pushes: &Receiver<PendingPush>,
    sequence: &mut u64,
) -> bool {
    loop {
        match pushes.try_recv() {
            Ok(pending) => {
                *sequence = sequence.saturating_add(1);
                if !send_json(
                    socket,
                    &Push {
                        channel: pending.channel,
                        sequence: *sequence,
                        data: pending.data,
                    },
                ) {
                    return false;
                }
            }
            Err(TryRecvError::Empty) => return true,
            Err(TryRecvError::Disconnected) => return false,
        }
    }
}

fn send_json<T: serde::Serialize>(socket: &mut WebSocket<TcpStream>, value: &T) -> bool {
    serde_json::to_string(value)
        .ok()
        .is_some_and(|text| socket.send(Message::text(text)).is_ok())
}

fn is_read_timeout(error: &WebSocketError) -> bool {
    matches!(
        error,
        WebSocketError::Io(error)
            if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
    )
}

#[cfg(test)]
mod tests;
