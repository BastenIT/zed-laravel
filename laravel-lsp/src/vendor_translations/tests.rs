use super::*;
use std::fs;
use tempfile::TempDir;

/// Build a fake vendor tree at `vendor/<vendor>/<package>/` with a service
/// provider file at the standard location.
fn fake_vendor_package(project: &Path, vendor: &str, pkg: &str, provider: &str) -> PathBuf {
    let provider_dir = project.join("vendor").join(vendor).join(pkg).join("src");
    fs::create_dir_all(&provider_dir).unwrap();
    let provider_path = provider_dir.join(format!("{}.php", provider));
    provider_path
}

#[test]
fn extracts_single_load_translations_from_registration() {
    let project = TempDir::new().unwrap();
    let provider = fake_vendor_package(project.path(), "acme", "billing", "BillingServiceProvider");

    let lang_dir = provider.parent().unwrap().join("../resources/lang");
    fs::create_dir_all(&lang_dir).unwrap();
    fs::write(
        &provider,
        r#"<?php
namespace Acme\Billing;
class BillingServiceProvider {
    public function boot() {
        $this->loadTranslationsFrom(__DIR__.'/../resources/lang', 'billing');
    }
}
"#,
    )
    .unwrap();

    let map = scan_vendor_translation_namespaces(project.path());
    let resolved = map.get("billing").expect("should find billing namespace");
    assert!(
        resolved.ends_with("resources/lang"),
        "expected resolved to end with resources/lang, got: {:?}",
        resolved
    );
}

#[test]
fn preserves_registration_for_not_yet_published_in_root_dir() {
    // Regression (issue #248, second-review blocker): the namespace map is built
    // once and cached for the LSP's lifetime, so a registration whose lang dir is
    // absent at scan time — a fresh clone, a package pre-`vendor:publish`, or one
    // mid-`composer install` — must NOT be dropped. It has to survive so the
    // namespaced key resolves once the directory appears, without an editor
    // restart. The lexical containment guard admits the speculative in-root dir
    // while still refusing `..` escapes; the fail-closed read-site guard in
    // `translation_lookup` provides the actual read-time security.
    let project = TempDir::new().unwrap();
    let provider = fake_vendor_package(project.path(), "acme", "billing", "BillingServiceProvider");
    // Deliberately do NOT create the lang dir on disk — this is the unpublished case.
    fs::write(
        &provider,
        "<?php\nclass X { public function boot() { $this->loadTranslationsFrom(__DIR__.'/../resources/lang', 'billing'); } }\n",
    )
    .unwrap();

    let map = scan_vendor_translation_namespaces(project.path());
    let resolved = map
        .get("billing")
        .expect("a not-yet-published in-root registration must be preserved, not dropped");
    assert!(
        resolved.ends_with("resources/lang"),
        "must store the registered in-root dir, got: {resolved:?}"
    );
    assert!(
        !resolved.exists(),
        "precondition: the registered dir must not exist on disk yet"
    );
}

#[test]
fn ignores_non_provider_php_files() {
    // A non-provider file with `loadTranslationsFrom` in a docblock should
    // be skipped by the filename gate.
    let project = TempDir::new().unwrap();
    let non_provider = project.path().join("vendor/acme/billing/src/Helpers.php");
    fs::create_dir_all(non_provider.parent().unwrap()).unwrap();
    fs::write(
        &non_provider,
        r#"<?php
namespace Acme\Billing;
// $this->loadTranslationsFrom(__DIR__.'/../lang', 'billing');
class Helpers {}
"#,
    )
    .unwrap();

    let map = scan_vendor_translation_namespaces(project.path());
    assert!(map.is_empty(), "non-provider files must be ignored");
}

#[test]
fn ignores_providers_without_load_translations_from_call() {
    let project = TempDir::new().unwrap();
    let provider = fake_vendor_package(project.path(), "acme", "billing", "BillingServiceProvider");
    fs::write(
        &provider,
        r#"<?php
class BillingServiceProvider {
    public function boot() {
        $this->loadViewsFrom(__DIR__.'/../views', 'billing');
    }
}
"#,
    )
    .unwrap();

    let map = scan_vendor_translation_namespaces(project.path());
    assert!(
        map.is_empty(),
        "providers without loadTranslationsFrom must contribute nothing"
    );
}

