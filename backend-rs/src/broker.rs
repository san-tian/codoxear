use crate::runtime::{
    compute_idle_from_log, default_app_dir, discover_open_log_for_process_in_sessions,
    token_update_from_obj,
};
use serde_json::{json, Value};
use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const BRACKETED_PASTE_START: &[u8] = b"\x1b[200~";
const BRACKETED_PASTE_END: &[u8] = b"\x1b[201~";
const OUTPUT_TAIL_MAX: usize = 256 * 1024;
const BUSY_HINT_TAIL_MAX: usize = 4096;
const BUSY_QUIET_SECONDS_DEFAULT: f64 = 3.0;
static SIGWINCH_PENDING: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
pub struct BrokerState {
    pub agent_pid: i64,
    pub pty_master_fd: RawFd,
    pub cwd: String,
    pub start_ts: f64,
    pub sock_path: PathBuf,
    pub agent_backend: String,
    pub owner: Option<String>,
    pub log_path: Option<PathBuf>,
    pub session_id: Option<String>,
    pub busy: bool,
    pub output_tail: String,
    pub token: Option<Value>,
    pub resume_session_id: Option<String>,
    pub model_provider: Option<String>,
    pub preferred_auth_method: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
    pub transport: Option<String>,
    pub tmux_session: Option<String>,
    pub tmux_window: Option<String>,
    pub spawn_nonce: Option<String>,
    mirror_output: bool,
    term_query_buf: Vec<u8>,
    busy_hint_tail: String,
    busy_hint_last_seen: Option<Instant>,
    busy_from_pty_hint: bool,
}

impl BrokerState {
    fn metadata_value(&self) -> Value {
        json!({
            "session_id": self.session_id,
            "owner": self.owner,
            "broker_pid": std::process::id(),
            "sessiond_pid": std::process::id(),
            "codex_pid": self.agent_pid,
            "cwd": self.cwd,
            "start_ts": self.start_ts,
            "log_path": self.log_path.as_ref().map(|path| path.display().to_string()),
            "sock_path": self.sock_path.display().to_string(),
            "agent_backend": self.agent_backend,
            "resume_session_id": self.resume_session_id,
            "model_provider": self.model_provider,
            "preferred_auth_method": self.preferred_auth_method,
            "model": self.model,
            "reasoning_effort": self.reasoning_effort,
            "service_tier": self.service_tier,
            "transport": self.transport,
            "tmux_session": self.tmux_session,
            "tmux_window": self.tmux_window,
            "spawn_nonce": self.spawn_nonce,
        })
    }
}

#[derive(Debug)]
struct BrokerConfig {
    cwd: PathBuf,
    agent_args: Vec<String>,
    app_dir: PathBuf,
    agent_backend: String,
    agent_bin: String,
    agent_home: PathBuf,
    sessions_dir: PathBuf,
    owner: Option<String>,
    mirror_output: bool,
    mirror_input: bool,
}

#[derive(Clone)]
struct BrokerRuntime {
    state: Arc<Mutex<BrokerState>>,
    stop: Arc<AtomicBool>,
    sessions_dir: PathBuf,
}

pub fn main_entry() -> Result<(), String> {
    let config = parse_args(env::args().skip(1).collect())?;
    run_broker(config)
}

fn parse_args(args: Vec<String>) -> Result<BrokerConfig, String> {
    let mut cwd = env::current_dir().map_err(|err| format!("current dir: {err}"))?;
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--cwd" => {
                index += 1;
                let Some(raw) = args.get(index) else {
                    return Err("--cwd requires a value".to_string());
                };
                cwd = expand_path(raw)?;
                index += 1;
            }
            "--" => {
                index += 1;
                break;
            }
            value => {
                return Err(format!("unknown broker argument: {value}"));
            }
        }
    }
    let mut agent_args = args[index..].to_vec();
    let app_dir = default_app_dir()?;
    let agent_backend = env::var("CODEX_WEB_AGENT_BACKEND")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "codex".to_string());
    let (agent_bin, agent_home) = if agent_backend == "pi" {
        (
            env::var("PI_BIN").ok().filter(|value| !value.trim().is_empty()).unwrap_or_else(|| "pi".to_string()),
            env_path("PI_HOME").unwrap_or_else(|| home_dir().join(".pi")),
        )
    } else {
        (
            env::var("CODEX_BIN")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "codex".to_string()),
            env_path("CODEX_HOME").unwrap_or_else(|| home_dir().join(".codex")),
        )
    };
    let sessions_dir = agent_sessions_dir(&agent_backend, &agent_home);
    let owner = clean_env("CODEX_WEB_OWNER");
    if agent_backend == "pi" {
        agent_args = ensure_pi_session_arg(&agent_args, &cwd, &sessions_dir)?;
    }
    agent_args = web_owned_codex_args(&agent_backend, owner.as_deref(), &agent_args);
    Ok(BrokerConfig {
        cwd,
        agent_args,
        app_dir,
        agent_backend,
        agent_bin,
        agent_home,
        sessions_dir,
        owner,
        mirror_output: true,
        mirror_input: should_mirror_stdin(),
    })
}

fn web_owned_codex_args(agent_backend: &str, owner: Option<&str>, args: &[String]) -> Vec<String> {
    if agent_backend != "codex" || owner != Some("web") {
        return args.to_vec();
    }
    let mut out = vec![
        "-c".to_string(),
        "disable_response_storage=false".to_string(),
        "-c".to_string(),
        "disable_paste_burst=true".to_string(),
    ];
    out.extend(args.iter().cloned());
    out
}

