use anyhow::Context;
use anyhow::Result;
use pretty_assertions::assert_eq;

use super::*;

#[tokio::test]
async fn small_stream_stays_inline() {
    assert_eq!(
        capture_stream(&b"hook output"[..], "stdout").await,
        "hook output"
    );
}

#[tokio::test]
async fn oversized_stream_is_bounded_and_spilled_while_reading() -> Result<()> {
    let output = vec![b'x'; INLINE_OUTPUT_LIMIT_BYTES + 1024];
    let captured = capture_stream(output.as_slice(), "stdout").await;
    let path = captured
        .lines()
        .last()
        .and_then(|line| line.strip_prefix("[hook stdout truncated; full output saved to: "))
        .and_then(|line| line.strip_suffix(']'))
        .context("spill path footer")?;

    assert!(captured.len() < output.len() + 512);
    assert_eq!(fs::read(path).await?, output);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        assert_eq!(fs::metadata(path).await?.permissions().mode() & 0o777, 0o600);
    }
    fs::remove_file(path).await?;
    Ok(())
}
