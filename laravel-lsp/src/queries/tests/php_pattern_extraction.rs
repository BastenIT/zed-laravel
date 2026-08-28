//! Tests for the baseline `extract_all_php_patterns` flow across the
//! canonical Laravel helpers (`view()`, `env()`, `config()`,
//! `Route::middleware()`, etc.).

use super::super::*;
use crate::parser::{language_php, parse_php};

#[test]
fn test_extract_all_php_patterns_views() {
    let php_code = r#"<?php
    return view('users.profile');
    Route::view('/home', 'welcome');
    echo view("admin.dashboard");
    "#;

    let tree = parse_php(php_code).expect("Should parse PHP");
    let lang = language_php();
    let patterns =
        extract_all_php_patterns(&tree, php_code, &lang).expect("Should extract patterns");

    assert_eq!(patterns.views.len(), 3, "Should find 3 view calls");

    let view_names: Vec<&str> = patterns.views.iter().map(|m| m.view_name).collect();
    assert!(view_names.contains(&"users.profile"));
    assert!(view_names.contains(&"welcome"));
    assert!(view_names.contains(&"admin.dashboard"));

    let welcome = patterns
        .views
        .iter()
        .find(|v| v.view_name == "welcome")
        .unwrap();
    assert!(
        welcome.is_route_view,
        "Route::view() should set is_route_view=true"
    );

    let users = patterns
        .views
        .iter()
        .find(|v| v.view_name == "users.profile")
        .unwrap();
    assert!(
        !users.is_route_view,
        "view() should set is_route_view=false"
    );
}

#[test]
fn test_extract_all_php_patterns_views_ternary_first_argument() {
    // `view()`'s first argument isn't a plain string literal — Pattern 1
    // never fires — but every arm is still a resolvable view name, so each
    // one gets its own ViewMatch (mirrors the real fixture in
    // GuestTestPage::render()).
    let php_code = r#"<?php
    return view($this->template === 'button' ? 'common-google::test-button' : 'common-google::test-form');
    "#;

    let tree = parse_php(php_code).expect("Should parse PHP");
    let lang = language_php();
    let patterns =
        extract_all_php_patterns(&tree, php_code, &lang).expect("Should extract patterns");

    let view_names: Vec<&str> = patterns.views.iter().map(|m| m.view_name).collect();
    assert_eq!(
        patterns.views.len(),
        2,
        "both ternary arms should be captured, got {view_names:?}"
    );
    assert!(view_names.contains(&"common-google::test-button"));
    assert!(view_names.contains(&"common-google::test-form"));
    assert!(
        patterns.views.iter().all(|v| !v.is_route_view),
        "ternary arms of view() are not Route::view()"
    );
}

#[test]
fn test_extract_all_php_patterns_views_null_coalesce_first_argument() {
    let php_code = r#"<?php
    return view($override ?? 'pages.default');
    "#;

    let tree = parse_php(php_code).expect("Should parse PHP");
    let lang = language_php();
    let patterns =
        extract_all_php_patterns(&tree, php_code, &lang).expect("Should extract patterns");

    assert_eq!(patterns.views.len(), 1, "got {:?}", patterns.views);
    assert_eq!(patterns.views[0].view_name, "pages.default");
}

#[test]
fn test_extract_all_php_patterns_views_match_first_argument() {
    let php_code = r#"<?php
    return view(match ($type) {
        'a' => 'pages.a',
        default => 'pages.b',
    });
    "#;

    let tree = parse_php(php_code).expect("Should parse PHP");
    let lang = language_php();
    let patterns =
        extract_all_php_patterns(&tree, php_code, &lang).expect("Should extract patterns");

    let view_names: Vec<&str> = patterns.views.iter().map(|m| m.view_name).collect();
    assert_eq!(patterns.views.len(), 2, "got {view_names:?}");
    assert!(view_names.contains(&"pages.a"));
    assert!(view_names.contains(&"pages.b"));
}

#[test]
fn test_extract_all_php_patterns_views_conditional_argument_skips_interpolated_arm() {
    // An interpolated arm (`"ns::{$x}"`) isn't a resolvable literal — same
    // rule Pattern 1 applies to a direct `view()` argument — so only the
    // plain-literal arm is captured.
    let php_code = r#"<?php
    return view($cond ? "ns::{$dynamic}" : 'pages.fallback');
    "#;

    let tree = parse_php(php_code).expect("Should parse PHP");
    let lang = language_php();
    let patterns =
        extract_all_php_patterns(&tree, php_code, &lang).expect("Should extract patterns");

    assert_eq!(patterns.views.len(), 1, "got {:?}", patterns.views);
    assert_eq!(patterns.views[0].view_name, "pages.fallback");
}