fn run_broker(mut config: BrokerConfig) -> Result<(), String> {
    if config.agent_backend == "pi" {
        config.agent_args = ensure_pi_session_arg(&config.agent_args, &config.cwd, &config.sessions_dir)?;
    }
    let (rows, cols) = terminal_size();
    let (master_fd, slave_fd) = open_pty(rows, cols)?;
    let child = spawn_agent(&config, slave_fd, rows, cols)?;
    let sock_dir = config.app_dir.join("socks");
    fs::create_dir_all(&sock_dir).map_err(|err| format!("create {}: {err}", sock_dir.display()))?;
    let sock_path = sock_dir.join(format!("broker-{}.sock", std::process::id()));
    let start_ts = epoch_now();
    let declared_log_path = session_log_path_from_args(&config.agent_args, &config.agent_backend, &config.sessions_dir);
    let (initial_log_path, initial_session_id) = declared_log_path
        .as_ref()
        .filter(|path| path.exists())
        .map(|path| (Some(path.clone()), session_id_from_log(path, &config.agent_backend)))
        .unwrap_or((None, None));
    let state = BrokerState {
        agent_pid: i64::from(child.id()),
        pty_master_fd: master_fd,
        cwd: config.cwd.display().to_string(),
        start_ts,
        sock_path,
        agent_backend: config.agent_backend.clone(),
        owner: config.owner.clone(),
        log_path: initial_log_path,
        session_id: initial_session_id,
        busy: false,
        output_tail: String::new(),
        token: None,
        resume_session_id: clean_env("CODEX_WEB_RESUME_SESSION_ID")
            .or_else(|| resume_session_id_from_args(&config.agent_args, &config.agent_backend, &config.sessions_dir)),
        model_provider: clean_env("CODEX_WEB_MODEL_PROVIDER"),
        preferred_auth_method: clean_env("CODEX_WEB_PREFERRED_AUTH_METHOD"),
        model: clean_env("CODEX_WEB_MODEL"),
        reasoning_effort: clean_env("CODEX_WEB_REASONING_EFFORT"),
        service_tier: clean_env("CODEX_WEB_SERVICE_TIER"),
        transport: clean_env("CODEX_WEB_TRANSPORT"),
        tmux_session: clean_env("CODEX_WEB_TMUX_SESSION"),
        tmux_window: clean_env("CODEX_WEB_TMUX_WINDOW"),
        spawn_nonce: clean_env("CODEX_WEB_SPAWN_NONCE"),
        mirror_output: config.mirror_output,
        term_query_buf: Vec::new(),
        busy_hint_tail: String::new(),
        busy_hint_last_seen: None,
        busy_from_pty_hint: false,
    };
    write_metadata(&state)?;
    let runtime = BrokerRuntime {
        state: Arc::new(Mutex::new(state)),
        stop: Arc::new(AtomicBool::new(false)),
        sessions_dir: config.sessions_dir.clone(),
    };
    let stdin_termios = if config.mirror_input {
        enable_raw_stdin().ok()
    } else {
        None
    };
    spawn_socket_thread(runtime.clone());
    spawn_pty_reader_thread(runtime.clone());
    if config.mirror_input {
        spawn_stdin_thread(runtime.clone());
    }
    install_sigwinch_handler();
    spawn_resize_thread(runtime.clone());
    spawn_log_discovery_thread(runtime.clone());
    spawn_log_idle_thread(runtime.clone());
    spawn_busy_hint_idle_thread(runtime.clone());
    let mut child = child;
    loop {
        if runtime.stop.load(Ordering::Relaxed) {
            terminate_process_group(i64::from(child.id()));
            break;
        }
        match child.try_wait() {
            Ok(Some(_status)) => break,
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(_err) => break,
        }
    }
    runtime.stop.store(true, Ordering::Relaxed);
    if let Some(termios) = stdin_termios.as_ref() {
        let _ = restore_stdin(termios);
    }
    let _ = unsafe { libc::close(master_fd) };
    if let Ok(state) = runtime.state.lock() {
        let _ = fs::remove_file(&state.sock_path);
        let _ = fs::remove_file(state.sock_path.with_extension("json"));
    }
    Ok(())
}

fn spawn_stdin_thread(runtime: BrokerRuntime) {
    thread::spawn(move || {
        let fd = match runtime.state.lock() {
            Ok(state) => state.pty_master_fd,
            Err(_) => return,
        };
        let pty_fd = unsafe { libc::dup(fd) };
        if pty_fd < 0 {
            return;
        }
        let reached_eof = copy_fd_to_pty(libc::STDIN_FILENO, pty_fd, &runtime.stop).unwrap_or(true);
        let _ = unsafe { libc::close(pty_fd) };
        if reached_eof {
            runtime.stop.store(true, Ordering::Relaxed);
        }
    });
}

fn spawn_resize_thread(runtime: BrokerRuntime) {
    thread::spawn(move || {
        let fd = match runtime.state.lock() {
            Ok(state) => state.pty_master_fd,
            Err(_) => return,
        };
        let pty_fd = unsafe { libc::dup(fd) };
        if pty_fd < 0 {
            return;
        }
        while !runtime.stop.load(Ordering::Relaxed) {
            if SIGWINCH_PENDING.swap(false, Ordering::Relaxed) {
                if let Some((rows, cols)) = terminal_size_from_fd(libc::STDIN_FILENO, (40, 120))
                    .or_else(|| terminal_size_from_fd(libc::STDOUT_FILENO, (40, 120)))
                {
                    let _ = set_pty_winsize(pty_fd, rows, cols);
                }
            }
            thread::sleep(Duration::from_millis(100));
        }
        let _ = unsafe { libc::close(pty_fd) };
    });
}

fn spawn_agent(config: &BrokerConfig, slave_fd: RawFd, rows: u16, cols: u16) -> Result<std::process::Child, String> {
    let mut command = if config.owner.as_deref() == Some("web") {
        let shell = env::var("SHELL").ok().filter(|value| !value.trim().is_empty()).unwrap_or_else(|| "/bin/bash".to_string());
        let mut cmd = Command::new(shell);
        let inline = format!(
            "cd {} && exec {}",
            shell_quote(&config.cwd.display().to_string()),
            shell_join(
                std::iter::once(config.agent_bin.clone())
                    .chain(config.agent_args.iter().cloned())
                    .collect::<Vec<_>>()
                    .as_slice(),
            ),
        );
        cmd.args(["-l", "-i", "-c", &inline]);
        cmd
    } else {
        let mut cmd = Command::new(&config.agent_bin);
        cmd.args(config.agent_args.iter().map(String::as_str));
        cmd
    };
    let stdin_fd = unsafe { libc::dup(slave_fd) };
    let stdout_fd = unsafe { libc::dup(slave_fd) };
    let stderr_fd = unsafe { libc::dup(slave_fd) };
    if stdin_fd < 0 || stdout_fd < 0 || stderr_fd < 0 {
        return Err("dup pty slave failed".to_string());
    }
    command
        .current_dir(&config.cwd)
        .env("TERM", env::var("TERM").ok().filter(|value| !value.trim().is_empty()).unwrap_or_else(|| "xterm-256color".to_string()))
        .env("COLUMNS", cols.to_string())
        .env("LINES", rows.to_string())
        .stdin(unsafe { Stdio::from(File::from_raw_fd(stdin_fd)) })
        .stdout(unsafe { Stdio::from(File::from_raw_fd(stdout_fd)) })
        .stderr(unsafe { Stdio::from(File::from_raw_fd(stderr_fd)) });
    if config.agent_backend == "pi" {
        command.env("PI_HOME", &config.agent_home).env_remove("CODEX_HOME");
    } else {
        command.env("CODEX_HOME", &config.agent_home).env_remove("PI_HOME");
    }
    unsafe {
        command.pre_exec(move || {
            libc::setsid();
            libc::ioctl(slave_fd, libc::TIOCSCTTY, 0);
            Ok(())
        });
    }
    let child = command.spawn().map_err(|err| format!("spawn {}: {err}", config.agent_bin))?;
    let _ = unsafe { libc::close(slave_fd) };
    Ok(child)
}

fn spawn_socket_thread(runtime: BrokerRuntime) {
    thread::spawn(move || {
        if let Err(err) = socket_server(runtime.clone()) {
            eprintln!("error: rust broker socket server crashed: {err}");
            runtime.stop.store(true, Ordering::Relaxed);
        }
    });
}

