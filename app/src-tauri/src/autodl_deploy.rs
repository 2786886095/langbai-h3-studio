//! Reproducible AutoDL deployment contract. The first release only prepares a
//! Studio-owned staging area; model download/start are added as journaled stages.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::{Read, Write},
    net::IpAddr,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{model_bundle, ssh_tunnel::SshTunnelConfig};

pub const TARGET_ROOT: &str = "/workspace/LangbaiH3Studio";
pub const REMOTE_COMMAND: &str = "sh -s -- langbai-h3-deploy-v1";
pub const STATUS_REMOTE_COMMAND: &str = "sh -s -- langbai-h3-status-v1";
const MAX_PROTOCOL_BYTES: usize = 256 * 1024;

pub const DEPLOY_SCRIPT: &str = r#"set -eu
umask 077
ROOT=/workspace/LangbaiH3Studio
STATE="$ROOT/state/deployments"
case "${CONFIG_B64:-}" in (*[!A-Za-z0-9+/=]*|'') exit 64;; esac
case "${DEPLOYMENT_ID:-}" in (h3-[a-f0-9]*) ;; (*) exit 64;; esac
CONFIG="$(printf '%s' "$CONFIG_B64" | base64 -d)"
ID="$DEPLOYMENT_ID"
DIR="$STATE/$ID"
mkdir -p "$DIR/logs" "$ROOT/staging" "$ROOT/models"
printf '%s' "$CONFIG" > "$DIR/manifest.json.tmp"
mv "$DIR/manifest.json.tmp" "$DIR/manifest.json"
emit() {
  encoded="$(printf '%s' "$3" | base64 | tr -d '\n')"
  printf 'event\t%s\t%s\t%s\n' "$1" "$2" "$encoded" | tee -a "$DIR/journal.tsv"
}
emit 1 preflight '部署配置已写入隔离目录'
emit 2 completed '预检阶段完成'
"#;