#[test]
fn test_extract_all_php_patterns_views_elvis_first_argument() {
    // Elvis `$x ?: 'fallback'` parses as a conditional_expression with NO
    // `body` field — this exercises the walker's absent-body path.
    let php_code = r#"<?php
    return view($x ?: 'pages.fallback');
    "#;

    let tree = parse_php(php_code).expect("Should parse PHP");
    let lang = language_php();
    let patterns =
        extract_all_php_patterns(&tree, php_code, &lang).expect("Should extract patterns");

    assert_eq!(patterns.views.len(), 1, "got {:?}", patterns.views);
    assert_eq!(patterns.views[0].view_name, "pages.fallback");
}

#[test]
fn test_extract_all_php_patterns_views_parenthesized_conditional_argument() {
    // The outer parentheses make the argument's direct child a
    // parenthesized_expression, hiding the conditional from the bare
    // conditional_expression capture.
    let php_code = r#"<?php
    return view(($cond ? 'pages.a' : 'pages.b'));
    "#;

    let tree = parse_php(php_code).expect("Should parse PHP");
    let lang = language_php();
    let patterns =
        extract_all_php_patterns(&tree, php_code, &lang).expect("Should extract patterns");

    let view_names: Vec<&str> = patterns.views.iter().map(|m| m.view_name).collect();
    assert_eq!(patterns.views.len(), 2, "got {view_names:?}");
    assert!(view_names.contains(&"pages.a"));
    assert!(view_names.contains(&"pages.b"));
}

#[test]
fn test_extract_all_php_patterns_views_parenthesized_literal_argument() {
    // A plain literal wrapped in parentheses is hidden from Pattern 1 too —
    // it requires the string as a DIRECT child of the argument.
    let php_code = r#"<?php
    return view(('pages.home'));
    "#;

    let tree = parse_php(php_code).expect("Should parse PHP");
    let lang = language_php();
    let patterns =
        extract_all_php_patterns(&tree, php_code, &lang).expect("Should extract patterns");

    assert_eq!(patterns.views.len(), 1, "got {:?}", patterns.views);
    assert_eq!(patterns.views[0].view_name, "pages.home");
    assert!(!patterns.views[0].is_route_view);
}

#[test]
fn test_extract_all_php_patterns_view_make_ternary_first_argument() {
    let php_code = r#"<?php
    return View::make($compact ? 'widgets.small' : 'widgets.large');
    "#;

    let tree = parse_php(php_code).expect("Should parse PHP");
    let lang = language_php();
    let patterns =
        extract_all_php_patterns(&tree, php_code, &lang).expect("Should extract patterns");

    let view_names: Vec<&str> = patterns.views.iter().map(|m| m.view_name).collect();
    assert_eq!(patterns.views.len(), 2, "got {view_names:?}");
    assert!(view_names.contains(&"widgets.small"));
    assert!(view_names.contains(&"widgets.large"));
    assert!(patterns.views.iter().all(|v| !v.is_route_view));
}

#[test]
fn test_extract_all_php_patterns_view_make_qualified_null_coalesce_argument() {
    let php_code = r#"<?php
    return \Illuminate\Support\Facades\View::make($override ?? 'pages.default');
    "#;

    let tree = parse_php(php_code).expect("Should parse PHP");
    let lang = language_php();
    let patterns =
        extract_all_php_patterns(&tree, php_code, &lang).expect("Should extract patterns");

    assert_eq!(patterns.views.len(), 1, "got {:?}", patterns.views);
    assert_eq!(patterns.views[0].view_name, "pages.default");
}

#[test]
fn test_extract_all_php_patterns_route_view_conditional_second_argument() {
    // The ternary sits in the SECOND argument (the view name). The bare
    // (argument) placeholder in the query keeps the route-path argument out:
    // a ternary in the path must not be captured, so exactly two names come
    // back and both are the view-argument arms.
    let php_code = r#"<?php
    Route::view($legacy ? '/old-home' : '/home', $dark ? 'home.dark' : 'home.light');
    "#;

    let tree = parse_php(php_code).expect("Should parse PHP");
    let lang = language_php();
    let patterns =
        extract_all_php_patterns(&tree, php_code, &lang).expect("Should extract patterns");

    let view_names: Vec<&str> = patterns.views.iter().map(|m| m.view_name).collect();
    assert_eq!(
        patterns.views.len(),
        2,
        "only the view-argument arms should be captured, got {view_names:?}"
    );
    assert!(view_names.contains(&"home.dark"));
    assert!(view_names.contains(&"home.light"));
    assert!(
        patterns.views.iter().all(|v| v.is_route_view),
        "Route::view names are route views"
    );
}

