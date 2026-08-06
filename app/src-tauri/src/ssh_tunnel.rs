//! Windows OpenSSH local-forward manager for a remote, loopback-only ComfyUI.
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs,
    io::BufReader,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const STDERR_LIMIT: usize = 8192;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshTunnelConfig {
    pub host: String,
    pub user: String,
    pub port: u16,
    pub identity_file: PathBuf,
    pub known_hosts_file: PathBuf,
    pub remote_comfy_port: u16,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TunnelPhase {
    Starting,
    Ready,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshTunnelStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub endpoint: Option<String>,
    pub local_port: Option<u16>,
    pub started_at: Option<u64>,
    pub exit_code: Option<i32>,
    pub phase: TunnelPhase,
    pub error_code: Option<String>,
    pub error: Option<String>,
}

pub struct SshTunnelProcess {
    child: Child,
    endpoint: String,
    local_port: u16,
    started_at: u64,
    stderr: Arc<Mutex<VecDeque<u8>>>,
}

#[derive(Default)]
enum TunnelSlot {
    #[default]
    Empty,
    Starting,
    Running(SshTunnelProcess),
}

#[derive(Default)]
pub struct SshTunnelState(Mutex<TunnelSlot>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshLaunchPlan {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub endpoint: String,
    pub local_port: u16,
}

fn validate_atom(value: &str, label: &str, allow_colon: bool) -> Result<(), String> {
    let bad = value.is_empty()
        || value.starts_with('-')
        || value.chars().any(|c| c.is_control() || c.is_whitespace())
        || value.contains(['@', '/', '\\'])
        || (!allow_colon && value.contains(':'));
    if bad {
        Err(format!("{label}格式无效"))
    } else {
        Ok(())
    }
}

fn canonical_file(path: &Path, label: &str) -> Result<PathBuf, String> {
    let value = fs::canonicalize(path).map_err(|e| format!("读取{label}失败：{e}"))?;
    if !value.is_file() {
        return Err(format!("{label}必须是普通文件"));
    }
    Ok(value)
}

pub fn system_ssh_path() -> Result<PathBuf, String> {
    let windows = std::env::var_os("WINDIR").ok_or("找不到 Windows 系统目录")?;
    let expected = PathBuf::from(windows)
        .join("System32")
        .join("OpenSSH")
        .join("ssh.exe");
    let canonical = canonical_file(&expected, "Windows OpenSSH 客户端")?;
    if !canonical
        .to_string_lossy()
        .to_ascii_lowercase()
        .ends_with(r"\system32\openssh\ssh.exe")
    {
        return Err("Windows OpenSSH 客户端路径异常".into());
    }
    Ok(canonical)
}

pub fn allocate_loopback_port() -> Result<u16, String> {
    let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .map_err(|e| format!("分配本地隧道端口失败：{e}"))?;
    Ok(listener.local_addr().map_err(|e| e.to_string())?.port())
}

pub fn build_launch_plan(
    config: &SshTunnelConfig,
    program: PathBuf,
    local_port: u16,
) -> Result<SshLaunchPlan, String> {
    validate_atom(&config.host, "SSH 主机", true)?;
    validate_atom(&config.user, "SSH 用户名", false)?;
    if config.port == 0 || config.remote_comfy_port == 0 || local_port == 0 {
        return Err("SSH 或 ComfyUI 端口无效".into());
    }
    // A colon is only valid in a syntactically valid IPv6 literal.
    if config.host.contains(':') && config.host.parse::<IpAddr>().is_err() {
        return Err("SSH 主机格式无效".into());
    }
    let identity = canonical_file(&config.identity_file, "SSH 私钥")?;
    let known_hosts = canonical_file(&config.known_hosts_file, "known_hosts")?;
    if fs::metadata(&known_hosts).map_err(|e| e.to_string())?.len() == 0 {
        return Err("known_hosts 不能为空".into());
    }
    let destination = if config.host.contains(':') {
        format!("{}@[{}]", config.user, config.host)
    } else {
        format!("{}@{}", config.user, config.host)
    };
    let args = vec![
        "-F".into(),
        "NUL".into(),
        "-N".into(),
        "-T".into(),
        "-p".into(),
        config.port.to_string(),
        "-i".into(),
        identity.to_string_lossy().into_owned(),
        "-L".into(),
        format!(
            "127.0.0.1:{local_port}:127.0.0.1:{}",
            config.remote_comfy_port
        ),
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "PasswordAuthentication=no".into(),
        "-o".into(),
        "KbdInteractiveAuthentication=no".into(),
        "-o".into(),
        "IdentitiesOnly=yes".into(),
        "-o".into(),
        "ForwardAgent=no".into(),
        "-o".into(),
        "ClearAllForwardings=yes".into(),
        "-o".into(),
        "ExitOnForwardFailure=yes".into(),
        "-o".into(),
        "StrictHostKeyChecking=yes".into(),
        "-o".into(),
        format!("UserKnownHostsFile={}", known_hosts.to_string_lossy()),
        "-o".into(),
        "GlobalKnownHostsFile=NUL".into(),
        "-o".into(),
        "ServerAliveInterval=15".into(),
        "-o".into(),
        "ServerAliveCountMax=3".into(),
        "-o".into(),
        "ConnectTimeout=10".into(),
        "-o".into(),
        "ConnectionAttempts=1".into(),
        "-o".into(),
        "RequestTTY=no".into(),
        "-o".into(),
        "PermitLocalCommand=no".into(),
        destination,
    ];
    Ok(SshLaunchPlan {
        program,
        args,
        endpoint: format!("http://127.0.0.1:{local_port}"),
        local_port,
    })
}

fn stderr_reader(stderr: std::process::ChildStderr) -> Arc<Mutex<VecDeque<u8>>> {
    let shared = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_LIMIT)));
    let output = shared.clone();
    std::thread::spawn(move || {
        for byte in BufReader::new(stderr).bytes().filter_map(Result::ok) {
            if let Ok(mut tail) = output.lock() {
                if tail.len() == STDERR_LIMIT {
                    tail.pop_front();
                }
                tail.push_back(byte);
            }
        }
    });
    shared
}