fn socket_server(runtime: BrokerRuntime) -> Result<(), String> {
    let sock_path = runtime.state.lock().map_err(|_| "broker state lock poisoned".to_string())?.sock_path.clone();
    if sock_path.exists() {
        let _ = fs::remove_file(&sock_path);
    }
    let listener = UnixListener::bind(&sock_path).map_err(|err| format!("bind {}: {err}", sock_path.display()))?;
    fs::set_permissions(&sock_path, fs::Permissions::from_mode(0o600))
        .map_err(|err| format!("chmod {}: {err}", sock_path.display()))?;
    listener
        .set_nonblocking(true)
        .map_err(|err| format!("set nonblocking {}: {err}", sock_path.display()))?;
    while !runtime.stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _addr)) => {
                let next = runtime.clone();
                thread::spawn(move || handle_conn(next, stream));
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(50)),
            Err(err) => return Err(format!("accept {}: {err}", sock_path.display())),
        }
    }
    Ok(())
}

fn handle_conn(runtime: BrokerRuntime, mut stream: UnixStream) {
    let cloned = match stream.try_clone() {
        Ok(value) => value,
        Err(err) => {
            let _ = send_json_line(&mut stream, &json!({"error": format!("clone stream: {err}")}));
            return;
        }
    };
    let mut reader = BufReader::new(cloned);
    let mut line = String::new();
    if reader.read_line(&mut line).ok().filter(|n| *n > 0).is_none() {
        return;
    }
    let request = match serde_json::from_str::<Value>(line.trim()) {
        Ok(value) => value,
        Err(err) => {
            let _ = send_json_line(&mut stream, &json!({"error": format!("invalid json: {err}")}));
            return;
        }
    };
    let cmd = request.get("cmd").and_then(Value::as_str).unwrap_or_default();
    match cmd {
        "state" => {
            let response = runtime
                .state
                .lock()
                .map(|state| json!({"busy": state.busy, "queue_len": 0, "token": state.token}))
                .unwrap_or_else(|_| json!({"error": "no state"}));
            let _ = send_json_line(&mut stream, &response);
        }
        "tail" => {
            let response = runtime
                .state
                .lock()
                .map(|state| json!({"tail": state.output_tail}))
                .unwrap_or_else(|_| json!({"tail": ""}));
            let _ = send_json_line(&mut stream, &response);
        }
        "send" => {
            let Some(text) = request.get("text").and_then(Value::as_str).filter(|value| !value.trim().is_empty()) else {
                let _ = send_json_line(&mut stream, &json!({"error": "text required"}));
                return;
            };
            let enter = request
                .get("enter_seq")
                .and_then(Value::as_str)
                .map(seq_bytes)
                .unwrap_or_else(default_enter_seq);
            let fd = {
                let Ok(mut state) = runtime.state.lock() else {
                    let _ = send_json_line(&mut stream, &json!({"error": "no state"}));
                    return;
                };
                state.busy = true;
                state.busy_from_pty_hint = false;
                state.busy_hint_last_seen = None;
                state.pty_master_fd
            };
            let _ = send_json_line(&mut stream, &json!({"queued": false, "queue_len": 0}));
            let _ = inject_text(fd, text, &enter);
        }
        "keys" => {
            let Some(seq) = request.get("seq").and_then(Value::as_str).filter(|value| !value.is_empty()) else {
                let _ = send_json_line(&mut stream, &json!({"error": "seq required"}));
                return;
            };
            let bytes = seq_bytes(seq);
            let fd = match runtime.state.lock() {
                Ok(state) => state.pty_master_fd,
                Err(_) => {
                    let _ = send_json_line(&mut stream, &json!({"error": "no state"}));
                    return;
                }
            };
            let response = json!({"ok": true, "queued": false, "n": bytes.len(), "key_queue_len": 0});
            let _ = send_json_line(&mut stream, &response);
            let _ = write_all_fd(fd, &bytes);
        }
        "shutdown" => {
            let _ = send_json_line(&mut stream, &json!({"ok": true}));
            runtime.stop.store(true, Ordering::Relaxed);
        }
        _ => {
            let _ = send_json_line(&mut stream, &json!({"error": "unknown cmd"}));
        }
    }
}

fn spawn_pty_reader_thread(runtime: BrokerRuntime) {
    thread::spawn(move || {
        let fd = match runtime.state.lock() {
            Ok(state) => state.pty_master_fd,
            Err(_) => return,
        };
        let mut buf = [0u8; 4096];
        while !runtime.stop.load(Ordering::Relaxed) {
            let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
            if n <= 0 {
                break;
            }
            let bytes = &buf[..n as usize];
            let mut state = match runtime.state.lock() {
                Ok(value) => value,
                Err(_) => break,
            };
            if state.mirror_output {
                let _ = std::io::stdout().write_all(bytes);
                let _ = std::io::stdout().flush();
            }
            reply_to_terminal_queries(&mut state, bytes);
            let text = String::from_utf8_lossy(bytes);
            state.output_tail.push_str(&text);
            trim_utf8_tail(&mut state.output_tail, OUTPUT_TAIL_MAX);
            update_busy_from_pty_text(&mut state, &text);
        }
        runtime.stop.store(true, Ordering::Relaxed);
    });
}

fn trim_utf8_tail(text: &mut String, max_bytes: usize) {
    if text.len() <= max_bytes {
        return;
    }
    let mut start = text.len().saturating_sub(max_bytes);
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    text.drain(..start);
}

fn update_busy_from_pty_text(state: &mut BrokerState, text: &str) {
    let cleaned = strip_ansi(text);
    if cleaned.is_empty() {
        return;
    }
    let previous_tail = state.busy_hint_tail.clone();
    state.busy_hint_tail.push_str(&cleaned);
    trim_utf8_tail(&mut state.busy_hint_tail, BUSY_HINT_TAIL_MAX);
    if pty_busy_hint_seen(&previous_tail, &cleaned) {
        if !state.busy || state.busy_from_pty_hint {
            state.busy_from_pty_hint = true;
            state.busy_hint_last_seen = Some(Instant::now());
        }
        state.busy = true;
    }
}

fn clear_stale_pty_busy_hint(state: &mut BrokerState, now: Instant, quiet: Duration) -> bool {
    if !state.busy {
        state.busy_from_pty_hint = false;
        state.busy_hint_last_seen = None;
        return false;
    }
    if !state.busy_from_pty_hint {
        return false;
    }
    let Some(last_seen) = state.busy_hint_last_seen else {
        return false;
    };
    if now.saturating_duration_since(last_seen) < quiet {
        return false;
    }
    state.busy = false;
    state.busy_from_pty_hint = false;
    state.busy_hint_last_seen = None;
    true
}

fn pty_busy_hint_seen(tail: &str, cleaned: &str) -> bool {
    ["esc to interrupt", "compacting context", "compacting conversation"]
        .iter()
        .any(|phrase| hint_seen_in_new_text(tail, cleaned, phrase))
}

fn hint_seen_in_new_text(tail: &str, cleaned: &str, phrase: &str) -> bool {
    let cleaned_lower = cleaned.to_ascii_lowercase();
    let phrase_lower = phrase.to_ascii_lowercase();
    if cleaned_lower.contains(&phrase_lower) {
        return true;
    }
    let overlap = phrase_lower.len().saturating_sub(1);
    if overlap == 0 {
        return false;
    }
    let stitched = format!("{}{}", utf8_suffix(tail, overlap).to_ascii_lowercase(), cleaned_lower);
    stitched
        .find(&phrase_lower)
        .map(|pos| pos + phrase_lower.len() > overlap)
        .unwrap_or(false)
}