#[test]
fn test_extract_all_php_patterns_route_view_parenthesized_literal_second_argument() {
    let php_code = r#"<?php
    Route::view('/about', ('pages.about'));
    "#;

    let tree = parse_php(php_code).expect("Should parse PHP");
    let lang = language_php();
    let patterns =
        extract_all_php_patterns(&tree, php_code, &lang).expect("Should extract patterns");

    assert_eq!(patterns.views.len(), 1, "got {:?}", patterns.views);
    assert_eq!(patterns.views[0].view_name, "pages.about");
    assert!(patterns.views[0].is_route_view);
}

#[test]
fn test_extract_all_php_patterns_volt_route_match_second_argument() {
    let php_code = r#"<?php
    Volt::route('/counter', match ($variant) {
        'v2' => 'counter.v2',
        default => 'counter.v1',
    });
    "#;

    let tree = parse_php(php_code).expect("Should parse PHP");
    let lang = language_php();
    let patterns =
        extract_all_php_patterns(&tree, php_code, &lang).expect("Should extract patterns");

    let view_names: Vec<&str> = patterns.views.iter().map(|m| m.view_name).collect();
    assert_eq!(patterns.views.len(), 2, "got {view_names:?}");
    assert!(view_names.contains(&"counter.v2"));
    assert!(view_names.contains(&"counter.v1"));
    assert!(patterns.views.iter().all(|v| v.is_route_view));
}

#[test]
fn test_extract_all_php_patterns_view_property_literal() {
    // Filament-style `protected string $view = '…';` — the class-property
    // counterpart of a `view()` call, not reachable via a function-call
    // query at all.
    let php_code = r#"<?php
    class ContractViewPage
    {
        protected string $view = 'legal-contractmanagement::filament.pages.contract-edit-page';
    }
    "#;

    let tree = parse_php(php_code).expect("Should parse PHP");
    let lang = language_php();
    let patterns =
        extract_all_php_patterns(&tree, php_code, &lang).expect("Should extract patterns");

    assert_eq!(patterns.views.len(), 1, "got {:?}", patterns.views);
    assert_eq!(
        patterns.views[0].view_name,
        "legal-contractmanagement::filament.pages.contract-edit-page"
    );
    assert!(!patterns.views[0].is_route_view);
}

#[test]
fn test_extract_all_php_patterns_view_property_static_variant() {
    // Filament `Widget`s declare `$view` as `static`.
    let php_code = r#"<?php
    class StatsWidget
    {
        protected static string $view = 'widgets.stats';
    }
    "#;

    let tree = parse_php(php_code).expect("Should parse PHP");
    let lang = language_php();
    let patterns =
        extract_all_php_patterns(&tree, php_code, &lang).expect("Should extract patterns");

    assert_eq!(patterns.views.len(), 1, "got {:?}", patterns.views);
    assert_eq!(patterns.views[0].view_name, "widgets.stats");
}

#[test]
fn test_extract_all_php_patterns_view_property_non_literal_is_skipped() {
    // `self::VIEW` isn't a string literal — no resolvable render site.
    let php_code = r#"<?php
    class DynamicPage
    {
        const VIEW = 'pages.dynamic';
        protected string $view = self::VIEW;
    }
    "#;

    let tree = parse_php(php_code).expect("Should parse PHP");
    let lang = language_php();
    let patterns =
        extract_all_php_patterns(&tree, php_code, &lang).expect("Should extract patterns");

    assert!(patterns.views.is_empty(), "got {:?}", patterns.views);
}

#[test]
fn test_extract_all_php_patterns_env() {
    let php_code = r#"<?php
    $name = env('APP_NAME', 'Laravel');
    $debug = env("APP_DEBUG");
    "#;

    let tree = parse_php(php_code).expect("Should parse PHP");
    let lang = language_php();
    let patterns =
        extract_all_php_patterns(&tree, php_code, &lang).expect("Should extract patterns");

    assert_eq!(patterns.env_calls.len(), 2, "Should find 2 env calls");
    assert_eq!(patterns.env_calls[0].var_name, "APP_NAME");
    assert_eq!(patterns.env_calls[1].var_name, "APP_DEBUG");
}

#[test]
fn test_extract_all_php_patterns_middleware() {
    let php_code = r#"<?php
    Route::middleware('auth')->group(function () {});
    Route::middleware(['auth', 'verified'])->get('/dashboard');
    "#;

    let tree = parse_php(php_code).expect("Should parse PHP");
    let lang = language_php();
    let patterns =
        extract_all_php_patterns(&tree, php_code, &lang).expect("Should extract patterns");

    let middleware_names: Vec<&str> = patterns
        .middleware_calls
        .iter()
        .map(|m| m.middleware_name)
        .collect();

    assert!(
        middleware_names.contains(&"auth"),
        "Should find 'auth' middleware"
    );
    assert!(
        middleware_names.contains(&"verified"),
        "Should find 'verified' middleware"
    );
}

