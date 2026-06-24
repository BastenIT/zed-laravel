//! Tests for the fail-closed root-containment guard in
//! `LaravelLanguageServer::create_translation_location_from_salsa` (issue #248) —
//! the namespaced-key goto branch, a sibling of the #130 → #143 → #148 → #199
//! containment-guard chain.
//!
//! Goto-definition on a namespaced translation key (`namespace::file.key`)
//! resolves the namespace to a lang directory — the merged vendor/app
//! registration map when present, else the published `lang/vendor/<namespace>/`
//! fallback — then probes `<lang_dir>/en/<file>.php` and hands the client a
//! navigation target. Both the fallback `lang_dir` (the raw `namespace` from the
//! key) and the appended file segment are user-controlled text after `::`, so a
//! `..` sequence could resolve the target outside the project root. The guard
//! refuses such a target with the fail-closed `path_within_root` — the same check
//! every sibling goto handler applies — so the containment invariant holds
//! uniformly on the translation goto path too.
//!
//! These tests drive the private async method directly via
//! `tower_lsp::LspService` / `inner()`, mirroring the component-navigation
//! containment tests.

use crate::LaravelLanguageServer;
use laravel_lsp::salsa_impl::TranslationReferenceData;
use std::fs;
use std::path::Path;
use tower_lsp::LspService;

/// Build a server instance for testing. `LspService::new` wires up a real
/// `Client`; `inner()` hands back the `LaravelLanguageServer` so we can call its
/// private methods directly.
fn test_server() -> LaravelLanguageServer {
    let (service, _socket) = LspService::new(LaravelLanguageServer::new);
    service.inner().clone()
}

/// Set the server root so `create_translation_location_from_salsa` resolves the
/// project root. No `app/Providers/` or `vendor/` is seeded, so the namespace
/// map is empty and resolution takes the `lang/vendor/<namespace>/` fallback —
/// the branch where a `..`-laden namespace escapes the root.
async fn set_root(server: &LaravelLanguageServer, root: &Path) {
    *server.root_path.write().await = Some(root.to_path_buf());
}

/// A translation reference for `key`; the cursor coordinates are irrelevant to
/// containment, only the key drives namespace/file resolution.
fn trans_ref(key: &str) -> TranslationReferenceData {
    TranslationReferenceData {
        key: key.to_string(),
        line: 0,
        column: 0,
        end_column: 0,
    }
}

#[tokio::test]
async fn out_of_root_namespaced_target_returns_none() {
    // The namespace climbs out of the project root via `..`, and the target file
    // genuinely exists at the escaped location — so a `None` result can only come
    // from the containment guard, never a missing file. `lang/vendor/../../../evil`
    // resolves to `<tmp>/evil`, a sibling of the project root.
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("project");
    fs::create_dir_all(&root).unwrap();

    // Materialize the escaped target on disk: <tmp>/evil/en/messages.php.
    let evil = tmp.path().join("evil/en");
    fs::create_dir_all(&evil).unwrap();
    fs::write(evil.join("messages.php"), "<?php return ['x' => 'pwned'];").unwrap();

    let server = test_server();
    set_root(&server, &root).await;

    let result = server
        .create_translation_location_from_salsa(&trans_ref("../../../evil::messages.x"))
        .await;

    assert!(
        result.is_none(),
        "a namespaced goto target that escapes the project root must never resolve, \
         even though the file exists on disk — the fail-closed containment guard \
         refuses it"
    );
}

#[tokio::test]
async fn in_root_namespaced_target_still_resolves() {
    // Positive control: a published namespaced key resolves through the
    // `lang/vendor/<namespace>/` fallback to a real in-root file, passing the
    // guard's allow branch — proving the new check doesn't regress in-root goto.
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("project");
    let published = root.join("lang/vendor/shop/en");
    fs::create_dir_all(&published).unwrap();
    fs::write(
        published.join("messages.php"),
        "<?php return ['greeting' => 'hi'];",
    )
    .unwrap();

    let server = test_server();
    set_root(&server, &root).await;

    let result = server
        .create_translation_location_from_salsa(&trans_ref("shop::messages.greeting"))
        .await;

    assert!(
        result.is_some(),
        "an in-root published namespaced key must resolve through the guard's allow \
         branch to its lang/vendor file"
    );
}