fn utf8_suffix(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut start = text.len().saturating_sub(max_bytes);
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

fn strip_ansi(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut idx = 0usize;
    while idx < bytes.len() {
        if bytes[idx] != 0x1b {
            if let Some(ch) = text[idx..].chars().next() {
                out.push(ch);
                idx += ch.len_utf8();
            } else {
                break;
            }
            continue;
        }
        idx += 1;
        if idx >= bytes.len() {
            break;
        }
        match bytes[idx] {
            b']' => {
                idx += 1;
                while idx < bytes.len() {
                    if bytes[idx] == 0x07 {
                        idx += 1;
                        break;
                    }
                    if bytes[idx] == 0x1b && idx + 1 < bytes.len() && bytes[idx + 1] == b'\\' {
                        idx += 2;
                        break;
                    }
                    idx += 1;
                }
            }
            b'[' => {
                idx += 1;
                while idx < bytes.len() {
                    let byte = bytes[idx];
                    idx += 1;
                    if (0x40..=0x7e).contains(&byte) {
                        break;
                    }
                }
            }
            _ => {
                idx += 1;
            }
        }
    }
    out
}

fn spawn_log_discovery_thread(runtime: BrokerRuntime) {
    thread::spawn(move || {
        while !runtime.stop.load(Ordering::Relaxed) {
            let (pid, cwd, backend, current) = match runtime.state.lock() {
                Ok(state) => (
                    state.agent_pid,
                    state.cwd.clone(),
                    state.agent_backend.clone(),
                    state.log_path.clone(),
                ),
                Err(_) => return,
            };
            if current.as_ref().map(|path| path.exists()).unwrap_or(false) {
                thread::sleep(Duration::from_millis(250));
                continue;
            }
            if let Some(path) =
                discover_open_log_for_process_in_sessions(pid, &cwd, &backend, &runtime.sessions_dir)
            {
                if let Some(session_id) = session_id_from_log(&path, &backend) {
                    if let Ok(mut state) = runtime.state.lock() {
                        state.log_path = Some(path);
                        state.session_id = Some(session_id);
                        let _ = write_metadata(&state);
                    }
                }
            }
            thread::sleep(Duration::from_millis(250));
        }
    });
}

fn spawn_log_idle_thread(runtime: BrokerRuntime) {
    thread::spawn(move || {
        let mut token_scan_path: Option<PathBuf> = None;
        let mut token_scan_offset = 0u64;
        while !runtime.stop.load(Ordering::Relaxed) {
            let log_path = match runtime.state.lock() {
                Ok(state) => state.log_path.clone(),
                Err(_) => return,
            };
            if let Some(path) = log_path.as_ref() {
                if token_scan_path.as_ref() != Some(path) {
                    token_scan_path = Some(path.clone());
                    token_scan_offset = 0;
                }
                if let Ok((next_offset, token_update)) = scan_token_updates_from_log(path, token_scan_offset) {
                    token_scan_offset = next_offset;
                    if let Some(token) = token_update {
                        if let Ok(mut state) = runtime.state.lock() {
                            state.token = Some(token);
                        }
                    }
                }
                if let Some(idle) = compute_idle_from_log(path) {
                    if let Ok(mut state) = runtime.state.lock() {
                        state.busy = !idle;
                        state.busy_from_pty_hint = false;
                        state.busy_hint_last_seen = None;
                        if idle && state.resume_session_id.is_some() {
                            state.resume_session_id = None;
                            let _ = write_metadata(&state);
                        }
                    }
                }
            }
            thread::sleep(Duration::from_millis(250));
        }
    });
}

fn spawn_busy_hint_idle_thread(runtime: BrokerRuntime) {
    thread::spawn(move || {
        let quiet = busy_quiet_duration();
        while !runtime.stop.load(Ordering::Relaxed) {
            if let Ok(mut state) = runtime.state.lock() {
                clear_stale_pty_busy_hint(&mut state, Instant::now(), quiet);
            }
            thread::sleep(Duration::from_millis(250));
        }
    });
}

fn busy_quiet_duration() -> Duration {
    let seconds = env::var("CODEX_WEB_BUSY_QUIET_SECONDS")
        .ok()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .unwrap_or(BUSY_QUIET_SECONDS_DEFAULT)
        .max(0.0);
    Duration::from_secs_f64(seconds)
}

fn scan_token_updates_from_log(path: &Path, mut offset: u64) -> Result<(u64, Option<Value>), String> {
    let len = fs::metadata(path).map_err(|err| format!("stat {}: {err}", path.display()))?.len();
    if offset > len {
        offset = 0;
    }
    let mut file = File::open(path).map_err(|err| format!("open {}: {err}", path.display()))?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|err| format!("seek {}: {err}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut next_offset = offset;
    let mut line = String::new();
    let mut latest = None;
    loop {
        line.clear();
        let bytes = reader
            .read_line(&mut line)
            .map_err(|err| format!("read {}: {err}", path.display()))?;
        if bytes == 0 {
            break;
        }
        let complete_line = line.ends_with('\n');
        let parsed = serde_json::from_str::<Value>(line.trim());
        if !complete_line && parsed.is_err() {
            break;
        }
        next_offset += bytes as u64;
        if let Ok(obj) = parsed {
            if let Some(token) = token_update_from_obj(&obj) {
                latest = Some(token);
            }
        }
    }
    Ok((next_offset, latest))
}

fn write_metadata(state: &BrokerState) -> Result<(), String> {
    if let Some(parent) = state.sock_path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create {}: {err}", parent.display()))?;
    }
    let meta_path = state.sock_path.with_extension("json");
    let raw = serde_json::to_string(&state.metadata_value())
        .map_err(|err| format!("serialize {}: {err}", meta_path.display()))?;
    fs::write(&meta_path, raw).map_err(|err| format!("write {}: {err}", meta_path.display()))?;
    fs::set_permissions(&meta_path, fs::Permissions::from_mode(0o600))
        .map_err(|err| format!("chmod {}: {err}", meta_path.display()))
}

fn inject_text(fd: RawFd, text: &str, suffix: &[u8]) -> Result<(), String> {
    let mut payload = Vec::with_capacity(BRACKETED_PASTE_START.len() + text.len() + BRACKETED_PASTE_END.len() + suffix.len());
    payload.extend_from_slice(BRACKETED_PASTE_START);
    payload.extend_from_slice(text.as_bytes());
    payload.extend_from_slice(BRACKETED_PASTE_END);
    payload.extend_from_slice(suffix);
    write_all_fd(fd, &payload)
}

fn write_all_fd(fd: RawFd, mut data: &[u8]) -> Result<(), String> {
    while !data.is_empty() {
        let n = unsafe { libc::write(fd, data.as_ptr().cast(), data.len()) };
        if n <= 0 {
            return Err("write to pty failed".to_string());
        }
        data = &data[n as usize..];
    }
    Ok(())
}

fn copy_fd_to_pty(input_fd: RawFd, pty_fd: RawFd, stop: &AtomicBool) -> Result<bool, String> {
    let mut buf = [0u8; 4096];
    while !stop.load(Ordering::Relaxed) {
        let n = unsafe { libc::read(input_fd, buf.as_mut_ptr().cast(), buf.len()) };
        if n < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(format!("read stdin: {err}"));
        }
        if n == 0 {
            return Ok(true);
        }
        write_all_fd(pty_fd, &buf[..n as usize])?;
    }
    Ok(false)
}

