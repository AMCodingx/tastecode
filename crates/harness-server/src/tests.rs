use super::*;
use harness_protocol::{DomainEvent, Item, ItemStatus, ItemType, MessageRole, ProviderId};
use harness_store::{NewCheckpoint, NewThread as StoreNewThread};
use serde_json::{Value, json};
use std::net::{IpAddr, Ipv4Addr, TcpStream};
use std::process::Command;
use std::time::Instant;
use tempfile::TempDir;
use tungstenite::client::IntoClientRequest as _;
use tungstenite::http::HeaderValue;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket, connect};

type ClientSocket = WebSocket<MaybeTlsStream<TcpStream>>;

fn start_test_server(
    access_token: Option<&str>,
    seed: impl FnOnce(&mut Store),
) -> (TempDir, ServerHandle) {
    let directory = tempfile::tempdir().unwrap();
    let store_path = directory.path().join("harness.db");
    let mut store = Store::open(&store_path).unwrap();
    seed(&mut store);
    store.close().unwrap();
    let server = start(ServerConfig {
        address: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
        access_token: access_token.map(str::to_owned),
        store_path,
    })
    .unwrap();
    (directory, server)
}

fn connect_native(server: &ServerHandle, suffix: &str) -> ClientSocket {
    let (mut socket, _) = connect(format!("ws://{}{}", server.address(), suffix)).unwrap();
    if let MaybeTlsStream::Plain(stream) = socket.get_mut() {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
    }
    socket
}

fn read_value(socket: &mut ClientSocket) -> Value {
    loop {
        match socket.read().unwrap() {
            Message::Text(text) => return serde_json::from_str(&text).unwrap(),
            Message::Ping(_) | Message::Pong(_) => continue,
            message => panic!("expected text frame, got {message:?}"),
        }
    }
}

fn send_request(socket: &mut ClientSocket, id: &str, method: &str, params: Value) {
    socket
        .send(Message::text(
            json!({ "id": id, "method": method, "params": params }).to_string(),
        ))
        .unwrap();
}

