//! ✧ commit-message suggestion: run a user-configured Agent CLI oneshot
//! against the pending diff (VS Code sparkle), falling back to a filename
//! heuristic when the CLI is missing, slow, or fails. Runs on a background
//! thread so the TUI stays responsive; the app polls the returned channel
//! from its refresh tick.
//!
//! Generator config is read only from Herdr's user plugin config directory
//! (`commit-message.toml`). Workspace / repo bait files are ignored.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use crate::git::DiffSource;

const CONFIG_FILE: &str = "commit-message.toml";
const MAX_CONFIG_BYTES: usize = 256 * 1024;
const DEFAULT_MAX_DIFF_BYTES: usize = 16 * 1024;
const DEFAULT_MAX_OUTPUT_BYTES: usize = 4096;
const DEFAULT_TIMEOUT_SECS: u64 = 60;
const MAX_TIMEOUT_SECS: u64 = 600;
const MAX_IO_BYTES: u64 = 1024 * 1024;

const PROMPT: &str = "Write a git commit message for the diff on stdin: one imperative \
                      subject line under 72 characters, no quotes, no trailing period. \
                      Reply with ONLY the message line.";

const PROMPT_PLACEHOLDERS: &[&str] = &[
    "diff",
    "files",
    "files_csv",
    "file_count",
    "branch",
    "repo_root",
    "source",
];
const ARGV_PLACEHOLDERS: &[&str] = &["prompt_file", "diff_file"];

static TEMP_SEQ: AtomicU64 = AtomicU64::new(1);

/// How an empty Commit click should use the generator. Sparkle / `A` always
/// only fills the box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoOnEmpty {
    Off,
    Fill,
    FillAndCommit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FallbackPolicy {
    Filenames,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputMode {
    PromptArgvDiffStdin,
    PromptAndDiffStdin,
    ArgvSubst,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputMode {
    LastUsableLine,
    RawTrim,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CwdMode {
    Repo,
    Workspace,
}

#[derive(Clone, Debug)]
struct Profile {
    command: Vec<String>,
    command_windows: Option<Vec<String>>,
    input: InputMode,
    timeout: Duration,
    max_diff_bytes: usize,
    max_output_bytes: usize,
    cwd: CwdMode,
    output: OutputMode,
    prompt: String,
}

#[derive(Clone, Debug)]
struct Config {
    enabled: bool,
    default_profile: String,
    auto_on_empty_commit: AutoOnEmpty,
    fallback: FallbackPolicy,
    profiles: BTreeMap<String, Profile>,
    builtin: bool,
}

impl Config {
    fn active(&self) -> &Profile {
        &self.profiles[&self.default_profile]
    }
}

#[derive(Debug)]
struct LoadResult {
    loaded: Result<Config, String>,
    /// Paths this load actually tried to read. Tests assert bait files never
    /// appear here; production only cares about `loaded`.
    #[allow(dead_code)]
    opened: Vec<PathBuf>,
}

/// Inputs the generator needs from the active SCM repo.
#[derive(Clone, Debug)]
pub struct SuggestRequest {
    pub diff: String,
    pub files: Vec<String>,
    pub source: DiffSource,
    pub branch: String,
    pub repo_root: PathBuf,
    pub workspace: PathBuf,
}

/// One generation attempt. `message` is filled on success and on filename
/// fallback; `generated` is true only when the Agent CLI produced the line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuggestOutcome {
    pub message: Option<String>,
    pub generated: bool,
    pub error: Option<String>,
}

struct TempFiles {
    paths: Vec<PathBuf>,
}

impl Drop for TempFiles {
    fn drop(&mut self) {
        for path in &self.paths {
            let _ = std::fs::remove_file(path);
        }
    }
}

struct Planned {
    argv: Vec<String>,
    stdin: Vec<u8>,
    cwd: PathBuf,
    timeout: Duration,
    max_output_bytes: usize,
    output: OutputMode,
    temps: TempFiles,
}

/// Spawn generation for `request`; the result arrives on the channel.
pub fn spawn(request: SuggestRequest) -> Receiver<SuggestOutcome> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(generate(&request));
    });
    rx
}

/// Empty-commit policy from the current user config. Invalid config does not
/// auto-run a generator.
pub fn auto_on_empty_commit() -> AutoOnEmpty {
    match load().loaded {
        Ok(cfg) => cfg.auto_on_empty_commit,
        Err(_) => AutoOnEmpty::Off,
    }
}

/// Read-only Settings summary of the resolved profile. No secrets, no prompt
/// body — executable + argv + timeout + prompt byte size, or the built-in tag.
pub fn settings_summary() -> String {
    match load().loaded {
        Err(error) => {
            let short = error.split(':').next().unwrap_or("invalid config");
            format!("invalid: {short}")
        }
        Ok(cfg) if cfg.builtin => "built-in: claude haiku".into(),
        Ok(cfg) if !cfg.enabled => "disabled".into(),
        Ok(cfg) => {
            let profile = cfg.active();
            let argv = resolved_command(profile).join(" ");
            format!(
                "{argv}  {}s  {}B",
                profile.timeout.as_secs(),
                profile.prompt.len()
            )
        }
    }
}

fn generate(request: &SuggestRequest) -> SuggestOutcome {
    let loaded = load();
    match loaded.loaded {
        Err(error) => fallback_outcome(&request.files, FallbackPolicy::Filenames, Some(error)),
        Ok(cfg) if !cfg.enabled => fallback_outcome(
            &request.files,
            cfg.fallback,
            Some("commit-message generation is disabled".into()),
        ),
        Ok(cfg) => match run_generator(&cfg, request) {
            Ok(message) => SuggestOutcome {
                message: Some(message),
                generated: true,
                error: None,
            },
            Err(error) => fallback_outcome(&request.files, cfg.fallback, Some(error)),
        },
    }
}