fn should_mirror_stdin() -> bool {
    if clean_env("CODEX_WEB_EMULATE_TERMINAL").as_deref() == Some("1") {
        return false;
    }
    unsafe { libc::isatty(libc::STDIN_FILENO) == 1 }
}

fn enable_raw_stdin() -> Result<libc::termios, String> {
    let fd = libc::STDIN_FILENO;
    let mut original = unsafe { std::mem::zeroed::<libc::termios>() };
    if unsafe { libc::tcgetattr(fd, &mut original) } != 0 {
        return Err(format!("tcgetattr stdin: {}", std::io::Error::last_os_error()));
    }
    let mut raw = original;
    unsafe {
        libc::cfmakeraw(&mut raw);
    }
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) } != 0 {
        return Err(format!("tcsetattr stdin raw: {}", std::io::Error::last_os_error()));
    }
    Ok(original)
}

fn restore_stdin(original: &libc::termios) -> Result<(), String> {
    let fd = libc::STDIN_FILENO;
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, original) } != 0 {
        return Err(format!("restore stdin: {}", std::io::Error::last_os_error()));
    }
    Ok(())
}

fn send_json_line(stream: &mut UnixStream, value: &Value) -> Result<(), String> {
    let mut raw = serde_json::to_vec(value).map_err(|err| format!("serialize socket response: {err}"))?;
    raw.push(b'\n');
    stream.write_all(&raw).map_err(|err| format!("write socket response: {err}"))
}

fn seq_bytes(raw: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            let mut buf = [0u8; 4];
            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            continue;
        }
        match chars.next() {
            Some('r') => out.push(b'\r'),
            Some('n') => out.push(b'\n'),
            Some('t') => out.push(b'\t'),
            Some('\\') => out.push(b'\\'),
            Some('x') => {
                let hi = chars.next().and_then(|value| value.to_digit(16));
                let lo = chars.next().and_then(|value| value.to_digit(16));
                match (hi, lo) {
                    (Some(hi), Some(lo)) => out.push(((hi << 4) + lo) as u8),
                    _ => out.extend_from_slice(b"\\x"),
                }
            }
            Some(other) => {
                out.push(b'\\');
                let mut buf = [0u8; 4];
                out.extend_from_slice(other.encode_utf8(&mut buf).as_bytes());
            }
            None => out.push(b'\\'),
        }
    }
    if out.is_empty() { vec![b'\r'] } else { out }
}

fn default_enter_seq() -> Vec<u8> {
    env::var("CODEX_WEB_ENTER_SEQ")
        .ok()
        .map(|value| seq_bytes(&value))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| vec![b'\r'])
}

fn reply_to_terminal_queries(state: &mut BrokerState, bytes: &[u8]) {
    state.term_query_buf.extend_from_slice(bytes);
    if state.term_query_buf.len() > 256 {
        let keep_from = state.term_query_buf.len() - 256;
        state.term_query_buf = state.term_query_buf[keep_from..].to_vec();
    }
    for (query, response) in [
        (b"\x1b[5n".as_slice(), b"\x1b[0n".as_slice()),
        (b"\x1b[6n".as_slice(), b"\x1b[1;1R".as_slice()),
        (b"\x1b[c".as_slice(), b"\x1b[?1;2c".as_slice()),
        (b"\x1b[>c".as_slice(), b"\x1b[>0;0;0c".as_slice()),
        (b"\x1b[?u".as_slice(), b"\x1b[?1u".as_slice()),
        (b"\x1b]10;?\x1b\\".as_slice(), b"\x1b]10;rgb:c0c0/c0c0/c0c0\x1b\\".as_slice()),
        (b"\x1b]11;?\x1b\\".as_slice(), b"\x1b]11;rgb:0000/0000/0000\x1b\\".as_slice()),
    ] {
        if contains_bytes(&state.term_query_buf, query) {
            let _ = write_all_fd(state.pty_master_fd, response);
            state.term_query_buf.clear();
        }
    }
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

fn session_id_from_rollout_path(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    for part in name.split(|ch| ch == '-' || ch == '.') {
        if part.len() == 12 && name.len() >= 36 {
            break;
        }
    }
    let bytes = name.as_bytes();
    for idx in 0..bytes.len().saturating_sub(36) {
        let candidate = &name[idx..idx + 36];
        if is_uuid_like(candidate) {
            return Some(candidate.to_ascii_lowercase());
        }
    }
    None
}

fn session_id_from_log(path: &Path, agent_backend: &str) -> Option<String> {
    if agent_backend == "pi" {
        let file = File::open(path).ok()?;
        let mut reader = BufReader::new(file);
        let mut line = String::new();
        reader.read_line(&mut line).ok().filter(|n| *n > 0)?;
        let value = serde_json::from_str::<Value>(line.trim()).ok()?;
        if value.get("type").and_then(Value::as_str) != Some("session") {
            return None;
        }
        return value
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
    }
    session_id_from_rollout_path(path)
}

fn is_uuid_like(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (idx, byte) in bytes.iter().enumerate() {
        if matches!(idx, 8 | 13 | 18 | 23) {
            if *byte != b'-' {
                return false;
            }
        } else if !byte.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

fn open_pty(rows: u16, cols: u16) -> Result<(RawFd, RawFd), String> {
    let mut master = 0;
    let mut slave = 0;
    let mut winsize = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let rc = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null(),
            &mut winsize,
        )
    };
    if rc != 0 {
        return Err("openpty failed".to_string());
    }
    Ok((master, slave))
}

fn set_pty_winsize(fd: RawFd, rows: u16, cols: u16) -> Result<(), String> {
    let mut winsize = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    if unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, &mut winsize) } != 0 {
        return Err(format!("set pty winsize: {}", std::io::Error::last_os_error()));
    }
    Ok(())
}

fn terminal_size() -> (u16, u16) {
    terminal_size_from_fd(libc::STDIN_FILENO, (40, 120))
        .or_else(|| terminal_size_from_fd(libc::STDOUT_FILENO, (40, 120)))
        .unwrap_or((40, 120))
}

fn terminal_size_from_fd(fd: RawFd, fallback: (u16, u16)) -> Option<(u16, u16)> {
    let mut winsize = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    if unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut winsize) } != 0 {
        return None;
    }
    let rows = if winsize.ws_row > 0 { winsize.ws_row } else { fallback.0 };
    let cols = if winsize.ws_col > 0 { winsize.ws_col } else { fallback.1 };
    Some((rows, cols))
}

extern "C" fn handle_sigwinch(_signal: libc::c_int) {
    SIGWINCH_PENDING.store(true, Ordering::Relaxed);
}

fn install_sigwinch_handler() {
    unsafe {
        libc::signal(libc::SIGWINCH, handle_sigwinch as libc::sighandler_t);
    }
}