fn git(cwd: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_welcome(socket: &mut ClientSocket) {
    assert_eq!(
        read_value(socket),
        json!({
            "channel": "server.welcome",
            "sequence": 1,
            "data": { "serverVersion": "0.0.0", "protocolVersion": 2 }
        })
    );
}

#[test]
fn live_transport_sends_welcome_drops_malformed_frames_and_reports_typed_errors() {
    let (_directory, server) = start_test_server(None, |_| {});
    let mut socket = connect_native(&server, "");
    assert_welcome(&mut socket);

    socket.send(Message::text("{not-json")).unwrap();
    send_request(&mut socket, "system", "system.info", json!({}));
    let system = read_value(&mut socket);
    assert_eq!(system["id"], "system");
    assert_eq!(system["result"]["serverVersion"], "0.0.0");
    assert_eq!(system["result"]["protocolVersion"], 2);
    assert!(matches!(
        system["result"]["platform"].as_str(),
        Some("darwin" | "linux" | "win32")
    ));

    send_request(&mut socket, "unknown", "constructor", json!({}));
    assert_eq!(
        read_value(&mut socket),
        json!({
            "id": "unknown",
            "error": {
                "code": "bad_request",
                "message": "unknown method: constructor"
            }
        })
    );
    send_request(
        &mut socket,
        "invalid",
        "search.sessions",
        json!({ "query": "   " }),
    );
    let invalid = read_value(&mut socket);
    assert_eq!(invalid["id"], "invalid");
    assert_eq!(invalid["error"]["code"], "bad_request");
    assert_eq!(
        invalid["error"]["message"],
        "invalid params for search.sessions"
    );

    socket.close(None).unwrap();
    server.close().unwrap();
}

#[test]
fn live_transport_serves_existing_project_history_search_usage_and_checkpoints() {
    let (_directory, server) = start_test_server(None, |store| {
        store.add_project("/repo", Some("Repository")).unwrap();
        store
            .add_thread(StoreNewThread {
                id: "thread-1".into(),
                project_path: "/repo".into(),
                provider: ProviderId::Codex,
                agent: None,
                title: "Native core".into(),
                created_at: Some(10),
                worktree_path: None,
                worktree_branch: None,
            })
            .unwrap();
        store.touch_thread("thread-1", true, Some(20)).unwrap();
        let seq = store
            .append_at(
                "thread-1",
                &DomainEvent::ItemCompleted {
                    item: Item {
                        id: "item-1".into(),
                        turn_id: "turn-1".into(),
                        item_type: ItemType::Message,
                        status: ItemStatus::Completed,
                        role: Some(MessageRole::Assistant),
                        text: Some("durable regression marker".into()),
                        command: None,
                        exit_code: None,
                        duration_ms: None,
                        path: None,
                        lines_added: None,
                        lines_removed: None,
                        created_at: 30.0,
                    },
                },
                30,
            )
            .unwrap();
        store
            .add_checkpoint_at(
                NewCheckpoint {
                    thread_id: "thread-1".into(),
                    seq,
                    commit: "abc123".into(),
                    label: "After native core".into(),
                },
                40,
            )
            .unwrap();
        store
            .append_at(
                "thread-1",
                &DomainEvent::ThreadError {
                    thread_id: "thread-1".into(),
                    message: "provider stopped".into(),
                },
                50,
            )
            .unwrap();
    });
    let mut socket = connect_native(&server, "");
    assert_welcome(&mut socket);

    send_request(&mut socket, "projects", "projects.list", json!({}));
    let projects = read_value(&mut socket);
    assert_eq!(projects["result"]["projects"][0]["name"], "Repository");
    assert_eq!(
        projects["result"]["projects"][0]["sessions"][0]["provider"],
        "codex"
    );
    assert_eq!(
        projects["result"]["projects"][0]["sessions"][0]["status"],
        "failed"
    );

    send_request(
        &mut socket,
        "history",
        "thread.history",
        json!({ "threadId": "thread-1", "afterSeq": 0 }),
    );
    let history = read_value(&mut socket);
    assert_eq!(history["result"]["events"].as_array().unwrap().len(), 2);
    assert_eq!(
        history["result"]["events"][0]["event"]["item"]["text"],
        "durable regression marker"
    );
    assert_eq!(history["result"]["running"], false);

    send_request(
        &mut socket,
        "search",
        "search.sessions",
        json!({ "query": "regression" }),
    );
    let search = read_value(&mut socket);
    assert_eq!(search["result"]["results"].as_array().unwrap().len(), 1);
    assert_eq!(search["result"]["results"][0]["threadId"], "thread-1");

    send_request(
        &mut socket,
        "checkpoints",
        "thread.checkpoints",
        json!({ "threadId": "thread-1" }),
    );
    let checkpoints = read_value(&mut socket);
    assert_eq!(
        checkpoints["result"]["checkpoints"][0]["label"],
        "After native core"
    );

    send_request(
        &mut socket,
        "usage",
        "usage.summary",
        json!({ "threadId": "thread-1" }),
    );
    let usage = read_value(&mut socket);
    assert_eq!(usage["result"]["session"]["totalTokens"], 0.0);
    assert_eq!(usage["result"]["limits"], json!([]));

    send_request(&mut socket, "projects-again", "projects.list", json!({}));
    let projects = read_value(&mut socket);
    assert_eq!(
        projects["result"]["projects"][0]["sessions"][0]["unread"],
        false
    );
    assert_eq!(
        projects["result"]["projects"][0]["sessions"][0]["status"],
        "failed"
    );

    socket.close(None).unwrap();
    server.close().unwrap();
}

#[test]
fn sidebar_and_lifecycle_pushes_are_ordered_per_connection_before_the_response() {
    let (_directory, server) = start_test_server(None, |store| {
        store.add_project("/repo", None).unwrap();
        store
            .add_thread(StoreNewThread {
                id: "thread-1".into(),
                project_path: "/repo".into(),
                provider: ProviderId::Api,
                agent: None,
                title: "Lifecycle".into(),
                created_at: Some(10),
                worktree_path: None,
                worktree_branch: None,
            })
            .unwrap();
    });
    let mut first = connect_native(&server, "");
    let mut second = connect_native(&server, "");
    assert_welcome(&mut first);
    assert_welcome(&mut second);

    send_request(
        &mut first,
        "sidebar",
        "sidebar.updateSettings",
        json!({ "mode": "classic", "autoSettleDays": null }),
    );
    assert_eq!(
        read_value(&mut first),
        json!({
            "channel": "sidebar.settings",
            "sequence": 2,
            "data": { "mode": "classic", "autoSettleDays": null }
        })
    );
    assert_eq!(
        read_value(&mut first),
        json!({
            "id": "sidebar",
            "result": { "mode": "classic", "autoSettleDays": null }
        })
    );
    assert_eq!(
        read_value(&mut second),
        json!({
            "channel": "sidebar.settings",
            "sequence": 2,
            "data": { "mode": "classic", "autoSettleDays": null }
        })
    );

    send_request(
        &mut first,
        "settle",
        "thread.settle",
        json!({ "threadId": "thread-1" }),
    );
    let lifecycle_push = read_value(&mut first);
    assert_eq!(lifecycle_push["channel"], "thread.lifecycle");
    assert_eq!(lifecycle_push["sequence"], 3);
    assert_eq!(lifecycle_push["data"]["lifecycle"]["state"], "settled");
    let lifecycle_response = read_value(&mut first);
    assert_eq!(lifecycle_response["id"], "settle");
    assert_eq!(
        lifecycle_response["result"]["lifecycle"]["state"],
        "settled"
    );
    let second_push = read_value(&mut second);
    assert_eq!(second_push["sequence"], 3);
    assert_eq!(second_push["channel"], "thread.lifecycle");

    first.close(None).unwrap();
    second.close(None).unwrap();
    server.close().unwrap();
}

#[test]
fn live_workspace_routes_report_and_switch_only_local_branches() {
    let repository = tempfile::tempdir().unwrap();
    git(repository.path(), &["init", "--initial-branch=main"]);
    git(
        repository.path(),
        &["config", "user.email", "test@example.com"],
    );
    git(repository.path(), &["config", "user.name", "Test"]);
    std::fs::write(repository.path().join("file.txt"), "initial\n").unwrap();
    git(repository.path(), &["add", "file.txt"]);
    git(repository.path(), &["commit", "-m", "initial"]);
    git(repository.path(), &["branch", "feature/native"]);

    let (_directory, server) = start_test_server(None, |_| {});
    let mut socket = connect_native(&server, "");
    assert_welcome(&mut socket);
    let path = repository.path().to_string_lossy();

    send_request(
        &mut socket,
        "branches",
        "workspace.branches",
        json!({ "path": path }),
    );
    assert_eq!(
        read_value(&mut socket)["result"]["branches"],
        json!(["main", "feature/native"])
    );
    send_request(
        &mut socket,
        "switch",
        "workspace.switchBranch",
        json!({ "path": path, "branch": "feature/native" }),
    );
    let switched = read_value(&mut socket);
    assert_eq!(switched["result"]["branch"], "feature/native");
    assert_eq!(switched["result"]["dirtyFiles"], 0);

    send_request(
        &mut socket,
        "revision",
        "workspace.switchBranch",
        json!({ "path": path, "branch": "HEAD~1" }),
    );
    let rejected = read_value(&mut socket);
    assert_eq!(rejected["error"]["code"], "internal");
    assert!(
        rejected["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unknown local branch")
    );

    socket.close(None).unwrap();
    server.close().unwrap();
}

#[test]
fn live_handshake_closes_untrusted_origins_and_missing_access_tokens() {
    let (_directory, server) = start_test_server(Some("correct token"), |_| {});

    let mut missing = connect_native(&server, "");
    match missing.read().unwrap() {
        Message::Close(Some(frame)) => {
            assert_eq!(frame.code, CloseCode::Policy);
            assert_eq!(frame.reason, "Access denied");
        }
        frame => panic!("expected access denial, got {frame:?}"),
    }

    let mut request = format!("ws://{}/?token=correct%20token", server.address())
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("Origin", HeaderValue::from_static("https://evil.example"));
    let (mut hostile, _) = connect(request).unwrap();
    match hostile.read().unwrap() {
        Message::Close(Some(frame)) => {
            assert_eq!(frame.code, CloseCode::Policy);
            assert_eq!(frame.reason, "Origin not allowed");
        }
        frame => panic!("expected origin denial, got {frame:?}"),
    }

    let mut allowed = connect_native(&server, "/?token=correct%20token");
    assert_welcome(&mut allowed);
    allowed.close(None).unwrap();
    server.close().unwrap();
}

#[test]
fn shutdown_closes_open_clients_and_joins_connection_workers() {
    let (_directory, server) = start_test_server(None, |_| {});
    let mut socket = connect_native(&server, "");
    assert_welcome(&mut socket);

    let started = Instant::now();
    server.close().unwrap();
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(matches!(socket.read().unwrap(), Message::Close(_)));
}
