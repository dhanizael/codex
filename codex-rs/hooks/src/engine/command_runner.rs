use std::io::ErrorKind;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::timeout;
use tracing::Span;
use tracing::warn;

use super::CommandShell;
use super::ConfiguredHandler;
use super::command_stream::capture_stream;
use super::dispatcher::hook_event_name_label;
use super::dispatcher::hook_execution_mode_label;
use super::dispatcher::hook_handler_type_label;
use super::dispatcher::hook_scope_label;
use super::dispatcher::hook_source_label;
use super::dispatcher::scope_for_event;
use codex_protocol::protocol::HookExecutionMode;
use codex_protocol::protocol::HookHandlerType;

#[derive(Debug)]
pub(crate) struct CommandRunResult {
    pub started_at: i64,
    pub completed_at: i64,
    pub duration_ms: i64,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub error: Option<String>,
}

#[tracing::instrument(
    name = "codex.hooks.command",
    level = "trace",
    skip_all,
    fields(
        hook.event_name = hook_event_name_label(handler.event_name),
        hook.handler_type = hook_handler_type_label(HookHandlerType::Command),
        hook.execution_mode = hook_execution_mode_label(HookExecutionMode::Sync),
        hook.scope = hook_scope_label(scope_for_event(handler.event_name)),
        hook.source = hook_source_label(handler.source),
        hook.display_order = handler.display_order,
        hook.configured_order = configured_order,
        hook.timeout_sec = handler.timeout_sec,
        hook.command_outcome = tracing::field::Empty,
    )
)]
pub(crate) async fn run_command(
    shell: &CommandShell,
    handler: &ConfiguredHandler,
    configured_order: usize,
    input_json: &str,
    cwd: &Path,
) -> CommandRunResult {
    let started_at = chrono::Utc::now().timestamp();
    let started = Instant::now();

    let mut command = build_command(shell, handler);
    command
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    #[cfg(unix)]
    command.process_group(0);

    #[cfg(windows)]
    let job = codex_utils_pty::JobObject::create().ok();

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            return finish_command_run(
                started_at,
                started,
                CommandRunCompletion {
                    exit_code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    error: Some(err.to_string()),
                    outcome: "spawn_error",
                },
            );
        }
    };

    let mut process_tree = ProcessTree::attach(
        &mut child,
        #[cfg(windows)]
        job,
    );

    let stdout_task = child
        .stdout
        .take()
        .map(|stdout| tokio::spawn(capture_stream(stdout, "stdout")));
    let stderr_task = child
        .stderr
        .take()
        .map(|stderr| tokio::spawn(capture_stream(stderr, "stderr")));

    if let Some(mut stdin) = child.stdin.take()
        && let Err(err) = stdin.write_all(input_json.as_bytes()).await
        && err.kind() != ErrorKind::BrokenPipe
    {
        process_tree.terminate(&mut child).await;
        let (stdout, stderr) = collect_outputs(stdout_task, stderr_task).await;
        return finish_command_run(
            started_at,
            started,
            CommandRunCompletion {
                exit_code: None,
                stdout,
                stderr,
                error: Some(format!("failed to write hook stdin: {err}")),
                outcome: "stdin_error",
            },
        );
    }

    let timeout_duration = Duration::from_secs(handler.timeout_sec);
    match timeout(timeout_duration, child.wait()).await {
        Ok(Ok(status)) => {
            process_tree.disarm();
            let (stdout, stderr) = collect_outputs(stdout_task, stderr_task).await;
            finish_command_run(
                started_at,
                started,
                CommandRunCompletion {
                    exit_code: status.code(),
                    stdout,
                    stderr,
                    error: None,
                    outcome: "completed",
                },
            )
        }
        Ok(Err(err)) => {
            process_tree.terminate(&mut child).await;
            let (stdout, stderr) = collect_outputs(stdout_task, stderr_task).await;
            finish_command_run(
                started_at,
                started,
                CommandRunCompletion {
                    exit_code: None,
                    stdout,
                    stderr,
                    error: Some(err.to_string()),
                    outcome: "wait_error",
                },
            )
        }
        Err(_) => {
            process_tree.terminate(&mut child).await;
            let (stdout, stderr) = collect_outputs(stdout_task, stderr_task).await;
            finish_command_run(
                started_at,
                started,
                CommandRunCompletion {
                    exit_code: None,
                    stdout,
                    stderr,
                    error: Some(format!("hook timed out after {}s", handler.timeout_sec)),
                    outcome: "timeout",
                },
            )
        }
    }
}