#[test]
fn captures_multiple_namespaces_across_packages() {
    let project = TempDir::new().unwrap();
    let p1 = fake_vendor_package(project.path(), "acme", "billing", "BillingServiceProvider");
    let p2 = fake_vendor_package(project.path(), "acme", "auth", "AuthServiceProvider");
    fs::write(
        &p1,
        "<?php\nclass X { public function boot() { $this->loadTranslationsFrom(__DIR__.'/../lang', 'billing'); } }\n",
    )
    .unwrap();
    fs::write(
        &p2,
        "<?php\nclass Y { public function boot() { $this->loadTranslationsFrom(__DIR__.'/../lang', 'auth'); } }\n",
    )
    .unwrap();

    let map = scan_vendor_translation_namespaces(project.path());
    assert!(map.contains_key("billing"));
    assert!(map.contains_key("auth"));
}

#[test]
fn returns_empty_when_vendor_dir_missing() {
    let project = TempDir::new().unwrap();
    // No vendor/ directory.
    let map = scan_vendor_translation_namespaces(project.path());
    assert!(map.is_empty());
}

#[test]
fn first_registration_wins_on_namespace_conflict() {
    // Two packages register the same namespace. First-match-wins.
    let project = TempDir::new().unwrap();
    let p1 = fake_vendor_package(project.path(), "first", "pkg", "FirstServiceProvider");
    let p2 = fake_vendor_package(project.path(), "second", "pkg", "SecondServiceProvider");
    fs::write(
        &p1,
        "<?php\nclass A { public function boot() { $this->loadTranslationsFrom(__DIR__.'/../lang', 'shared'); } }\n",
    )
    .unwrap();
    fs::write(
        &p2,
        "<?php\nclass B { public function boot() { $this->loadTranslationsFrom(__DIR__.'/../lang', 'shared'); } }\n",
    )
    .unwrap();

    let map = scan_vendor_translation_namespaces(project.path());
    let resolved = map.get("shared").expect("conflict must still resolve");
    // The path will contain either "first" or "second" depending on walk order —
    // accept either, but it must be a single deterministic entry.
    let s = resolved.to_string_lossy();
    assert!(s.contains("first") || s.contains("second"), "got: {}", s);
}

// ─── Fluent package-builder registrations (->name()->hasTranslations()) ──

#[test]
fn builder_has_translations_registers_short_name_namespace() {
    // The Filament shape: ->name('filament-tables')->hasTranslations(), with
    // translations at <pkg>/resources/lang (the builder's basePath convention).
    let project = TempDir::new().unwrap();
    let provider = fake_vendor_package(
        project.path(),
        "filament",
        "tables",
        "TablesServiceProvider",
    );
    let lang_dir = provider.parent().unwrap().join("../resources/lang/en");
    fs::create_dir_all(&lang_dir).unwrap();
    fs::write(
        lang_dir.join("table.php"),
        "<?php return ['grouping' => []];",
    )
    .unwrap();
    fs::write(
        &provider,
        r#"<?php
namespace Filament\Tables;
use Spatie\LaravelPackageTools\PackageServiceProvider;
class TablesServiceProvider extends PackageServiceProvider
{
    public function configurePackage(Package $package): void
    {
        $package
            ->name('filament-tables')
            ->hasTranslations()
            ->hasViews();
    }
}
"#,
    )
    .unwrap();

    let map = scan_vendor_translation_namespaces(project.path());
    let resolved = map
        .get("filament-tables")
        .expect("builder registration must yield the filament-tables namespace");
    assert!(
        resolved.join("en/table.php").exists(),
        "namespace must point at the package lang dir: {resolved:?}"
    );
}

#[test]
fn builder_name_strips_laravel_prefix_for_translation_namespace() {
    let project = TempDir::new().unwrap();
    let provider = fake_vendor_package(project.path(), "acme", "tools", "ToolsServiceProvider");
    fs::create_dir_all(provider.parent().unwrap().join("../resources/lang")).unwrap();
    fs::write(
        &provider,
        r#"<?php
class ToolsServiceProvider extends PackageServiceProvider
{
    public function configurePackage(Package $package): void
    {
        $package->name('laravel-tools')->hasTranslations();
    }
}
"#,
    )
    .unwrap();

    let map = scan_vendor_translation_namespaces(project.path());
    assert!(
        map.contains_key("tools"),
        "->name('laravel-tools') must register namespace 'tools', got {map:?}"
    );
}

#[test]
fn builder_preserves_registration_for_not_yet_published_in_root_dir() {
    // The builder path (`->name()->hasTranslations()`) now carries the same
    // lexical containment guard as the literal-call path (issue #248 follow-up).
    // Its derived `../resources/lang` dir is always in-root, so the guard must
    // admit it even when the directory doesn't exist yet — the registration is
    // preserved exactly as the old `canonicalize().unwrap_or(lang_dir)` did,
    // without weakening containment.
    let project = TempDir::new().unwrap();
    let provider = fake_vendor_package(project.path(), "acme", "tools", "ToolsServiceProvider");
    // Deliberately do NOT create resources/lang — the unpublished builder case.
    fs::write(
        &provider,
        r#"<?php
class ToolsServiceProvider extends PackageServiceProvider
{
    public function configurePackage(Package $package): void
    {
        $package->name('acme-tools')->hasTranslations();
    }
}
"#,
    )
    .unwrap();

    let map = scan_vendor_translation_namespaces(project.path());
    let resolved = map
        .get("acme-tools")
        .expect("a not-yet-published builder registration must be preserved");
    assert!(
        resolved.ends_with("resources/lang"),
        "must store the builder's in-root lang dir, got: {resolved:?}"
    );
}

