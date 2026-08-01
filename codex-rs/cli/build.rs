use std::path::Path;
use std::process::Command;

const ENHANCEMENT_REVISION: &str = "enhanced.1";

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-ObjC");
    }

    emit_build_provenance();
}

fn emit_build_provenance() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let source_root = Path::new(&manifest_dir).join("../..");
    let source_root = source_root.canonicalize().unwrap_or(source_root);

    println!("cargo:rustc-env=CODEX_ENHANCEMENT_REVISION={ENHANCEMENT_REVISION}");
    println!(
        "cargo:rustc-env=CODEX_BUILD_SOURCE_ROOT={}",
        source_root.display()
    );
    println!(
        "cargo:rustc-env=CODEX_BUILD_COMMIT={}",
        git_output(&source_root, &["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".to_string())
    );
    println!(
        "cargo:rustc-env=CODEX_BUILD_DIRTY={}",
        match source_status(&source_root) {
            Some(output) if output.is_empty() => "false",
            Some(_) => "true",
            None => "unknown",
        }
    );
    println!(
        "cargo:rustc-env=CODEX_BUILD_SOURCE_FINGERPRINT={}",
        source_fingerprint(&source_root).unwrap_or_else(|| "unknown".to_string())
    );

    // This deliberately missing path makes Cargo refresh provenance on every
    // invocation, including builds after unstaged changes outside this crate.
    println!(
        "cargo:rerun-if-changed={}",
        source_root.join(".codex-build-provenance-always").display()
    );
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