async fn collect_outputs(
    stdout_task: Option<tokio::task::JoinHandle<String>>,
    stderr_task: Option<tokio::task::JoinHandle<String>>,
) -> (String, String) {
    let stdout = async { join_output(stdout_task).await };
    let stderr = async { join_output(stderr_task).await };
    tokio::join!(stdout, stderr)
}

async fn join_output(task: Option<tokio::task::JoinHandle<String>>) -> String {
    match task {
        Some(task) => task.await.unwrap_or_default(),
        None => String::new(),
    }
}

struct ProcessTree {
    armed: bool,
    #[cfg(unix)]
    process_group_id: Option<u32>,
    #[cfg(windows)]
    job: Option<codex_utils_pty::JobObject>,
}

impl ProcessTree {
    fn attach(
        child: &mut tokio::process::Child,
        #[cfg(windows)] job: Option<codex_utils_pty::JobObject>,
    ) -> Self {
        #[cfg(unix)]
        let process_group_id = child.id();
        #[cfg(windows)]
        let job = job.and_then(|job| {
            let handle = child.raw_handle()?;
            if let Err(error) = job.assign_process(handle) {
                warn!("failed to contain hook process tree: {error}");
                return None;
            }
            Some(job)
        });
        Self {
            armed: true,
            #[cfg(unix)]
            process_group_id,
            #[cfg(windows)]
            job,
        }
    }

    async fn terminate(&mut self, child: &mut tokio::process::Child) {
        self.kill_tree();
        self.armed = false;

        if !matches!(child.try_wait(), Ok(Some(_))) {
            let _ = child.kill().await;
        }
    }

    fn disarm(&mut self) {
        #[cfg(windows)]
        if let Some(job) = self.job.as_ref()
            && let Err(error) = job.preserve_descendants()
        {
            warn!("failed to preserve hook descendants after normal exit: {error}");
        }
        self.armed = false;
    }

    fn kill_tree(&self) {
        #[cfg(unix)]
        if let Some(process_group_id) = self.process_group_id
            && let Err(error) = codex_utils_pty::process_group::kill_process_group(process_group_id)
        {
            warn!("failed to terminate hook process group {process_group_id}: {error}");
        }

        #[cfg(windows)]
        if let Some(job) = self.job.as_ref()
            && let Err(error) = job.terminate()
        {
            warn!("failed to terminate hook job: {error}");
        }
    }
}

impl Drop for ProcessTree {
    fn drop(&mut self) {
        if self.armed {
            self.kill_tree();
        }
    }
}

struct CommandRunCompletion {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    error: Option<String>,
    outcome: &'static str,
}

fn finish_command_run(
    started_at: i64,
    started: Instant,
    completion: CommandRunCompletion,
) -> CommandRunResult {
    Span::current().record("hook.command_outcome", completion.outcome);
    CommandRunResult {
        started_at,
        completed_at: chrono::Utc::now().timestamp(),
        duration_ms: started.elapsed().as_millis().try_into().unwrap_or(i64::MAX),
        exit_code: completion.exit_code,
        stdout: completion.stdout,
        stderr: completion.stderr,
        error: completion.error,
    }
}

fn build_command(shell: &CommandShell, handler: &ConfiguredHandler) -> Command {
    let mut command = if shell.program.is_empty() {
        default_shell_command()
    } else {
        Command::new(&shell.program)
    };
    if shell.program.is_empty() {
        #[cfg(windows)]
        command.raw_arg(format!(r#""{}""#, handler.command));

        #[cfg(not(windows))]
        command.arg(&handler.command);
    } else {
        command.args(&shell.args);

        #[cfg(windows)]
        if shell.args.iter().any(|arg| arg.eq_ignore_ascii_case("/c")) {
            command.raw_arg(format!(r#""{}""#, handler.command));
        } else {
            command.arg(&handler.command);
        }

        #[cfg(not(windows))]
        command.arg(&handler.command);
    }
    command.envs(&handler.env);
    command
}

fn default_shell_command() -> Command {
    #[cfg(windows)]
    {
        let comspec = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string());
        let mut command = Command::new(comspec);
        command.arg("/C");
        command
    }

    #[cfg(not(windows))]
    {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let mut command = Command::new(shell);
        command.arg("-lc");
        command
    }
}

#[cfg(test)]
#[path = "command_runner_tests.rs"]
mod tests;
