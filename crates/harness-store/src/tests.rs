use super::*;
use rusqlite::Connection;
use serde_json::json;

fn thread(id: &str, project_path: &str, provider: ProviderId) -> NewThread {
    NewThread {
        id: id.into(),
        project_path: project_path.into(),
        provider,
        agent: None,
        title: id.into(),
        created_at: None,
        worktree_path: None,
        worktree_branch: None,
    }
}

#[test]
fn projects_keep_names_pinning_and_hidden_history_owners() {
    let store = Store::memory().unwrap();
    let project = store.add_project("/home/me/work/harness", None).unwrap();
    assert_eq!(project.name, "harness");
    store.rename_project(&project.path, "My thing").unwrap();
    store.add_project(&project.path, None).unwrap();
    store.set_project_pinned(&project.path, true).unwrap();
    assert_eq!(
        store.project(&project.path).unwrap().unwrap().name,
        "My thing"
    );
    assert!(store.project(&project.path).unwrap().unwrap().pinned);

    store
        .add_thread(thread("t1", &project.path, ProviderId::Codex))
        .unwrap();
    store.remove_project(&project.path).unwrap();
    assert!(store.project(&project.path).unwrap().is_none());
    assert!(store.thread("t1").unwrap().is_some());
    store.add_project(&project.path, None).unwrap();
    assert_eq!(store.threads(Some(&project.path)).unwrap().len(), 1);
}

#[test]
fn threads_keep_provider_identity_order_and_worktree_ownership() {
    let mut store = Store::memory().unwrap();
    store.add_project("/repo", None).unwrap();
    let mut old = thread("old", "/repo", ProviderId::Acp);
    old.agent = Some("gemini".into());
    old.created_at = Some(1_000);
    store.add_thread(old).unwrap();
    let mut isolated = thread("isolated", "/repo", ProviderId::Codex);
    isolated.created_at = Some(2_000);
    isolated.worktree_path = Some("/trees/isolated".into());
    isolated.worktree_branch = Some("harness/isolated".into());
    store.add_thread(isolated).unwrap();

    assert_eq!(
        store
            .threads(Some("/repo"))
            .unwrap()
            .iter()
            .map(|thread| thread.id.as_str())
            .collect::<Vec<_>>(),
        ["isolated", "old"]
    );
    assert_eq!(
        store.thread("old").unwrap().unwrap().agent.as_deref(),
        Some("gemini")
    );
    assert_eq!(store.worktrees().unwrap().len(), 1);
    assert!(matches!(
        store.delete_thread("isolated"),
        Err(StoreError::IsolatedCheckout)
    ));
    store.forget_worktree("isolated").unwrap();
    store.delete_thread("isolated").unwrap();
    assert!(store.thread("isolated").unwrap().is_none());
}

#[test]
fn lifecycle_and_sidebar_settings_survive_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("harness.db");
    let store = Store::open(&path).unwrap();
    store.add_project("/repo", None).unwrap();
    let mut stored = thread("t1", "/repo", ProviderId::Codex);
    stored.created_at = Some(10);
    store.add_thread(stored).unwrap();
    store.snooze_thread("t1", 100, Some(20)).unwrap();
    store
        .update_sidebar_settings(SidebarSettingsUpdate {
            mode: Some(SidebarMode::Classic),
            auto_settle_days: Some(None),
        })
        .unwrap();
    store.close().unwrap();

    let reopened = Store::open(&path).unwrap();
    assert_eq!(
        reopened.thread("t1").unwrap().unwrap().lifecycle,
        ThreadLifecycle::Snoozed {
            snoozed_at: 20,
            wake_at: 100
        }
    );
    assert_eq!(
        reopened.sidebar_settings().unwrap(),
        SidebarSettings {
            mode: SidebarMode::Classic,
            auto_settle_days: None
        }
    );
    assert!(reopened.due_snoozed_threads(99).unwrap().is_empty());
    assert_eq!(reopened.due_snoozed_threads(100).unwrap()[0].id, "t1");
}

