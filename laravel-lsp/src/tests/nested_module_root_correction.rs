//! A project root that has drifted onto a nested module must be pulled back
//! out the next time a file is opened — not stay stuck for the session.
//!
//! `try_discover_from_file` historically had no arm for this. A discovered
//! root that *encloses* the active one is neither "outside the current root"
//! nor "more specific than it", so both guards fell through to "keep current"
//! and a too-deep root survived every subsequent file open. The cache-poison
//! guard could only unstick it at startup; within a session nothing could.
//!
//! Same family as `self_corrects_from_linked_worktree_back_to_main` in
//! `laravel_root_worktree_discovery.rs` — a wrong root is sticky unless
//! something is allowed to correct upward (issue #289).

use crate::LaravelLanguageServer;
use std::path::PathBuf;
use tempfile::TempDir;
use tower_lsp::LspService;

fn backend() -> LaravelLanguageServer {
    let (service, _socket) = LspService::new(LaravelLanguageServer::new);
    service.inner().clone()
}

/// Workspace root plus one composer-merge-plugin style module beneath it that
/// matches the same markers.
fn modular_workspace() -> (TempDir, PathBuf, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::write(root.join("composer.json"), "{}").unwrap();
    std::fs::write(root.join("artisan"), "").unwrap();
    std::fs::create_dir_all(root.join("app")).unwrap();
    std::fs::create_dir_all(root.join("resources")).unwrap();

    let module = root.join("app/Legal/GuaranteeLabel");
    std::fs::create_dir_all(module.join("app")).unwrap();
    std::fs::create_dir_all(module.join("resources")).unwrap();
    std::fs::write(module.join("composer.json"), "{}").unwrap();
    (tmp, root, module)
}

#[tokio::test]
async fn corrects_a_root_that_has_drifted_onto_a_nested_module() {
    let (_tmp, root, module) = modular_workspace();
    let file = module.join("app/Service.php");
    std::fs::write(&file, "<?php\n").unwrap();

    let backend = backend();
    *backend.workspace_root.write().await = Some(root.clone());
    // The drifted state: root_path sitting on the module, as an older build
    // (or a poisoned cache) would leave it.
    *backend.root_path.write().await = Some(module.clone());

    backend.try_discover_from_file(&file).await;

    assert_eq!(
        *backend.root_path.read().await,
        Some(root),
        "opening a module file must pull the root back to the workspace"
    );
}

/// The correction is gated on the enclosing candidate actually being the
/// outermost project. With no workspace there is no fence to judge against, so
/// it must fail closed and leave the root alone rather than guess.
#[tokio::test]
async fn does_not_correct_upward_without_a_workspace_fence() {
    let (_tmp, _root, module) = modular_workspace();
    let file = module.join("app/Service.php");
    std::fs::write(&file, "<?php\n").unwrap();

    let backend = backend();
    // No workspace_root set — the fence is absent.
    *backend.root_path.write().await = Some(module.clone());

    backend.try_discover_from_file(&file).await;

    assert_eq!(
        *backend.root_path.read().await,
        Some(module),
        "without a fence the root must be left exactly as it was"
    );
}

/// A legitimately nested sub-app — outermost on its own path because the
/// workspace manifest carries no Laravel markers — must not be corrected
/// upward to the workspace.
#[tokio::test]
async fn does_not_correct_a_legitimate_sub_app_upward() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::write(root.join("composer.json"), "{}").unwrap();

    let app = root.join("apps/web");
    std::fs::create_dir_all(app.join("app")).unwrap();
    std::fs::create_dir_all(app.join("resources")).unwrap();
    std::fs::write(app.join("composer.json"), "{}").unwrap();
    let file = app.join("app/Service.php");
    std::fs::write(&file, "<?php\n").unwrap();

    let backend = backend();
    *backend.workspace_root.write().await = Some(root);
    *backend.root_path.write().await = Some(app.clone());

    backend.try_discover_from_file(&file).await;

    assert_eq!(
        *backend.root_path.read().await,
        Some(app),
        "a real sub-app is already the outermost project on its path"
    );
}
