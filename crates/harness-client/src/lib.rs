//! Native Harness protocol transport.
//!
//! The transport owns reconnect and sequence tracking, while callers own
//! application-level resync. No provider behavior belongs in this crate.

use async_channel::{Receiver as EventReceiver, Sender as EventSender};
use harness_protocol::{InboundFrame, Push, Request, Response};
use serde::Serialize;
use serde_json::Value;
use std::collections::VecDeque;
use std::io::ErrorKind;
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Error as WebSocketError, Message, WebSocket, connect};
use url::Url;

const READ_POLL_INTERVAL: Duration = Duration::from_millis(40);
const INITIAL_RECONNECT_DELAY: Duration = Duration::from_millis(100);
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint(String);

impl Endpoint {
    pub fn loopback(port: u16) -> Self {
        Self(format!("ws://127.0.0.1:{port}"))
    }

    pub fn parse(value: &str) -> Result<Self, ClientError> {
        let url = Url::parse(value).map_err(|_| ClientError::InvalidEndpoint)?;
        if url.scheme() != "ws"
            || url.host_str() != Some("127.0.0.1")
            || url.port().is_none()
            || url.username() != ""
            || url.password().is_some()
        {
            return Err(ClientError::InvalidEndpoint);
        }
        Ok(Self(url.to_string()))
    }