fn terminate_process_group(pid: i64) {
    if pid <= 0 {
        return;
    }
    unsafe {
        libc::killpg(pid as libc::pid_t, libc::SIGTERM);
    }
    thread::sleep(Duration::from_millis(200));
    unsafe {
        libc::killpg(pid as libc::pid_t, libc::SIGKILL);
    }
}

fn resume_session_id_from_args(args: &[String], agent_backend: &str, sessions_dir: &Path) -> Option<String> {
    if agent_backend == "pi" {
        for pair in args.windows(2) {
            if pair[0] != "--session" {
                continue;
            }
            let raw = pair[1].trim();
            if raw.is_empty() {
                return None;
            }
            if raw.ends_with(".jsonl") {
                return session_log_path_from_args(args, agent_backend, sessions_dir)
                    .as_ref()
                    .and_then(|path| session_id_from_log(path, agent_backend));
            }
            return Some(raw.to_string());
        }
        return None;
    }
    args.windows(2)
        .find(|pair| pair[0] == "resume")
        .map(|pair| pair[1].trim().to_string())
        .filter(|value| !value.is_empty())
}

fn session_log_path_from_args(args: &[String], agent_backend: &str, sessions_dir: &Path) -> Option<PathBuf> {
    if agent_backend != "pi" {
        return None;
    }
    for pair in args.windows(2) {
        if pair[0] != "--session" {
            continue;
        }
        let raw = pair[1].trim();
        if raw.is_empty() || !raw.ends_with(".jsonl") {
            return None;
        }
        let path = expand_path(raw).ok()?;
        if path.starts_with(sessions_dir) {
            return Some(path);
        }
        return None;
    }
    None
}

fn ensure_pi_session_arg(args: &[String], cwd: &Path, sessions_dir: &Path) -> Result<Vec<String>, String> {
    let mut out = args.to_vec();
    if out.iter().any(|value| value == "--session") {
        return Ok(out);
    }
    let Some(session_dir) = pi_session_dir_from_args(&out, cwd, sessions_dir)? else {
        return Ok(out);
    };
    fs::create_dir_all(&session_dir).map_err(|err| format!("create {}: {err}", session_dir.display()))?;
    out.push("--session".to_string());
    out.push(pi_new_session_log_path(&session_dir).display().to_string());
    Ok(out)
}

fn pi_session_dir_from_args(args: &[String], cwd: &Path, sessions_dir: &Path) -> Result<Option<PathBuf>, String> {
    if args.iter().any(|value| value == "--no-session") {
        return Ok(None);
    }
    for pair in args.windows(2) {
        if pair[0] != "--session-dir" {
            continue;
        }
        let raw = pair[1].trim();
        if raw.is_empty() {
            return Ok(None);
        }
        let path = expand_path(raw)?;
        return Ok(Some(if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        }));
    }
    Ok(Some(sessions_dir.join(pi_session_dir_name(&cwd.display().to_string()))))
}

fn pi_session_dir_name(cwd: &str) -> String {
    let normalized = cwd
        .trim_start_matches(['/', '\\'])
        .replace(['/', '\\', ':'], "-");
    format!("--{normalized}--")
}

fn pi_new_session_log_path(session_dir: &Path) -> PathBuf {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    session_dir.join(format!("{}-{}.jsonl", now_ms, std::process::id()))
}

fn agent_sessions_dir(agent_backend: &str, agent_home: &Path) -> PathBuf {
    if agent_backend == "pi" {
        agent_home.join("agent").join("sessions")
    } else {
        agent_home.join("sessions")
    }
}

fn expand_path(raw: &str) -> Result<PathBuf, String> {
    let value = raw.trim();
    if value.is_empty() {
        return Err("cwd required".to_string());
    }
    let path = if value == "~" {
        home_dir()
    } else if let Some(rest) = value.strip_prefix("~/") {
        home_dir().join(rest)
    } else {
        PathBuf::from(value)
    };
    Ok(if path.is_absolute() {
        path
    } else {
        env::current_dir().map_err(|err| format!("current dir: {err}"))?.join(path)
    })
}

