//! Discover translation-namespace registrations across vendor packages.
//!
//! Laravel packages register their translations in a `ServiceProvider::boot()`
//! method via:
//!
//! ```php
//! $this->loadTranslationsFrom(__DIR__.'/../resources/lang', 'package-namespace');
//! ```
//!
//! The published location for those translations is `lang/vendor/<namespace>/`
//! in the host project, which [`crate::translation_lookup`] already handles.
//! This module fills the gap for translations that **haven't been published** —
//! it walks `vendor/` for service providers that call `loadTranslationsFrom`,
//! extracts each `(namespace, directory)` pair, and returns a map the
//! resolver can fall back to when the published path doesn't exist.
//!
//! No on-disk cache yet — the scan runs once at LSP startup and the result
//! lives in memory. A composer.lock-keyed cache (like
//! [`crate::config::scan_vendor_for_component_aliases`]) is a worthwhile
//! follow-up once the scan time becomes a noticeable cost on first hover.

use lazy_static::lazy_static;
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

lazy_static! {
    /// Matches `$this->loadTranslationsFrom(<first-arg>, 'namespace')`, capturing
    /// the raw first-argument expression and the namespace. The first argument is
    /// resolved to an absolute lang directory by [`resolve_load_translations_arg`],
    /// which understands the `__DIR__`, `dirname(__DIR__)`, `lang_path()` and
    /// `base_path()` forms Laravel app and package providers use in practice.
    static ref LOAD_TRANSLATIONS_RE: Regex = Regex::new(
        r#"\$this->loadTranslationsFrom\s*\(\s*([^,]+?)\s*,\s*['"]([^'"]+)['"]\s*\)"#
    ).unwrap();

    /// `lang_path('app')` — the argument resolves to `<root>/lang/<arg>`.
    static ref LANG_PATH_ARG_RE: Regex = Regex::new(
        r#"^lang_path\s*\(\s*['"]([^'"]*)['"]\s*\)$"#
    ).unwrap();

    /// `base_path('lang/custom')` — the argument resolves to `<root>/<arg>`.
    static ref BASE_PATH_ARG_RE: Regex = Regex::new(
        r#"^base_path\s*\(\s*['"]([^'"]*)['"]\s*\)$"#
    ).unwrap();

    /// `dirname(__DIR__).'/lang'` — `__DIR__` is the provider directory, so
    /// `dirname()` climbs one level and the literal is joined onto that parent.
    static ref DIRNAME_DIR_ARG_RE: Regex = Regex::new(
        r#"^dirname\s*\(\s*__DIR__\s*\)\s*\.\s*['"]([^'"]+)['"]$"#
    ).unwrap();

    /// `__DIR__.'/../resources/lang'` — the literal is joined onto the provider
    /// directory.
    static ref DIR_ARG_RE: Regex = Regex::new(
        r#"^__DIR__\s*\.\s*['"]([^'"]+)['"]$"#
    ).unwrap();

    /// Matches a fluent package-builder name declaration: `->name('package')`.
    /// Builder-convention providers (e.g. Filament via laravel-package-tools)
    /// never call `loadTranslationsFrom` with literal arguments — the real call
    /// runs in a base class as
    /// `$this->loadTranslationsFrom($computedDir, $this->package->shortName())`.
    /// This pair of patterns reconstructs that registration form, the same way
    /// the view-namespace discovery in [`crate::salsa_impl`] does for
    /// `->hasViews()`.
    static ref BUILDER_NAME_RE: Regex = Regex::new(
        r#"->name\s*\(\s*['"]([^'"]+)['"]\s*\)"#
    ).unwrap();

    /// Matches the builder translation capability: `->hasTranslations()`.
    /// Unlike `->hasViews('ns')` there is no explicit-namespace argument —
    /// the namespace is always the package short-name.
    static ref BUILDER_HAS_TRANSLATIONS_RE: Regex = Regex::new(
        r#"->hasTranslations\s*\(\s*\)"#
    ).unwrap();
}

