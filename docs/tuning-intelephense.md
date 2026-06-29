# 🔧 Tuning Intelephense

[← Back to README](../README.md)

If you use Intelephense as your PHP language server, goto-definition on a class name (e.g. `User`) can land you in a Zed multi-buffer with several unrelated files — the actual class, plus generated stubs from [barryvdh/laravel-ide-helper](https://github.com/barryvdh/laravel-ide-helper), `.phpstorm.meta.php`, and `class User` stub templates that packages like Jetstream ship under `vendor/*/stubs/`. Intelephense indexes all of them by default.

Zed merges goto responses across every running LSP and only dedupes identical locations — distinct paths from the same logical click stay separate. So even though `laravel-lsp` returns nothing for bare class references (we don't claim that pattern), Intelephense's noisy results show through unfiltered.

This matters more now that `laravel-lsp` **resolves facades, macros, and mixins to their concrete implementation** ([#254](https://github.com/mike-bronner/zed-laravel/pull/254)). `Cmd+Click` on `Auth::user()` already lands on the real impl; the IDE-helper stubs and `.phpstorm.meta.php` just pile duplicate facade/Eloquent hits onto the same multibuffer. The recommended baseline below excludes them so our resolution stands alone.

## Recommended baseline

Tell Intelephense to skip the three sources of duplicate hits:

- `vendor/*/stubs/` — scaffold templates (Jetstream, Filament, etc.), never loaded at runtime.
- `_ide_helper*.php` — the ide-helper facade/model stubs that `laravel-lsp` now supersedes for goto-definition and hover.
- `.phpstorm.meta.php` — PhpStorm container-binding type hints, covered by our `Binding` goto.

```json
{
  "lsp": {
    "intelephense": {
      "initialization_options": {
        "licenceKey": "~/intelephense/licence.txt"
      },
      "settings": {
        "files": {
          "exclude": [
            "**/stubs/**",
            "**/_ide_helper*.php",
            "**/.phpstorm.meta.php"
          ]
        }
      }
    }
  }
}
```

**The trade-off, stated plainly.** Excluding `_ide_helper*.php` costs you Intelephense completion for facade methods that *third-party packages* graft on (Scout's `search()`, Telescope, Spatie permissions, etc.) — `laravel-lsp` resolves those for goto/hover but does **not** offer them as completions. **Core framework facade completion is unaffected** (`Auth::user()`, `Cache::get()`, `Route::get()` …): Intelephense reads the `@method` PHPDoc tags directly off the facade source at `vendor/laravel/framework/src/Illuminate/Support/Facades/*.php`, independent of the helper. If you lean on package-added facade methods enough to want their completion back, keep `_ide_helper*.php` out of the exclude list — see [Keeping the IDE helper](#keeping-the-ide-helper) below.

Two things about this shape that catch people:

- **`initialization_options` vs `settings` are siblings.** `initialization_options` is the right home for the four startup-only keys Intelephense accepts (`licenceKey`, `clearCache`, `storagePath`, `globalStoragePath`); drop the block entirely if you don't have a licence. Everything else (including `files.exclude`) belongs in `settings`. Nesting `settings` inside `initialization_options` makes Intelephense silently ignore it.
- **No `intelephense` namespace inside `settings`.** Most VSCode-style Intelephense docs show keys nested under an `intelephense` object (e.g. `intelephense.files.exclude`). Don't do that here — Zed's PHP extension wraps your `settings` block inside `{ "intelephense": ... }` before sending it to the server. Adding your own `intelephense` key creates `intelephense.intelephense.files.exclude`, which the server silently ignores. Put `files.exclude` directly under `settings`.

After saving, restart Intelephense (`Cmd+Shift+P → lsp: restart`). The stub, IDE-helper, and meta-file results disappear from goto, leaving the real class plus `laravel-lsp`'s resolution.

> ⚠️ **Cache caveat.** Intelephense keeps a persistent symbol index on disk, so already-indexed symbols can still surface after you add excludes. To force a rebuild, add `"clearCache": true` to `initialization_options` for one startup (then remove it — leaving it `true` re-indexes from scratch every launch). As a fallback, wipe `~/Library/Application Support/intelephense/` (macOS) while Zed isn't running.

## Keeping the IDE helper

The baseline exclude trades a little Intelephense completion fidelity for a clean goto. If that trade doesn't suit you, drop `_ide_helper*.php` (and/or `.phpstorm.meta.php`) from the array. Here's exactly what each pattern costs and what `laravel-lsp` already covers, so you can decide per pattern:

| Excluded | Intelephense loses | `laravel-lsp` covers |
|---|---|---|
| `_ide_helper*.php` | Eloquent dynamic attributes/methods (`$user->name`, `User::find()`), plus extra methods added to facades by third-party packages (e.g. Scout, Telescope, Spatie permissions) | Eloquent completion from the actual DB schema (more accurate than ide-helper docblocks), and facade/macro/mixin **goto + hover**. Completion of package-added facade methods is **not** backfilled. **Core framework facade resolution (`Auth::user()`, `Cache::get()`, `Route::get()`, etc.) keeps working without ide-helper** — Intelephense reads the `@method` PHPDoc tags directly off the facade source files at `vendor/laravel/framework/src/Illuminate/Support/Facades/*.php`. |
| `.phpstorm.meta.php` | Container-binding type narrowing (`app('cache')` → `CacheManager`) | Container-binding goto via the `Binding` pattern. |

Rule of thumb: if you lean on completion for package-added facade methods (Scout's `Searchable` methods, Telescope extending `Gate`, etc.), keep `_ide_helper*.php` indexed and accept the extra goto entries. Otherwise the baseline exclude is the cleaner default.

## Per-project

Drop the same exclude patterns into an `.intelephense.json` at your project root. Unlike the Zed `settings` block, this file is read directly by Intelephense, so it uses the standard namespaced shape:

```json
{
  "files.exclude": [
    "**/stubs/**",
    "**/_ide_helper*.php",
    "**/.phpstorm.meta.php"
  ]
}
```