fn fallback_outcome(
    files: &[String],
    policy: FallbackPolicy,
    error: Option<String>,
) -> SuggestOutcome {
    let message = match policy {
        FallbackPolicy::Filenames => Some(fallback(files)),
        FallbackPolicy::None => None,
    };
    SuggestOutcome {
        message,
        generated: false,
        error,
    }
}

fn load() -> LoadResult {
    load_from(crate::state::plugin_config_dir().as_deref())
}

fn load_from(config_dir: Option<&Path>) -> LoadResult {
    let Some(dir) = config_dir else {
        return LoadResult {
            loaded: Ok(builtin_config()),
            opened: Vec::new(),
        };
    };
    let path = dir.join(CONFIG_FILE);
    let opened = vec![path.clone()];
    match std::fs::read(&path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => LoadResult {
            loaded: Ok(builtin_config()),
            opened,
        },
        Err(err) => LoadResult {
            loaded: Err(format!("{}: {err}", path.display())),
            opened,
        },
        Ok(bytes) => {
            if bytes.len() > MAX_CONFIG_BYTES {
                return LoadResult {
                    loaded: Err(format!(
                        "{} is larger than {MAX_CONFIG_BYTES} bytes",
                        path.display()
                    )),
                    opened,
                };
            }
            LoadResult {
                loaded: parse_config(&bytes),
                opened,
            }
        }
    }
}

fn builtin_config() -> Config {
    let mut profiles = BTreeMap::new();
    profiles.insert(
        "claude".into(),
        Profile {
            command: vec![
                "claude".into(),
                "-p".into(),
                "--model".into(),
                "haiku".into(),
                "--strict-mcp-config".into(),
            ],
            command_windows: None,
            input: InputMode::PromptArgvDiffStdin,
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            max_diff_bytes: DEFAULT_MAX_DIFF_BYTES,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
            cwd: CwdMode::Repo,
            output: OutputMode::LastUsableLine,
            prompt: PROMPT.to_string(),
        },
    );
    Config {
        enabled: true,
        default_profile: "claude".into(),
        auto_on_empty_commit: AutoOnEmpty::Off,
        fallback: FallbackPolicy::Filenames,
        profiles,
        builtin: true,
    }
}

fn parse_config(bytes: &[u8]) -> Result<Config, String> {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let text = std::str::from_utf8(bytes).map_err(|_| "commit-message.toml is not valid UTF-8")?;
    let raw: RawRoot = toml::from_str(text).map_err(|err| format!("commit-message.toml: {err}"))?;
    let commit = raw.commit_message;
    if commit.profiles.is_empty() {
        return Err("commit-message.toml: no profiles configured".into());
    }
    if !commit.profiles.contains_key(&commit.default_profile) {
        return Err(format!(
            "commit-message.toml: default_profile {:?} is not a configured profile",
            commit.default_profile
        ));
    }
    let mut profiles = BTreeMap::new();
    for (id, raw_profile) in commit.profiles {
        profiles.insert(id.clone(), parse_profile(&id, raw_profile)?);
    }
    Ok(Config {
        enabled: commit.enabled,
        default_profile: commit.default_profile,
        auto_on_empty_commit: parse_auto_on_empty(commit.auto_on_empty_commit.as_deref())?,
        fallback: parse_fallback(commit.fallback.as_deref())?,
        profiles,
        builtin: false,
    })
}

#[derive(serde::Deserialize)]
struct RawRoot {
    commit_message: RawCommit,
}

#[derive(serde::Deserialize)]
struct RawCommit {
    #[serde(default = "default_true")]
    enabled: bool,
    default_profile: String,
    #[serde(default)]
    auto_on_empty_commit: Option<String>,
    #[serde(default)]
    fallback: Option<String>,
    #[serde(default)]
    profiles: BTreeMap<String, RawProfile>,
}

#[derive(serde::Deserialize)]
struct RawProfile {
    command: toml::Value,
    #[serde(default)]
    command_windows: Option<toml::Value>,
    #[serde(default)]
    input: Option<String>,
    #[serde(default)]
    timeout_seconds: Option<u64>,
    #[serde(default)]
    max_diff_bytes: Option<u64>,
    #[serde(default)]
    max_output_bytes: Option<u64>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    output: Option<String>,
    prompt: String,
}

fn default_true() -> bool {
    true
}

