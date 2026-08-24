use super::*;

const REGISTRARS: &[&str] = &["loadLivewireComponentsFrom"];

fn registrars() -> Vec<String> {
    REGISTRARS.iter().map(|s| s.to_string()).collect()
}

fn module_layout() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let provider_path = root.join("app/Common/UI/app/Providers/AppServiceProvider.php");
    std::fs::create_dir_all(provider_path.parent().unwrap()).unwrap();
    std::fs::create_dir_all(root.join("app/Common/UI/app/Livewire")).unwrap();
    (tmp, root, provider_path)
}

#[test]
fn extracts_wrapper_registrar_call_deriving_namespace_from_file() {
    let (_tmp, root, provider_path) = module_layout();
    let source = r#"<?php

namespace App\Common\UI\Providers;

use App\Base\Providers\AbstractModuleServiceProvider;

class AppServiceProvider extends AbstractModuleServiceProvider
{
    public function boot(): void
    {
        $this->loadLivewireComponentsFrom(__DIR__.'/../Livewire', 'common-ui');
    }
}
"#;
    let map = extract_livewire_namespaces(source, &provider_path, &root, &registrars());
    let reg = map.get("common-ui").expect("common-ui registered");
    assert_eq!(reg.class_namespace, "App\\Common\\UI\\Livewire");
    assert_eq!(
        reg.class_path,
        root.join("app/Common/UI/app/Livewire")
            .canonicalize()
            .unwrap()
    );
}

#[test]
fn extracts_direct_add_namespace_with_named_arguments_in_any_order() {
    let (_tmp, root, provider_path) = module_layout();
    let source = r#"<?php

namespace App\Common\UI\Providers;

use Livewire\Livewire;

class AppServiceProvider
{
    public function boot(): void
    {
        Livewire::addNamespace(
            classPath: __DIR__.'/../Livewire',
            namespace: 'common-ui',
            classNamespace: 'App\\Common\\UI\\Livewire',
        );
    }
}
"#;
    let map = extract_livewire_namespaces(source, &provider_path, &root, &registrars());
    let reg = map.get("common-ui").expect("common-ui registered");
    assert_eq!(reg.class_namespace, "App\\Common\\UI\\Livewire");
    assert_eq!(
        reg.class_path,
        root.join("app/Common/UI/app/Livewire")
            .canonicalize()
            .unwrap()
    );
}

#[test]
fn extracts_direct_add_namespace_positional() {
    let (_tmp, root, provider_path) = module_layout();
    let source = r#"<?php
namespace App\Common\UI\Providers;
use Livewire\Livewire;
class AppServiceProvider {
    public function boot(): void {
        Livewire::addNamespace('common-ui', 'App\\Common\\UI\\Livewire', __DIR__.'/../Livewire');
    }
}
"#;
    let map = extract_livewire_namespaces(source, &provider_path, &root, &registrars());
    assert_eq!(
        map.get("common-ui").unwrap().class_namespace,
        "App\\Common\\UI\\Livewire"
    );
}

#[test]
fn skips_calls_with_variable_arguments() {
    let (_tmp, root, provider_path) = module_layout();
    // The abstract base class's own forwarding call — every argument is a
    // variable or expression, statically unresolvable, must not register.
    let source = r#"<?php
namespace App\Base\Providers;
use Livewire\Livewire;
abstract class AbstractModuleServiceProvider {
    protected function loadLivewireComponentsFrom(string $path, string $prefix = ''): void {
        Livewire::addNamespace(
            namespace: $prefix,
            classNamespace: Str::beforeLast(static::class, '\\Providers\\').'\\Livewire',
            classPath: $path,
        );
    }
}
"#;
    let map = extract_livewire_namespaces(source, &provider_path, &root, &registrars());
    assert!(map.is_empty());
}

#[test]
fn ignores_unlisted_wrapper_methods() {
    let (_tmp, root, provider_path) = module_layout();
    let source = r#"<?php
namespace App\Common\UI\Providers;
class AppServiceProvider {
    public function boot(): void {
        $this->someOtherLoader(__DIR__.'/../Livewire', 'common-ui');
    }
}
"#;
    let map = extract_livewire_namespaces(source, &provider_path, &root, &registrars());
    assert!(map.is_empty());
}

#[test]
fn module_root_namespace_variants() {
    assert_eq!(
        module_root_namespace("App\\Common\\UI\\Providers"),
        "App\\Common\\UI"
    );
    assert_eq!(
        module_root_namespace("App\\Common\\UI\\Providers\\Nested"),
        "App\\Common\\UI"
    );
    assert_eq!(
        module_root_namespace("App\\NoProviders"),
        "App\\NoProviders"
    );
}
