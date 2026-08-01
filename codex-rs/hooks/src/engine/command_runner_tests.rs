use std::collections::HashMap;
use std::fs;
#[cfg(unix)]
use std::time::Duration;

use codex_protocol::protocol::HookEventName;
use codex_protocol::protocol::HookSource;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::CommandShell;
use super::ConfiguredHandler;
use super::run_command;

#[cfg(windows)]
#[tokio::test]
async fn cmd_shell_runs_quoted_hook_command_path() {
    let temp = tempdir().expect("create temp dir");
    let hook_dir = temp.path().join("hook with spaces");
    fs::create_dir(&hook_dir).expect("create hook dir");
    let hook_path = hook_dir.join("hook.cmd");
    fs::write(
        &hook_path,
        "@echo off\r\nif not \"%~1\"==\"notify\" exit /B 7\r\necho hook-ran\r\n",
    )
    .expect("write hook command");
    let source_path =
        AbsolutePathBuf::try_from(hook_path.clone()).expect("absolute hook command path");
    let handler = ConfiguredHandler {
        event_name: HookEventName::SessionStart,
        matcher: None,
        command: format!(r#""{}" notify"#, hook_path.display()),
        timeout_sec: 10,
        status_message: None,
        additional_context_limit: Default::default(),
        source_path,
        source: HookSource::User,
        display_order: 0,
        env: HashMap::new(),
    };
    let shells = [
        CommandShell {
            program: String::new(),
            args: Vec::new(),
        },
        CommandShell {
            program: std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string()),
            args: vec!["/c".to_string()],
        },
    ];

    for shell in shells {
        let result = run_command(
            &shell,
            &handler,
            /*configured_order*/ 0,
            "{}",
            temp.path(),
        )
        .await;

        assert_eq!(result.exit_code, Some(0), "stderr: {}", result.stderr);
        assert_eq!(result.stdout.trim(), "hook-ran");
        assert!(result.error.is_none());
    }
}

#[tokio::test]
async fn fast_exiting_hook_preserves_stdout_when_stdin_is_not_consumed() {
    let temp = tempdir().expect("create temp dir");
    let source_path = AbsolutePathBuf::try_from(temp.path().join("hooks.json"))
        .expect("absolute hook configuration path");
    let handler = ConfiguredHandler {
        event_name: HookEventName::SessionStart,
        matcher: None,
        command: "echo hook-ran".to_string(),
        timeout_sec: 10,
        status_message: None,
        additional_context_limit: Default::default(),
        source_path,
        source: HookSource::User,
        display_order: 0,
        env: HashMap::new(),
    };
    let shell = CommandShell {
        program: String::new(),
        args: Vec::new(),
    };
    let input_json = format!(r#"{{"padding":"{}"}}"#, "x".repeat(1024 * 1024));

    let result = run_command(
        &shell,
        &handler,
        /*configured_order*/ 0,
        &input_json,
        temp.path(),
    )
    .await;

    assert_eq!(result.exit_code, Some(0), "stderr: {}", result.stderr);
    assert_eq!(result.stdout.trim(), "hook-ran");
    assert_eq!(result.error, None);
}

#[cfg(unix)]
fn unix_handler(temp: &tempfile::TempDir, command: &str, timeout_sec: u64) -> ConfiguredHandler {
    ConfiguredHandler {
        event_name: HookEventName::SessionStart,
        matcher: None,
        command: command.to_string(),
        timeout_sec,
        status_message: None,
        additional_context_limit: Default::default(),
        source_path: AbsolutePathBuf::try_from(temp.path().join("hooks.json"))
            .expect("absolute hook configuration path"),
        source: HookSource::User,
        display_order: 0,
        env: HashMap::new(),
    }
}

#[cfg(unix)]
#[tokio::test]
async fn noisy_hook_spills_during_capture_and_returns_bounded_preview() {
    let temp = tempdir().expect("create temp dir");
    let handler = unix_handler(&temp, "yes x | head -c 1100000", /*timeout_sec*/ 10);
    let result = run_command(
        &CommandShell {
            program: String::new(),
            args: Vec::new(),
        },
        &handler,
        /*configured_order*/ 0,
        "{}",
        temp.path(),
    )
    .await;

    assert_eq!(result.exit_code, Some(0), "stderr: {}", result.stderr);
    assert!(result.stdout.len() < 1_100_000);
    assert!(result.stdout.contains("full output saved to:"));
    let spill_path = result
        .stdout
        .lines()
        .last()
        .and_then(|line| line.split_once("full output saved to: "))
        .map(|(_, path)| path.trim_end_matches(']'))
        .expect("spill path footer");
    fs::remove_file(spill_path).expect("remove hook output spill");
}

#[cfg(unix)]
#[tokio::test]
async fn timed_out_hook_preserves_partial_output_and_kills_descendants() {
    let temp = tempdir().expect("create temp dir");
    let pid_path = temp.path().join("descendant.pid");
    let command = format!(
        "sleep 30 & child=$!; printf '%s' \"$child\" > '{}'; printf partial; wait",
        pid_path.display()
    );
    let handler = unix_handler(&temp, &command, /*timeout_sec*/ 1);
    let result = run_command(
        &CommandShell {
            program: String::new(),
            args: Vec::new(),
        },
        &handler,
        /*configured_order*/ 0,
        "{}",
        temp.path(),
    )
    .await;

    assert_eq!(result.exit_code, None);
    assert_eq!(result.stdout, "partial");
    assert_eq!(result.error.as_deref(), Some("hook timed out after 1s"));
    let pid = fs::read_to_string(&pid_path)
        .expect("read descendant pid")
        .parse::<u32>()
        .expect("parse descendant pid");
    assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
}

#[cfg(unix)]
#[tokio::test]
async fn cancelled_hook_kills_descendants() {
    let temp = tempdir().expect("create temp dir");
    let pid_path = temp.path().join("cancelled-descendant.pid");
    let command = format!(
        "sleep 30 & child=$!; printf '%s' \"$child\" > '{}'; wait",
        pid_path.display()
    );
    let handler = unix_handler(&temp, &command, /*timeout_sec*/ 30);
    let cwd = temp.path().to_path_buf();
    let task = tokio::spawn(async move {
        run_command(
            &CommandShell {
                program: String::new(),
                args: Vec::new(),
            },
            &handler,
            /*configured_order*/ 0,
            "{}",
            &cwd,
        )
        .await
    });

    for _ in 0..100 {
        if pid_path.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let pid = fs::read_to_string(&pid_path)
        .expect("read descendant pid")
        .parse::<u32>()
        .expect("parse descendant pid");
    task.abort();
    let _ = task.await;

    for _ in 0..100 {
        if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("cancelled hook descendant {pid} is still running");
}