fn parse_profile(id: &str, raw: RawProfile) -> Result<Profile, String> {
    let command = argv_of(&raw.command, "command")?;
    let command_windows = match raw.command_windows {
        Some(value) => Some(argv_of(&value, "command_windows")?),
        None => None,
    };
    let input = parse_input(raw.input.as_deref())?;
    let timeout_secs = raw.timeout_seconds.unwrap_or(DEFAULT_TIMEOUT_SECS);
    if timeout_secs == 0 || timeout_secs > MAX_TIMEOUT_SECS {
        return Err(format!(
            "commit-message.toml: profile {id}: timeout_seconds must be 1..={MAX_TIMEOUT_SECS}"
        ));
    }
    let max_diff_bytes =
        bounded_size(raw.max_diff_bytes, DEFAULT_MAX_DIFF_BYTES, "max_diff_bytes")?;
    let max_output_bytes = bounded_size(
        raw.max_output_bytes,
        DEFAULT_MAX_OUTPUT_BYTES,
        "max_output_bytes",
    )?;
    if raw.prompt.trim().is_empty() {
        return Err(format!(
            "commit-message.toml: profile {id}: prompt must not be empty"
        ));
    }
    validate_placeholders(&raw.prompt, PROMPT_PLACEHOLDERS, "prompt")?;
    let command_allowed = match input {
        InputMode::ArgvSubst => ARGV_PLACEHOLDERS,
        _ => &[][..],
    };
    for arg in &command {
        validate_placeholders(arg, command_allowed, "command")?;
    }
    if let Some(win) = &command_windows {
        for arg in win {
            validate_placeholders(arg, command_allowed, "command_windows")?;
        }
    }
    if input == InputMode::ArgvSubst {
        let haystack: String = command
            .iter()
            .chain(command_windows.iter().flatten())
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        if !haystack.contains("{prompt_file}") && !haystack.contains("{diff_file}") {
            return Err(format!(
                "commit-message.toml: profile {id}: argv_subst requires {{prompt_file}} or {{diff_file}} in command"
            ));
        }
    }
    if input == InputMode::PromptArgvDiffStdin
        && placeholders_in(&raw.prompt).iter().any(|n| n == "diff")
    {
        return Err(format!(
            "commit-message.toml: profile {id}: {{diff}} cannot go through argv; use prompt_and_diff_stdin or argv_subst"
        ));
    }
    Ok(Profile {
        command,
        command_windows,
        input,
        timeout: Duration::from_secs(timeout_secs),
        max_diff_bytes,
        max_output_bytes,
        cwd: parse_cwd(raw.cwd.as_deref())?,
        output: parse_output(raw.output.as_deref())?,
        prompt: raw.prompt,
    })
}

fn argv_of(value: &toml::Value, field: &str) -> Result<Vec<String>, String> {
    match value {
        toml::Value::String(_) => Err(format!(
            "commit-message.toml: {field} must be an argv array, not a shell string"
        )),
        toml::Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let Some(text) = item.as_str() else {
                    return Err(format!(
                        "commit-message.toml: {field} entries must be strings"
                    ));
                };
                out.push(text.to_string());
            }
            if out.is_empty() || out[0].trim().is_empty() {
                return Err(format!(
                    "commit-message.toml: {field} must be a non-empty argv array"
                ));
            }
            Ok(out)
        }
        _ => Err(format!(
            "commit-message.toml: {field} must be an argv array"
        )),
    }
}

fn bounded_size(value: Option<u64>, default: usize, field: &str) -> Result<usize, String> {
    let n = value.unwrap_or(default as u64);
    if n == 0 || n > MAX_IO_BYTES {
        return Err(format!(
            "commit-message.toml: {field} must be 1..={MAX_IO_BYTES}"
        ));
    }
    Ok(n as usize)
}

fn parse_input(value: Option<&str>) -> Result<InputMode, String> {
    match value.unwrap_or("prompt_argv_diff_stdin") {
        "prompt_argv_diff_stdin" => Ok(InputMode::PromptArgvDiffStdin),
        "prompt_and_diff_stdin" => Ok(InputMode::PromptAndDiffStdin),
        "argv_subst" => Ok(InputMode::ArgvSubst),
        other => Err(format!(
            "commit-message.toml: unknown input {other:?}; expected prompt_argv_diff_stdin, prompt_and_diff_stdin, or argv_subst"
        )),
    }
}

fn parse_output(value: Option<&str>) -> Result<OutputMode, String> {
    match value.unwrap_or("last_usable_line") {
        "last_usable_line" => Ok(OutputMode::LastUsableLine),
        "raw_trim" => Ok(OutputMode::RawTrim),
        other => Err(format!(
            "commit-message.toml: unknown output {other:?}; expected last_usable_line or raw_trim"
        )),
    }
}

fn parse_cwd(value: Option<&str>) -> Result<CwdMode, String> {
    match value.unwrap_or("repo") {
        "repo" => Ok(CwdMode::Repo),
        "workspace" => Ok(CwdMode::Workspace),
        other => Err(format!(
            "commit-message.toml: unknown cwd {other:?}; expected repo or workspace"
        )),
    }
}

fn parse_auto_on_empty(value: Option<&str>) -> Result<AutoOnEmpty, String> {
    match value.unwrap_or("off") {
        "off" => Ok(AutoOnEmpty::Off),
        "fill" => Ok(AutoOnEmpty::Fill),
        "fill_and_commit" => Ok(AutoOnEmpty::FillAndCommit),
        other => Err(format!(
            "commit-message.toml: unknown auto_on_empty_commit {other:?}; expected off, fill, or fill_and_commit"
        )),
    }
}

fn parse_fallback(value: Option<&str>) -> Result<FallbackPolicy, String> {
    match value.unwrap_or("filenames") {
        "filenames" => Ok(FallbackPolicy::Filenames),
        "none" => Ok(FallbackPolicy::None),
        other => Err(format!(
            "commit-message.toml: unknown fallback {other:?}; expected filenames or none"
        )),
    }
}

fn is_ident(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => chars.all(|c| c.is_ascii_alphanumeric() || c == '_'),
        _ => false,
    }
}

fn placeholders_in(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else { break };
        let name = &after[..end];
        if is_ident(name) {
            names.push(name.to_string());
        }
        rest = &after[end + 1..];
    }
    names
}

fn validate_placeholders(text: &str, allowed: &[&str], field: &str) -> Result<(), String> {
    for name in placeholders_in(text) {
        if !allowed.contains(&name.as_str()) {
            return Err(format!(
                "commit-message.toml: unknown placeholder {{{name}}} in {field}"
            ));
        }
    }
    Ok(())
}

