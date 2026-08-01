use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use super::output_cleanup::ACTIVE_MARKER_SUFFIX;
use super::output_cleanup::OUTPUT_ROOT;
use super::output_cleanup::schedule_cleanup_once;

pub(super) struct OutputSpill {
    file: File,
    path: PathBuf,
    active_marker_path: PathBuf,
}

impl OutputSpill {
    pub(super) fn create(codex_home: &Path, session_id: &str, call_id: &str) -> io::Result<Self> {
        schedule_cleanup_once(codex_home, session_id);
        let directory = codex_home.join(OUTPUT_ROOT).join(session_id);
        std::fs::create_dir_all(&directory)?;
        set_private_directory_permissions(&directory)?;

        let call_hash = codex_config::sha256_hex(call_id.as_bytes());
        let active_marker_path = directory.join(format!("{call_hash}{ACTIVE_MARKER_SUFFIX}"));
        create_active_marker(&active_marker_path)?;
        let filename = format!("{call_hash}.log");
        let path = directory.join(filename);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let file = match options.open(&path) {
            Ok(file) => file,
            Err(error) => {
                let _ = std::fs::remove_file(&active_marker_path);
                return Err(error);
            }
        };
        Ok(Self {
            file,
            path,
            active_marker_path,
        })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.file.write_all(bytes)
    }
}

impl Drop for OutputSpill {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_file(&self.active_marker_path)
            && error.kind() != io::ErrorKind::NotFound
        {
            tracing::warn!(
                %error,
                path = %self.active_marker_path.display(),
                "failed to remove unified exec active marker"
            );
        }
    }
}

fn create_active_marker(path: &Path) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut marker = options.open(path)?;
    write!(marker, "{}", std::process::id())
}

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spill_preserves_exact_bytes_in_private_file() {
        let home = tempfile::tempdir().expect("create temp codex home");
        let mut spill =
            OutputSpill::create(home.path(), "session-id", "call/id").expect("create output spill");
        spill.write(b"stdout\0stderr\n").expect("write spill");

        assert_eq!(
            std::fs::read(spill.path()).expect("read spill"),
            b"stdout\0stderr\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mode = std::fs::metadata(spill.path())
                .expect("read spill metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn spill_refuses_to_replace_an_existing_call_file() {
        let home = tempfile::tempdir().expect("create temp codex home");
        let first = OutputSpill::create(home.path(), "session-id", "same-call")
            .expect("create first spill");

        let error = OutputSpill::create(home.path(), "session-id", "same-call")
            .err()
            .expect("second spill must fail");

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert!(first.path().exists());
    }

    #[test]
    fn spill_removes_active_marker_on_drop() {
        let home = tempfile::tempdir().expect("create temp codex home");
        let spill =
            OutputSpill::create(home.path(), "session-id", "call-id").expect("create output spill");
        let marker = spill.active_marker_path.clone();
        assert!(marker.exists());

        drop(spill);

        assert!(!marker.exists());
    }
}
