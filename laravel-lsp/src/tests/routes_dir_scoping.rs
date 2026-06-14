//! Tests for `is_in_routes_dir`, the gate that decides whether the
//! declaration-fallback walk runs for a file (issue #105).
//!
//! The old heuristic matched *any* path component named `routes`, so a Folio
//! page mounted at `routes/pages/about.php` was treated as a route file and
//! the Folio branch in `classify_with_decl_fallback` was bypassed — yielding
//! no references. With a known project root the gate now matches only files
//! whose immediate parent is exactly `<root>/routes`. With no root it keeps
//! the broad fallback so real route files are never missed.

use crate::is_in_routes_dir;
use std::path::Path;

#[test]
fn root_known_matches_direct_child_of_routes() {
    // `routes/web.php` is a direct child of `<root>/routes` → the gate fires
    // so route-name declarations there still resolve.
    let root = Path::new("/project");
    let file = Path::new("/project/routes/web.php");
    assert!(
        is_in_routes_dir(file, Some(root)),
        "a file whose immediate parent is <root>/routes must match"
    );
}

#[test]
fn root_known_matches_api_routes_file() {
    // `routes/api.php` — the other common direct child — must keep resolving.
    let root = Path::new("/project");
    let file = Path::new("/project/routes/api.php");
    assert!(
        is_in_routes_dir(file, Some(root)),
        "routes/api.php is a direct child of <root>/routes and must match"
    );
}

#[test]
fn root_known_rejects_folio_page_nested_under_routes() {
    // `routes/pages/about.php` is a Folio page, not a route file. Its parent
    // is `<root>/routes/pages`, not `<root>/routes`, so the gate must NOT fire
    // — leaving the Folio branch reachable (the issue #105 fix).
    let root = Path::new("/project");
    let file = Path::new("/project/routes/pages/about.php");
    assert!(
        !is_in_routes_dir(file, Some(root)),
        "a Folio page nested under routes/ must NOT be treated as a route file"
    );
}

#[test]
fn root_known_rejects_routes_component_outside_project_root() {
    // A `routes` directory somewhere else entirely (e.g. a vendored package)
    // is not the project's routes/, so with a known root it must not match.
    let root = Path::new("/project");
    let file = Path::new("/other/routes/web.php");
    assert!(
        !is_in_routes_dir(file, Some(root)),
        "a routes/ directory outside the known project root must not match"
    );
}

#[test]
fn root_none_keeps_broad_match() {
    // Without a root we can't anchor the match, so any `routes` component
    // matches — including a nested Folio page. This is the deliberate
    // permissive fallback so real route files are never missed.
    let direct = Path::new("/project/routes/web.php");
    let nested = Path::new("/project/routes/pages/about.php");
    assert!(
        is_in_routes_dir(direct, None),
        "root-None: a direct routes/ child matches via the broad heuristic"
    );
    assert!(
        is_in_routes_dir(nested, None),
        "root-None: any path with a `routes` component matches the broad heuristic"
    );
}

#[test]
fn root_none_rejects_path_without_routes_component() {
    // The broad heuristic still says no when there's no `routes` component.
    let file = Path::new("/project/app/Http/Controllers/HomeController.php");
    assert!(
        !is_in_routes_dir(file, None),
        "root-None: a path with no `routes` component must not match"
    );
}
