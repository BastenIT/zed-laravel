//! Unit tests for `is_in_routes_dir` — the gate deciding whether the
//! declaration-fallback route walk runs for a given file. Only a file whose
//! immediate parent is the project root's own `routes/` directory qualifies;
//! a `routes` component nested deeper — a package's `vendor/.../routes/` or a
//! Folio mount (issue #98) — or a Folio page nested *below* `<root>/routes/`
//! (issue #105) must not be mistaken for it.

use crate::is_in_routes_dir;
use std::path::Path;

#[test]
fn matches_project_root_routes_dir() {
    assert!(is_in_routes_dir(
        Some(Path::new("/project")),
        Path::new("/project/routes/web.php")
    ));
}

#[test]
fn matches_api_routes_file() {
    // `routes/api.php` — the other common direct child — must keep resolving.
    assert!(is_in_routes_dir(
        Some(Path::new("/project")),
        Path::new("/project/routes/api.php")
    ));
}

#[test]
fn rejects_folio_page_nested_under_root_routes() {
    // A Folio page mounted at `<root>/routes/pages/about.php` is NOT a route
    // file — its parent is `<root>/routes/pages`, not `<root>/routes`. The gate
    // must not fire, leaving the Folio branch in `classify_with_decl_fallback`
    // reachable (issue #105). The earlier `starts_with(<root>/routes)` check
    // wrongly accepted this; the immediate-parent test rejects it.
    assert!(!is_in_routes_dir(
        Some(Path::new("/project")),
        Path::new("/project/routes/pages/about.php")
    ));
}

#[test]
fn rejects_package_routes_below_root() {
    // A package's own `routes/` under vendor/ is not the project's route dir.
    assert!(!is_in_routes_dir(
        Some(Path::new("/project")),
        Path::new("/project/vendor/somepackage/routes/web.php")
    ));
}

#[test]
fn rejects_folio_style_nested_routes_component() {
    // A Folio page path with `routes` as a non-root component must not be
    // treated as the conventional routes dir (issue #98).
    assert!(!is_in_routes_dir(
        Some(Path::new("/project")),
        Path::new("/project/pages/routes/index.php")
    ));
}

#[test]
fn rejects_when_root_unknown() {
    // No project root to anchor against → never trigger the fallback walk.
    assert!(!is_in_routes_dir(None, Path::new("/any/routes/file.php")));
}
