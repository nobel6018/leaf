//! 구독 로그인된 AI CLI 호출. API 키가 없으므로 HTTP API 대신 CLI를 하위 프로세스로 부른다(sori와 같은 방식).
//!
//! 도구는 모두 끄고 빈 임시 폴더에서 돌린다. OCR 글자에 지시문이 섞여 와도 모델이 파일을 읽거나 명령을 실행할 수 없다.
//!
//! @see docs/ai.md

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::AiError;

/// 테스트와 설정에서 claude 실행 파일을 바꿔 끼우는 환경 변수
pub const CLAUDE_BIN_ENV: &str = "LEAF_CLAUDE_BIN";
pub const CODEX_BIN_ENV: &str = "LEAF_CODEX_BIN";

/// 팜플렛 한 부 정리는 보통 1분 안쪽이다. 넉넉히 잡되 멈춘 프로세스를 영원히 기다리지는 않는다
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Provider {
    #[default]
    Claude,
    Codex,
}

impl Provider {
    pub fn program_name(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
        }
    }

    fn env_var(self) -> &'static str {
        match self {
            Provider::Claude => CLAUDE_BIN_ENV,
            Provider::Codex => CODEX_BIN_ENV,
        }
    }

    /// summary.json `generatedBy`에 남기는 이름
    pub fn label(self) -> &'static str {
        match self {
            Provider::Claude => "claude -p",
            Provider::Codex => "codex exec",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub provider: Provider,
    /// 실행 파일 경로. 없으면 환경 변수(`LEAF_CLAUDE_BIN`, `LEAF_CODEX_BIN`), 그다음 로그인 셸 PATH에서 찾는다
    pub program: Option<PathBuf>,
    pub timeout: Duration,
    /// 비우면 CLI 기본 모델
    pub model: Option<String>,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: Provider::Claude,
            program: None,
            timeout: DEFAULT_TIMEOUT,
            model: None,
        }
    }
}

impl LlmConfig {
    pub fn with_program(program: impl Into<PathBuf>) -> Self {
        Self {
            program: Some(program.into()),
            ..Self::default()
        }
    }

    pub fn resolve_program(&self) -> Result<PathBuf, AiError> {
        let not_found = || AiError::CliNotFound {
            program: self.provider.program_name().to_string(),
        };
        if let Some(program) = &self.program {
            return if program.is_file() {
                Ok(program.clone())
            } else {
                Err(not_found())
            };
        }
        if let Some(from_env) = std::env::var_os(self.provider.env_var()).filter(|v| !v.is_empty())
        {
            let path = PathBuf::from(from_env);
            return if path.is_file() {
                Ok(path)
            } else {
                Err(not_found())
            };
        }
        find_in_path(self.provider.program_name()).ok_or_else(not_found)
    }
}

/// 프롬프트를 표준 입력으로 넘기고 모델 응답 글자를 받는다
pub fn run_prompt(config: &LlmConfig, prompt: &str) -> Result<String, AiError> {
    let program = config.resolve_program()?;
    let name = config.provider.program_name().to_string();

    let workdir = std::env::temp_dir().join(format!("leaf-ai-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workdir).map_err(crate::error::io_error(&workdir))?;
    let output_file = workdir.join("answer.txt");
    let result = run_in(config, &program, &name, prompt, &workdir, &output_file);
    let _ = std::fs::remove_dir_all(&workdir);
    result
}

fn run_in(
    config: &LlmConfig,
    program: &Path,
    name: &str,
    prompt: &str,
    workdir: &Path,
    output_file: &Path,
) -> Result<String, AiError> {
    let mut cmd = Command::new(program);
    match config.provider {
        Provider::Claude => {
            cmd.args([
                "-p",
                "--tools",
                "",
                "--no-session-persistence",
                "--strict-mcp-config",
                "--output-format",
                "text",
            ]);
            if let Some(model) = &config.model {
                cmd.args(["--model", model]);
            }
        }
        Provider::Codex => {
            cmd.args([
                "exec",
                "--sandbox",
                "read-only",
                "--skip-git-repo-check",
                "--ephemeral",
                "--color",
                "never",
            ]);
            if let Some(model) = &config.model {
                cmd.args(["--model", model]);
            }
            cmd.arg("-o").arg(output_file).arg("-");
        }
    }
    // PATH에서 찾은 CLI는 같은 PATH로 돌린다(npm 설치판은 node를 PATH에서 찾는다)
    let searched =
        config.program.is_none() && std::env::var_os(config.provider.env_var()).is_none();
    if searched && let Some(path) = login_path() {
        cmd.env("PATH", path);
    }
    cmd.current_dir(workdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // 시간 초과 때 CLI가 띄운 하위 프로세스(node 등)까지 한 번에 끝내려고 새 프로세스 그룹으로 띄운다
        cmd.process_group(0);
    }

    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AiError::CliNotFound {
                program: name.to_string(),
            }
        } else {
            AiError::CliFailed {
                program: name.to_string(),
                code: None,
                detail: format!("실행하지 못했습니다: {e}"),
            }
        }
    })?;

    // 긴 프롬프트를 쓰는 동안 CLI가 출력을 내면 파이프가 막힐 수 있어 입출력을 각각 스레드로 돌린다
    let mut stdin = child.stdin.take().expect("stdin은 piped로 열었다");
    let prompt_bytes = prompt.as_bytes().to_vec();
    let writer = thread::spawn(move || {
        let _ = stdin.write_all(&prompt_bytes);
    });
    let stdout_reader = spawn_reader(child.stdout.take());
    let stderr_reader = spawn_reader(child.stderr.take());

    let status = match wait_with_timeout(&mut child, config.timeout) {
        Some(status) => status,
        None => {
            kill_tree(&mut child);
            return Err(AiError::Timeout {
                program: name.to_string(),
                secs: config.timeout.as_secs(),
            });
        }
    };
    // CLI가 끝난 뒤에도 남은 하위 프로세스가 파이프를 쥐고 있으면 읽기가 끝나지 않는다
    kill_tree(&mut child);
    let _ = writer.join();
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();

    if !status.success() {
        let detail = if stderr.trim().is_empty() {
            &stdout
        } else {
            &stderr
        };
        let detail = tail(detail, 800);
        if looks_like_login_error(&detail) {
            return Err(AiError::NotLoggedIn {
                program: name.to_string(),
                detail,
            });
        }
        return Err(AiError::CliFailed {
            program: name.to_string(),
            code: status.code(),
            detail,
        });
    }

    let text = match config.provider {
        Provider::Claude => stdout,
        Provider::Codex => std::fs::read_to_string(output_file).unwrap_or(stdout),
    };
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(AiError::EmptyResponse {
            program: name.to_string(),
        });
    }
    // 로그인이 풀린 claude는 종료 코드 0으로 안내 문구만 찍기도 한다
    if text.len() < 400 && !text.contains('{') && looks_like_login_error(&text) {
        return Err(AiError::NotLoggedIn {
            program: name.to_string(),
            detail: text,
        });
    }
    Ok(text)
}

