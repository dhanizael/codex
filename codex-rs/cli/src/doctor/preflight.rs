use std::env;
use std::fs::OpenOptions;
use std::net::Ipv4Addr;
use std::net::TcpListener;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use super::CheckStatus;
use super::DoctorCheck;
use super::DoctorIssue;
use super::executable_path_exists;

#[derive(Clone, Debug, Eq, PartialEq)]
struct PreflightInputs {
    cwd: PathBuf,
    cwd_readable: Result<(), String>,
    codex_home: PathBuf,
    codex_home_writable: Result<(), String>,
    loopback_bind: Result<(), String>,
    shell: Option<PathBuf>,
    shell_executable: Result<(), String>,
    path_entries: usize,
}

impl PreflightInputs {
    fn detect(cwd: &Path, codex_home: &Path) -> Self {
        let shell = configured_shell();
        Self {
            cwd: cwd.to_path_buf(),
            cwd_readable: std::fs::read_dir(cwd)
                .map(|_| ())
                .map_err(|error| error.to_string()),
            codex_home: codex_home.to_path_buf(),
            codex_home_writable: probe_directory_write(codex_home),
            loopback_bind: TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
                .map(|_| ())
                .map_err(|error| error.to_string()),
            shell_executable: shell
                .as_deref()
                .ok_or_else(|| "shell environment variable is not set".to_string())
                .and_then(executable_path_exists),
            shell,
            path_entries: env::var_os("PATH")
                .map(|path| env::split_paths(&path).count())
                .unwrap_or_default(),
        }
    }
}

pub(super) fn execution_preflight_check(cwd: &Path, codex_home: &Path) -> DoctorCheck {
    execution_preflight_check_from_inputs(PreflightInputs::detect(cwd, codex_home))
}

fn execution_preflight_check_from_inputs(inputs: PreflightInputs) -> DoctorCheck {
    let mut details = vec![
        result_detail("working directory", &inputs.cwd, &inputs.cwd_readable),
        result_detail(
            "CODEX_HOME writable",
            &inputs.codex_home,
            &inputs.codex_home_writable,
        ),
        simple_result_detail("loopback bind", &inputs.loopback_bind),
        format!(
            "shell: {}",
            inputs
                .shell
                .as_deref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "not configured".to_string())
        ),
        simple_result_detail("shell executable", &inputs.shell_executable),
        format!("PATH entries: {}", inputs.path_entries),
    ];

    let hard_failures = [
        ("working directory", &inputs.cwd_readable),
        ("CODEX_HOME writable", &inputs.codex_home_writable),
    ];
    let warnings = [
        ("loopback bind", &inputs.loopback_bind),
        ("shell executable", &inputs.shell_executable),
    ];
    let status = if hard_failures.iter().any(|(_, result)| result.is_err()) {
        CheckStatus::Fail
    } else if warnings.iter().any(|(_, result)| result.is_err()) || inputs.path_entries == 0 {
        CheckStatus::Warning
    } else {
        CheckStatus::Ok
    };
    let summary = match status {
        CheckStatus::Ok => "local execution capabilities are ready",
        CheckStatus::Warning => "local execution has capability limitations",
        CheckStatus::Fail => "local execution has blocking filesystem failures",
    };
    let mut check = DoctorCheck::new("preflight.execution", "preflight", status, summary);
    check.details.append(&mut details);

    for (field, result) in hard_failures {
        if let Err(error) = result {
            check = check.issue(
                DoctorIssue::new(CheckStatus::Fail, format!("{field} check failed"))
                    .measured(error.as_str())
                    .expected("accessible before starting an agent turn")
                    .remedy("Fix the directory path or permissions, then rerun codex doctor --preflight.")
                    .field(field),
            );
        }
    }
    for (field, result) in warnings {
        if let Err(error) = result {
            check = check.issue(
                DoctorIssue::new(CheckStatus::Warning, format!("{field} is unavailable"))
                    .measured(error.as_str())
                    .expected("available for tools that require this capability")
                    .remedy(capability_remedy(field))
                    .field(field),
            );
        }
    }
    if inputs.path_entries == 0 {
        check = check.issue(
            DoctorIssue::new(CheckStatus::Warning, "PATH is empty or unavailable")
                .expected("at least one PATH entry")
                .remedy("Set PATH before starting Codex so shell commands can resolve tools.")
                .field("PATH entries"),
        );
    }
    check
}

fn configured_shell() -> Option<PathBuf> {
    #[cfg(windows)]
    let shell = env::var_os("COMSPEC");
    #[cfg(not(windows))]
    let shell = env::var_os("SHELL").or_else(|| Some("/bin/sh".into()));
    shell.map(PathBuf::from)
}

fn probe_directory_write(directory: &Path) -> Result<(), String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let path = directory.join(format!(".codex-preflight-{}-{nonce}", std::process::id()));
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| error.to_string())?;
    std::fs::remove_file(&path).map_err(|error| error.to_string())
}

fn result_detail(label: &str, path: &Path, result: &Result<(), String>) -> String {
    match result {
        Ok(()) => format!("{label}: {} (ok)", path.display()),
        Err(error) => format!("{label}: {} ({error})", path.display()),
    }
}

fn simple_result_detail(label: &str, result: &Result<(), String>) -> String {
    match result {
        Ok(()) => format!("{label}: available"),
        Err(error) => format!("{label}: unavailable ({error})"),
    }
}

fn capability_remedy(field: &str) -> &'static str {
    match field {
        "loopback bind" => {
            "Allow local loopback binding or run loopback-dependent tests/tools outside the restrictive sandbox."
        }
        "shell executable" => "Fix SHELL/COMSPEC or install the configured shell executable.",
        _ => "Fix the reported local capability, then rerun codex doctor --preflight.",
    }
}

#[cfg(test)]
#[path = "preflight_tests.rs"]
mod tests;