/// Walk `vendor/` for service providers that register translation namespaces.
/// Returns a map of `namespace → absolute lang directory`.
///
/// The scan applies two cheap gates before parsing any file:
/// - **Filename**: must contain `ServiceProvider`
/// - **Content substring**: must contain `loadTranslationsFrom`
///
/// Roughly the same shape as
/// [`crate::config::scan_vendor_for_component_aliases`] — these two scans
/// could share a single vendor-walk pass once we add the persistent cache.
pub fn scan_vendor_translation_namespaces(root: &Path) -> HashMap<String, PathBuf> {
    let vendor = root.join("vendor");
    if !vendor.is_dir() {
        return HashMap::new();
    }

    let mut namespaces: HashMap<String, PathBuf> = HashMap::new();

    for entry in walkdir::WalkDir::new(&vendor)
        .max_depth(10)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("php") {
            continue;
        }
        let filename_matches = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.contains("ServiceProvider"))
            .unwrap_or(false);
        if !filename_matches {
            continue;
        }

        let Ok(source) = fs::read_to_string(path) else {
            continue;
        };
        if !source.contains("loadTranslationsFrom") && !source.contains("hasTranslations") {
            continue;
        }

        extract_translations_from(&source, path, root, &mut namespaces);
        extract_builder_translations_from(&source, path, root, &mut namespaces);
    }

    namespaces
}

/// Walk `app/Providers/` for application service providers that register
/// translation namespaces via `loadTranslationsFrom`. Returns a map of
/// `namespace → absolute lang directory`, resolved exactly as the vendor scan
/// resolves its registrations (see [`resolve_load_translations_arg`]).
///
/// App providers are the usual home for registrations the vendor scan never
/// sees — e.g. `loadTranslationsFrom(lang_path('app'), 'app')` in
/// `AppServiceProvider`. Unlike the vendor scan, there is no `ServiceProvider`
/// filename gate: any `*.php` under `app/Providers/` is eligible, gated only on
/// containing a `loadTranslationsFrom` call.
pub fn scan_app_translation_namespaces(root: &Path) -> HashMap<String, PathBuf> {
    let providers = root.join("app").join("Providers");
    if !providers.is_dir() {
        return HashMap::new();
    }

    let mut namespaces: HashMap<String, PathBuf> = HashMap::new();

    for entry in walkdir::WalkDir::new(&providers)
        .max_depth(10)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("php") {
            continue;
        }
        let Ok(source) = fs::read_to_string(path) else {
            continue;
        };
        if !source.contains("loadTranslationsFrom") {
            continue;
        }

        extract_translations_from(&source, path, root, &mut namespaces);
    }

    namespaces
}

/// Resolve a `loadTranslationsFrom` first-argument expression to an absolute
/// lang directory. `provider_dir` is the directory holding the service-provider
/// file (PHP's `__DIR__`); `root` is the project root (the base for the
/// `lang_path`/`base_path` helpers). Returns `None` for argument forms this
/// scanner doesn't model, so an unrecognized call contributes no entry rather
/// than a wrong one.
fn resolve_load_translations_arg(arg: &str, provider_dir: &Path, root: &Path) -> Option<PathBuf> {
    if let Some(cap) = LANG_PATH_ARG_RE.captures(arg) {
        return Some(
            root.join("lang")
                .join(strip_path_prefix(cap.get(1)?.as_str())),
        );
    }
    if let Some(cap) = BASE_PATH_ARG_RE.captures(arg) {
        return Some(root.join(strip_path_prefix(cap.get(1)?.as_str())));
    }
    if let Some(cap) = DIRNAME_DIR_ARG_RE.captures(arg) {
        return Some(
            provider_dir
                .parent()?
                .join(strip_path_prefix(cap.get(1)?.as_str())),
        );
    }
    if let Some(cap) = DIR_ARG_RE.captures(arg) {
        return Some(provider_dir.join(strip_path_prefix(cap.get(1)?.as_str())));
    }
    None
}

/// Strip a leading `/` or `./` so the fragment joins onto the receiver instead
/// of being treated as absolute (Rust's `Path::join` discards the receiver when
/// the argument starts with `/`).
fn strip_path_prefix(fragment: &str) -> &str {
    fragment.trim_start_matches('/').trim_start_matches("./")
}