#[test]
fn lifecycle_updates_preserve_wake_and_unread_semantics() {
    let store = Store::memory().unwrap();
    store.add_project("/repo", None).unwrap();
    store
        .add_thread(thread("t1", "/repo", ProviderId::Codex))
        .unwrap();
    store
        .settle_thread("t1", SettleReason::Inactivity, Some(20))
        .unwrap();
    let active = store.touch_thread("t1", true, Some(30)).unwrap();
    assert_eq!(
        active,
        ThreadLifecycle::Active {
            keep_active: false,
            woke_at: Some(30)
        }
    );
    assert!(store.thread("t1").unwrap().unwrap().unread);
    store.set_thread_keep_active("t1", true, Some(40)).unwrap();
    assert!(store.inactive_threads(10_000).unwrap().is_empty());
    store.mark_thread_read("t1").unwrap();
    let stored = store.thread("t1").unwrap().unwrap();
    assert!(!stored.unread);
    assert_eq!(
        stored.lifecycle,
        ThreadLifecycle::Active {
            keep_active: true,
            woke_at: None
        }
    );
}

#[test]
fn design_runs_and_diff_decisions_replace_and_delete_cleanly() {
    let mut store = Store::memory().unwrap();
    store.add_project("/repo", None).unwrap();
    store
        .add_thread(thread("t1", "/repo", ProviderId::Codex))
        .unwrap();
    store
        .set_design_run("t1", &json!({ "phase": "brief" }))
        .unwrap();
    store
        .set_design_run("t1", &json!({ "phase": "brand" }))
        .unwrap();
    assert_eq!(
        store.design_run("t1").unwrap(),
        Some(json!({ "phase": "brand" }))
    );
    store
        .set_diff_decision("t1", "hunk:one", DiffDecision::Accept)
        .unwrap();
    store
        .set_diff_decision("t1", "hunk:one", DiffDecision::Reject)
        .unwrap();
    assert_eq!(
        store.diff_decision("t1", "hunk:one").unwrap(),
        Some(DiffDecision::Reject)
    );
    store.delete_thread("t1").unwrap();
    assert!(store.design_run("t1").unwrap().is_none());
    assert!(store.diff_decision("t1", "hunk:one").unwrap().is_none());
}

#[test]
fn opening_an_old_database_adds_new_columns_without_losing_rows() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("old.db");
    let old = Connection::open(&path).unwrap();
    old.execute_batch(
        "CREATE TABLE projects (path TEXT PRIMARY KEY, name TEXT NOT NULL, created_at INTEGER NOT NULL);
         CREATE TABLE threads (
           id TEXT PRIMARY KEY, project_path TEXT NOT NULL, provider TEXT NOT NULL,
           agent TEXT, title TEXT NOT NULL, created_at INTEGER NOT NULL, closed_at INTEGER);
         CREATE TABLE events (
           seq INTEGER PRIMARY KEY AUTOINCREMENT, thread_id TEXT NOT NULL,
           at INTEGER NOT NULL, payload TEXT NOT NULL);
         INSERT INTO projects VALUES ('/repo', 'Old project', 1);
         INSERT INTO threads VALUES ('t1', '/repo', 'codex', NULL, 'Old session', 1, NULL);",
    )
    .unwrap();
    old.close().unwrap();

    let migrated = Store::open(&path).unwrap();
    assert_eq!(
        migrated.project("/repo").unwrap().unwrap().name,
        "Old project"
    );
    let thread = migrated.thread("t1").unwrap().unwrap();
    assert_eq!(thread.title, "Old session");
    assert_eq!(
        thread.lifecycle,
        ThreadLifecycle::Active {
            keep_active: false,
            woke_at: None
        }
    );
    migrated.set_project_pinned("/repo", true).unwrap();
    migrated.set_thread_pinned("t1", true).unwrap();
    assert!(migrated.project("/repo").unwrap().unwrap().pinned);
    assert!(migrated.thread("t1").unwrap().unwrap().pinned);
}

#[test]
fn missing_thread_lifecycle_mutations_fail_loudly() {
    let store = Store::memory().unwrap();
    assert!(matches!(
        store.settle_thread("missing", SettleReason::Manual, Some(1)),
        Err(StoreError::ThreadNotFound)
    ));
    assert!(matches!(
        store.mark_thread_read("missing"),
        Err(StoreError::ThreadNotFound)
    ));
}