#[test]
fn test_extract_helper_identifiers_fires_for_each_curated_helper() {
    // Every curated helper's NAME token is captured — including arg-less forms
    // (`auth()`, `app()`) and string-arg forms (`route('home')`).
    let php_code = r#"<?php
    route('home');
    view('welcome');
    config('app.name');
    auth();
    app('cache');
    session('key');
    cache('users');
    "#;

    let tree = parse_php(php_code).expect("Should parse PHP");
    let lang = language_php();
    let patterns =
        extract_all_php_patterns(&tree, php_code, &lang).expect("Should extract patterns");

    let names: Vec<&str> = patterns.helper_identifiers.iter().map(|h| h.name).collect();

    for helper in ["route", "view", "config", "auth", "app", "session", "cache"] {
        assert!(
            names.contains(&helper),
            "Should capture the `{helper}` helper identifier, got {names:?}"
        );
    }
    assert_eq!(
        patterns.helper_identifiers.len(),
        7,
        "Exactly the seven curated helpers, got {names:?}"
    );
}

#[test]
fn test_extract_helper_identifiers_ignores_non_curated_helpers() {
    // `bcrypt`/`abort` are real Laravel helpers but outside the curated set —
    // Intelephense owns them, so we must not capture them (the dedup policy).
    let php_code = r#"<?php
    bcrypt('secret');
    abort(404);
    collect([1, 2, 3]);
    str('x')->upper();
    "#;

    let tree = parse_php(php_code).expect("Should parse PHP");
    let lang = language_php();
    let patterns =
        extract_all_php_patterns(&tree, php_code, &lang).expect("Should extract patterns");

    assert!(
        patterns.helper_identifiers.is_empty(),
        "Non-curated helpers must not be captured, got {:?}",
        patterns
            .helper_identifiers
            .iter()
            .map(|h| h.name)
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_extract_helper_identifiers_skips_method_and_static_calls() {
    // Only bare global calls match — `$obj->route()` (member call) and
    // `Router::route()` (static call) are different node kinds.
    let php_code = r#"<?php
    $router->route('home');
    Router::route('home');
    $app->config('x');
    "#;

    let tree = parse_php(php_code).expect("Should parse PHP");
    let lang = language_php();
    let patterns =
        extract_all_php_patterns(&tree, php_code, &lang).expect("Should extract patterns");

    assert!(
        patterns.helper_identifiers.is_empty(),
        "Method / static calls must not match the global-helper pattern, got {:?}",
        patterns
            .helper_identifiers
            .iter()
            .map(|h| h.name)
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_extract_helper_identifier_position_is_the_name_span() {
    // The captured span is the identifier itself, not the string argument —
    // hover must fire on `route`, not on `'home'`.
    let php_code = "<?php\nroute('home');\n";

    let tree = parse_php(php_code).expect("Should parse PHP");
    let lang = language_php();
    let patterns =
        extract_all_php_patterns(&tree, php_code, &lang).expect("Should extract patterns");

    let route = patterns
        .helper_identifiers
        .iter()
        .find(|h| h.name == "route")
        .expect("route helper identifier");
    assert_eq!(route.row, 1, "on the second line (0-based)");
    assert_eq!(route.column, 0, "starts at column 0");
    assert_eq!(route.end_column, 5, "ends after the 5-char name `route`");
}

#[test]
fn test_view_property_matches_are_flagged_and_one_per_class() {
    // Property-form ViewMatches carry `is_property_site` (goto/hover yes,
    // missing-view diagnostic no) and every declaring class emits its own —
    // call-form matches stay unflagged.
    let php_code = r#"<?php
    class PageA { protected string $view = 'pages.a'; }
    class PageB {
        protected string $view = 'pages.b';
        public function fallback() { return view('pages.fallback'); }
    }
    "#;

    let tree = parse_php(php_code).expect("Should parse PHP");
    let lang = language_php();
    let patterns =
        extract_all_php_patterns(&tree, php_code, &lang).expect("Should extract patterns");

    let mut property_names: Vec<&str> = patterns
        .views
        .iter()
        .filter(|v| v.is_property_site)
        .map(|v| v.view_name)
        .collect();
    property_names.sort();
    assert_eq!(property_names, vec!["pages.a", "pages.b"]);

    let call = patterns
        .views
        .iter()
        .find(|v| v.view_name == "pages.fallback")
        .expect("call-form match");
    assert!(!call.is_property_site);
}
