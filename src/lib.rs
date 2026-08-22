use std::fs;
use zed_extension_api::{self as zed, Result};

/// Extension version - used for versioned binary directory
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Unsuffixed binary names to look for on the user's `PATH`, in preference
/// order, when the automatic download is blocked and they've placed the
/// server there by hand (see `docs/troubleshooting.md`).
///
/// The pre-rebrand `laravel-lsp` stays as a fallback: the published
/// troubleshooting doc told users to install under that name for every
/// release before the "Laravel CE" rename, and silently failing to find a
/// binary they already installed would look exactly like the download
/// problem they worked around in the first place.
const PATH_BINARY_NAMES: [&str; 2] = ["laravel-ce-lsp", "laravel-lsp"];

/// The main struct for our Laravel extension
struct LaravelExtension {
    /// Cached path to the language server binary
    cached_binary_path: Option<String>,
}

impl zed::Extension for LaravelExtension {
    fn new() -> Self {
        LaravelExtension {
            cached_binary_path: None,
        }
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let binary_path = self.language_server_binary_path(worktree)?;

        Ok(zed::Command {
            command: binary_path,
            args: vec![],
            env: worktree.shell_env(),
        })
    }

    fn language_server_initialization_options(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<Option<zed::serde_json::Value>> {
        // Forward `lsp.laravel-lsp.initialization_options` to the server's
        // `initialize` params. Without this Zed sends `None`, and any settings
        // a user places under `initialization_options` are silently dropped.
        Ok(
            zed::settings::LspSettings::for_worktree("laravel-lsp", worktree)
                .ok()
                .and_then(|s| s.initialization_options),
        )
    }

    fn language_server_workspace_configuration(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<Option<zed::serde_json::Value>> {
        // Forward `lsp.laravel-lsp.settings` so the server's
        // `workspace/configuration` pull (and `didChangeConfiguration`) actually
        // carries the user's settings. Zed does NOT do this automatically for
        // extension-provided servers — without this hook it answers the pull
        // with `{}`, so every setting (codeLens.enabled, blade.directiveSpacing,
        // diagnostics.severity, …) stays at its default.
        Ok(
            zed::settings::LspSettings::for_worktree("laravel-lsp", worktree)
                .ok()
                .and_then(|s| s.settings),
        )
    }

    fn language_server_additional_workspace_configuration(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        target_language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<Option<zed::serde_json::Value>> {
        // Quiet shellcheck's SC2034 ("appears unused") on `.env` files in
        // Laravel worktrees. Zed classifies `.env` as Shell Script and runs
        // bash-language-server on it, so every `KEY=value` line is flagged as
        // an unused shell variable — but a Laravel `.env` is read at runtime
        // through `env()`/`config()`, never inside the file.
        //
        // Delivered through Zed's *additional* workspace-configuration hook:
        // Zed merges another adapter's contribution into the bash server's
        // own configuration, which reaches the server even on Zed releases
        // where `lsp.bash-language-server.settings` itself is dropped
        // (fixed upstream in zed-industries/zed#57487, post-1.8.2).
        //
        // Scoped to worktrees whose composer.json depends on `laravel/*` or
        // `illuminate/*` (NOT to `artisan` presence — package repos have no
        // artisan). Note this mutes SC2034 for every shell file the bash
        // server lints in such a worktree, not only `.env`. Opt out via
        // `lsp.laravel-lsp.settings.shellcheck.suppressUnusedVarWarnings: false`.
        if target_language_server_id.as_ref() != "bash-language-server" {
            return Ok(None);
        }
        let own_settings = zed::settings::LspSettings::for_worktree("laravel-lsp", worktree)
            .ok()
            .and_then(|s| s.settings);
        if !Self::shellcheck_suppression_enabled(own_settings.as_ref()) {
            return Ok(None);
        }
        let composer = worktree.read_text_file("composer.json").ok();
        if !Self::is_laravel_worktree(composer.as_deref()) {
            return Ok(None);
        }
        // Start from the user's own bash-language-server arguments so the
        // injected config extends rather than replaces them (Zed's merge
        // gives this hook's value precedence over the user's).
        let user_args = zed::settings::LspSettings::for_worktree("bash-language-server", worktree)
            .ok()
            .and_then(|s| s.settings)
            .and_then(|s| {
                s.get("bashIde")
                    .and_then(|b| b.get("shellcheckArguments"))
                    .cloned()
            });
        let args = Self::shellcheck_args_with_sc2034_excluded(user_args.as_ref());
        Ok(Some(zed::serde_json::json!({
            "bashIde": { "shellcheckArguments": args }
        })))
    }
}

impl LaravelExtension {
    /// Get or download the language server binary
    ///
    /// Search order:
    /// 1. Check cached path (verify still exists)
    /// 2. Check versioned extension directory (laravel-lsp-{VERSION}/)
    /// 3. Try system PATH via worktree.which()
    /// 4. Download from GitHub releases
    fn language_server_binary_path(&mut self, worktree: &zed::Worktree) -> Result<String> {
        // Step 1: Check cached path
        if let Some(cached_path) = &self.cached_binary_path {
            if fs::metadata(cached_path).is_ok() {
                return Ok(cached_path.clone());
            }
        }

        let binary_name = Self::get_platform_binary_name();
        let version_dir = format!("laravel-lsp-{}", VERSION);
        let binary_path = format!("{}/{}", version_dir, binary_name);

        // Step 2: Check versioned extension directory
        if fs::metadata(&binary_path).is_ok() {
            self.cached_binary_path = Some(binary_path.clone());
            return Ok(binary_path);
        }

        // Step 3: Try system PATH
        if let Some(path) = worktree.which(&binary_name) {
            self.cached_binary_path = Some(path.clone());
            return Ok(path);
        }

        // Also try the unsuffixed names in PATH, current name first.
        if let Some(path) = Self::which_generic(|name| worktree.which(name)) {
            self.cached_binary_path = Some(path.clone());
            return Ok(path);
        }

        // Step 4: Download from GitHub releases
        let downloaded_path = self.download_binary(&binary_name, &version_dir)?;
        self.cached_binary_path = Some(downloaded_path.clone());
        Ok(downloaded_path)
    }

    /// Download the binary from GitHub releases
    fn download_binary(&self, binary_name: &str, version_dir: &str) -> Result<String> {
        let binary_path = format!("{}/{}", version_dir, binary_name);

        // Check if already downloaded
        if fs::metadata(&binary_path).is_ok() {
            return Ok(binary_path);
        }

        let (os, _arch) = zed::current_platform();
        let archive_ext = match os {
            zed::Os::Windows => "zip",
            _ => "tar.gz",
        };
        let archive_name = format!("{}.{}", binary_name, archive_ext);

        let release_url = format!(
            "https://github.com/mike-bronner/zed-laravel/releases/download/{}/{}",
            VERSION, archive_name
        );

        let file_type = match os {
            zed::Os::Windows => zed::DownloadedFileType::Zip,
            _ => zed::DownloadedFileType::GzipTar,
        };

        // Download and extract
        zed::download_file(&release_url, version_dir, file_type)
            .map_err(|e| format!("Failed to download Laravel CE LSP binary: {}", e))?;

        // Verify extraction succeeded
        if fs::metadata(&binary_path).is_err() {
            return Err(format!(
                "Binary not found after extraction. Expected at: {}",
                binary_path
            ));
        }

        // Make the binary executable via the Zed host (extensions run as WASM,
        // so std::os::unix::fs is unavailable here).
        zed::make_file_executable(&binary_path)
            .map_err(|e| format!("Failed to make Laravel CE LSP binary executable: {}", e))?;

        Ok(binary_path)
    }

    /// Get platform-specific binary name
    fn get_platform_binary_name() -> String {
        let (os, arch) = zed::current_platform();
        Self::platform_binary_name(os, arch)
    }

    /// Map a platform triple to the published release-asset binary name.
    ///
    /// The Linux assets are statically-linked musl builds, so a single
    /// binary per arch runs on glibc distros, musl distros (Alpine), and
    /// loader-less setups (NixOS) alike — no libc detection needed.
    fn platform_binary_name(os: zed::Os, arch: zed::Architecture) -> String {
        match (os, arch) {
            (zed::Os::Windows, zed::Architecture::X8664) => {
                "laravel-ce-lsp-windows-x64.exe".to_string()
            }
            (zed::Os::Windows, zed::Architecture::Aarch64) => {
                "laravel-ce-lsp-windows-arm64.exe".to_string()
            }
            (zed::Os::Windows, _) => "laravel-ce-lsp.exe".to_string(),
            (zed::Os::Mac, zed::Architecture::Aarch64) => "laravel-ce-lsp-macos-arm64".to_string(),
            (zed::Os::Mac, zed::Architecture::X8664) => "laravel-ce-lsp-macos-x64".to_string(),
            (zed::Os::Mac, _) => "laravel-ce-lsp".to_string(),
            (zed::Os::Linux, zed::Architecture::X8664) => "laravel-ce-lsp-linux-x64".to_string(),
            (zed::Os::Linux, zed::Architecture::Aarch64) => {
                "laravel-ce-lsp-linux-arm64".to_string()
            }
            (zed::Os::Linux, _) => "laravel-ce-lsp".to_string(),
        }
    }

    /// Resolve the server binary from the user's `PATH` by its unsuffixed
    /// name, trying each of [`PATH_BINARY_NAMES`] in order.
    ///
    /// Takes the lookup as a closure rather than a `&Worktree` so the
    /// preference order is testable — `Worktree` is a host-provided handle
    /// that can't be constructed outside a running Zed.
    fn which_generic(mut which: impl FnMut(&str) -> Option<String>) -> Option<String> {
        PATH_BINARY_NAMES.iter().find_map(|name| which(name))
    }

    /// Whether the shellcheck SC2034 suppression is enabled. Default `true`;
    /// only an explicit
    /// `{ "shellcheck": { "suppressUnusedVarWarnings": false } }` in
    /// `lsp.laravel-lsp.settings` turns it off.
    fn shellcheck_suppression_enabled(settings: Option<&zed::serde_json::Value>) -> bool {
        settings
            .and_then(|s| s.get("shellcheck"))
            .and_then(|s| s.get("suppressUnusedVarWarnings"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true)
    }

    /// Whether a composer.json marks this worktree as Laravel-related:
    /// any `require` or `require-dev` entry under the `laravel/` or
    /// `illuminate/` vendor namespaces (covers full apps AND packages,
    /// which have no `artisan`). Missing or malformed composer.json → not
    /// Laravel — never inject into unrelated projects.
    fn is_laravel_worktree(composer_json: Option<&str>) -> bool {
        let Some(json) = composer_json else {
            return false;
        };
        let Ok(composer) = zed::serde_json::from_str::<zed::serde_json::Value>(json) else {
            return false;
        };
        ["require", "require-dev"].iter().any(|section| {
            composer
                .get(section)
                .and_then(|deps| deps.as_object())
                .is_some_and(|deps| {
                    deps.keys()
                        .any(|k| k.starts_with("laravel/") || k.starts_with("illuminate/"))
                })
        })
    }

    /// The user's `bashIde.shellcheckArguments` (array form, or the
    /// space-separated string form bash-language-server also accepts) with
    /// `--exclude=SC2034` appended — unless an argument already mentions
    /// SC2034, in which case the user's arguments pass through untouched
    /// (they've already made a call about that rule; shellcheck accepts
    /// repeated `--exclude` flags, so appending is otherwise safe).
    fn shellcheck_args_with_sc2034_excluded(
        existing: Option<&zed::serde_json::Value>,
    ) -> Vec<String> {
        let mut args: Vec<String> = match existing {
            Some(zed::serde_json::Value::Array(items)) => items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect(),
            Some(zed::serde_json::Value::String(s)) => {
                s.split_whitespace().map(str::to_string).collect()
            }
            _ => Vec::new(),
        };
        if !args.iter().any(|a| a.contains("SC2034")) {
            args.push("--exclude=SC2034".to_string());
        }
        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zed::{Architecture, Os};

    #[test]
    fn linux_names() {
        assert_eq!(
            LaravelExtension::platform_binary_name(Os::Linux, Architecture::X8664),
            "laravel-ce-lsp-linux-x64"
        );
        assert_eq!(
            LaravelExtension::platform_binary_name(Os::Linux, Architecture::Aarch64),
            "laravel-ce-lsp-linux-arm64"
        );
    }

    #[test]
    fn mac_and_windows_names() {
        assert_eq!(
            LaravelExtension::platform_binary_name(Os::Mac, Architecture::Aarch64),
            "laravel-ce-lsp-macos-arm64"
        );
        assert_eq!(
            LaravelExtension::platform_binary_name(Os::Mac, Architecture::X8664),
            "laravel-ce-lsp-macos-x64"
        );
        assert_eq!(
            LaravelExtension::platform_binary_name(Os::Windows, Architecture::X8664),
            "laravel-ce-lsp-windows-x64.exe"
        );
        assert_eq!(
            LaravelExtension::platform_binary_name(Os::Windows, Architecture::Aarch64),
            "laravel-ce-lsp-windows-arm64.exe"
        );
    }

    /// A `worktree.which()` stand-in that only knows about `present`.
    fn path_with(present: &'static [&'static str]) -> impl FnMut(&str) -> Option<String> {
        move |name| present.contains(&name).then(|| format!("/usr/bin/{name}"))
    }

    #[test]
    fn path_lookup_finds_the_current_name() {
        assert_eq!(
            LaravelExtension::which_generic(path_with(&["laravel-ce-lsp"])),
            Some("/usr/bin/laravel-ce-lsp".to_string())
        );
    }

    #[test]
    fn path_lookup_falls_back_to_the_pre_rebrand_name() {
        assert_eq!(
            LaravelExtension::which_generic(path_with(&["laravel-lsp"])),
            Some("/usr/bin/laravel-lsp".to_string()),
            "a binary installed under the old name must keep working"
        );
    }

    #[test]
    fn path_lookup_prefers_the_current_name_over_the_legacy_one() {
        assert_eq!(
            LaravelExtension::which_generic(path_with(&["laravel-lsp", "laravel-ce-lsp"])),
            Some("/usr/bin/laravel-ce-lsp".to_string()),
            "with both on PATH the rebranded binary wins"
        );
    }

    #[test]
    fn path_lookup_reports_nothing_when_no_binary_is_installed() {
        assert_eq!(LaravelExtension::which_generic(path_with(&[])), None);
    }

    #[test]
    fn path_lookup_stops_at_the_first_hit() {
        let mut probed = Vec::new();
        LaravelExtension::which_generic(|name| {
            probed.push(name.to_string());
            (name == "laravel-ce-lsp").then(|| name.to_string())
        });
        assert_eq!(
            probed,
            ["laravel-ce-lsp"],
            "the legacy name must not be probed once the current one resolves"
        );
    }

    // ── shellcheck SC2034 suppression (additional workspace configuration) ──

    fn json(s: &str) -> zed::serde_json::Value {
        zed::serde_json::from_str(s).unwrap()
    }

    #[test]
    fn laravel_app_detected_via_require() {
        assert!(LaravelExtension::is_laravel_worktree(Some(
            r#"{"require": {"php": "^8.2", "laravel/framework": "^12.0"}}"#
        )));
    }

    #[test]
    fn laravel_package_detected_via_illuminate_in_require_dev() {
        // Packages depend on illuminate/* components and have no artisan.
        assert!(LaravelExtension::is_laravel_worktree(Some(
            r#"{"require-dev": {"illuminate/support": "^12.0"}}"#
        )));
    }

    #[test]
    fn non_laravel_php_project_is_not_detected() {
        assert!(!LaravelExtension::is_laravel_worktree(Some(
            r#"{"require": {"symfony/console": "^7.0"}}"#
        )));
    }

    #[test]
    fn vendor_prefix_must_match_at_the_namespace_boundary() {
        // "laravelish/tools" is not the laravel/ vendor namespace.
        assert!(!LaravelExtension::is_laravel_worktree(Some(
            r#"{"require": {"laravelish/tools": "^1.0"}}"#
        )));
    }

    #[test]
    fn missing_or_malformed_composer_json_fails_closed() {
        assert!(!LaravelExtension::is_laravel_worktree(None));
        assert!(!LaravelExtension::is_laravel_worktree(Some("not json {")));
        assert!(!LaravelExtension::is_laravel_worktree(Some("{}")));
    }

    #[test]
    fn suppression_defaults_to_enabled() {
        assert!(LaravelExtension::shellcheck_suppression_enabled(None));
        assert!(LaravelExtension::shellcheck_suppression_enabled(Some(
            &json(r#"{"codeLens": {"enabled": true}}"#)
        )));
    }

    #[test]
    fn suppression_can_be_opted_out() {
        assert!(!LaravelExtension::shellcheck_suppression_enabled(Some(
            &json(r#"{"shellcheck": {"suppressUnusedVarWarnings": false}}"#)
        )));
    }

    #[test]
    fn exclusion_added_when_user_has_no_arguments() {
        assert_eq!(
            LaravelExtension::shellcheck_args_with_sc2034_excluded(None),
            ["--exclude=SC2034"]
        );
    }

    #[test]
    fn user_array_arguments_are_preserved_and_extended() {
        assert_eq!(
            LaravelExtension::shellcheck_args_with_sc2034_excluded(Some(&json(
                r#"["--exclude=SC1091", "--severity=warning"]"#
            ))),
            ["--exclude=SC1091", "--severity=warning", "--exclude=SC2034"]
        );
    }

    #[test]
    fn user_string_arguments_are_split_like_the_server_splits_them() {
        assert_eq!(
            LaravelExtension::shellcheck_args_with_sc2034_excluded(Some(&json(
                r#""--exclude=SC1091 --severity=warning""#
            ))),
            ["--exclude=SC1091", "--severity=warning", "--exclude=SC2034"]
        );
    }

    #[test]
    fn arguments_already_mentioning_sc2034_pass_through_untouched() {
        // The user made an explicit call about this rule — even a combined
        // exclude list counts; never double-append.
        assert_eq!(
            LaravelExtension::shellcheck_args_with_sc2034_excluded(Some(&json(
                r#"["--exclude=SC1091,SC2034"]"#
            ))),
            ["--exclude=SC1091,SC2034"]
        );
    }
}

zed::register_extension!(LaravelExtension);
