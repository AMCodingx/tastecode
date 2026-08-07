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

fn completed_message(id: &str, text: &str, created_at: f64) -> DomainEvent {
    DomainEvent::ItemCompleted {
        item: Item {
            id: id.into(),
            turn_id: id.into(),
            item_type: ItemType::Message,
            status: ItemStatus::Completed,
            role: Some(MessageRole::Assistant),
            text: Some(text.into()),
            command: None,
            exit_code: None,
            duration_ms: None,
            path: None,
            lines_added: None,
            lines_removed: None,
            created_at,
        },
    }
}

fn git(cwd: &std::path::Path, args: &[&str]) -> String {
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
    String::from_utf8_lossy(&output.stdout).trim().into()
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
fn live_worktree_routes_refuse_unsaved_work_and_forced_discard_keeps_the_branch() {
    let repository_root = tempfile::tempdir().unwrap();
    let repository = repository_root.path().join("repo");
    let worktree_root = repository_root.path().join("worktrees");
    let output = Command::new("git")
        .args(["init", "-b", "main"])
        .arg(&repository)
        .output()
        .unwrap();
    assert!(output.status.success());
    git(&repository, &["config", "user.email", "test@example.com"]);
    git(&repository, &["config", "user.name", "Test"]);
    std::fs::write(repository.join("file.txt"), "initial\n").unwrap();
    git(&repository, &["add", "."]);
    git(&repository, &["commit", "-m", "initial"]);
    let worktree =
        harness_workspace::create_worktree(&repository, "thread-aaaaaaaaaaaa", &worktree_root)
            .unwrap();
    let worktree_path = worktree.path.clone();
    let worktree_branch = worktree.branch.clone();
    let repository_text = repository.to_string_lossy().into_owned();
    let worktree_text = worktree.path.to_string_lossy().into_owned();

    let (_directory, server) = start_test_server(None, |store| {
        store.add_project(&repository_text, None).unwrap();
        store
            .add_thread(StoreNewThread {
                id: "thread-1".into(),
                project_path: repository_text.clone(),
                provider: ProviderId::Codex,
                agent: None,
                title: "Isolated".into(),
                created_at: Some(10),
                worktree_path: Some(worktree_text.clone()),
                worktree_branch: Some(worktree_branch.clone()),
            })
            .unwrap();
    });
    let mut socket = connect_native(&server, "");
    assert_welcome(&mut socket);

    send_request(
        &mut socket,
        "clean",
        "thread.unsavedWork",
        json!({ "threadId": "thread-1" }),
    );
    assert_eq!(
        read_value(&mut socket)["result"],
        json!({ "isolated": true, "uncommitted": false })
    );
    std::fs::write(worktree_path.join("untracked.txt"), "unsaved\n").unwrap();
    send_request(
        &mut socket,
        "dirty",
        "thread.unsavedWork",
        json!({ "threadId": "thread-1" }),
    );
    assert_eq!(read_value(&mut socket)["result"]["uncommitted"], true);

    send_request(
        &mut socket,
        "refuse",
        "thread.discardWorktree",
        json!({ "threadId": "thread-1" }),
    );
    let refused = read_value(&mut socket);
    assert_eq!(refused["error"]["code"], "internal");
    assert!(
        refused["error"]["message"]
            .as_str()
            .unwrap()
            .contains("uncommitted changes")
    );
    assert!(worktree_path.exists());

    send_request(
        &mut socket,
        "discard",
        "thread.discardWorktree",
        json!({ "threadId": "thread-1", "force": true }),
    );
    assert_eq!(
        read_value(&mut socket),
        json!({ "id": "discard", "result": {} })
    );
    assert!(!worktree_path.exists());
    assert!(git(&repository, &["branch", "--list", &worktree_branch]).contains(&worktree_branch));

    send_request(
        &mut socket,
        "forgotten",
        "thread.unsavedWork",
        json!({ "threadId": "thread-1" }),
    );
    assert_eq!(
        read_value(&mut socket)["result"],
        json!({ "isolated": false, "uncommitted": false })
    );

    socket.close(None).unwrap();
    server.close().unwrap();
}

#[test]
fn startup_forgets_only_worktrees_whose_directories_are_already_gone() {
    let repository_root = tempfile::tempdir().unwrap();
    let repository = repository_root.path().join("repo");
    let worktree_root = repository_root.path().join("worktrees");
    let output = Command::new("git")
        .args(["init", "-b", "main"])
        .arg(&repository)
        .output()
        .unwrap();
    assert!(output.status.success());
    git(&repository, &["config", "user.email", "test@example.com"]);
    git(&repository, &["config", "user.name", "Test"]);
    std::fs::write(repository.join("file.txt"), "initial\n").unwrap();
    git(&repository, &["add", "."]);
    git(&repository, &["commit", "-m", "initial"]);
    let missing =
        harness_workspace::create_worktree(&repository, "thread-aaaaaaaaaaaa", &worktree_root)
            .unwrap();
    let present =
        harness_workspace::create_worktree(&repository, "thread-bbbbbbbbbbbb", &worktree_root)
            .unwrap();
    std::fs::write(present.path.join("uncommitted.txt"), "keep\n").unwrap();
    std::fs::remove_dir_all(&missing.path).unwrap();
    let repository_text = repository.to_string_lossy().into_owned();

    let (_directory, server) = start_test_server(None, |store| {
        store.add_project(&repository_text, None).unwrap();
        for (id, worktree) in [("missing", &missing), ("present", &present)] {
            store
                .add_thread(StoreNewThread {
                    id: id.into(),
                    project_path: repository_text.clone(),
                    provider: ProviderId::Codex,
                    agent: None,
                    title: id.into(),
                    created_at: Some(if id == "missing" { 10 } else { 20 }),
                    worktree_path: Some(worktree.path.to_string_lossy().into_owned()),
                    worktree_branch: Some(worktree.branch.clone()),
                })
                .unwrap();
        }
    });
    let mut socket = connect_native(&server, "");
    assert_welcome(&mut socket);
    send_request(&mut socket, "projects", "projects.list", json!({}));
    let projects = read_value(&mut socket);
    let sessions = projects["result"]["projects"][0]["sessions"]
        .as_array()
        .unwrap();
    let missing_session = sessions
        .iter()
        .find(|session| session["id"] == "missing")
        .unwrap();
    let present_session = sessions
        .iter()
        .find(|session| session["id"] == "present")
        .unwrap();
    assert!(missing_session.get("worktreeBranch").is_none());
    assert_eq!(present_session["worktreeBranch"], present.branch);
    assert!(present.path.join("uncommitted.txt").exists());

    socket.close(None).unwrap();
    server.close().unwrap();
}

#[test]
fn live_checkpoint_restore_keeps_files_and_conversation_reversible_together() {
    let repository = tempfile::tempdir().unwrap();
    git(repository.path(), &["init", "--initial-branch=main"]);
    git(
        repository.path(),
        &["config", "user.email", "test@example.com"],
    );
    git(repository.path(), &["config", "user.name", "Test"]);
    std::fs::write(repository.path().join("tracked.txt"), "original\n").unwrap();
    git(repository.path(), &["add", "."]);
    git(repository.path(), &["commit", "-m", "initial"]);
    let before = harness_workspace::take_snapshot(repository.path()).unwrap();
    std::fs::write(repository.path().join("tracked.txt"), "changed\n").unwrap();
    std::fs::write(repository.path().join("added.txt"), "temporary\n").unwrap();
    let repository_text = repository.path().to_string_lossy().into_owned();

    let (_directory, server) = start_test_server(None, |store| {
        store.add_project(&repository_text, None).unwrap();
        store
            .add_thread(StoreNewThread {
                id: "thread-1".into(),
                project_path: repository_text.clone(),
                provider: ProviderId::Codex,
                agent: None,
                title: "Restore".into(),
                created_at: Some(10),
                worktree_path: None,
                worktree_branch: None,
            })
            .unwrap();
        let checkpoint_seq = store
            .append_at(
                "thread-1",
                &completed_message("keep", "keep this", 10.0),
                10,
            )
            .unwrap();
        store
            .add_checkpoint_at(
                NewCheckpoint {
                    thread_id: "thread-1".into(),
                    seq: checkpoint_seq,
                    commit: before.commit.clone(),
                    label: "Before change".into(),
                },
                20,
            )
            .unwrap();
        store
            .append_at(
                "thread-1",
                &completed_message("tail", "temporary tail", 30.0),
                30,
            )
            .unwrap();
    });
    let mut socket = connect_native(&server, "");
    assert_welcome(&mut socket);

    send_request(
        &mut socket,
        "changed",
        "thread.changedSince",
        json!({ "threadId": "thread-1", "checkpointId": 1 }),
    );
    let mut files = read_value(&mut socket)["result"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    files.sort();
    assert_eq!(files, ["added.txt", "tracked.txt"]);

    send_request(
        &mut socket,
        "restore",
        "thread.restore",
        json!({ "threadId": "thread-1", "checkpointId": 1 }),
    );
    let restored = read_value(&mut socket);
    let undo = restored["result"]["undo"].as_str().unwrap().to_owned();
    assert_eq!(
        std::fs::read_to_string(repository.path().join("tracked.txt")).unwrap(),
        "original\n"
    );
    assert!(!repository.path().join("added.txt").exists());

    send_request(
        &mut socket,
        "truncated",
        "thread.history",
        json!({ "threadId": "thread-1" }),
    );
    assert_eq!(
        read_value(&mut socket)["result"]["events"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    send_request(
        &mut socket,
        "undo",
        "thread.undoRestore",
        json!({ "threadId": "thread-1", "undo": undo }),
    );
    assert_eq!(
        read_value(&mut socket),
        json!({ "id": "undo", "result": {} })
    );
    assert_eq!(
        std::fs::read_to_string(repository.path().join("tracked.txt")).unwrap(),
        "changed\n"
    );
    assert_eq!(
        std::fs::read_to_string(repository.path().join("added.txt")).unwrap(),
        "temporary\n"
    );
    send_request(
        &mut socket,
        "history",
        "thread.history",
        json!({ "threadId": "thread-1" }),
    );
    assert_eq!(
        read_value(&mut socket)["result"]["events"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    send_request(
        &mut socket,
        "spent",
        "thread.undoRestore",
        json!({ "threadId": "thread-1", "undo": undo }),
    );
    let spent = read_value(&mut socket);
    assert_eq!(spent["error"]["code"], "internal");
    assert_eq!(spent["error"]["message"], "restore can no longer be undone");

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