fn truncate_diff(diff: &str, cap: usize) -> String {
    let mut input = String::with_capacity(diff.len().min(cap));
    for c in diff.chars() {
        if input.len() + c.len_utf8() > cap {
            input.push_str("\n[diff truncated]");
            break;
        }
        input.push(c);
    }
    input
}

fn branch_label(branch: &str) -> &str {
    if branch.trim().is_empty() {
        "HEAD"
    } else {
        branch
    }
}

fn render_prompt(template: &str, request: &SuggestRequest, truncated_diff: &str) -> String {
    let files_nl = request.files.join("\n");
    let files_csv = request.files.join(", ");
    let file_count = request.files.len().to_string();
    let branch = branch_label(&request.branch);
    let repo_root = request.repo_root.display().to_string();
    let source = request.source.as_str();
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            out.push_str(rest);
            return out;
        };
        let name = &after[..end];
        out.push_str(&rest[..start]);
        let value = match name {
            "diff" => truncated_diff,
            "files" => files_nl.as_str(),
            "files_csv" => files_csv.as_str(),
            "file_count" => file_count.as_str(),
            "branch" => branch,
            "repo_root" => repo_root.as_str(),
            "source" => source,
            _ => {
                out.push('{');
                out.push_str(name);
                out.push('}');
                rest = &after[end + 1..];
                continue;
            }
        };
        out.push_str(value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn resolved_command(profile: &Profile) -> &[String] {
    #[cfg(windows)]
    if let Some(windows) = &profile.command_windows {
        return windows;
    }
    &profile.command
}

fn program_candidates(program: &str) -> Vec<String> {
    let mut out = vec![program.to_string()];
    #[cfg(windows)]
    if Path::new(program).extension().is_none() {
        out.push(format!("{program}.cmd"));
    }
    out
}

fn plan(cfg: &Config, request: &SuggestRequest) -> Result<Planned, String> {
    let profile = cfg.active();
    let truncated = truncate_diff(&request.diff, profile.max_diff_bytes);
    let rendered = render_prompt(&profile.prompt, request, &truncated);
    let cwd = match profile.cwd {
        CwdMode::Repo => request.repo_root.clone(),
        CwdMode::Workspace => request.workspace.clone(),
    };
    let mut argv = resolved_command(profile).to_vec();
    let mut stdin = Vec::new();
    let mut temps = TempFiles { paths: Vec::new() };
    match profile.input {
        InputMode::PromptArgvDiffStdin => {
            argv.push(rendered);
            stdin = truncated.into_bytes();
        }
        InputMode::PromptAndDiffStdin => {
            if placeholders_in(&profile.prompt)
                .iter()
                .any(|name| name == "diff")
            {
                stdin = rendered.into_bytes();
            } else {
                stdin = format!("{rendered}\n\n{truncated}").into_bytes();
            }
        }
        InputMode::ArgvSubst => {
            let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
            let pid = std::process::id();
            let prompt_path = std::env::temp_dir().join(format!("hs-sg-{pid}-{seq}-p.txt"));
            let diff_path = std::env::temp_dir().join(format!("hs-sg-{pid}-{seq}-d.txt"));
            std::fs::write(&prompt_path, rendered.as_bytes())
                .map_err(|err| format!("temp prompt file: {err}"))?;
            temps.paths.push(prompt_path.clone());
            std::fs::write(&diff_path, truncated.as_bytes())
                .map_err(|err| format!("temp diff file: {err}"))?;
            temps.paths.push(diff_path.clone());
            let prompt_s = prompt_path.display().to_string();
            let diff_s = diff_path.display().to_string();
            for arg in &mut argv {
                *arg = arg
                    .replace("{prompt_file}", &prompt_s)
                    .replace("{diff_file}", &diff_s);
            }
        }
    }
    Ok(Planned {
        argv,
        stdin,
        cwd,
        timeout: profile.timeout,
        max_output_bytes: profile.max_output_bytes,
        output: profile.output,
        temps,
    })
}

fn run_generator(cfg: &Config, request: &SuggestRequest) -> Result<String, String> {
    let planned = plan(cfg, request)?;
    let (mut child, resolved) = spawn_child(&planned.argv, &planned.cwd)?;
    if let Some(mut stdin) = child.stdin.take()
        && !planned.stdin.is_empty()
        && stdin.write_all(&planned.stdin).is_err()
    {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("{resolved} failed: could not write stdin"));
    }
    let raw = wait_with_timeout(child, planned.timeout, planned.max_output_bytes, &resolved)?;
    // Keep temps alive until the child exits (argv_subst).
    drop(planned.temps);
    let parsed = match planned.output {
        OutputMode::LastUsableLine => clean_reply(&raw),
        OutputMode::RawTrim => raw_trim(&raw),
    };
    parsed.ok_or_else(|| format!("{resolved} failed: empty output"))
}

fn spawn_child(argv: &[String], cwd: &Path) -> Result<(Child, String), String> {
    if !cwd.is_dir() {
        return Err(format!("cwd is not a directory: {}", cwd.display()));
    }
    let program = &argv[0];
    let args = &argv[1..];
    let mut last_err = None;
    for candidate in program_candidates(program) {
        let mut cmd = Command::new(&candidate);
        cmd.args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        strip_herdr_env(&mut cmd);
        match cmd.spawn() {
            Ok(child) => return Ok((child, candidate)),
            Err(err) => last_err = Some((candidate, err)),
        }
    }
    let (name, err) = last_err.expect("program_candidates is non-empty");
    Err(format!("{name} failed: {err}"))
}