/// Apply [`LOAD_TRANSLATIONS_RE`] to the given source. Each match contributes
/// a `namespace → absolute_lang_dir` entry. The first argument is resolved by
/// [`resolve_load_translations_arg`], which covers the `__DIR__`,
/// `dirname(__DIR__)`, `lang_path()` and `base_path()` forms; `root` is the
/// project root the path helpers are relative to.
///
/// First-match-wins on namespace conflict — service-provider boot order is
/// non-deterministic and we have no good way to rank packages without a full
/// composer dependency graph.
fn extract_translations_from(
    source: &str,
    provider_path: &Path,
    root: &Path,
    namespaces: &mut HashMap<String, PathBuf>,
) {
    let Some(provider_dir) = provider_path.parent() else {
        return;
    };

    for cap in LOAD_TRANSLATIONS_RE.captures_iter(source) {
        let (Some(arg), Some(ns)) = (cap.get(1), cap.get(2)) else {
            continue;
        };
        let Some(lang_dir) = resolve_load_translations_arg(arg.as_str().trim(), provider_dir, root)
        else {
            continue;
        };
        // Refuse anything that escapes the project root, but keep a not-yet-
        // published in-root lang dir. The four argument forms all capture an
        // arbitrary string, so a crafted `loadTranslationsFrom('../../etc', 'ns')`
        // could otherwise seed the map with an out-of-root directory the resolver
        // would later read from (issue #248). `path_within_root_lexical` refuses
        // that escape — it collapses interior `..`/`.` and rejects an out-of-root
        // candidate *before* any disk probe — while admitting a speculative in-root
        // dir that doesn't exist yet. That admission matters: this map is built
        // once and cached for the LSP's lifetime (`vendor_translation_namespaces_for`
        // never re-scans it), so a fail-closed `canonicalize()` here would
        // permanently drop every registration whose dir is absent at first scan —
        // a fresh clone, pre-`vendor:publish`, or a mid-`composer install` package —
        // the exact unpublished case this module exists to serve. The real read-time
        // security is the fail-closed `path_within_root` at the read site
        // (`translation_lookup::resolve_namespaced_in_dir`); this is the codebase's
        // standard lexical-at-candidate / fail-closed-at-read split.
        if !crate::path_containment::path_within_root_lexical(&lang_dir, root) {
            continue;
        }
        namespaces
            .entry(ns.as_str().to_string())
            .or_insert_with(|| crate::route_discovery::normalize_path(&lang_dir));
    }
}

/// Reconstruct the fluent package-builder translation registration:
/// `$package->name('filament-tables')->hasTranslations()`. The builder's base
/// class registers `loadTranslationsFrom(<pkg>/resources/lang, shortName())`
/// at runtime — both arguments computed, invisible to [`LOAD_TRANSLATIONS_RE`].
/// The namespace is the package short-name (leading `laravel-` stripped) and
/// the directory follows the builder's `basePath('/../resources/lang')`
/// convention: one level up from the provider's `src/` dir.
fn extract_builder_translations_from(
    source: &str,
    provider_path: &Path,
    root: &Path,
    namespaces: &mut HashMap<String, PathBuf>,
) {
    if !BUILDER_HAS_TRANSLATIONS_RE.is_match(source) {
        return;
    }
    let Some(name_cap) = BUILDER_NAME_RE.captures(source) else {
        return;
    };
    let Some(package_name) = name_cap.get(1) else {
        return;
    };
    let namespace = crate::salsa_impl::builder_short_name(package_name.as_str());
    if namespace.is_empty() {
        return;
    }

    let Some(provider_dir) = provider_path.parent() else {
        return;
    };
    let lang_dir = provider_dir.join("../resources/lang");
    // Apply the same containment guard as `extract_translations_from` so the
    // "every read site is guarded" invariant holds uniformly — this builder
    // registration feeds the same merged map the goto/hover/diagnostic paths
    // read. The builder dir is derived from the provider path so it is normally
    // in-root, but the lexical guard refuses a `..` escape before any disk probe
    // while preserving a not-yet-published in-root dir; the read site is
    // fail-closed (issue #248).
    if !crate::path_containment::path_within_root_lexical(&lang_dir, root) {
        return;
    }
    namespaces
        .entry(namespace)
        .or_insert_with(|| crate::route_discovery::normalize_path(&lang_dir));
}

#[cfg(test)]
mod tests;
