//! Namespaced translation-key completion (`ns::file.key`).
//!
//! `get_all_translation_keys` used to enumerate only the project's own root
//! `lang/` catalogue, so a project keeping a namespace's translations only
//! under its registered directory (vendor package, module, or an app
//! `loadTranslationsFrom` call — never published to `lang/vendor/…`) got
//! zero completions for that namespace's keys. It now also walks every
//! registered namespace — the same map hover/goto/diagnostics already share
//! via `vendor_translation_namespaces_for` — emitting `{ns}::{file}.{key}`.
//!
//! These tests drive the private async method directly on a server built
//! through the `tower_lsp::LspService` harness, priming `root_path` and
//! `vendor_translation_namespaces` so the scan runs purely against a tempdir
//! (same pattern as `translation_namespace_navigation.rs`).

use crate::LaravelLanguageServer;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tower_lsp::LspService;

/// Build a backend with the project root and the namespace map primed, so
/// `vendor_translation_namespaces_for` returns the cache without scanning
/// disk.
async fn backend_with_namespaces(
    root: &Path,
    namespaces: HashMap<String, PathBuf>,
) -> LaravelLanguageServer {
    let (service, _socket) = LspService::new(LaravelLanguageServer::new);
    let backend = service.inner().clone();
    *backend.root_path.write().await = Some(root.to_path_buf());
    *backend.vendor_translation_namespaces.write().await = Some(Arc::new(namespaces));
    backend
}

/// Write a one-key catalogue at `<lang_dir>/en/<file>.php`. `key` may be
/// dotted (`"details.title"`) — each segment becomes its own nested,
/// one-per-line PHP array level, matching how Laravel catalogues actually
/// declare nested keys AND the shape `parse_translation_keys`'s line-based
/// scan expects (it tracks array open/close per line, not a literal dotted
/// string key).
fn write_catalogue(lang_dir: &Path, file: &str, key: &str, value: &str) {
    let dir = lang_dir.join("en");
    fs::create_dir_all(&dir).unwrap();

    let segments: Vec<&str> = key.split('.').collect();
    let mut source = String::from("<?php\nreturn [\n");
    for (depth, segment) in segments.iter().enumerate() {
        let indent = "    ".repeat(depth + 1);
        if depth + 1 == segments.len() {
            source.push_str(&format!("{indent}'{segment}' => '{value}',\n"));
        } else {
            source.push_str(&format!("{indent}'{segment}' => [\n"));
        }
    }
    for depth in (0..segments.len().saturating_sub(1)).rev() {
        let indent = "    ".repeat(depth + 1);
        source.push_str(&format!("{indent}],\n"));
    }
    source.push_str("];\n");

    fs::write(dir.join(format!("{file}.php")), source).unwrap();
}

#[tokio::test]
async fn namespaced_lang_dir_yields_prefixed_completions_alongside_root() {
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path().to_path_buf();

    // The project's own root catalogue — must still be scanned.
    write_catalogue(&root.join("lang"), "messages", "welcome", "Welcome");

    // A namespace whose catalogues live only under its registered dir,
    // never published to `lang/vendor/…` (the fixture shape:
    // legal-contractmanagement::contract-management.details.title).
    let ns_lang_dir = root.join("app/Legal/ContractManagement/lang");
    write_catalogue(
        &ns_lang_dir,
        "contract-management",
        "details.title",
        "Vertragsdetails",
    );
    let mut namespaces = HashMap::new();
    namespaces.insert("legal-contractmanagement".to_string(), ns_lang_dir);

    let backend = backend_with_namespaces(&root, namespaces).await;
    let keys = backend.get_all_translation_keys().await;
    let all_keys: Vec<&str> = keys.iter().map(|k| k.key.as_str()).collect();

    assert!(
        all_keys.contains(&"messages.welcome"),
        "root catalogue should still be scanned, got {all_keys:?}"
    );
    assert!(
        all_keys.contains(&"legal-contractmanagement::contract-management.details.title"),
        "namespaced catalogue should be scanned and key-prefixed, got {all_keys:?}"
    );
}

#[tokio::test]
async fn namespace_only_project_still_completes() {
    // No root `lang/` at all — a project whose ONLY catalogues live under a
    // registered namespace used to get zero completions.
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path().to_path_buf();

    let ns_lang_dir = root.join("app/Legal/ContractManagement/lang");
    write_catalogue(
        &ns_lang_dir,
        "contract-management",
        "details.title",
        "Vertragsdetails",
    );
    let mut namespaces = HashMap::new();
    namespaces.insert("legal-contractmanagement".to_string(), ns_lang_dir);

    let backend = backend_with_namespaces(&root, namespaces).await;
    let keys = backend.get_all_translation_keys().await;
    let all_keys: Vec<&str> = keys.iter().map(|k| k.key.as_str()).collect();

    assert_eq!(keys.len(), 1, "got {all_keys:?}");
    assert_eq!(
        keys[0].key,
        "legal-contractmanagement::contract-management.details.title"
    );
    assert!(
        keys[0].source.starts_with("legal-contractmanagement::"),
        "namespaced source label should carry the namespace too, got {:?}",
        keys[0].source
    );
}

#[test]
fn translation_call_context_passes_namespace_prefix_through_unmangled() {
    // The `::` in a namespaced prefix must survive into `StringContext.prefix`
    // verbatim — the completion filter does a plain `key.starts_with(prefix)`,
    // so any mangling here would silently break namespaced completion.
    let line = "__('legal-contractmanagement::contract-management.details.";
    let ctx = LaravelLanguageServer::get_translation_call_context(line, line.len() as u32)
        .expect("cursor sits inside a __() call");
    assert_eq!(
        ctx.prefix,
        "legal-contractmanagement::contract-management.details."
    );
}

/// Regression: values longer than 50 characters with a multibyte character
/// straddling the truncation index must not panic (`&s[..47]` was a byte
/// slice; index 47 landing inside 'č' aborted the whole server the first
/// time namespaced catalogues — full of non-ASCII values — were enumerated
/// for completion).
#[test]
fn translation_value_truncation_is_char_boundary_safe() {
    // 46 ASCII chars, then a two-byte 'č' occupying bytes 46..48, then
    // padding past the 50-char threshold.
    let value: String = "a".repeat(46) + "čééééééé";
    let line = format!("'key' => '{}',", value);
    let display = LaravelLanguageServer::extract_translation_value(&line);
    assert!(display.ends_with("..."));
    assert!(display.chars().count() <= 50);
}

/// Regression: a directive whose args carry a second parameter —
/// `@include('view', ['data' => $x])`, `@lang('key', ['name' => $n])` —
/// must still yield its first quoted string. The old extractor rejected
/// any args containing a comma, so goto and the missing-view diagnostic
/// were dead for every data-carrying directive.
#[test]
fn directive_first_string_extraction_survives_data_arguments() {
    let cases = [
        (
            "('ns::pages.editor.block-outline', ['rowBlocks' => $rowBlocks])",
            Some("ns::pages.editor.block-outline"),
        ),
        ("('plain.view')", Some("plain.view")),
        ("(\"double.quoted\", ['a' => 1])", Some("double.quoted")),
        // First token not a string literal (condition-first directives are
        // handled by the second-arg extractor) — must yield None.
        ("($condition, 'view.name')", None),
        ("('')", None),
    ];
    for (args, expected) in cases {
        assert_eq!(
            LaravelLanguageServer::extract_view_from_directive_args(args).as_deref(),
            expected,
            "args: {args}"
        );
    }
}
