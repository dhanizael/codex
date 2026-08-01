use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Duration;
use std::time::SystemTime;

pub(super) const OUTPUT_ROOT: &str = "tool-output";
pub(super) const ACTIVE_MARKER_SUFFIX: &str = ".active";
const DEFAULT_RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const DEFAULT_MAX_BYTES: u64 = 512 * 1024 * 1024;

static CLEANED_HOMES: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

#[derive(Clone, Copy)]
struct CleanupPolicy {
    retention: Duration,
    max_bytes: u64,
}

impl Default for CleanupPolicy {
    fn default() -> Self {
        Self {
            retention: DEFAULT_RETENTION,
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }
}

#[derive(Debug)]
struct Candidate {
    path: PathBuf,
    modified: SystemTime,
    bytes: u64,
    eligible: bool,
}

pub(super) fn schedule_cleanup_once(codex_home: &Path, protected_session: &str) {
    let cleaned_homes = CLEANED_HOMES.get_or_init(|| Mutex::new(HashSet::new()));
    let mut cleaned_homes = cleaned_homes
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !cleaned_homes.insert(codex_home.to_path_buf()) {
        return;
    }
    drop(cleaned_homes);

    let codex_home = codex_home.to_path_buf();
    let protected_session = protected_session.to_string();
    if let Err(error) = std::thread::Builder::new()
        .name("codex-output-cleanup".to_string())
        .spawn(move || {
            if let Err(error) = cleanup(
                &codex_home,
                &protected_session,
                CleanupPolicy::default(),
                SystemTime::now(),
            ) {
                tracing::warn!(
                    %error,
                    path = %codex_home.join(OUTPUT_ROOT).display(),
                    "failed to clean unified exec output spills"
                );
            }
        })
    {
        tracing::warn!(
            %error,
            "failed to schedule unified exec output cleanup"
        );
    }
}

fn cleanup(
    codex_home: &Path,
    protected_session: &str,
    policy: CleanupPolicy,
    now: SystemTime,
) -> io::Result<()> {
    let root = codex_home.join(OUTPUT_ROOT);
    let session_entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };

    let mut candidates = Vec::new();
    for session_entry in session_entries {
        let session_entry = session_entry?;
        if !session_entry.file_type()?.is_dir() {
            continue;
        }
        let session_path = session_entry.path();
        let protected = session_entry.file_name().to_string_lossy() == protected_session;
        let active = session_is_active(&session_path)?;
        for entry in fs::read_dir(&session_path)? {
            let entry = entry?;
            if !entry.file_type()?.is_file()
                || entry.path().extension().and_then(|value| value.to_str()) != Some("log")
            {
                continue;
            }
            let metadata = entry.metadata()?;
            candidates.push(Candidate {
                path: entry.path(),
                modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                bytes: metadata.len(),
                eligible: !protected && !active,
            });
        }
    }

    let mut total_bytes = candidates
        .iter()
        .map(|candidate| candidate.bytes)
        .sum::<u64>();
    for candidate in &mut candidates {
        let expired = now
            .duration_since(candidate.modified)
            .ok()
            .is_some_and(|age| age >= policy.retention);
        if candidate.eligible && expired && remove_candidate(candidate)? {
            total_bytes = total_bytes.saturating_sub(candidate.bytes);
            candidate.bytes = 0;
        }
    }

    candidates.sort_by(|left, right| match left.modified.cmp(&right.modified) {
        Ordering::Equal => left.path.cmp(&right.path),
        ordering => ordering,
    });
    for candidate in &mut candidates {
        if total_bytes <= policy.max_bytes {
            break;
        }
        if candidate.eligible && candidate.bytes > 0 && remove_candidate(candidate)? {
            total_bytes = total_bytes.saturating_sub(candidate.bytes);
            candidate.bytes = 0;
        }
    }
    Ok(())
}

