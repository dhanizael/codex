//! Captures how this Codex process was launched.
//!
//! Runtime diagnostics answer provenance questions that are hard to infer from
//! user reports: which binary is running, which install channel it resembles,
//! which platform it targets, and whether the search command comes from bundled
//! package files or from PATH.

use std::env;
use std::path::Path;
use std::process::Command;

use codex_install_context::InstallContext;
use codex_install_context::InstallMethod;

use super::CheckStatus;
use super::DoctorCheck;
use super::describe_install_context;
use super::doctor_install_context;
use super::push_path_detail;
use crate::CODEX_CLI_VERSION;

/// Builds the process provenance row for the current Codex executable.
///
/// This check is informational and should not fail on its own; inconsistent
/// install state is reported by the installation and update checks instead.
pub(super) fn runtime_check() -> DoctorCheck {
    let current_exe = env::current_exe().ok();
    let install_context = doctor_install_context(current_exe.as_deref());
    let os = env::consts::OS;
    let arch = env::consts::ARCH;
    let platform = format!("{os}-{arch}");
    let install_method = install_method_name(&install_context);
    let build = BuildProvenance::embedded();
    let parity = source_parity(&build);
    let mut details = vec![
        format!("version: {CODEX_CLI_VERSION}"),
        format!("upstream version: {}", env!("CARGO_PKG_VERSION")),
        format!("enhancement revision: {}", build.enhancement_revision),
        format!("platform: {platform}"),
        format!(
            "install method: {}",
            describe_install_context(&install_context)
        ),
        format!("build commit: {}", build.commit),
        format!("build source dirty: {}", build.dirty),
        format!("build source fingerprint: {}", build.source_fingerprint),
        format!("build source root: {}", build.source_root),
    ];
    details.extend(parity.details);
    push_path_detail(&mut details, "current executable", current_exe.as_deref());

    let mut check = DoctorCheck::new(
        "runtime.provenance",
        "runtime",
        parity.status,
        format!(
            "running {install_method} on {platform}; source parity {}",
            parity.summary
        ),
    )
    .details(details);
    if parity.status == CheckStatus::Warning {
        check = check.remediation(
            "Rebuild and atomically reinstall codex-enhanced from the current source tree.",
        );
    }
    check
}

/// Verifies that the search command selected by the install context is usable.
///
/// Package-layout installs should point at a bundled ripgrep binary, while local
/// installs without that layout usually resolve rg from PATH. A warning here
/// means features that depend on file search may degrade even when the CLI
/// launches.
pub(super) fn search_check() -> DoctorCheck {
    let current_exe = env::current_exe().ok();
    let install_context = doctor_install_context(current_exe.as_deref());
    let rg_command = install_context.rg_command();
    let provider = search_provider(&install_context);
    let mut details = vec![
        format!("search command: {}", rg_command.display()),
        format!("search provider: {provider}"),
    ];

    let status = if rg_command.components().count() > 1 {
        match std::fs::metadata(&rg_command) {
            Ok(metadata) if metadata.is_file() => {
                details.push("search command readiness: file exists".to_string());
                CheckStatus::Ok
            }
            Ok(_) => {
                details.push("search command readiness: path is not a file".to_string());
                CheckStatus::Warning
            }
            Err(err) => {
                details.push(format!("search command readiness: {err}"));
                CheckStatus::Warning
            }
        }
    } else {
        match Command::new(&rg_command).arg("--version").output() {
            Ok(output) if output.status.success() => {
                let version = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .next()
                    .unwrap_or("rg version unknown")
                    .to_string();
                details.push(format!("search command readiness: {version}"));
                CheckStatus::Ok
            }
            Ok(output) => {
                details.push(format!(
                    "search command readiness: exited with status {}",
                    output.status
                ));
                CheckStatus::Warning
            }
            Err(err) => {
                details.push(format!("search command readiness: {err}"));
                CheckStatus::Warning
            }
        }
    };

    let summary = match status {
        CheckStatus::Ok => format!("search is OK ({provider})"),
        CheckStatus::Warning => "search command could not be verified".to_string(),
        CheckStatus::Fail => unreachable!(),
    };
    let mut check = DoctorCheck::new("runtime.search", "search", status, summary).details(details);
    if status != CheckStatus::Ok {
        check = check.remediation("Install ripgrep or repair the bundled Codex package.");
    }
    check
}

