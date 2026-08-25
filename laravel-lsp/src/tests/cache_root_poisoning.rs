//! A cache written while the project root was hijacked by a nested module
//! directory would re-pin that module as the root on every subsequent start,
//! outliving the session that created it (issue #289).
//!
//! `load_cache_data` discards such a cache and asks for a full rescan. The
//! test that matters is the *contract*: it must return the complete rescan set
//! and must **not** touch `root_path` / `initialized_root`, because
//! `initialize()` has already set those from the workspace `root_uri` before
//! `load_cache_data` is ever called — a cross-file dependency roughly 15,000
//! lines apart that nothing else pins.

use crate::LaravelLanguageServer;
use laravel_lsp::cache_manager::{CacheManager, CachedLaravelConfig, RescanType};
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tower_lsp::LspService;

fn backend() -> LaravelLanguageServer {
    let (service, _socket) = LspService::new(LaravelLanguageServer::new);
    service.inner().clone()
}

/// Workspace root with `composer.json` + `artisan` + `app/` + `resources/`,
/// and one composer-merge-plugin style module beneath it that matches the same
/// markers.
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

/// Write a real on-disk cache for `workspace_root` whose recorded Laravel root
/// is `cached_root`.
fn write_cache_with_root(workspace_root: &Path, cached_root: &Path) {
    let mut cache = CacheManager::load(workspace_root);
    cache.set_laravel_config(CachedLaravelConfig {
        root: cached_root.to_path_buf(),
        ..Default::default()
    });
    cache.save().expect("cache should persist");
}

#[tokio::test]
async fn poisoned_nested_root_is_discarded_and_leaves_root_path_untouched() {
    let (_tmp, root, module) = modular_workspace();
    write_cache_with_root(&root, &module);

    let backend = backend();
    // What `initialize()` does before `load_cache_data` runs.
    *backend.root_path.write().await = Some(root.clone());
    *backend.initialized_root.write().await = Some(root.clone());

    let rescans = backend.load_cache_data(&root).await;

    assert_eq!(
        rescans,
        vec![RescanType::Vendor, RescanType::App, RescanType::NodeModules],
        "a poisoned cache must ask for a full rescan"
    );
    assert_eq!(
        *backend.root_path.read().await,
        Some(root.clone()),
        "load_cache_data must leave the root initialize() set"
    );
    assert_eq!(
        *backend.initialized_root.read().await,
        Some(root),
        "load_cache_data must leave initialized_root alone"
    );
}

/// The discriminator is committed state, not a gitignored one: installing a
/// full dependency tree inside the module must not rescue the poisoned cache.
#[tokio::test]
async fn a_module_with_an_installed_vendor_tree_is_still_poisoned() {
    let (_tmp, root, module) = modular_workspace();
    std::fs::create_dir_all(module.join("vendor")).unwrap();
    std::fs::write(module.join("vendor/autoload.php"), "<?php").unwrap();
    write_cache_with_root(&root, &module);

    let backend = backend();
    *backend.root_path.write().await = Some(root.clone());

    let rescans = backend.load_cache_data(&root).await;

    assert_eq!(
        rescans,
        vec![RescanType::Vendor, RescanType::App, RescanType::NodeModules],
        "a vendor/ tree must not promote a module to project root"
    );
}

/// A genuinely nested project — outermost on its own path, because the
/// workspace manifest carries no Laravel markers — is legitimate and must be
/// loaded rather than discarded.
#[tokio::test]
async fn a_legitimately_nested_root_is_kept() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    // Tooling-only manifest: no artisan, no app/ + resources/ pair.
    std::fs::write(root.join("composer.json"), "{}").unwrap();

    let app = root.join("apps/web");
    std::fs::create_dir_all(app.join("app")).unwrap();
    std::fs::create_dir_all(app.join("resources")).unwrap();
    std::fs::write(app.join("composer.json"), "{}").unwrap();
    write_cache_with_root(&root, &app);

    let backend = backend();
    *backend.root_path.write().await = Some(root.clone());

    let rescans = backend.load_cache_data(&root).await;

    assert_ne!(
        rescans,
        vec![RescanType::Vendor, RescanType::App, RescanType::NodeModules],
        "a real sub-app must not be treated as a poisoned cache"
    );
    assert_eq!(
        *backend.root_path.read().await,
        Some(app),
        "a legitimate cached root is adopted"
    );
}