fn tail(value: &Arc<Mutex<VecDeque<u8>>>) -> String {
    value
        .lock()
        .map(|mut v| String::from_utf8_lossy(v.make_contiguous()).into_owned())
        .unwrap_or_default()
}

pub fn classify_ssh_error(stderr: &str) -> (&'static str, &'static str) {
    let s = stderr.to_ascii_lowercase();
    if s.contains("host key verification failed")
        || s.contains("remote host identification has changed")
    {
        (
            "host_key_mismatch",
            "SSH 主机密钥校验失败，请重新核对服务商提供的指纹",
        )
    } else if s.contains("permission denied") {
        ("authentication_failed", "SSH 公钥认证失败")
    } else if s.contains("administratively prohibited") {
        ("forwarding_denied", "远端 SSH 服务禁止该端口转发")
    } else if s.contains("address already in use") || s.contains("cannot listen to port") {
        ("local_port_busy", "本地隧道端口被占用")
    } else if s.contains("connection timed out") {
        ("connection_timeout", "SSH 连接超时")
    } else if s.contains("connection refused") {
        ("connection_refused", "SSH 连接被拒绝")
    } else if s.contains("could not resolve hostname") {
        ("dns_failed", "SSH 主机名解析失败")
    } else {
        ("ssh_failed", "SSH 隧道进程异常退出")
    }
}

fn failed(stderr: &str, exit_code: Option<i32>) -> SshTunnelStatus {
    let (code, message) = classify_ssh_error(stderr);
    SshTunnelStatus {
        running: false,
        pid: None,
        endpoint: None,
        local_port: None,
        started_at: None,
        exit_code,
        phase: TunnelPhase::Failed,
        error_code: Some(code.into()),
        error: Some(message.into()),
    }
}

fn spawn(plan: &SshLaunchPlan) -> Result<SshTunnelProcess, String> {
    let mut command = Command::new(&plan.program);
    command
        .args(&plan.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("启动 Windows OpenSSH 失败：{e}"))?;
    let stderr = child
        .stderr
        .take()
        .map(stderr_reader)
        .ok_or("读取 SSH 错误输出失败")?;
    Ok(SshTunnelProcess {
        child,
        endpoint: plan.endpoint.clone(),
        local_port: plan.local_port,
        started_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        stderr,
    })
}

async fn wait_ready(process: &mut SshTunnelProcess) -> Result<(), SshTunnelStatus> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(1))
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|_| failed("", None))?;
    let url = format!("{}/object_info", process.endpoint);
    for _ in 0..100 {
        if let Ok(Some(status)) = process.child.try_wait() {
            return Err(failed(&tail(&process.stderr), status.code()));
        }
        if client
            .get(&url)
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let _ = process.child.kill();
    let _ = process.child.wait();
    Err(SshTunnelStatus {
        running: false,
        pid: None,
        endpoint: None,
        local_port: None,
        started_at: None,
        exit_code: None,
        phase: TunnelPhase::Failed,
        error_code: Some("remote_comfy_unavailable".into()),
        error: Some("SSH 已连接，但 20 秒内没有检测到远端 ComfyUI /object_info".into()),
    })
}

