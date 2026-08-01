use std::path::PathBuf;

use clap::Parser;
use pretty_assertions::assert_eq;

use crate::doctor::DoctorCommand;

use super::*;

fn ready_inputs() -> PreflightInputs {
    PreflightInputs {
        cwd: PathBuf::from("/work"),
        cwd_readable: Ok(()),
        codex_home: PathBuf::from("/codex-home"),
        codex_home_writable: Ok(()),
        loopback_bind: Ok(()),
        shell: Some(PathBuf::from("/bin/sh")),
        shell_executable: Ok(()),
        path_entries: 3,
    }
}

#[test]
fn ready_preflight_is_ok() {
    let check = execution_preflight_check_from_inputs(ready_inputs());

    assert_eq!(check.status, CheckStatus::Ok);
    assert_eq!(check.summary, "local execution capabilities are ready");
    assert!(check.issues.is_empty());
}

#[test]
fn doctor_command_accepts_preflight_mode() {
    let command = DoctorCommand::try_parse_from(["doctor", "--preflight"])
        .expect("parse doctor preflight command");

    assert!(command.preflight);
}

#[test]
fn filesystem_failure_blocks_preflight() {
    let mut inputs = ready_inputs();
    inputs.codex_home_writable = Err("permission denied".to_string());

    let check = execution_preflight_check_from_inputs(inputs);

    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(check.issues.len(), 1);
    assert_eq!(check.issues[0].severity, CheckStatus::Fail);
}

#[test]
fn optional_capability_failure_warns_without_blocking() {
    let mut inputs = ready_inputs();
    inputs.loopback_bind = Err("operation not permitted".to_string());

    let check = execution_preflight_check_from_inputs(inputs);

    assert_eq!(check.status, CheckStatus::Warning);
    assert_eq!(check.issues.len(), 1);
    assert_eq!(check.issues[0].fields, vec!["loopback bind"]);
}