fn env_path(key: &str) -> Option<PathBuf> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn clean_env(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn home_dir() -> PathBuf {
    env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/"))
}

fn epoch_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn shell_join(items: &[String]) -> String {
    items.iter().map(|item| shell_quote(item)).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::{
        copy_fd_to_pty, ensure_pi_session_arg, is_uuid_like, resume_session_id_from_args,
        open_pty, run_broker, scan_token_updates_from_log, seq_bytes, session_id_from_log,
        session_id_from_rollout_path, session_log_path_from_args, set_pty_winsize, shell_quote,
        strip_ansi, terminal_size_from_fd, trim_utf8_tail, update_busy_from_pty_text,
        clear_stale_pty_busy_hint, web_owned_codex_args, write_all_fd, BrokerConfig, BrokerState,
    };
    use serde_json::{json, Value};
    use std::fs;
    use std::io::{BufRead, BufReader, Write};
    use std::os::fd::RawFd;
    use std::os::unix::net::UnixStream;
    use std::path::Path;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn broker_seq_bytes_decodes_python_style_escape_sequences() {
        assert_eq!(seq_bytes("\\x1b"), vec![0x1b]);
        assert_eq!(seq_bytes("hi\\r"), b"hi\r".to_vec());
        assert_eq!(seq_bytes("\\n\\t\\\\"), b"\n\t\\".to_vec());
    }

    #[test]
    fn rust_broker_prepends_headless_web_codex_config_args() {
        let args = vec!["--model".to_string(), "gpt-5.4".to_string()];
        let normalized = web_owned_codex_args("codex", Some("web"), &args);
        assert_eq!(
            normalized,
            vec![
                "-c",
                "disable_response_storage=false",
                "-c",
                "disable_paste_burst=true",
                "--model",
                "gpt-5.4",
            ]
        );
        assert_eq!(web_owned_codex_args("codex", None, &args), args);
        assert_eq!(web_owned_codex_args("pi", Some("web"), &args), args);
    }

    #[test]
    fn broker_extracts_codex_session_id_from_rollout_filename() {
        let path = Path::new("/tmp/rollout-2026-04-28T00-00-00-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee.jsonl");
        assert_eq!(
            session_id_from_rollout_path(path).as_deref(),
            Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
        );
        assert!(is_uuid_like("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"));
    }

    #[test]
    fn rust_broker_scans_codex_token_updates_incrementally() {
        let app_dir = temp_app_dir("broker-token");
        let log_path = app_dir.join("rollout-2026-04-28T00-00-00-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee.jsonl");
        let first = json!({
            "type": "event_msg",
            "timestamp": "2026-04-28T00:00:00Z",
            "payload": {
                "type": "token_count",
                "info": {
                    "total_token_usage": {},
                    "model_context_window": 128000,
                    "last_token_usage": {"total_tokens": 64000}
                }
            }
        });
        fs::write(&log_path, format!("{first}\n")).unwrap();

        let (offset, token) = scan_token_updates_from_log(&log_path, 0).unwrap();
        let token = token.unwrap();
        assert_eq!(token["context_window"], 128000);
        assert_eq!(token["tokens_in_context"], 64000);
        assert_eq!(token["tokens_remaining"], 64000);
        assert_eq!(token["percent_remaining"], 55);
        assert_eq!(token["baseline_tokens"], 12000);
        assert_eq!(token["as_of"], "2026-04-28T00:00:00Z");

        let second = json!({
            "type": "event_msg",
            "timestamp": "2026-04-28T00:00:01Z",
            "payload": {
                "type": "token_count",
                "info": {
                    "total_token_usage": {},
                    "model_context_window": 128000,
                    "last_token_usage": {"total_tokens": 118000}
                }
            }
        });
        let mut file = fs::OpenOptions::new().append(true).open(&log_path).unwrap();
        writeln!(file, "{{not-json").unwrap();
        writeln!(file, "{second}").unwrap();

        let (_next_offset, token) = scan_token_updates_from_log(&log_path, offset).unwrap();
        let token = token.unwrap();
        assert_eq!(token["tokens_in_context"], 118000);
        assert_eq!(token["tokens_remaining"], 10000);
        assert_eq!(token["percent_remaining"], 9);
        assert_eq!(token["as_of"], "2026-04-28T00:00:01Z");
    }

    #[test]
    fn rust_broker_trims_output_tail_on_utf8_boundary() {
        let mut tail = format!("prefix {}", "─".repeat(80));
        trim_utf8_tail(&mut tail, 159);
        assert!(tail.len() <= 159);
        assert!(tail.is_char_boundary(0));
        assert!(tail.chars().all(|ch| ch == '─'));
    }

    #[test]
    fn rust_broker_strips_terminal_ansi_for_busy_hints() {
        assert_eq!(
            strip_ansi("\x1b[?25lEsc to interrupt\x1b[0m\x1b]11;?\x1b\\"),
            "Esc to interrupt"
        );
    }

    #[test]
    fn rust_broker_marks_busy_from_split_pty_hints() {
        let mut state = test_broker_state();
        update_busy_from_pty_text(&mut state, "Esc to ");
        assert!(!state.busy);
        update_busy_from_pty_text(&mut state, "interrupt");
        assert!(state.busy);
        assert!(state.busy_from_pty_hint);

        state.busy = false;
        update_busy_from_pty_text(&mut state, "\x1b[2KCompacting conversation");
        assert!(state.busy);
        assert!(state.busy_from_pty_hint);
    }

    #[test]
    fn rust_broker_clears_only_stale_pty_hint_busy_state() {
        let mut state = test_broker_state();
        let now = Instant::now();
        state.busy = true;
        state.busy_from_pty_hint = true;
        state.busy_hint_last_seen = Some(now - Duration::from_secs(4));
        assert!(clear_stale_pty_busy_hint(&mut state, now, Duration::from_secs(3)));
        assert!(!state.busy);
        assert!(!state.busy_from_pty_hint);

        state.busy = true;
        state.busy_from_pty_hint = false;
        state.busy_hint_last_seen = Some(now - Duration::from_secs(4));
        assert!(!clear_stale_pty_busy_hint(&mut state, now, Duration::from_secs(3)));
        assert!(state.busy);
    }

    #[test]
    fn rust_broker_reads_terminal_size_from_pty() {
        let (master_fd, slave_fd) = open_pty(33, 101).unwrap();
        assert_eq!(terminal_size_from_fd(master_fd, (40, 120)), Some((33, 101)));
        assert_eq!(terminal_size_from_fd(slave_fd, (40, 120)), Some((33, 101)));
        let _ = unsafe { libc::close(master_fd) };
        let _ = unsafe { libc::close(slave_fd) };
        assert_eq!(terminal_size_from_fd(-1, (40, 120)), None);
    }

    #[test]
    fn rust_broker_updates_pty_winsize_after_start() {
        let (master_fd, slave_fd) = open_pty(33, 101).unwrap();
        set_pty_winsize(master_fd, 44, 132).unwrap();
        assert_eq!(terminal_size_from_fd(master_fd, (40, 120)), Some((44, 132)));
        assert_eq!(terminal_size_from_fd(slave_fd, (40, 120)), Some((44, 132)));
        let _ = unsafe { libc::close(master_fd) };
        let _ = unsafe { libc::close(slave_fd) };
    }

    #[test]
    fn rust_broker_copies_terminal_input_to_pty() {
        let mut input = [0; 2];
        let mut output = [0; 2];
        assert_eq!(unsafe { libc::pipe(input.as_mut_ptr()) }, 0);
        assert_eq!(unsafe { libc::pipe(output.as_mut_ptr()) }, 0);
        let input_read = input[0];
        let input_write = input[1];
        let output_read = output[0];
        let output_write = output[1];
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = stop.clone();
        let handle = thread::spawn(move || {
            let result = copy_fd_to_pty(input_read, output_write, &stop_for_thread);
            let _ = unsafe { libc::close(input_read) };
            let _ = unsafe { libc::close(output_write) };
            result
        });

        write_all_fd(input_write, b"hello from terminal\n").unwrap();
        let _ = unsafe { libc::close(input_write) };
        let copied = read_fd_to_end(output_read);
        let _ = unsafe { libc::close(output_read) };

        assert_eq!(handle.join().unwrap().unwrap(), true);
        assert_eq!(copied, b"hello from terminal\n");
    }

    #[test]
    fn rust_broker_injects_pi_session_path_for_new_sessions() {
        let app_dir = temp_app_dir("broker-pi-session");
        let sessions_dir = app_dir.join("pi-home").join("agent").join("sessions");
        let args = ensure_pi_session_arg(
            &["--model".to_string(), "gpt-5.4".to_string()],
            Path::new("/tmp/pi-work"),
            &sessions_dir,
        )
        .unwrap();
        assert_eq!(&args[..2], ["--model", "gpt-5.4"]);
        assert_eq!(args[2], "--session");
        let session_path = PathBuf::from(&args[3]);
        assert!(session_path.starts_with(sessions_dir.join("--tmp-pi-work--")));
    }

    #[test]
    fn rust_broker_reads_pi_resume_session_id_from_log_arg() {
        let app_dir = temp_app_dir("broker-pi-resume");
        let sessions_dir = app_dir.join("pi-home").join("agent").join("sessions");
        fs::create_dir_all(&sessions_dir).unwrap();
        let log_path = sessions_dir.join("resume.jsonl");
        fs::write(&log_path, r#"{"type":"session","id":"resume-a","cwd":"/tmp"}"#).unwrap();
        let args = vec!["--session".to_string(), log_path.display().to_string()];
        assert_eq!(
            session_log_path_from_args(&args, "pi", &sessions_dir).as_deref(),
            Some(log_path.as_path())
        );
        assert_eq!(session_id_from_log(&log_path, "pi").as_deref(), Some("resume-a"));
        assert_eq!(resume_session_id_from_args(&args, "pi", &sessions_dir).as_deref(), Some("resume-a"));
    }

    #[test]
    fn rust_broker_serves_socket_state_tail_keys_and_shutdown() {
        let app_dir = temp_app_dir("broker-socket");
        let config = BrokerConfig {
            cwd: app_dir.clone(),
            agent_args: vec![
                "-c".to_string(),
                "printf READY; while true; do sleep 1; done".to_string(),
            ],
            app_dir: app_dir.clone(),
            agent_backend: "codex".to_string(),
            agent_bin: "sh".to_string(),
            agent_home: app_dir.join("codex-home"),
            sessions_dir: app_dir.join("codex-home").join("sessions"),
            owner: None,
            mirror_output: false,
            mirror_input: false,
        };
        let handle = thread::spawn(move || run_broker(config));
        let sock_path = wait_for_sock(&app_dir);
        let state = socket_request(&sock_path, json!({"cmd":"state"}));
        assert_eq!(state.get("queue_len").and_then(Value::as_i64), Some(0));
        assert_eq!(state.get("busy").and_then(Value::as_bool), Some(false));

        let tail = wait_for_tail(&sock_path);
        assert!(tail.contains("READY"));

        let keys = socket_request(&sock_path, json!({"cmd":"keys","seq":"\\x1b"}));
        assert_eq!(keys.get("ok").and_then(Value::as_bool), Some(true));
        assert_eq!(keys.get("n").and_then(Value::as_i64), Some(1));

        let shutdown = socket_request(&sock_path, json!({"cmd":"shutdown"}));
        assert_eq!(shutdown.get("ok").and_then(Value::as_bool), Some(true));
        let result = handle.join().unwrap();
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn rust_broker_serves_socket_for_pi_backend() {
        let app_dir = temp_app_dir("broker-pi-socket");
        let pi_home = app_dir.join("pi-home");
        let config = BrokerConfig {
            cwd: app_dir.clone(),
            agent_args: vec![
                "-c".to_string(),
                "printf READY; while true; do sleep 1; done".to_string(),
            ],
            app_dir: app_dir.clone(),
            agent_backend: "pi".to_string(),
            agent_bin: "sh".to_string(),
            agent_home: pi_home.clone(),
            sessions_dir: pi_home.join("agent").join("sessions"),
            owner: None,
            mirror_output: false,
            mirror_input: false,
        };
        let handle = thread::spawn(move || run_broker(config));
        let sock_path = wait_for_sock(&app_dir);
        let meta: Value = serde_json::from_str(&fs::read_to_string(sock_path.with_extension("json")).unwrap()).unwrap();
        assert_eq!(meta["agent_backend"], "pi");
        assert!(meta["sock_path"].as_str().unwrap_or_default().ends_with(".sock"));

        let tail = wait_for_tail(&sock_path);
        assert!(tail.contains("READY"));
        let shutdown = socket_request(&sock_path, json!({"cmd":"shutdown"}));
        assert_eq!(shutdown.get("ok").and_then(Value::as_bool), Some(true));
        let result = handle.join().unwrap();
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn rust_broker_discovers_pi_session_log_after_start() {
        let app_dir = temp_app_dir("broker-pi-discovery");
        let pi_home = app_dir.join("pi-home");
        let sessions_dir = pi_home.join("agent").join("sessions");
        let session_dir = sessions_dir.join("--tmp-pi-discovery--");
        fs::create_dir_all(&session_dir).unwrap();
        let log_path = session_dir.join("live.jsonl");
        let mut header = json!({
            "type": "session",
            "id": "pi-live",
            "cwd": app_dir.display().to_string(),
        })
        .to_string();
        header.push('\n');
        let script = format!(
            "exec 3>{}; printf %s {} >&3; printf READY; while true; do sleep 1; done",
            shell_quote(&log_path.display().to_string()),
            shell_quote(&header),
        );
        let config = BrokerConfig {
            cwd: app_dir.clone(),
            agent_args: vec!["-c".to_string(), script],
            app_dir: app_dir.clone(),
            agent_backend: "pi".to_string(),
            agent_bin: "sh".to_string(),
            agent_home: pi_home.clone(),
            sessions_dir,
            owner: None,
            mirror_output: false,
            mirror_input: false,
        };
        let handle = thread::spawn(move || run_broker(config));
        let sock_path = wait_for_sock(&app_dir);
        let meta = wait_for_meta_session_id(&sock_path, "pi-live");
        let expected_log_path = log_path.display().to_string();
        assert_eq!(meta["log_path"].as_str(), Some(expected_log_path.as_str()));

        let shutdown = socket_request(&sock_path, json!({"cmd":"shutdown"}));
        assert_eq!(shutdown.get("ok").and_then(Value::as_bool), Some(true));
        let result = handle.join().unwrap();
        assert!(result.is_ok(), "{result:?}");
    }

    fn temp_app_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "codoxear-rs-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(path.join("socks")).unwrap();
        path
    }

    fn test_broker_state() -> BrokerState {
        let app_dir = temp_app_dir("broker-state");
        BrokerState {
            agent_pid: 1,
            pty_master_fd: -1,
            cwd: app_dir.display().to_string(),
            start_ts: 0.0,
            sock_path: app_dir.join("socks").join("broker.sock"),
            agent_backend: "codex".to_string(),
            owner: None,
            log_path: None,
            session_id: None,
            busy: false,
            output_tail: String::new(),
            token: None,
            resume_session_id: None,
            model_provider: None,
            preferred_auth_method: None,
            model: None,
            reasoning_effort: None,
            service_tier: None,
            transport: None,
            tmux_session: None,
            tmux_window: None,
            spawn_nonce: None,
            mirror_output: false,
            term_query_buf: Vec::new(),
            busy_hint_tail: String::new(),
            busy_hint_last_seen: None,
            busy_from_pty_hint: false,
        }
    }

    fn wait_for_sock(app_dir: &Path) -> PathBuf {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            let socks = fs::read_dir(app_dir.join("socks"))
                .unwrap()
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("sock"))
                .collect::<Vec<_>>();
            if let Some(path) = socks.into_iter().next() {
                return path;
            }
            thread::sleep(Duration::from_millis(25));
        }
        panic!("broker socket was not published");
    }

    fn wait_for_tail(sock_path: &Path) -> String {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            let payload = socket_request(sock_path, json!({"cmd":"tail"}));
            let tail = payload.get("tail").and_then(Value::as_str).unwrap_or_default().to_string();
            if tail.contains("READY") {
                return tail;
            }
            thread::sleep(Duration::from_millis(25));
        }
        panic!("broker tail never contained READY");
    }

    fn wait_for_meta_session_id(sock_path: &Path, expected: &str) -> Value {
        let meta_path = sock_path.with_extension("json");
        let deadline = Instant::now() + Duration::from_secs(4);
        while Instant::now() < deadline {
            if let Ok(raw) = fs::read_to_string(&meta_path) {
                if let Ok(meta) = serde_json::from_str::<Value>(&raw) {
                    if meta.get("session_id").and_then(Value::as_str) == Some(expected) {
                        return meta;
                    }
                }
            }
            thread::sleep(Duration::from_millis(25));
        }
        panic!("broker metadata never reported session_id {expected}");
    }

    fn socket_request(sock_path: &Path, request: Value) -> Value {
        let mut stream = UnixStream::connect(sock_path).unwrap();
        let mut raw = serde_json::to_vec(&request).unwrap();
        raw.push(b'\n');
        stream.write_all(&raw).unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        serde_json::from_str(line.trim()).unwrap()
    }

    fn read_fd_to_end(fd: RawFd) -> Vec<u8> {
        let mut out = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
            if n <= 0 {
                break;
            }
            out.extend_from_slice(&buf[..n as usize]);
        }
        out
    }
}
