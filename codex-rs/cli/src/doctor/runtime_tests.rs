use std::fs;
use std::process::Command;

use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::BuildProvenance;
use super::CheckStatus;
use super::git_output;
use super::source_fingerprint;
use super::source_parity;

#[test]
fn source_parity_detects_matching_and_changed_source() {
    let repository = TempDir::new().expect("create repository");
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    git(repository.path(), &["init", "--quiet"]);
    git(repository.path(), &["config", "user.name", "Codex Test"]);
    git(
        repository.path(),
        &["config", "user.email", "codex@example.invalid"],
    );
    fs::write(repository.path().join("tracked.txt"), "initial\n").expect("write tracked file");
    git(repository.path(), &["add", "tracked.txt"]);
    git(repository.path(), &["commit", "--quiet", "-m", "initial"]);

    let commit = git_output(repository.path(), &["rev-parse", "HEAD"]).expect("read commit");
    let source_root = repository.path().to_string_lossy();
    let fingerprint = source_fingerprint(repository.path()).expect("fingerprint source");
    let build = BuildProvenance {
        commit: &commit,
        dirty: "false",
        enhancement_revision: "test",
        source_root: &source_root,
        source_fingerprint: &fingerprint,
    };
    let matched = source_parity(&build);
    assert_eq!(matched.status, CheckStatus::Ok);
    assert_eq!(matched.summary, "matched");

    fs::write(repository.path().join("tracked.txt"), "changed\n").expect("change tracked file");
    let mismatched = source_parity(&build);
    assert_eq!(mismatched.status, CheckStatus::Warning);
    assert_eq!(mismatched.summary, "mismatched");
}

fn git(repository: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .status()
        .expect("run git");
    assert!(status.success(), "git command failed: {args:?}");
}
