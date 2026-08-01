use std::collections::HashSet;

use codex_utils_output_truncation::approx_bytes_for_tokens;

pub(super) fn prepare_model_output(text: &str, failed: bool, max_tokens: usize) -> String {
    let text = strip_ansi_sgr(text);
    let text = coalesce_repeated_diagnostics(&text);
    if !failed || text.len() <= approx_bytes_for_tokens(max_tokens) {
        return text;
    }

    let diagnostics = extract_key_diagnostics(&text, max_tokens / 5);
    if diagnostics.is_empty() {
        return text;
    }

    format!(
        "Key diagnostics extracted from full output:\n{diagnostics}\n\nFull output (head/tail preserved below):\n{text}"
    )
}

fn extract_key_diagnostics(text: &str, max_tokens: usize) -> String {
    let max_bytes = approx_bytes_for_tokens(max_tokens);
    let mut seen = HashSet::new();
    let mut diagnostics = String::new();

    for line in text.lines() {
        let trimmed = line.trim_start();
        let is_diagnostic = trimmed.starts_with("error:")
            || trimmed.starts_with("fatal:")
            || trimmed.starts_with("panicked at")
            || trimmed.starts_with("-->")
            || trimmed == "failures:"
            || trimmed.starts_with("test result: FAILED");
        if !is_diagnostic || !seen.insert(trimmed) {
            continue;
        }

        let separator_bytes = usize::from(!diagnostics.is_empty());
        if diagnostics
            .len()
            .saturating_add(separator_bytes)
            .saturating_add(trimmed.len())
            > max_bytes
        {
            break;
        }
        if !diagnostics.is_empty() {
            diagnostics.push('\n');
        }
        diagnostics.push_str(trimmed);
    }

    diagnostics
}

fn strip_ansi_sgr(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut copied_through = 0usize;
    let mut index = 0usize;

    while index + 2 < bytes.len() {
        if bytes[index] != 0x1b || bytes[index + 1] != b'[' {
            index += 1;
            continue;
        }

        let mut end = index + 2;
        while end < bytes.len()
            && (bytes[end].is_ascii_digit() || matches!(bytes[end], b';' | b':'))
        {
            end += 1;
        }
        if end >= bytes.len() || bytes[end] != b'm' {
            index += 1;
            continue;
        }

        output.push_str(&text[copied_through..index]);
        copied_through = end + 1;
        index = copied_through;
    }

    output.push_str(&text[copied_through..]);
    output
}

fn coalesce_repeated_diagnostics(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut lines = text.split_inclusive('\n').peekable();

    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        let is_diagnostic = trimmed.starts_with("warning:") || trimmed.starts_with("error:");
        if !is_diagnostic {
            output.push_str(line);
            continue;
        }

        let mut count = 1usize;
        while lines.peek().is_some_and(|next| *next == line) {
            lines.next();
            count = count.saturating_add(1);
        }

        output.push_str(line);
        if count >= 3 {
            if !line.ends_with('\n') {
                output.push('\n');
            }
            output.push_str(&format!(
                "[same diagnostic repeated {} more times]\n",
                count - 1
            ));
        } else if count == 2 {
            output.push_str(line);
        }
    }

    output
}