#[test]
fn builder_without_has_translations_registers_nothing() {
    // ->hasViews() alone (no ->hasTranslations()) must not synthesize a
    // translation namespace.
    let project = TempDir::new().unwrap();
    let provider = fake_vendor_package(project.path(), "acme", "ui", "UiServiceProvider");
    fs::write(
        &provider,
        r#"<?php
class UiServiceProvider extends PackageServiceProvider
{
    public function configurePackage(Package $package): void
    {
        $package->name('acme-ui')->hasViews();
    }
}
"#,
    )
    .unwrap();

    let map = scan_vendor_translation_namespaces(project.path());
    assert!(
        map.is_empty(),
        "no ->hasTranslations() means no translation namespace, got {map:?}"
    );
}

// ─── App service-provider registrations (app/Providers/**/*.php) ─────────

/// Write a service provider under `app/Providers/<name>.php` with the given
/// body and return its path.
fn write_app_provider(project: &Path, name: &str, body: &str) -> PathBuf {
    let dir = project.join("app").join("Providers");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.php"));
    fs::write(&path, body).unwrap();
    path
}

#[test]
fn scans_app_provider_with_lang_path_argument() {
    let project = TempDir::new().unwrap();
    fs::create_dir_all(project.path().join("lang/app")).unwrap();
    write_app_provider(
        project.path(),
        "AppServiceProvider",
        r#"<?php
namespace App\Providers;
class AppServiceProvider {
    public function boot(): void {
        $this->loadTranslationsFrom(lang_path('app'), 'app');
    }
}
"#,
    );

    let map = scan_app_translation_namespaces(project.path());
    let resolved = map.get("app").expect("should find app namespace");
    assert!(
        resolved.ends_with("lang/app"),
        "lang_path('app') must resolve to <root>/lang/app, got: {resolved:?}"
    );
}

#[test]
fn scans_app_provider_with_base_path_argument() {
    let project = TempDir::new().unwrap();
    fs::create_dir_all(project.path().join("lang/custom")).unwrap();
    write_app_provider(
        project.path(),
        "TranslationServiceProvider",
        r#"<?php
class TranslationServiceProvider {
    public function boot(): void {
        $this->loadTranslationsFrom(base_path('lang/custom'), 'custom');
    }
}
"#,
    );

    let map = scan_app_translation_namespaces(project.path());
    let resolved = map.get("custom").expect("should find custom namespace");
    assert!(
        resolved.ends_with("lang/custom"),
        "base_path('lang/custom') must resolve to <root>/lang/custom, got: {resolved:?}"
    );
}

#[test]
fn scans_app_provider_with_dir_concat_argument() {
    // The existing `__DIR__.'...'` form must keep working for app providers.
    let project = TempDir::new().unwrap();
    // app/Providers/AppServiceProvider.php + __DIR__.'/../../lang/app' → <root>/lang/app
    fs::create_dir_all(project.path().join("lang/app")).unwrap();
    write_app_provider(
        project.path(),
        "AppServiceProvider",
        r#"<?php
class AppServiceProvider {
    public function boot(): void {
        $this->loadTranslationsFrom(__DIR__.'/../../lang/app', 'app');
    }
}
"#,
    );

    let map = scan_app_translation_namespaces(project.path());
    let resolved = map.get("app").expect("should find app namespace");
    assert!(
        resolved.ends_with("lang/app"),
        "__DIR__.'/../../lang/app' must resolve to <root>/lang/app, got: {resolved:?}"
    );
}

#[test]
fn vendor_scan_resolves_dirname_dir_argument() {
    // `dirname(__DIR__).'/lang'` climbs one level from the provider directory.
    let project = TempDir::new().unwrap();
    let provider = fake_vendor_package(project.path(), "acme", "billing", "BillingServiceProvider");
    // provider lives in vendor/acme/billing/src — dirname(__DIR__) is .../billing
    fs::create_dir_all(provider.parent().unwrap().join("../lang")).unwrap();
    fs::write(
        &provider,
        "<?php\nclass X { public function boot() { $this->loadTranslationsFrom(dirname(__DIR__).'/lang', 'billing'); } }\n",
    )
    .unwrap();

    let map = scan_vendor_translation_namespaces(project.path());
    let resolved = map.get("billing").expect("should find billing namespace");
    assert!(
        resolved.ends_with("billing/lang"),
        "dirname(__DIR__).'/lang' must resolve to the package root's lang dir, got: {resolved:?}"
    );
}