pub const STATUS_SCRIPT: &str = r#"set -eu
ROOT=/workspace/LangbaiH3Studio
case "${DEPLOYMENT_ID:-}" in (h3-[a-f0-9]*) ;; (*) exit 64;; esac
JOURNAL="$ROOT/state/deployments/$DEPLOYMENT_ID/journal.tsv"
[ -f "$JOURNAL" ] || exit 66
cat "$JOURNAL"
"#;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RemoteTarget {
    StudioManaged,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum H3Variant {
    Fl2va,
    Ref2va,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccelerationChoice {
    Native,
    KjH3SageAttention,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RemoteModelStrategy {
    DownloadMissing,
    ReuseThenDownloadMissing,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoDlDeployPreflightInput {
    pub connection: SshTunnelConfig,
    pub target: RemoteTarget,
    pub variants: Vec<H3Variant>,
    pub acceleration: AccelerationChoice,
    pub model_strategy: RemoteModelStrategy,
    pub remote_comfy_port: u16,
    pub available_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDownloadItem {
    pub relative_path: String,
    pub size: u64,
    pub sha256: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AutoDlDeployPlan {
    pub deployment_id: String,
    pub target_path: String,
    pub remote_comfy_port: u16,
    pub required_bytes: u64,
    pub available_bytes: u64,
    pub download_files: Vec<RemoteDownloadItem>,
    pub rollback_supported: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeployLaunchPlan {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub stdin: Vec<u8>,
    pub script_sha256: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeployStage {
    Preflight,
    Locking,
    RuntimeStaging,
    H3Patch,
    NodeInstall,
    ModelReuse,
    ModelDownload,
    ModelVerify,
    ConfigWrite,
    ComfyStart,
    HealthCheck,
    Cleanup,
    Completed,
    Failed,
    RollingBack,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AutoDlDeployProgress {
    pub sequence: u64,
    pub stage: DeployStage,
    pub message: String,
}

fn selected_files(variants: &[H3Variant]) -> Result<Vec<RemoteDownloadItem>, String> {
    if variants.is_empty() {
        return Err(
            "\u{81f3}\u{5c11}\u{9009}\u{62e9}\u{4e00}\u{4e2a} H3 \u{6a21}\u{578b}\u{53d8}\u{4f53}"
                .into(),
        );
    }
    let wanted: HashSet<&str> = variants
        .iter()
        .map(|v| match v {
            H3Variant::Fl2va => "fl2va",
            H3Variant::Ref2va => "ref2va",
        })
        .collect();
    let mut unique = BTreeMap::new();
    for bundle in model_bundle::builtins()? {
        if !wanted.contains(bundle.variant.as_str()) {
            continue;
        }
        for file in &bundle.files {
            let item = RemoteDownloadItem {
                relative_path: file.relative_path.clone(),
                size: file.size,
                sha256: file.sha256.clone(),
                url: bundle.download_url(file),
            };
            if let Some(old) = unique.insert(item.relative_path.clone(), item.clone()) {
                if old.sha256 != item.sha256 || old.size != item.size {
                    return Err("\u{5171}\u{4eab}\u{6a21}\u{578b}\u{6587}\u{4ef6}\u{6e05}\u{5355}\u{51b2}\u{7a81}".into());
                }
            }
        }
    }
    Ok(unique.into_values().collect())
}

pub fn build_preflight(input: &AutoDlDeployPreflightInput) -> Result<AutoDlDeployPlan, String> {
    if input.remote_comfy_port == 0 || input.remote_comfy_port != input.connection.remote_comfy_port
    {
        return Err("\u{8fdc}\u{7aef} ComfyUI \u{7aef}\u{53e3}\u{4e0d}\u{4e00}\u{81f4}".into());
    }
    let files = selected_files(&input.variants)?;
    let required_bytes = files
        .iter()
        .try_fold(0u64, |sum, f| sum.checked_add(f.size))
        .ok_or_else(|| {
            "\u{6a21}\u{578b}\u{4f53}\u{79ef}\u{8ba1}\u{7b97}\u{6ea2}\u{51fa}".to_string()
        })?;
    let reserve = 12 * 1024 * 1024 * 1024u64;
    if input.available_bytes < required_bytes.saturating_add(reserve) {
        return Err(
            "AutoDL \u{53ef}\u{7528}\u{78c1}\u{76d8}\u{7a7a}\u{95f4}\u{4e0d}\u{8db3}".into(),
        );
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "\u{7cfb}\u{7edf}\u{65f6}\u{95f4}\u{5f02}\u{5e38}")?
        .as_nanos();
    let mut h = Sha256::new();
    h.update(now.to_le_bytes());
    h.update(input.connection.host.as_bytes());
    Ok(AutoDlDeployPlan {
        deployment_id: format!("h3-{}", &format!("{:x}", h.finalize())[..20]),
        target_path: TARGET_ROOT.into(), remote_comfy_port: input.remote_comfy_port,
        required_bytes, available_bytes: input.available_bytes, download_files: files,
        rollback_supported: true,
        warnings: vec!["\u{5f53}\u{524d}\u{4ec5}\u{521b}\u{5efa}\u{9694}\u{79bb}\u{6682}\u{5b58}\u{548c}\u{90e8}\u{7f72}\u{8bb0}\u{5f55}\u{ff0c}\u{5c1a}\u{672a}\u{4e0b}\u{8f7d}\u{6a21}\u{578b}\u{6216}\u{542f}\u{52a8} ComfyUI".into()],
    })
}

fn validate_deployment_id(value: &str) -> Result<(), String> {
    if value.len() != 23
        || !value.starts_with("h3-")
        || !value[3..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("\u{90e8}\u{7f72}\u{7f16}\u{53f7}\u{65e0}\u{6548}".into());
    }
    Ok(())
}

#[tauri::command]
pub fn autodl_deploy_preflight(
    input: AutoDlDeployPreflightInput,
) -> Result<AutoDlDeployPlan, String> {
    build_preflight(&input)
}

fn canonical_file(path: &Path, label: &str) -> Result<PathBuf, String> {
    let p = fs::canonicalize(path).map_err(|_| format!("{label}\u{4e0d}\u{5b58}\u{5728}"))?;
    if !fs::metadata(&p)
        .map_err(|_| format!("{label}\u{65e0}\u{6cd5}\u{8bfb}\u{53d6}"))?
        .is_file()
    {
        return Err(format!("{label}\u{5fc5}\u{987b}\u{662f}\u{6587}\u{4ef6}"));
    }
    Ok(p)
}

fn valid_atom(value: &str, ipv6: bool) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && !value.chars().any(|c| c.is_control() || c.is_whitespace())
        && !value.contains(['@', '/', '\\'])
        && (ipv6 || !value.contains(':'))
        && (!value.contains(':') || value.parse::<IpAddr>().is_ok())
}

pub fn build_deploy_launch_plan(
    config: &SshTunnelConfig,
    program: PathBuf,
    plan: &AutoDlDeployPlan,
) -> Result<DeployLaunchPlan, String> {
    if !valid_atom(&config.host, true) || !valid_atom(&config.user, false) || config.port == 0 {
        return Err("SSH \u{8fde}\u{63a5}\u{53c2}\u{6570}\u{65e0}\u{6548}".into());
    }
    let key = canonical_file(&config.identity_file, "SSH \u{79c1}\u{94a5}")?;
    let hosts = canonical_file(&config.known_hosts_file, "known_hosts")?;
    if fs::metadata(&hosts).map_err(|e| e.to_string())?.len() == 0 {
        return Err("known_hosts \u{4e0d}\u{80fd}\u{4e3a}\u{7a7a}".into());
    }
    let destination = if config.host.contains(':') {
        format!("{}@[{}]", config.user, config.host)
    } else {
        format!("{}@{}", config.user, config.host)
    };
    let args = vec![
        "-F",
        "NUL",
        "-T",
        "-p",
        &config.port.to_string(),
        "-i",
        &key.to_string_lossy(),
        "-o",
        "BatchMode=yes",
        "-o",
        "PasswordAuthentication=no",
        "-o",
        "KbdInteractiveAuthentication=no",
        "-o",
        "IdentitiesOnly=yes",
        "-o",
        "ForwardAgent=no",
        "-o",
        "ClearAllForwardings=yes",
        "-o",
        "StrictHostKeyChecking=yes",
        "-o",
        &format!("UserKnownHostsFile={}", hosts.to_string_lossy()),
        "-o",
        "GlobalKnownHostsFile=NUL",
        &destination,
        REMOTE_COMMAND,
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    let json = serde_json::to_vec(plan)
        .map_err(|_| "\u{90e8}\u{7f72}\u{8ba1}\u{5212}\u{5e8f}\u{5217}\u{5316}\u{5931}\u{8d25}")?;
    let stdin = format!(
        "CONFIG_B64='{}'\nDEPLOYMENT_ID='{}'\n{}",
        BASE64.encode(json),
        plan.deployment_id,
        DEPLOY_SCRIPT
    )
    .into_bytes();
    Ok(DeployLaunchPlan {
        program,
        args,
        stdin,
        script_sha256: format!("{:x}", Sha256::digest(DEPLOY_SCRIPT.as_bytes())),
    })
}

pub fn build_status_launch_plan(
    config: &SshTunnelConfig,
    program: PathBuf,
    deployment_id: &str,
) -> Result<DeployLaunchPlan, String> {
    validate_deployment_id(deployment_id)?;
    if !valid_atom(&config.host, true) || !valid_atom(&config.user, false) || config.port == 0 {
        return Err("SSH \u{8fde}\u{63a5}\u{53c2}\u{6570}\u{65e0}\u{6548}".into());
    }
    let key = canonical_file(&config.identity_file, "SSH \u{79c1}\u{94a5}")?;
    let hosts = canonical_file(&config.known_hosts_file, "known_hosts")?;
    if fs::metadata(&hosts).map_err(|e| e.to_string())?.len() == 0 {
        return Err("known_hosts \u{4e0d}\u{80fd}\u{4e3a}\u{7a7a}".into());
    }
    let destination = if config.host.contains(':') {
        format!("{}@[{}]", config.user, config.host)
    } else {
        format!("{}@{}", config.user, config.host)
    };
    let args = vec![
        "-F".to_string(),
        "NUL".into(),
        "-T".into(),
        "-p".into(),
        config.port.to_string(),
        "-i".into(),
        key.to_string_lossy().into_owned(),
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
        "StrictHostKeyChecking=yes".into(),
        "-o".into(),
        format!("UserKnownHostsFile={}", hosts.to_string_lossy()),
        "-o".into(),
        "GlobalKnownHostsFile=NUL".into(),
        destination,
        STATUS_REMOTE_COMMAND.into(),
    ];
    Ok(DeployLaunchPlan {
        program,
        args,
        stdin: format!("DEPLOYMENT_ID='{}'\n{}", deployment_id, STATUS_SCRIPT).into_bytes(),
        script_sha256: format!("{:x}", Sha256::digest(STATUS_SCRIPT.as_bytes())),
    })
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AutoDlDeployPrepareResult {
    pub plan: AutoDlDeployPlan,
    pub progress: Vec<AutoDlDeployProgress>,
    pub script_sha256: String,
}

fn read_limited(mut reader: impl Read + Send + 'static) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut output = Vec::new();
        let mut buffer = [0u8; 4096];
        while output.len() <= MAX_PROTOCOL_BYTES {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => output.extend_from_slice(&buffer[..n]),
            }
        }
        let _ = tx.send(output);
    });
    rx
}

fn run_prepare(launch: DeployLaunchPlan) -> Result<Vec<AutoDlDeployProgress>, String> {
    let mut command = Command::new(&launch.program);
    command
        .args(&launch.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|_| "\u{542f}\u{52a8} Windows OpenSSH \u{5931}\u{8d25}".to_string())?;
    child
        .stdin
        .take()
        .ok_or("\u{65e0}\u{6cd5}\u{5199}\u{5165} SSH stdin")?
        .write_all(&launch.stdin)
        .map_err(
            |_| "\u{53d1}\u{9001}\u{56fa}\u{5b9a}\u{90e8}\u{7f72}\u{811a}\u{672c}\u{5931}\u{8d25}",
        )?;
    let stdout = read_limited(
        child
            .stdout
            .take()
            .ok_or("\u{65e0}\u{6cd5}\u{8bfb}\u{53d6} SSH stdout")?,
    );
    let stderr = read_limited(
        child
            .stderr
            .take()
            .ok_or("\u{65e0}\u{6cd5}\u{8bfb}\u{53d6} SSH stderr")?,
    );
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|_| "\u{8bfb}\u{53d6} SSH \u{72b6}\u{6001}\u{5931}\u{8d25}")?
        {
            break status;
        }
        if started.elapsed() >= Duration::from_secs(45) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("AutoDL \u{8fdc}\u{7aef}\u{51c6}\u{5907}\u{8d85}\u{65f6}".into());
        }
        thread::sleep(Duration::from_millis(50));
    };
    let out = stdout
        .recv_timeout(Duration::from_secs(2))
        .unwrap_or_default();
    let err = stderr
        .recv_timeout(Duration::from_secs(2))
        .unwrap_or_default();
    if !status.success() {
        let detail = String::from_utf8_lossy(&err);
        let summary = detail.lines().last().unwrap_or_default().trim();
        return Err(if summary.is_empty() {
            "AutoDL \u{8fdc}\u{7aef}\u{51c6}\u{5907}\u{5931}\u{8d25}".into()
        } else {
            format!(
                "AutoDL \u{8fdc}\u{7aef}\u{51c6}\u{5907}\u{5931}\u{8d25}\u{ff1a}{}",
                summary.chars().take(240).collect::<String>()
            )
        });
    }
    let progress = parse_progress(&out)?;
    if !matches!(
        progress.last().map(|item| &item.stage),
        Some(DeployStage::Completed)
    ) {
        return Err("AutoDL \u{8fdc}\u{7aef}\u{51c6}\u{5907}\u{672a}\u{8fd4}\u{56de}\u{5b8c}\u{6210}\u{4e8b}\u{4ef6}".into());
    }
    Ok(progress)
}

#[tauri::command]
pub async fn autodl_deploy_prepare(
    input: AutoDlDeployPreflightInput,
    deployment_id: String,
) -> Result<AutoDlDeployPrepareResult, String> {
    validate_deployment_id(&deployment_id)?;
    let mut plan = build_preflight(&input)?;
    plan.deployment_id = deployment_id;
    let launch = build_deploy_launch_plan(
        &input.connection,
        crate::ssh_tunnel::system_ssh_path()?,
        &plan,
    )?;
    let script_sha256 = launch.script_sha256.clone();
    let progress = tauri::async_runtime::spawn_blocking(move || run_prepare(launch))
        .await
        .map_err(|_| {
            "AutoDL \u{8fdc}\u{7aef}\u{51c6}\u{5907}\u{4efb}\u{52a1}\u{5f02}\u{5e38}".to_string()
        })??;
    Ok(AutoDlDeployPrepareResult {
        plan,
        progress,
        script_sha256,
    })
}

#[tauri::command]
pub async fn autodl_deploy_status(
    config: SshTunnelConfig,
    deployment_id: String,
) -> Result<Vec<AutoDlDeployProgress>, String> {
    let launch = build_status_launch_plan(
        &config,
        crate::ssh_tunnel::system_ssh_path()?,
        &deployment_id,
    )?;
    tauri::async_runtime::spawn_blocking(move || run_prepare(launch))
        .await
        .map_err(|_| {
            "AutoDL \u{8fdc}\u{7aef}\u{72b6}\u{6001}\u{4efb}\u{52a1}\u{5f02}\u{5e38}".to_string()
        })?
}

fn stage(value: &str) -> Option<DeployStage> {
    Some(match value {
        "preflight" => DeployStage::Preflight,
        "locking" => DeployStage::Locking,
        "runtime_staging" => DeployStage::RuntimeStaging,
        "h3_patch" => DeployStage::H3Patch,
        "node_install" => DeployStage::NodeInstall,
        "model_reuse" => DeployStage::ModelReuse,
        "model_download" => DeployStage::ModelDownload,
        "model_verify" => DeployStage::ModelVerify,
        "config_write" => DeployStage::ConfigWrite,
        "comfy_start" => DeployStage::ComfyStart,
        "health_check" => DeployStage::HealthCheck,
        "cleanup" => DeployStage::Cleanup,
        "completed" => DeployStage::Completed,
        "failed" => DeployStage::Failed,
        "rolling_back" => DeployStage::RollingBack,
        _ => return None,
    })
}

pub fn parse_progress(bytes: &[u8]) -> Result<Vec<AutoDlDeployProgress>, String> {
    if bytes.len() > MAX_PROTOCOL_BYTES {
        return Err("\u{8fdc}\u{7aef}\u{8fdb}\u{5ea6}\u{8f93}\u{51fa}\u{8fc7}\u{5927}".into());
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "\u{8fdc}\u{7aef}\u{8fdb}\u{5ea6}\u{7f16}\u{7801}\u{65e0}\u{6548}")?;
    let mut out = Vec::new();
    let mut previous = 0;
    for line in text.lines() {
        let parts: Vec<_> = line.split('\t').collect();
        if parts.len() != 4 || parts[0] != "event" {
            return Err("\u{8fdc}\u{7aef}\u{8fdb}\u{5ea6}\u{534f}\u{8bae}\u{65e0}\u{6548}".into());
        }
        let sequence = parts[1]
            .parse::<u64>()
            .map_err(|_| "sequence \u{65e0}\u{6548}")?;
        if sequence <= previous {
            return Err("sequence \u{5fc5}\u{987b}\u{4e25}\u{683c}\u{9012}\u{589e}".into());
        }
        previous = sequence;
        let stage = stage(parts[2]).ok_or("\u{672a}\u{77e5}\u{90e8}\u{7f72}\u{9636}\u{6bb5}")?;
        let raw = BASE64
            .decode(parts[3])
            .map_err(|_| "\u{8fdb}\u{5ea6}\u{6d88}\u{606f} Base64 \u{65e0}\u{6548}")?;
        let message = String::from_utf8(raw)
            .map_err(|_| "\u{8fdb}\u{5ea6}\u{6d88}\u{606f}\u{4e0d}\u{662f} UTF-8")?;
        out.push(AutoDlDeployProgress {
            sequence,
            stage,
            message,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    fn cfg(root: &Path) -> SshTunnelConfig {
        let k = root.join("id");
        let h = root.join("hosts");
        fs::write(&k, "k").unwrap();
        fs::write(&h, "host key").unwrap();
        SshTunnelConfig {
            host: "gpu.example".into(),
            user: "root".into(),
            port: 22,
            identity_file: k,
            known_hosts_file: h,
            remote_comfy_port: 8188,
        }
    }
    fn input(root: &Path, variants: Vec<H3Variant>) -> AutoDlDeployPreflightInput {
        AutoDlDeployPreflightInput {
            connection: cfg(root),
            target: RemoteTarget::StudioManaged,
            variants,
            acceleration: AccelerationChoice::Native,
            model_strategy: RemoteModelStrategy::DownloadMissing,
            remote_comfy_port: 8188,
            available_bytes: 200_000_000_000,
        }
    }
    #[test]
    fn shared_files_are_deduplicated() {
        let t = tempdir().unwrap();
        let one = build_preflight(&input(t.path(), vec![H3Variant::Fl2va])).unwrap();
        let both =
            build_preflight(&input(t.path(), vec![H3Variant::Fl2va, H3Variant::Ref2va])).unwrap();
        assert_eq!(one.download_files.len(), 4);
        assert_eq!(both.download_files.len(), 5);
        assert!(both.required_bytes < one.required_bytes * 2);
    }
    #[test]
    fn command_is_fixed_and_config_only_in_stdin() {
        let t = tempdir().unwrap();
        let i = input(t.path(), vec![H3Variant::Fl2va]);
        let p = build_preflight(&i).unwrap();
        let l = build_deploy_launch_plan(&i.connection, "ssh.exe".into(), &p).unwrap();
        assert_eq!(l.args.last().unwrap(), REMOTE_COMMAND);
        assert!(!l.args.join(" ").contains(&p.deployment_id));
        assert!(String::from_utf8(l.stdin).unwrap().contains("CONFIG_B64='"));
    }
    #[test]
    fn rejects_injection_and_bad_protocol() {
        let t = tempdir().unwrap();
        let mut i = input(t.path(), vec![H3Variant::Fl2va]);
        i.connection.host = "x;touch /tmp/x".into();
        let p = build_preflight(&i).unwrap();
        assert!(build_deploy_launch_plan(&i.connection, "ssh.exe".into(), &p).is_err());
        assert!(parse_progress(b"event\t1\tunknown\tb2s=\n").is_err());
        assert!(parse_progress(b"event\t1\tpreflight\tb2s=\nevent\t1\tcompleted\tb2s=\n").is_err());
    }
    #[test]
    fn parses_progress_lines() {
        let s = format!(
            "event\t1\tpreflight\t{}\nevent\t2\tcompleted\t{}\n",
            BASE64.encode("ok"),
            BASE64.encode("done")
        );
        let p = parse_progress(s.as_bytes()).unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[1].stage, DeployStage::Completed);
    }

    #[test]
    fn status_query_uses_fixed_command_and_id_only_in_stdin() {
        let t = tempdir().unwrap();
        let c = cfg(t.path());
        let launch =
            build_status_launch_plan(&c, "ssh.exe".into(), "h3-0123456789abcdefabcd").unwrap();
        assert_eq!(launch.args.last().unwrap(), STATUS_REMOTE_COMMAND);
        assert!(!launch.args.join(" ").contains("h3-0123456789abcdefabcd"));
        assert!(
            String::from_utf8(launch.stdin)
                .unwrap()
                .contains("DEPLOYMENT_ID='h3-0123456789abcdefabcd'")
        );
        assert!(build_status_launch_plan(&c, "ssh.exe".into(), "../../etc").is_err());
    }
}
