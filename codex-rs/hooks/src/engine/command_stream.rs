use std::collections::VecDeque;
use std::fs::OpenOptions;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use tokio::fs;
use tokio::fs::File;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tracing::warn;
use uuid::Uuid;

const INLINE_OUTPUT_LIMIT_BYTES: usize = 1024 * 1024;
const PREVIEW_HEAD_BYTES: usize = INLINE_OUTPUT_LIMIT_BYTES / 2;
const PREVIEW_TAIL_BYTES: usize = INLINE_OUTPUT_LIMIT_BYTES / 2;

pub(super) async fn capture_stream<R>(mut reader: R, stream_name: &str) -> String
where
    R: AsyncRead + Unpin,
{
    let mut inline = Vec::new();
    let mut tail = VecDeque::with_capacity(PREVIEW_TAIL_BYTES);
    let mut spill: Option<(PathBuf, File)> = None;
    let mut buffer = vec![0; 8 * 1024];

    loop {
        let count = match reader.read(&mut buffer).await {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) => {
                warn!("failed to read hook {stream_name}: {error}");
                break;
            }
        };
        let chunk = &buffer[..count];

        if spill.is_none() && inline.len().saturating_add(chunk.len()) <= INLINE_OUTPUT_LIMIT_BYTES
        {
            inline.extend_from_slice(chunk);
            continue;
        }

        if spill.is_none() {
            match create_spill_file(stream_name).await {
                Ok((path, mut file)) => {
                    if let Err(error) = file.write_all(&inline).await {
                        warn!(
                            "failed to initialize hook output spill {}: {error}",
                            path.display()
                        );
                    } else {
                        spill = Some((path, file));
                    }
                }
                Err(error) => warn!("failed to create hook output spill: {error}"),
            }
        }

        if let Some((path, file)) = spill.as_mut()
            && let Err(error) = file.write_all(chunk).await
        {
            warn!(
                "failed to write hook output spill {}: {error}",
                path.display()
            );
            spill = None;
        }
        append_bounded_preview(&mut inline, &mut tail, chunk);
    }

    if let Some((path, mut file)) = spill {
        if let Err(error) = file.flush().await {
            warn!(
                "failed to flush hook output spill {}: {error}",
                path.display()
            );
        }
        let mut preview = inline;
        preview.extend(tail);
        let preview = String::from_utf8_lossy(&preview);
        return format!(
            "{preview}\n\n[hook {stream_name} truncated; full output saved to: {}]",
            path.display()
        );
    }

    String::from_utf8_lossy(&inline).to_string()
}

fn append_bounded_preview(inline: &mut Vec<u8>, tail: &mut VecDeque<u8>, chunk: &[u8]) {
    if inline.len() > PREVIEW_HEAD_BYTES {
        inline.truncate(PREVIEW_HEAD_BYTES);
    }
    for byte in chunk {
        if tail.len() == PREVIEW_TAIL_BYTES {
            tail.pop_front();
        }
        tail.push_back(*byte);
    }
}

async fn create_spill_file(stream_name: &str) -> std::io::Result<(PathBuf, File)> {
    let directory = std::env::temp_dir()
        .join("hook_outputs")
        .join("command_runs");
    fs::create_dir_all(&directory).await?;
    #[cfg(unix)]
    fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).await?;
    let path = directory.join(format!("{}-{stream_name}.txt", Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let file = options.open(&path)?;
    Ok((path, File::from_std(file)))
}

#[cfg(test)]
#[path = "command_stream_tests.rs"]
mod tests;