fn spawn_reader<R: Read + Send + 'static>(source: Option<R>) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut source) = source {
            let _ = source.read_to_end(&mut bytes);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

fn wait_with_timeout(child: &mut Child, timeout: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) if Instant::now() >= deadline => return None,
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(_) => return None,
        }
    }
}

fn kill_tree(child: &mut Child) {
    #[cfg(unix)]
    {
        let pid = child.id() as libc::pid_t;
        // process_group(0)으로 띄워 그룹 id가 곧 자식 pid다
        unsafe {
            libc::killpg(pid, libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn looks_like_login_error(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "not logged in",
        "/login",
        "please log in",
        "login required",
        "invalid api key",
        "authentication_error",
        "oauth token has expired",
        "codex login",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn tail(text: &str, max_chars: usize) -> String {
    let text = text.trim();
    let start = text
        .char_indices()
        .rev()
        .nth(max_chars.saturating_sub(1))
        .map_or(0, |(i, _)| i);
    text[start..].to_string()
}

/// Finder에서 연 macOS 앱은 /usr/bin:/bin 정도만 물려받아 ~/.local/bin, Homebrew의 CLI를 못 찾는다.
/// 대화형 로그인 셸(-ilc)로 PATH를 묻고, 인사말 같은 다른 출력을 피하려고 표시 문자열 사이 값만 쓴다
fn login_path() -> Option<&'static str> {
    static PATH: OnceLock<Option<String>> = OnceLock::new();
    PATH.get_or_init(|| {
        let inherited = std::env::var("PATH").unwrap_or_default();
        if cfg!(windows) {
            return Some(inherited);
        }
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        let from_shell = Command::new(shell)
            .args(["-ilc", "printf '__LEAF_PATH__%s__LEAF_PATH__' \"$PATH\""])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()
            .and_then(|out| {
                let text = String::from_utf8_lossy(&out.stdout).into_owned();
                text.split("__LEAF_PATH__").nth(1).map(str::to_string)
            })
            .unwrap_or_default();
        let home = std::env::var("HOME").unwrap_or_default();
        let extra = [
            format!("{home}/.local/bin"),
            format!("{home}/.claude/local"),
            format!("{home}/.npm-global/bin"),
            "/opt/homebrew/bin".to_string(),
            "/usr/local/bin".to_string(),
        ];
        let mut parts: Vec<String> = Vec::new();
        for p in from_shell
            .split(':')
            .chain(inherited.split(':'))
            .map(str::to_string)
            .chain(extra)
        {
            if !p.is_empty() && !parts.contains(&p) {
                parts.push(p);
            }
        }
        Some(parts.join(":"))
    })
    .as_deref()
}

fn find_in_path(program: &str) -> Option<PathBuf> {
    let path = login_path()?;
    let suffixes: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };
    std::env::split_paths(path).find_map(|dir| {
        suffixes
            .iter()
            .map(|ext| dir.join(format!("{program}{ext}")))
            .find(|p| p.is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 로그인_안내_문구를_알아본다() {
        assert!(looks_like_login_error("Invalid API key, Please run /login"));
        assert!(looks_like_login_error("Error: Not logged in"));
        assert!(!looks_like_login_error("rate limit exceeded"));
    }

    #[test]
    fn 긴_오류_메시지는_뒤쪽만_남긴다() {
        let text = "가".repeat(1000) + "끝";
        let cut = tail(&text, 10);
        assert_eq!(cut.chars().count(), 10);
        assert!(cut.ends_with('끝'));
    }
}