#[test]
fn scans_app_provider_with_dirname_dir_argument() {
    // `dirname(__DIR__).'/lang'` climbs one level from the provider directory
    // (app/Providers → app) for an app service provider, mirroring the vendor
    // scan's handling of the same form (AC: dirname(__DIR__) coverage for the
    // app scanner specifically).
    let project = TempDir::new().unwrap();
    // app/Providers/AppServiceProvider.php → dirname(__DIR__) is app, /lang → app/lang
    fs::create_dir_all(project.path().join("app/lang")).unwrap();
    write_app_provider(
        project.path(),
        "AppServiceProvider",
        r#"<?php
class AppServiceProvider {
    public function boot(): void {
        $this->loadTranslationsFrom(dirname(__DIR__).'/lang', 'app');
    }
}
"#,
    );

    let map = scan_app_translation_namespaces(project.path());
    let resolved = map.get("app").expect("should find app namespace");
    assert!(
        resolved.ends_with("app/lang"),
        "dirname(__DIR__).'/lang' must resolve to <root>/app/lang, got: {resolved:?}"
    );
}

#[test]
fn app_provider_wins_over_vendor_on_namespace_conflict() {
    // When a vendor package and an app service provider both register the same
    // namespace, the merge in `main.rs::vendor_translation_namespaces_for`
    // (`vendor.extend(app)`) makes the app entry win — App (2) outranks
    // Package (1) in the project's priority convention. This exercises that
    // documented `extend()` / app-wins semantics rather than assuming it.
    let project = TempDir::new().unwrap();

    // Vendor provider registers `shared` → vendor/acme/shared-pkg/lang.
    let vendor_provider = fake_vendor_package(
        project.path(),
        "acme",
        "shared-pkg",
        "SharedServiceProvider",
    );
    fs::create_dir_all(vendor_provider.parent().unwrap().join("../lang")).unwrap();
    fs::write(
        &vendor_provider,
        "<?php\nclass V { public function boot() { $this->loadTranslationsFrom(__DIR__.'/../lang', 'shared'); } }\n",
    )
    .unwrap();

    // App provider registers the same `shared` namespace → <root>/lang/app.
    fs::create_dir_all(project.path().join("lang/app")).unwrap();
    write_app_provider(
        project.path(),
        "AppServiceProvider",
        r#"<?php
class AppServiceProvider {
    public function boot(): void {
        $this->loadTranslationsFrom(lang_path('app'), 'shared');
    }
}
"#,
    );

    // Mirror the merge order in `vendor_translation_namespaces_for`.
    let mut merged = scan_vendor_translation_namespaces(project.path());
    merged.extend(scan_app_translation_namespaces(project.path()));

    let resolved = merged.get("shared").expect("conflict must still resolve");
    assert!(
        resolved.ends_with("lang/app"),
        "app registration must win the namespace clash, got: {resolved:?}"
    );
    assert!(
        !resolved.to_string_lossy().contains("shared-pkg"),
        "app registration must override the vendor entry, got: {resolved:?}"
    );
}

#[test]
fn refuses_registration_escaping_project_root() {
    // A crafted `loadTranslationsFrom` argument that climbs out of the project
    // root must never enter the namespace map — the resolver would otherwise read
    // translation files from outside the project tree (issue #248, path
    // traversal). The escape target is created on disk, so its rejection cannot be
    // attributed to a missing directory: the lexical containment guard refuses it
    // on root grounds alone (it collapses the interior `..` and sees the path land
    // outside the root) even though the directory exists.
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("project");
    fs::create_dir_all(&root).unwrap();
    // A sibling of the project root — inside the tempdir, but outside `root`.
    fs::create_dir_all(tmp.path().join("escape/lang")).unwrap();

    write_app_provider(
        &root,
        "AppServiceProvider",
        r#"<?php
class AppServiceProvider {
    public function boot(): void {
        $this->loadTranslationsFrom(base_path('../escape/lang'), 'evil');
    }
}
"#,
    );

    let map = scan_app_translation_namespaces(&root);
    assert!(
        !map.contains_key("evil"),
        "a registration resolving outside the project root must be refused, got: {map:?}"
    );
}

#[test]
fn app_scan_returns_empty_without_providers_dir() {
    let project = TempDir::new().unwrap();
    let map = scan_app_translation_namespaces(project.path());
    assert!(map.is_empty(), "no app/Providers means no namespaces");
}