#[tauri::command]
pub async fn ssh_tunnel_start(
    state: tauri::State<'_, SshTunnelState>,
    config: SshTunnelConfig,
) -> Result<SshTunnelStatus, String> {
    {
        let mut slot = state.0.lock().map_err(|_| "SSH 隧道状态锁异常")?;
        match &*slot {
            TunnelSlot::Empty => *slot = TunnelSlot::Starting,
            _ => return Err("SSH 隧道正在启动或已经运行".into()),
        }
    }
    let result = async {
        let ssh = system_ssh_path()?;
        for attempt in 0..3 {
            let port = allocate_loopback_port()?;
            let plan = build_launch_plan(&config, ssh.clone(), port)?;
            let mut process = spawn(&plan)?;
            match wait_ready(&mut process).await {
                Ok(()) => return Ok(process),
                Err(status)
                    if status.error_code.as_deref() == Some("local_port_busy") && attempt < 2 =>
                {
                    continue;
                }
                Err(status) => {
                    return Err(status.error.unwrap_or_else(|| "SSH 隧道启动失败".into()));
                }
            }
        }
        Err("本地隧道端口连续被占用".into())
    }
    .await;
    let mut slot = state.0.lock().map_err(|_| "SSH 隧道状态锁异常")?;
    match result {
        Ok(process) => {
            let status = SshTunnelStatus {
                running: true,
                pid: Some(process.child.id()),
                endpoint: Some(process.endpoint.clone()),
                local_port: Some(process.local_port),
                started_at: Some(process.started_at),
                exit_code: None,
                phase: TunnelPhase::Ready,
                error_code: None,
                error: None,
            };
            *slot = TunnelSlot::Running(process);
            Ok(status)
        }
        Err(error) => {
            *slot = TunnelSlot::Empty;
            Err(error)
        }
    }
}

#[tauri::command]
pub fn ssh_tunnel_status(
    state: tauri::State<'_, SshTunnelState>,
) -> Result<SshTunnelStatus, String> {
    let mut slot = state.0.lock().map_err(|_| "SSH 隧道状态锁异常")?;
    match &mut *slot {
        TunnelSlot::Empty => Ok(SshTunnelStatus {
            running: false,
            pid: None,
            endpoint: None,
            local_port: None,
            started_at: None,
            exit_code: None,
            phase: TunnelPhase::Stopped,
            error_code: None,
            error: None,
        }),
        TunnelSlot::Starting => Ok(SshTunnelStatus {
            running: false,
            pid: None,
            endpoint: None,
            local_port: None,
            started_at: None,
            exit_code: None,
            phase: TunnelPhase::Starting,
            error_code: None,
            error: None,
        }),
        TunnelSlot::Running(p) => match p
            .child
            .try_wait()
            .map_err(|e| format!("读取 SSH 状态失败：{e}"))?
        {
            None => Ok(SshTunnelStatus {
                running: true,
                pid: Some(p.child.id()),
                endpoint: Some(p.endpoint.clone()),
                local_port: Some(p.local_port),
                started_at: Some(p.started_at),
                exit_code: None,
                phase: TunnelPhase::Ready,
                error_code: None,
                error: None,
            }),
            Some(exit) => {
                let status = failed(&tail(&p.stderr), exit.code());
                *slot = TunnelSlot::Empty;
                Ok(status)
            }
        },
    }
}

#[tauri::command]
pub fn ssh_tunnel_stop(state: tauri::State<'_, SshTunnelState>) -> Result<SshTunnelStatus, String> {
    let mut slot = state.0.lock().map_err(|_| "SSH 隧道状态锁异常")?;
    let TunnelSlot::Running(mut p) = std::mem::take(&mut *slot) else {
        return Ok(SshTunnelStatus {
            running: false,
            pid: None,
            endpoint: None,
            local_port: None,
            started_at: None,
            exit_code: None,
            phase: TunnelPhase::Stopped,
            error_code: None,
            error: None,
        });
    };
    p.child
        .kill()
        .map_err(|e| format!("停止 SSH 隧道失败：{e}"))?;
    let exit = p
        .child
        .wait()
        .map_err(|e| format!("等待 SSH 隧道退出失败：{e}"))?;
    Ok(SshTunnelStatus {
        running: false,
        pid: Some(p.child.id()),
        endpoint: None,
        local_port: Some(p.local_port),
        started_at: Some(p.started_at),
        exit_code: exit.code(),
        phase: TunnelPhase::Stopped,
        error_code: None,
        error: None,
    })
}

use std::io::Read;