fn strip_herdr_env(cmd: &mut Command) {
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("HERDR_") {
            cmd.env_remove(key);
        }
    }
}

fn wait_with_timeout(
    mut child: Child,
    timeout: Duration,
    max_output_bytes: usize,
    program: &str,
) -> Result<String, String> {
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("{program} failed: no stdout pipe"))?;
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 4096];
        loop {
            match stdout.read(&mut tmp) {
                Ok(0) => break,
                Ok(n) => {
                    let room = max_output_bytes.saturating_add(1).saturating_sub(buf.len());
                    buf.extend_from_slice(&tmp[..n.min(room)]);
                    if buf.len() > max_output_bytes {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = tx.send(buf);
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{program} failed: timed out"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(err) => return Err(format!("{program} failed: {err}")),
        }
    };
    let buf = rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
    if buf.len() > max_output_bytes {
        return Err(format!("{program} failed: output too large"));
    }
    if !status.success() {
        let code = status.code().unwrap_or(-1);
        return Err(format!("{program} failed: exit {code}"));
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// The reply line, stripped of the quoting/fencing chat models sometimes add
/// despite instructions; `None` when nothing usable came back. Startup log
/// noise (MCP warnings and the like) can precede the reply on stdout, so this
/// takes the LAST usable line and drops warning-looking lines outright.
fn clean_reply(raw: &str) -> Option<String> {
    let line = raw.lines().map(str::trim).rfind(|l| {
        let lower = l.to_lowercase();
        !l.is_empty()
            && !l.starts_with("```")
            && !lower.contains("warn")
            && !lower.contains("error")
    })?;
    let line = line
        .trim_matches(['"', '\'', '`'])
        .trim_end_matches('.')
        .trim();
    (!line.is_empty()).then(|| line.to_string())
}

fn raw_trim(raw: &str) -> Option<String> {
    let line = raw.trim().lines().next()?.trim();
    (!line.is_empty()).then(|| line.to_string())
}

/// Filename-based fallback: good enough to save retyping, honest about scope.
fn fallback(files: &[String]) -> String {
    let name = |path: &String| path.rsplit('/').next().unwrap_or(path).to_string();
    match files {
        [] => "Update".to_string(),
        [only] => format!("Update {}", name(only)),
        [first, rest @ ..] => format!("Update {} and {} more", name(first), rest.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn set_test_env(key: &str, value: impl AsRef<std::ffi::OsStr>) {
        // SAFETY: caller holds ENV_LOCK; these tests never run concurrently with
        // other env mutators.
        unsafe { std::env::set_var(key, value) }
    }

    fn remove_test_env(key: &str) {
        // SAFETY: same as set_test_env.
        unsafe { std::env::remove_var(key) }
    }

    fn sample_request() -> SuggestRequest {
        let cwd = std::env::temp_dir();
        SuggestRequest {
            diff: "diff --git a/src/app.rs b/src/app.rs\n+fn main() {}\n".into(),
            files: vec!["src/app.rs".into(), "src/lib.rs".into()],
            source: DiffSource::Staged,
            branch: "main".into(),
            repo_root: cwd.clone(),
            workspace: cwd,
        }
    }

    fn unique_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hs-suggest-{tag}-{}-{}",
            std::process::id(),
            TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_cfg(dir: &Path, body: &str) {
        std::fs::write(dir.join(CONFIG_FILE), body).unwrap();
    }

    fn profile_block(command_line: &str, extra: &str) -> String {
        format!(
            r#"
[commit_message]
enabled = true
default_profile = "test"
auto_on_empty_commit = "off"
fallback = "filenames"

[commit_message.profiles.test]
command = {command_line}
input = "prompt_argv_diff_stdin"
timeout_seconds = 60
max_diff_bytes = 16384
max_output_bytes = 4096
cwd = "repo"
output = "last_usable_line"
prompt = """Write a git commit message. Reply with ONLY the message line."""
{extra}
"#
        )
    }

    fn fake_helper() -> &'static Path {
        static EXE: OnceLock<PathBuf> = OnceLock::new();
        EXE.get_or_init(|| {
            let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("suggest-fakes");
            std::fs::create_dir_all(&dir).unwrap();
            let src = dir.join("fake.rs");
            let exe = dir.join(if cfg!(windows) { "fake.exe" } else { "fake" });
            std::fs::write(
                &src,
                r#"
use std::io::Read;
fn main() {
    let mode = std::env::var("HS_FAKE_MODE").unwrap_or_else(|_| "ok".into());
    match mode.as_str() {
        "sleep" => {
            std::thread::sleep(std::time::Duration::from_secs(30));
            println!("too late");
        }
        "fail" => std::process::exit(2),
        "oversize" => print!("{}", "x".repeat(8192)),
        _ => {
            if let Ok(dir) = std::env::var("HS_RECORD_DIR") {
                let args: Vec<String> = std::env::args().collect();
                let _ = std::fs::write(format!("{dir}/argv.txt"), args.join("\n"));
                let mut stdin = Vec::new();
                let _ = std::io::stdin().read_to_end(&mut stdin);
                let _ = std::fs::write(format!("{dir}/stdin.bin"), &stdin);
                let herdr: Vec<String> = std::env::vars()
                    .filter(|(k, _)| k.starts_with("HERDR_"))
                    .map(|(k, _)| k)
                    .collect();
                let _ = std::fs::write(format!("{dir}/herdr_env.txt"), herdr.join("\n"));
                for (i, arg) in args.iter().enumerate() {
                    let p = std::path::Path::new(arg);
                    if p.is_file() {
                        let _ = std::fs::copy(p, format!("{dir}/arg{i}.dat"));
                    }
                }
            }
            println!("Add sidebar merge");
        }
    }
}
"#,
            )
            .unwrap();
            let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
            let status = Command::new(rustc)
                .arg(&src)
                .arg("-o")
                .arg(&exe)
                .status()
                .expect("rustc must spawn to build the fake generator");
            assert!(status.success(), "rustc failed to build the fake generator");
            exe
        })
        .as_path()
    }

    fn toml_path(path: &Path) -> String {
        path.display().to_string().replace('\\', "/")
    }

    #[test]
    fn reply_cleanup_strips_quotes_fences_and_periods() {
        assert_eq!(
            clean_reply("Add sidebar merge\n"),
            Some("Add sidebar merge".into())
        );
        assert_eq!(
            clean_reply("\"Fix the thing.\""),
            Some("Fix the thing".into())
        );
        assert_eq!(
            clean_reply("```\nRefactor launch flow\n```"),
            Some("Refactor launch flow".into())
        );
        assert_eq!(clean_reply("   \n\n"), None);
        assert_eq!(
            clean_reply("RendererWarning resource UID duplicate\nAdd auth docs\n"),
            Some("Add auth docs".into())
        );
        assert_eq!(clean_reply("[WARN] something\nERROR: nope\n"), None);
    }

    #[test]
    fn raw_trim_keeps_first_nonempty_line() {
        assert_eq!(raw_trim("  Add tests  \nmore\n"), Some("Add tests".into()));
        assert_eq!(raw_trim("\"quoted.\""), Some("\"quoted.\"".into()));
        assert_eq!(raw_trim("   \n"), None);
    }

    #[test]
    fn fallback_names_the_files() {
        assert_eq!(fallback(&[]), "Update");
        assert_eq!(fallback(&["src/app.rs".into()]), "Update app.rs");
        assert_eq!(
            fallback(&["src/app.rs".into(), "b".into(), "c".into()]),
            "Update app.rs and 2 more"
        );
    }

    #[test]
    fn default_without_config_file_is_claude_haiku_plan() {
        let dir = unique_dir("default");
        let loaded = load_from(Some(&dir));
        assert_eq!(loaded.opened, vec![dir.join(CONFIG_FILE)]);
        let cfg = loaded.loaded.unwrap();
        assert!(cfg.builtin);
        let planned = plan(&cfg, &sample_request()).unwrap();
        assert_eq!(
            planned.argv,
            vec![
                "claude",
                "-p",
                "--model",
                "haiku",
                "--strict-mcp-config",
                PROMPT
            ]
        );
        assert_eq!(planned.argv[0], "claude");
        assert!(
            !planned
                .argv
                .iter()
                .any(|a| a == "cmd.exe" || a == "sh" || a == "-c")
        );
        assert_eq!(
            planned.stdin,
            truncate_diff(&sample_request().diff, DEFAULT_MAX_DIFF_BYTES).into_bytes()
        );
        assert_eq!(planned.timeout, Duration::from_secs(60));
        assert_eq!(cfg.auto_on_empty_commit, AutoOnEmpty::Off);
        assert_eq!(cfg.fallback, FallbackPolicy::Filenames);
        assert_eq!(cfg.active().prompt, PROMPT);
    }

    #[test]
    fn trust_boundary_ignores_repo_and_parent_bait() {
        let root = unique_dir("trust");
        let parent = root.join("parent");
        let repo = parent.join("repo");
        let config = root.join("user-config");
        let state = root.join("plugin-state");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&state).unwrap();
        let bait = profile_block(r#"["pwned"]"#, "");
        std::fs::write(repo.join(CONFIG_FILE), &bait).unwrap();
        std::fs::write(repo.join(".herdr-sidebar.toml"), &bait).unwrap();
        std::fs::write(parent.join(CONFIG_FILE), &bait).unwrap();
        std::fs::write(state.join(CONFIG_FILE), &bait).unwrap();
        write_cfg(&config, &profile_block(r#"["trusted-bin"]"#, ""));

        let result = load_from(Some(&config));
        assert_eq!(result.opened, vec![config.join(CONFIG_FILE)]);
        assert!(
            result
                .opened
                .iter()
                .all(|p| p.starts_with(&config) && p.ends_with(CONFIG_FILE))
        );
        let cfg = result.loaded.expect("trusted config should parse");
        assert_eq!(cfg.active().command, vec!["trusted-bin".to_string()]);
        assert!(!cfg.builtin);

        let ignored = load_from(Some(&repo));
        assert_eq!(ignored.opened, vec![repo.join(CONFIG_FILE)]);
        assert_ne!(ignored.loaded.unwrap().active().command[0], "trusted-bin");
    }

    #[test]
    fn fail_closed_unknown_placeholder_shell_string_empty_command_oversize() {
        let dir = unique_dir("fail-closed");
        write_cfg(
            &dir,
            &profile_block(r#"["claude"]"#, r#"prompt = "use {nope}""#),
        );
        // The extra `prompt =` in extra plus the template's prompt — overwrite via a full body.
        write_cfg(
            &dir,
            r#"
[commit_message]
default_profile = "test"
[commit_message.profiles.test]
command = ["claude"]
prompt = "use {nope}"
"#,
        );
        let err = load_from(Some(&dir)).loaded.unwrap_err();
        assert!(err.contains("unknown placeholder {nope}"), "{err}");

        write_cfg(
            &dir,
            r#"
[commit_message]
default_profile = "test"
[commit_message.profiles.test]
command = "claude -p"
prompt = "hello"
"#,
        );
        let err = load_from(Some(&dir)).loaded.unwrap_err();
        assert!(err.contains("argv array"), "{err}");
        assert!(err.contains("shell string"), "{err}");

        write_cfg(
            &dir,
            r#"
[commit_message]
default_profile = "test"
[commit_message.profiles.test]
command = []
prompt = "hello"
"#,
        );
        let err = load_from(Some(&dir)).loaded.unwrap_err();
        assert!(err.contains("non-empty argv"), "{err}");

        let huge = unique_dir("oversize");
        let mut body = b"[commit_message]\ndefault_profile = \"x\"\n".to_vec();
        body.resize(MAX_CONFIG_BYTES + 8, b'x');
        std::fs::write(huge.join(CONFIG_FILE), body).unwrap();
        let err = load_from(Some(&huge)).loaded.unwrap_err();
        assert!(err.contains("larger than"), "{err}");

        let req = sample_request();
        let outcome = {
            // generate() calls load() from the env dir; drive fail-closed via generate's
            // loaded-error path using a parsed invalid result.
            fallback_outcome(&req.files, FallbackPolicy::Filenames, Some(err.clone()))
        };
        assert_eq!(outcome.message.as_deref(), Some("Update app.rs and 1 more"));
        assert!(!outcome.generated);
    }

    #[test]
    fn placeholders_substitute_diff_and_files_with_truncation_marker() {
        let dir = unique_dir("placeholders");
        write_cfg(
            &dir,
            r#"
[commit_message]
default_profile = "test"
[commit_message.profiles.test]
command = ["claude"]
input = "prompt_and_diff_stdin"
max_diff_bytes = 16
prompt = """src={source} files={files_csv} n={file_count} branch={branch}
{diff}"""
"#,
        );
        let cfg = load_from(Some(&dir)).loaded.unwrap();
        let mut req = sample_request();
        req.diff = "abcdefghijklmnopqrstuvwxyz".into();
        let planned = plan(&cfg, &req).unwrap();
        let stdin = String::from_utf8(planned.stdin).unwrap();
        assert!(stdin.contains("src=staged"));
        assert!(stdin.contains("files=src/app.rs, src/lib.rs"));
        assert!(stdin.contains("n=2"));
        assert!(stdin.contains("branch=main"));
        assert!(stdin.contains("[diff truncated]"));
        assert!(!stdin.contains("abcdefghijklmnopqrstuvwxyz"));
    }

    #[test]
    fn argv_spawn_receives_exact_argv_and_stdin_for_each_input_mode() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        set_test_env("HERDR_PANE_ID", "pane-test");
        set_test_env("HERDR_SOCKET_PATH", "not-a-real-socket");
        let helper = fake_helper();
        let helper_s = toml_path(helper);

        // prompt_argv_diff_stdin
        let dir = unique_dir("spawn-argv");
        let record = unique_dir("record-argv");
        write_cfg(
            &dir,
            &format!(
                r#"
[commit_message]
default_profile = "test"
[commit_message.profiles.test]
command = [{helper_s:?}, "--oneshot"]
input = "prompt_argv_diff_stdin"
timeout_seconds = 15
prompt = "PROMPTLINE"
"#
            ),
        );
        let cfg = load_from(Some(&dir)).loaded.unwrap();
        set_test_env("HS_RECORD_DIR", record.display().to_string());
        set_test_env("HS_FAKE_MODE", "ok");
        let outcome = run_generator(&cfg, &sample_request()).unwrap();
        assert_eq!(outcome, "Add sidebar merge");
        let argv = std::fs::read_to_string(record.join("argv.txt")).unwrap();
        let lines: Vec<&str> = argv.lines().collect();
        assert_eq!(lines[1], "--oneshot");
        assert_eq!(lines[2], "PROMPTLINE");
        assert!(
            !lines
                .iter()
                .any(|l| *l == "cmd.exe" || *l == "sh" || *l == "-c")
        );
        let stdin = std::fs::read(record.join("stdin.bin")).unwrap();
        assert_eq!(stdin, sample_request().diff.as_bytes());
        let herdr = std::fs::read_to_string(record.join("herdr_env.txt")).unwrap();
        assert!(
            herdr.trim().is_empty(),
            "Herdr control env leaked: {herdr:?}"
        );

        // prompt_and_diff_stdin
        let dir = unique_dir("spawn-stdin");
        let record = unique_dir("record-stdin");
        write_cfg(
            &dir,
            &format!(
                r#"
[commit_message]
default_profile = "test"
[commit_message.profiles.test]
command = [{helper_s:?}]
input = "prompt_and_diff_stdin"
timeout_seconds = 15
prompt = """HEADING
{{diff}}"""
"#
            ),
        );
        set_test_env("HS_RECORD_DIR", record.display().to_string());
        let cfg = load_from(Some(&dir)).loaded.unwrap();
        run_generator(&cfg, &sample_request()).unwrap();
        let stdin = String::from_utf8(std::fs::read(record.join("stdin.bin")).unwrap()).unwrap();
        assert!(stdin.starts_with("HEADING\n"));
        assert!(stdin.contains(&sample_request().diff));

        // argv_subst
        let dir = unique_dir("spawn-subst");
        let record = unique_dir("record-subst");
        write_cfg(
            &dir,
            &format!(
                r#"
[commit_message]
default_profile = "test"
[commit_message.profiles.test]
command = [{helper_s:?}, "{{prompt_file}}", "{{diff_file}}"]
input = "argv_subst"
timeout_seconds = 15
prompt = """PROMPT {{files}}"""
"#
            ),
        );
        set_test_env("HS_RECORD_DIR", record.display().to_string());
        let cfg = load_from(Some(&dir)).loaded.unwrap();
        run_generator(&cfg, &sample_request()).unwrap();
        let argv = std::fs::read_to_string(record.join("argv.txt")).unwrap();
        let lines: Vec<&str> = argv.lines().collect();
        assert_eq!(lines.len(), 3);
        let prompt = std::fs::read_to_string(record.join("arg1.dat")).unwrap();
        let diff = std::fs::read_to_string(record.join("arg2.dat")).unwrap();
        assert!(prompt.contains("PROMPT"));
        assert!(prompt.contains("src/app.rs"));
        assert!(diff.contains("diff --git"));
        let stdin = std::fs::read(record.join("stdin.bin")).unwrap();
        assert!(stdin.is_empty());

        remove_test_env("HS_RECORD_DIR");
        remove_test_env("HS_FAKE_MODE");
        remove_test_env("HERDR_PANE_ID");
        remove_test_env("HERDR_SOCKET_PATH");
    }

    #[test]
    fn timeout_kills_child_and_uses_fallback() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let helper = toml_path(fake_helper());
        let dir = unique_dir("timeout");
        write_cfg(
            &dir,
            &format!(
                r#"
[commit_message]
default_profile = "test"
fallback = "filenames"
[commit_message.profiles.test]
command = [{helper:?}]
input = "prompt_argv_diff_stdin"
timeout_seconds = 1
prompt = "PROMPTLINE"
"#
            ),
        );
        set_test_env("HS_FAKE_MODE", "sleep");
        let cfg = load_from(Some(&dir)).loaded.unwrap();
        let err = run_generator(&cfg, &sample_request()).unwrap_err();
        remove_test_env("HS_FAKE_MODE");
        assert!(err.contains("timed out"), "{err}");
        assert!(
            err.contains("fake") || err.to_lowercase().contains("sleep") || err.contains(".exe"),
            "timeout flash should name the resolved executable: {err}"
        );
        let outcome = fallback_outcome(&sample_request().files, cfg.fallback, Some(err));
        assert_eq!(outcome.message.as_deref(), Some("Update app.rs and 1 more"));
        assert!(!outcome.generated);
    }

    #[test]
    fn configured_command_is_what_spawns() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let helper = toml_path(fake_helper());
        let dir = unique_dir("custom-cmd");
        let record = unique_dir("record-custom");
        write_cfg(
            &dir,
            &format!(
                r#"
[commit_message]
default_profile = "test"
[commit_message.profiles.test]
command = [{helper:?}, "--model", "local"]
input = "prompt_argv_diff_stdin"
timeout_seconds = 15
prompt = "custom prompt text"
"#
            ),
        );
        set_test_env("HS_RECORD_DIR", record.display().to_string());
        set_test_env("HS_FAKE_MODE", "ok");
        let cfg = load_from(Some(&dir)).loaded.unwrap();
        assert!(!cfg.builtin);
        assert_eq!(cfg.active().prompt, "custom prompt text");
        run_generator(&cfg, &sample_request()).unwrap();
        let argv = std::fs::read_to_string(record.join("argv.txt")).unwrap();
        assert!(argv.contains("--model"));
        assert!(argv.contains("local"));
        assert!(argv.contains("custom prompt text"));
        assert!(!argv.contains(PROMPT));
        remove_test_env("HS_RECORD_DIR");
        remove_test_env("HS_FAKE_MODE");
    }

    #[test]
    fn windows_cmd_fallback_only_uses_configured_basename() {
        let candidates = program_candidates("claude");
        let candidates_exe = program_candidates("claude.exe");
        #[cfg(windows)]
        {
            assert_eq!(candidates, vec!["claude", "claude.cmd"]);
            assert_eq!(candidates_exe, vec!["claude.exe"]);
            assert!(!candidates.iter().any(|c| c.contains("codex")));
            let dir = unique_dir("cmd-windows");
            write_cfg(
                &dir,
                r#"
[commit_message]
default_profile = "test"
[commit_message.profiles.test]
command = ["other"]
command_windows = ["claude.cmd", "-p"]
prompt = "hello"
"#,
            );
            let cfg = load_from(Some(&dir)).loaded.unwrap();
            assert_eq!(
                resolved_command(cfg.active()),
                &["claude.cmd".to_string(), "-p".to_string()]
            );
            assert_eq!(
                program_candidates(&resolved_command(cfg.active())[0]),
                vec!["claude.cmd"]
            );
        }
        #[cfg(not(windows))]
        {
            assert_eq!(candidates, vec!["claude"]);
            assert_eq!(candidates_exe, vec!["claude.exe"]);
        }
    }

    #[test]
    fn generate_does_not_run_on_invalid_config() {
        let dir = unique_dir("no-spawn");
        write_cfg(
            &dir,
            r#"
[commit_message]
default_profile = "test"
[commit_message.profiles.test]
command = "rm -rf /"
prompt = "nope {undefined}"
"#,
        );
        let loaded = load_from(Some(&dir));
        assert!(loaded.loaded.is_err());
        // Invalid config never produces a Planned argv — there is no binary to run.
        assert!(plan(&builtin_config(), &sample_request()).is_ok());
    }

    #[test]
    fn settings_summary_builtin_tag() {
        let cfg = builtin_config();
        assert!(cfg.builtin);
        assert_eq!(cfg.active().command[0], "claude");
        assert!(cfg.active().command.contains(&"haiku".into()));
    }
}