    pub fn from_environment() -> Result<Self, ClientError> {
        match std::env::var("HARNESS_SERVER_URL") {
            Ok(value) => Self::parse(&value),
            Err(_) => Ok(Self::loopback(4311)),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    Connecting,
    Open,
    Reconnecting,
    Closed,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClientEvent {
    StateChanged(ConnectionState),
    Response(Response<Value>),
    Push(Push<Value>),
    SequenceGap { expected: u64, received: u64 },
    DecodeFailed { reason: String },
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("the Harness server URL must be ws://127.0.0.1:<port>")]
    InvalidEndpoint,
    #[error("the Harness transport is closed")]
    Closed,
    #[error("request serialization failed")]
    Serialize(#[source] serde_json::Error),
    #[error("failed to start the Harness transport")]
    Start(#[source] std::io::Error),
}

#[derive(Clone)]
pub struct ClientHandle {
    inner: Arc<ClientInner>,
}

struct ClientInner {
    commands: Sender<Command>,
    next_id: AtomicU64,
}

impl Drop for ClientInner {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
    }
}

enum Command {
    Send(String),
    Shutdown,
}

impl ClientHandle {
    pub fn start(endpoint: Endpoint) -> Result<(Self, EventReceiver<ClientEvent>), ClientError> {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = async_channel::unbounded();
        thread::Builder::new()
            .name("harness-protocol".into())
            .spawn(move || run_transport(endpoint, command_rx, event_tx))
            .map_err(ClientError::Start)?;

        Ok((
            Self {
                inner: Arc::new(ClientInner {
                    commands: command_tx,
                    next_id: AtomicU64::new(1),
                }),
            },
            event_rx,
        ))
    }

    pub fn request<P: Serialize>(&self, method: &str, params: P) -> Result<String, ClientError> {
        let number = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let id = format!("native-{number}");
        let request = Request::new(id.clone(), method, params);
        let text = serde_json::to_string(&request).map_err(ClientError::Serialize)?;
        self.inner
            .commands
            .send(Command::Send(text))
            .map_err(|_| ClientError::Closed)?;
        Ok(id)
    }
}

fn run_transport(
    endpoint: Endpoint,
    commands: Receiver<Command>,
    events: EventSender<ClientEvent>,
) {
    let mut pending = VecDeque::new();
    let mut state = ConnectionState::Connecting;
    let mut reconnect_delay = INITIAL_RECONNECT_DELAY;
    emit(&events, ClientEvent::StateChanged(state));

    loop {
        if drain_commands(&commands, &mut pending) {
            break;
        }

        match connect(endpoint.as_str()) {
            Ok((mut socket, _response)) => {
                configure_socket(&mut socket);
                state = ConnectionState::Open;
                reconnect_delay = INITIAL_RECONNECT_DELAY;
                emit(&events, ClientEvent::StateChanged(state));
                if run_connection(&mut socket, &commands, &events, &mut pending) {
                    break;
                }
            }
            Err(_error) => {}
        }

        if state != ConnectionState::Reconnecting {
            state = ConnectionState::Reconnecting;
            emit(&events, ClientEvent::StateChanged(state));
        }
        if collect_during_backoff(&commands, &mut pending, reconnect_delay) {
            break;
        }
        reconnect_delay = (reconnect_delay * 2).min(MAX_RECONNECT_DELAY);
    }

    emit(&events, ClientEvent::StateChanged(ConnectionState::Closed));
}

fn configure_socket(socket: &mut WebSocket<MaybeTlsStream<TcpStream>>) {
    if let MaybeTlsStream::Plain(stream) = socket.get_mut() {
        let _ = stream.set_nodelay(true);
        let _ = stream.set_read_timeout(Some(READ_POLL_INTERVAL));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    }
}

/// Returns true when the transport should shut down rather than reconnect.
fn run_connection(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    commands: &Receiver<Command>,
    events: &EventSender<ClientEvent>,
    pending: &mut VecDeque<String>,
) -> bool {
    let mut sequence = SequenceTracker::default();

    loop {
        match commands.try_recv() {
            Ok(Command::Send(text)) => pending.push_back(text),
            Ok(Command::Shutdown) | Err(TryRecvError::Disconnected) => {
                let _ = socket.close(None);
                return true;
            }
            Err(TryRecvError::Empty) => {}
        }

        while let Some(text) = pending.pop_front() {
            if socket.send(Message::text(text.clone())).is_err() {
                pending.push_front(text);
                return false;
            }
        }

        match socket.read() {
            Ok(Message::Text(text)) => decode_frame(text.as_str(), events, &mut sequence),
            Ok(Message::Close(_)) => return false,
            Ok(Message::Ping(_) | Message::Pong(_)) => {
                let _ = socket.flush();
            }
            Ok(Message::Binary(_) | Message::Frame(_)) => emit(
                events,
                ClientEvent::DecodeFailed {
                    reason: "expected a text WebSocket frame".into(),
                },
            ),
            Err(error) if is_read_timeout(&error) => {}
            Err(_) => return false,
        }
    }
}

fn decode_frame(text: &str, events: &EventSender<ClientEvent>, sequence: &mut SequenceTracker) {
    match serde_json::from_str::<InboundFrame>(text) {
        Ok(InboundFrame::Response(response)) => emit(events, ClientEvent::Response(response)),
        Ok(InboundFrame::Push(push)) => {
            if let Some((expected, received)) = sequence.observe(push.sequence) {
                emit(events, ClientEvent::SequenceGap { expected, received });
            }
            emit(events, ClientEvent::Push(push));
        }
        Err(error) => emit(
            events,
            ClientEvent::DecodeFailed {
                reason: error.to_string(),
            },
        ),
    }
}

fn is_read_timeout(error: &WebSocketError) -> bool {
    matches!(
        error,
        WebSocketError::Io(io_error)
            if matches!(io_error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
    )
}

fn drain_commands(commands: &Receiver<Command>, pending: &mut VecDeque<String>) -> bool {
    loop {
        match commands.try_recv() {
            Ok(Command::Send(text)) => pending.push_back(text),
            Ok(Command::Shutdown) | Err(TryRecvError::Disconnected) => return true,
            Err(TryRecvError::Empty) => return false,
        }
    }
}

fn collect_during_backoff(
    commands: &Receiver<Command>,
    pending: &mut VecDeque<String>,
    delay: Duration,
) -> bool {
    let deadline = Instant::now() + delay;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        match commands.recv_timeout(remaining) {
            Ok(Command::Send(text)) => pending.push_back(text),
            Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => return true,
            Err(RecvTimeoutError::Timeout) => return false,
        }
    }
}

fn emit(events: &EventSender<ClientEvent>, event: ClientEvent) {
    let _ = events.send_blocking(event);
}

#[derive(Default)]
struct SequenceTracker {
    last: Option<u64>,
}

impl SequenceTracker {
    fn observe(&mut self, sequence: u64) -> Option<(u64, u64)> {
        let gap = self
            .last
            .and_then(|last| (sequence != last.saturating_add(1)).then_some((last + 1, sequence)));
        self.last = Some(sequence);
        gap
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn endpoint_is_explicitly_loopback_only() {
        assert!(Endpoint::parse("ws://127.0.0.1:4311").is_ok());
        assert!(Endpoint::parse("ws://localhost:4311").is_err());
        assert!(Endpoint::parse("ws://192.168.1.20:4311").is_err());
        assert!(Endpoint::parse("wss://127.0.0.1:4311").is_err());
        assert!(Endpoint::parse("ws://127.0.0.1").is_err());
    }

    #[test]
    fn sequence_tracker_reports_duplicates_and_gaps() {
        let mut tracker = SequenceTracker::default();
        assert_eq!(tracker.observe(1), None);
        assert_eq!(tracker.observe(2), None);
        assert_eq!(tracker.observe(4), Some((3, 4)));
        assert_eq!(tracker.observe(4), Some((5, 4)));
    }

    #[test]
    fn request_ids_are_stable_and_monotonic_before_connection() {
        let endpoint = Endpoint::loopback(9);
        let (client, _events) = ClientHandle::start(endpoint).unwrap();
        let first = client.request("system.info", json!({})).unwrap();
        let second = client.request("projects.list", json!({})).unwrap();
        assert_eq!(first, "native-1");
        assert_eq!(second, "native-2");
    }
}
