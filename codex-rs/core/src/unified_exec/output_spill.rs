use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

pub(super) struct OutputSpill {
    file: File,
    path: PathBuf,
}

impl OutputSpill {
    pub(super) fn create(codex_home: &Path, session_id: &str, call_id: &str) -> io::Result<Self> {
        let directory = codex_home.join("tool-output").join(session_id);
        std::fs::create_dir_all(&directory)?;
        set_private_directory_permissions(&directory)?;

        let filename = format!("{}.log", codex_config::sha256_hex(call_id.as_bytes()));
        let path = directory.join(filename);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let file = options.open(&path)?;
        Ok(Self { file, path })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.file.write_all(bytes)
    }
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
}