fn install_method_name(context: &InstallContext) -> &'static str {
    match &context.method {
        InstallMethod::Standalone { .. } => "standalone",
        InstallMethod::Npm => "npm",
        InstallMethod::Bun => "bun",
        InstallMethod::Pnpm => "pnpm",
        InstallMethod::Brew => "brew",
        InstallMethod::Other => "local build",
    }
}

fn search_provider(context: &InstallContext) -> &'static str {
    let rg_command = context.rg_command();
    let from_package_layout = context
        .package_layout
        .as_ref()
        .and_then(|package_layout| package_layout.path_dir.as_ref())
        .is_some_and(|path_dir| rg_command.starts_with(path_dir));
    let from_legacy_standalone = matches!(
        &context.method,
        InstallMethod::Standalone {
            resources_dir: Some(resources_dir),
            ..
        } if rg_command.starts_with(resources_dir)
    );

    if from_package_layout || from_legacy_standalone {
        "bundled"
    } else {
        "system"
    }
}

struct BuildProvenance<'a> {
    commit: &'a str,
    dirty: &'a str,
    enhancement_revision: &'a str,
    source_root: &'a str,
    source_fingerprint: &'a str,
}

impl BuildProvenance<'static> {
    fn embedded() -> Self {
        Self {
            commit: option_env!("CODEX_BUILD_COMMIT")
                .or(option_env!("GIT_COMMIT"))
                .unwrap_or("unknown"),
            dirty: option_env!("CODEX_BUILD_DIRTY").unwrap_or("unknown"),
            enhancement_revision: option_env!("CODEX_ENHANCEMENT_REVISION").unwrap_or("unknown"),
            source_root: option_env!("CODEX_BUILD_SOURCE_ROOT").unwrap_or("unknown"),
            source_fingerprint: option_env!("CODEX_BUILD_SOURCE_FINGERPRINT").unwrap_or("unknown"),
        }
    }
}

struct SourceParity {
    status: CheckStatus,
    summary: &'static str,
    details: Vec<String>,
}

fn source_parity(build: &BuildProvenance<'_>) -> SourceParity {
    if build.source_root == "unknown" || build.commit == "unknown" {
        return SourceParity {
            status: CheckStatus::Warning,
            summary: "unverifiable",
            details: vec!["source parity: build provenance unavailable".to_string()],
        };
    }

    let source_root = Path::new(build.source_root);
    let Some(commit) = git_output(source_root, &["rev-parse", "HEAD"]) else {
        return SourceParity {
            status: CheckStatus::Ok,
            summary: "source unavailable",
            details: vec![
                "source parity: source tree is unavailable; embedded provenance retained"
                    .to_string(),
            ],
        };
    };
    let dirty = source_status(source_root).map(|output| !output.is_empty());
    let fingerprint = source_fingerprint(source_root);
    let matches = commit == build.commit
        && dirty == parse_dirty(build.dirty)
        && fingerprint.as_deref() == Some(build.source_fingerprint);
    let status = if matches {
        CheckStatus::Ok
    } else {
        CheckStatus::Warning
    };
    SourceParity {
        status,
        summary: if matches { "matched" } else { "mismatched" },
        details: vec![
            format!("current source commit: {commit}"),
            format!(
                "current source dirty: {}",
                dirty.map_or("unknown", |value| if value { "true" } else { "false" })
            ),
            format!(
                "current source fingerprint: {}",
                fingerprint.as_deref().unwrap_or("unknown")
            ),
            format!(
                "source parity: {}",
                if matches { "matched" } else { "mismatched" }
            ),
        ],
    }
}

fn parse_dirty(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn git_output(source_root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(source_root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn source_fingerprint(source_root: &Path) -> Option<String> {
    let status = source_status(source_root)?;
    let diff = git_output(
        source_root,
        &[
            "diff",
            "--binary",
            "HEAD",
            "--",
            ".",
            ":(exclude)codex-rs/Cargo.lock",
        ],
    )?;
    let mut hash = fnv1a(0xcbf29ce484222325, status.as_bytes());
    hash = fnv1a(hash, diff.as_bytes());
    for path in git_output(source_root, &["ls-files", "--others", "--exclude-standard"])?.lines() {
        hash = fnv1a(hash, path.as_bytes());
        if let Ok(contents) = std::fs::read(source_root.join(path)) {
            hash = fnv1a(hash, &contents);
        }
    }
    Some(format!("{hash:016x}"))
}

fn source_status(source_root: &Path) -> Option<String> {
    git_output(
        source_root,
        &[
            "status",
            "--porcelain",
            "--",
            ".",
            ":(exclude)codex-rs/Cargo.lock",
        ],
    )
}

fn fnv1a(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