fn remove_candidate(candidate: &Candidate) -> io::Result<bool> {
    match fs::remove_file(&candidate.path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn session_is_active(session_path: &Path) -> io::Result<bool> {
    for entry in fs::read_dir(session_path)? {
        let entry = entry?;
        let path = entry.path();
        if !entry.file_type()?.is_file()
            || !path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.ends_with(ACTIVE_MARKER_SUFFIX))
        {
            continue;
        }
        let pid = fs::read_to_string(&path)
            .ok()
            .and_then(|value| value.trim().parse::<u32>().ok());
        if pid.is_some_and(process_is_alive) {
            return Ok(true);
        }
        #[cfg(not(target_os = "linux"))]
        return Ok(true);
        #[cfg(target_os = "linux")]
        if let Err(error) = fs::remove_file(&path)
            && error.kind() != io::ErrorKind::NotFound
        {
            return Err(error);
        }
    }
    Ok(false)
}

#[cfg(target_os = "linux")]
fn process_is_alive(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

#[cfg(not(target_os = "linux"))]
fn process_is_alive(_pid: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::FileTimes;

    #[test]
    fn cleanup_removes_expired_then_oldest_until_under_budget() {
        let home = tempfile::tempdir().expect("create temp codex home");
        let now = SystemTime::now();
        let expired = write_log(
            home.path(),
            "expired",
            "expired.log",
            4,
            now - Duration::from_secs(20),
        );
        let oldest = write_log(
            home.path(),
            "oldest",
            "oldest.log",
            4,
            now - Duration::from_secs(8),
        );
        let newest = write_log(
            home.path(),
            "newest",
            "newest.log",
            4,
            now - Duration::from_secs(4),
        );

        cleanup(
            home.path(),
            "current",
            CleanupPolicy {
                retention: Duration::from_secs(10),
                max_bytes: 4,
            },
            now,
        )
        .expect("clean output spills");

        assert!(!expired.exists());
        assert!(!oldest.exists());
        assert!(newest.exists());
    }

    #[test]
    fn cleanup_preserves_current_and_active_sessions_even_over_budget() {
        let home = tempfile::tempdir().expect("create temp codex home");
        let now = SystemTime::now();
        let current = write_log(home.path(), "current", "current.log", 4, now);
        let active = write_log(home.path(), "active", "active.log", 4, now);
        fs::write(
            active
                .parent()
                .expect("active session directory")
                .join(format!("call{ACTIVE_MARKER_SUFFIX}")),
            std::process::id().to_string(),
        )
        .expect("write active marker");

        cleanup(
            home.path(),
            "current",
            CleanupPolicy {
                retention: Duration::ZERO,
                max_bytes: 0,
            },
            now,
        )
        .expect("clean output spills");

        assert!(current.exists());
        assert!(active.exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cleanup_reclaims_session_with_dead_process_marker() {
        let home = tempfile::tempdir().expect("create temp codex home");
        let now = SystemTime::now();
        let stale = write_log(home.path(), "stale", "stale.log", 4, now);
        let marker = stale
            .parent()
            .expect("stale session directory")
            .join(format!("call{ACTIVE_MARKER_SUFFIX}"));
        fs::write(&marker, u32::MAX.to_string()).expect("write dead process marker");

        cleanup(
            home.path(),
            "current",
            CleanupPolicy {
                retention: Duration::ZERO,
                max_bytes: 0,
            },
            now,
        )
        .expect("clean output spills");

        assert!(!marker.exists());
        assert!(!stale.exists());
    }

    fn write_log(
        home: &Path,
        session: &str,
        name: &str,
        bytes: usize,
        modified: SystemTime,
    ) -> PathBuf {
        let directory = home.join(OUTPUT_ROOT).join(session);
        fs::create_dir_all(&directory).expect("create session directory");
        let path = directory.join(name);
        fs::write(&path, vec![b'x'; bytes]).expect("write log");
        let file = fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open log");
        file.set_times(FileTimes::new().set_modified(modified))
            .expect("set log mtime");
        path
    }
}
