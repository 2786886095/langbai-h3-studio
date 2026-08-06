//! Read-only AutoDL / Linux environment probe for a remote MiniMax-H3 ComfyUI host.
//!
//! The remote shell fragment is deliberately a constant.  It receives no user data,
//! performs no writes, and emits a small line protocol which is decoded locally.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

use crate::ssh_tunnel::{self, SshTunnelConfig};

const PROBE_TIMEOUT: Duration = Duration::from_secs(25);
const OUTPUT_LIMIT: usize = 256 * 1024;

const H3_SOURCE_FILES: &[&str] = &[
    "comfy_extras/nodes_minimax_h3.py",
    "comfy/ldm/minimax/model.py",
    "comfy/ldm/minimax/audio_vae.py",
    "comfy/ldm/minimax/vae.py",
    "comfy/text_encoders/minimax.py",
    "comfy/text_encoders/qwen3vl.py",
    "nodes.py",
];

const FL2VA_MODEL_FILES: &[(&str, u64)] = &[
    (
        "diffusion_models/minimax_h3_fl2va_pruned_int8_convrot.safetensors",
        20_970_379_616,
    ),
    (
        "text_encoders/qwen3vl_32b_minimax_h3_nvfp4_awq.safetensors",
        15_687_142_551,
    ),
    ("vae/minimax_h3_audio_vae_fp32.safetensors", 605_254_808),
    ("vae/minimax_h3_video_vae_fp16.safetensors", 5_207_808_496),
];

const REF2VA_MODEL_FILES: &[(&str, u64)] = &[
    (
        "diffusion_models/minimax_h3_ref2va_pruned_int8_convrot.safetensors",
        20_970_379_616,
    ),
    (
        "text_encoders/qwen3vl_32b_minimax_h3_nvfp4_awq.safetensors",
        15_687_142_551,
    ),
    ("vae/minimax_h3_audio_vae_fp32.safetensors", 605_254_808),
    ("vae/minimax_h3_video_vae_fp16.safetensors", 5_207_808_496),
];

/// A fixed, POSIX-shell read-only inventory. Do not add interpolated user values here.
///
/// The protocol is `<kind>\\t<base64 field>...`; base64 keeps host supplied text out of
/// the line grammar and permits OS/GPU names containing punctuation or newlines.
const REMOTE_PROBE_COMMAND: &str = r#"sh -c '
enc() { printf "%s" "$1" | base64 | tr -d "\n"; }
rec() { printf "%s" "$1"; shift; for v in "$@"; do printf "\t"; enc "$v"; done; printf "\n"; }
val() { if [ -e "$1" ]; then printf "1"; else printf "0"; fi; }
size() { if [ -f "$1" ]; then wc -c < "$1" | tr -d " "; else printf "0"; fi; }
os="$( ( . /etc/os-release 2>/dev/null; printf "%s" "${PRETTY_NAME:-}" ) 2>/dev/null )"
[ -n "$os" ] || os="$(uname -srm 2>/dev/null || printf unknown)"
rec os "$os"
rec gpu "$(nvidia-smi --query-gpu=name,memory.total,driver_version --format=csv,noheader,nounits 2>/dev/null || true)"
rec ram_mib "$(set -- $(grep MemTotal /proc/meminfo 2>/dev/null || true); printf "%s" "${2:-}")"
py=""
if command -v python3 >/dev/null 2>&1; then py="$(python3 --version 2>&1)"; elif command -v python >/dev/null 2>&1; then py="$(python --version 2>&1)"; fi
rec python "$py"
rec disks "$(df -B1 -P / /root /workspace /root/autodl-tmp 2>/dev/null | tail -n +2 || true)"
seen=""
for d in /root/ComfyUI /workspace/ComfyUI /root/autodl-tmp/ComfyUI "$HOME/ComfyUI" "$HOME/ComfyUI_windows_portable/ComfyUI"; do
  [ -d "$d" ] || continue
  case "|$seen|" in *"|$d|"*) continue;; esac
  seen="${seen}${seen:+|}$d"
  rec comfy "$d"
  for f in comfy_extras/nodes_minimax_h3.py comfy/ldm/minimax/model.py comfy/ldm/minimax/audio_vae.py comfy/ldm/minimax/vae.py comfy/text_encoders/minimax.py comfy/text_encoders/qwen3vl.py nodes.py; do
    rec source "$d" "$f" "$(val "$d/$f")"
  done
  for item in \
    "fl2va|diffusion_models/minimax_h3_fl2va_pruned_int8_convrot.safetensors|20970379616" \
    "fl2va|text_encoders/qwen3vl_32b_minimax_h3_nvfp4_awq.safetensors|15687142551" \
    "fl2va|vae/minimax_h3_audio_vae_fp32.safetensors|605254808" \
    "fl2va|vae/minimax_h3_video_vae_fp16.safetensors|5207808496" \
    "ref2va|diffusion_models/minimax_h3_ref2va_pruned_int8_convrot.safetensors|20970379616" \
    "ref2va|text_encoders/qwen3vl_32b_minimax_h3_nvfp4_awq.safetensors|15687142551" \
    "ref2va|vae/minimax_h3_audio_vae_fp32.safetensors|605254808" \
    "ref2va|vae/minimax_h3_video_vae_fp16.safetensors|5207808496"; do
      variant="${item%%|*}"; rest="${item#*|}"; rel="${rest%%|*}"; expected="${rest##*|}"; path="$d/models/$rel"
      rec model "$d" "$variant" "$rel" "$expected" "$(val "$path")" "$(size "$path")"
  done
  kj=0
  if [ -f "$d/custom_nodes/ComfyUI-KJNodes/nodes/ltxv_nodes.py" ] && grep -Fq "MiniMaxH3MemoryEfficientSageAttentionPatch" "$d/custom_nodes/ComfyUI-KJNodes/nodes/ltxv_nodes.py" 2>/dev/null; then kj=1; fi
  rec kj_sage "$d" "$kj"
done
rec done "ok"
'"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshProbeLaunchPlan {
    pub program: PathBuf,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AutoDlRemoteProbe {
    pub os: String,
    pub gpus: Vec<RemoteGpu>,
    pub total_vram_mib: u64,
    pub ram_total_mib: Option<u64>,
    pub python: Option<String>,
    pub disks: Vec<RemoteDisk>,
    pub comfyui_candidates: Vec<RemoteComfyUiCandidate>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteGpu {
    pub name: String,
    pub vram_mib: Option<u64>,
    pub driver_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDisk {
    pub filesystem: String,
    pub total_bytes: Option<u64>,
    pub used_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
    pub mount_point: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteComfyUiCandidate {
    pub path: String,
    pub h3_source_files: Vec<RemoteFileStatus>,
    pub model_variants: Vec<RemoteModelVariant>,
    pub kj_h3_sage_attention_present: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFileStatus {
    pub relative_path: String,
    pub present: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteModelVariant {
    pub id: String,
    pub files: Vec<RemoteModelFileStatus>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteModelFileStatus {
    pub relative_path: String,
    pub expected_size_bytes: u64,
    pub present: bool,
    pub size_bytes: u64,
}

fn validate_atom(value: &str, label: &str, allow_colon: bool) -> Result<(), String> {
    let invalid = value.is_empty()
        || value.starts_with('-')
        || value.chars().any(|c| c.is_control() || c.is_whitespace())
        || value.contains(['@', '/', '\\'])
        || (!allow_colon && value.contains(':'));
    if invalid {
        Err(format!("{label}\u{683c}\u{5f0f}\u{65e0}\u{6548}"))
    } else {
        Ok(())
    }
}

fn canonical_regular_file(path: &Path, label: &str) -> Result<PathBuf, String> {
    let path = fs::canonicalize(path)
        .map_err(|_| format!("{label}\u{6587}\u{4ef6}\u{4e0d}\u{53ef}\u{8bfb}\u{53d6}"))?;
    if !path.is_file() {
        return Err(format!(
            "{label}\u{5fc5}\u{987b}\u{662f}\u{666e}\u{901a}\u{6587}\u{4ef6}"
        ));
    }
    Ok(path)
}

/// Creates a strict Windows OpenSSH invocation for exactly [`REMOTE_PROBE_COMMAND`].
pub fn build_probe_launch_plan(
    config: &SshTunnelConfig,
    program: PathBuf,
) -> Result<SshProbeLaunchPlan, String> {
    validate_atom(&config.host, "SSH\u{4e3b}\u{673a}", true)?;
    validate_atom(&config.user, "SSH\u{7528}\u{6237}\u{540d}", false)?;
    if config.port == 0 {
        return Err("SSH\u{7aef}\u{53e3}\u{65e0}\u{6548}".into());
    }
    if config.host.contains(':') && config.host.parse::<std::net::IpAddr>().is_err() {
        return Err("SSH\u{4e3b}\u{673a}\u{683c}\u{5f0f}\u{65e0}\u{6548}".into());
    }
    let identity = canonical_regular_file(&config.identity_file, "SSH\u{79c1}\u{94a5}")?;
    let known_hosts = canonical_regular_file(&config.known_hosts_file, "known_hosts")?;
    if fs::metadata(&known_hosts)
        .map_err(|_| "known_hosts\u{6587}\u{4ef6}\u{4e0d}\u{53ef}\u{8bfb}\u{53d6}")?
        .len()
        == 0
    {
        return Err("known_hosts\u{4e0d}\u{80fd}\u{4e3a}\u{7a7a}".into());
    }
    let destination = if config.host.contains(':') {
        format!("{}@[{}]", config.user, config.host)
    } else {
        format!("{}@{}", config.user, config.host)
    };
    Ok(SshProbeLaunchPlan {
        program,
        args: vec![
            "-F".into(),
            "NUL".into(),
            "-T".into(),
            "-p".into(),
            config.port.to_string(),
            "-i".into(),
            identity.to_string_lossy().into_owned(),
            "-o".into(),
            "BatchMode=yes".into(),
            "-o".into(),
            "IdentitiesOnly=yes".into(),
            "-o".into(),
            "PreferredAuthentications=publickey".into(),
            "-o".into(),
            "PasswordAuthentication=no".into(),
            "-o".into(),
            "KbdInteractiveAuthentication=no".into(),
            "-o".into(),
            "StrictHostKeyChecking=yes".into(),
            "-o".into(),
            format!("UserKnownHostsFile={}", known_hosts.to_string_lossy()),
            "-o".into(),
            "GlobalKnownHostsFile=NUL".into(),
            "-o".into(),
            "ForwardAgent=no".into(),
            "-o".into(),
            "ClearAllForwardings=yes".into(),
            "-o".into(),
            "ConnectTimeout=15".into(),
            "-o".into(),
            "ConnectionAttempts=1".into(),
            "-o".into(),
            "RequestTTY=no".into(),
            "-o".into(),
            "PermitLocalCommand=no".into(),
            destination,
            REMOTE_PROBE_COMMAND.into(),
        ],
    })
}

fn read_limited(mut reader: impl Read + Send + 'static) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        while out.len() <= OUTPUT_LIMIT {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => out.extend_from_slice(&buf[..n]),
            }
        }
        if out.len() > OUTPUT_LIMIT {
            out.truncate(OUTPUT_LIMIT + 1);
        }
        let _ = tx.send(out);
    });
    rx
}

fn probe_error(stderr: &str, timed_out: bool) -> String {
    if timed_out {
        return "\u{8fdc}\u{7aef}\u{73af}\u{5883}\u{68c0}\u{67e5}\u{8d85}\u{65f6}\u{ff0c}\u{8bf7}\u{786e}\u{8ba4}AutoDL\u{5b9e}\u{4f8b}\u{6b63}\u{5728}\u{8fd0}\u{884c}\u{4e14}SSH\u{53ef}\u{8fde}\u{63a5}".into();
    }
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("host key verification failed")
        || lower.contains("remote host identification has changed")
    {
        "SSH\u{4e3b}\u{673a}\u{5bc6}\u{94a5}\u{6821}\u{9a8c}\u{5931}\u{8d25}\u{ff0c}\u{8bf7}\u{6838}\u{5bf9}AutoDL\u{5b9e}\u{4f8b}\u{6307}\u{7eb9}".into()
    } else if lower.contains("permission denied") {
        "SSH\u{516c}\u{94a5}\u{8ba4}\u{8bc1}\u{5931}\u{8d25}".into()
    } else if lower.contains("connection timed out") {
        "SSH\u{8fde}\u{63a5}\u{8d85}\u{65f6}".into()
    } else if lower.contains("connection refused") {
        "SSH\u{8fde}\u{63a5}\u{88ab}\u{62d2}\u{7edd}".into()
    } else if lower.contains("could not resolve hostname") {
        "SSH\u{4e3b}\u{673a}\u{540d}\u{89e3}\u{6790}\u{5931}\u{8d25}".into()
    } else {
        "\u{8fdc}\u{7aef}\u{73af}\u{5883}\u{68c0}\u{67e5}\u{5931}\u{8d25}\u{ff0c}\u{8bf7}\u{68c0}\u{67e5}SSH\u{8fde}\u{63a5}\u{4e0e}\u{5b9e}\u{4f8b}\u{72b6}\u{6001}".into()
    }
}

fn run_probe(plan: SshProbeLaunchPlan) -> Result<Vec<u8>, String> {
    let mut command = Command::new(&plan.program);
    command
        .args(&plan.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|_| "\u{542f}\u{52a8}Windows OpenSSH\u{5931}\u{8d25}".to_string())?;
    let stdout = read_limited(
        child
            .stdout
            .take()
            .ok_or("\u{8bfb}\u{53d6}SSH\u{8f93}\u{51fa}\u{5931}\u{8d25}")?,
    );
    let stderr = read_limited(
        child
            .stderr
            .take()
            .ok_or("\u{8bfb}\u{53d6}SSH\u{9519}\u{8bef}\u{8f93}\u{51fa}\u{5931}\u{8d25}")?,
    );
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child
            .try_wait()
            .map_err(|_| "\u{8bfb}\u{53d6}SSH\u{8fdb}\u{7a0b}\u{72b6}\u{6001}\u{5931}\u{8d25}")?
        {
            Some(status) => break status,
            None if started.elapsed() >= PROBE_TIMEOUT => {
                timed_out = true;
                let _ = child.kill();
                break child.wait().map_err(
                    |_| "\u{505c}\u{6b62}\u{8d85}\u{65f6}SSH\u{68c0}\u{67e5}\u{5931}\u{8d25}",
                )?;
            }
            None => std::thread::sleep(Duration::from_millis(100)),
        }
    };
    let output = stdout
        .recv_timeout(Duration::from_secs(2))
        .unwrap_or_default();
    let error = stderr
        .recv_timeout(Duration::from_secs(2))
        .unwrap_or_default();
    if !status.success() || timed_out {
        return Err(probe_error(&String::from_utf8_lossy(&error), timed_out));
    }
    if output.len() > OUTPUT_LIMIT {
        return Err("\u{8fdc}\u{7aef}\u{73af}\u{5883}\u{68c0}\u{67e5}\u{8f93}\u{51fa}\u{5f02}\u{5e38}\u{8fc7}\u{5927}".into());
    }
    Ok(output)
}

fn decode(value: &str) -> Result<String, String> {
    let bytes = BASE64
        .decode(value)
        .map_err(|_| "\u{8fdc}\u{7aef}\u{73af}\u{5883}\u{68c0}\u{67e5}\u{8fd4}\u{56de}\u{683c}\u{5f0f}\u{65e0}\u{6548}")?;
    String::from_utf8(bytes).map_err(|_| "杩滅鐜妫€鏌ヨ繑鍥炰簡鏃犳晥鏂囨湰".to_string())
}

fn flag(value: &str) -> Result<bool, String> {
    match decode(value)?.as_str() {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err("\u{8fdc}\u{7aef}\u{73af}\u{5883}\u{68c0}\u{67e5}\u{8fd4}\u{56de}\u{4e86}\u{65e0}\u{6548}\u{72b6}\u{6001}".into()),
    }
}

fn number(value: &str) -> Result<u64, String> {
    let value = decode(value)?;
    if value.is_empty() {
        return Ok(0);
    }
    value
        .parse()
        .map_err(|_| "\u{8fdc}\u{7aef}\u{73af}\u{5883}\u{68c0}\u{67e5}\u{8fd4}\u{56de}\u{4e86}\u{65e0}\u{6548}\u{6570}\u{503c}".into())
}

fn expected_model_files(variant: &str) -> Option<&'static [(&'static str, u64)]> {
    match variant {
        "fl2va" => Some(FL2VA_MODEL_FILES),
        "ref2va" => Some(REF2VA_MODEL_FILES),
        _ => None,
    }
}

/// Decodes the fixed remote line protocol. This function never executes a command.
pub fn parse_probe_output(bytes: &[u8]) -> Result<AutoDlRemoteProbe, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "\u{8fdc}\u{7aef}\u{73af}\u{5883}\u{68c0}\u{67e5}\u{8fd4}\u{56de}\u{4e86}\u{65e0}\u{6548}UTF-8")?;
    let mut os = None;
    let mut gpu_lines = String::new();
    let mut ram_total_mib = None;
    let mut python = None;
    let mut disks = Vec::new();
    let mut candidates = BTreeMap::<String, RemoteComfyUiCandidate>::new();
    let mut complete = false;

    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let fields: Vec<_> = line.split('\t').collect();
        match fields.first().copied() {
            Some("os") if fields.len() == 2 => os = Some(decode(fields[1])?),
            Some("gpu") if fields.len() == 2 => gpu_lines = decode(fields[1])?,
            Some("ram_mib") if fields.len() == 2 => {
                let n = number(fields[1])?;
                ram_total_mib = (n != 0).then_some(n);
            }
            Some("python") if fields.len() == 2 => {
                let value = decode(fields[1])?;
                python = (!value.is_empty()).then_some(value);
            }
            Some("disks") if fields.len() == 2 => disks = parse_disks(&decode(fields[1])?),
            Some("comfy") if fields.len() == 2 => {
                let path = decode(fields[1])?;
                if !path.starts_with('/') {
                    return Err("\u{8fdc}\u{7aef}ComfyUI\u{8def}\u{5f84}\u{683c}\u{5f0f}\u{65e0}\u{6548}".into());
                }
                candidates
                    .entry(path.clone())
                    .or_insert_with(|| RemoteComfyUiCandidate {
                        path,
                        h3_source_files: Vec::new(),
                        model_variants: Vec::new(),
                        kj_h3_sage_attention_present: false,
                    });
            }
            Some("source") if fields.len() == 4 => {
                let path = decode(fields[1])?;
                let relative_path = decode(fields[2])?;
                if !H3_SOURCE_FILES.contains(&relative_path.as_str()) {
                    return Err("\u{8fdc}\u{7aef}H3\u{6e90}\u{7801}\u{6761}\u{76ee}\u{65e0}\u{6548}".into());
                }
                let candidate = candidates
                    .get_mut(&path)
                    .ok_or("\u{8fdc}\u{7aef}H3\u{6e90}\u{7801}\u{6ca1}\u{6709}\u{5bf9}\u{5e94}ComfyUI\u{8def}\u{5f84}")?;
                candidate.h3_source_files.push(RemoteFileStatus {
                    relative_path,
                    present: flag(fields[3])?,
                });
            }
            Some("model") if fields.len() == 7 => {
                let path = decode(fields[1])?;
                let variant = decode(fields[2])?;
                let relative_path = decode(fields[3])?;
                let expected_size_bytes = number(fields[4])?;
                let present = flag(fields[5])?;
                let size_bytes = number(fields[6])?;
                if !expected_model_files(&variant).is_some_and(|files| {
                    files.contains(&(relative_path.as_str(), expected_size_bytes))
                }) {
                    return Err("\u{8fdc}\u{7aef}H3\u{6a21}\u{578b}\u{6761}\u{76ee}\u{65e0}\u{6548}".into());
                }
                let candidate = candidates
                    .get_mut(&path)
                    .ok_or("\u{8fdc}\u{7aef}H3\u{6a21}\u{578b}\u{6ca1}\u{6709}\u{5bf9}\u{5e94}ComfyUI\u{8def}\u{5f84}")?;
                let model_variant = candidate
                    .model_variants
                    .iter_mut()
                    .find(|v| v.id == variant);
                let file = RemoteModelFileStatus {
                    relative_path,
                    expected_size_bytes,
                    present,
                    size_bytes,
                };
                if let Some(v) = model_variant {
                    v.files.push(file);
                } else {
                    candidate.model_variants.push(RemoteModelVariant {
                        id: variant,
                        files: vec![file],
                    });
                }
            }
            Some("kj_sage") if fields.len() == 3 => {
                let path = decode(fields[1])?;
                let candidate = candidates
                    .get_mut(&path)
                    .ok_or("\u{8fdc}\u{7aef}KJ\u{8282}\u{70b9}\u{6ca1}\u{6709}\u{5bf9}\u{5e94}ComfyUI\u{8def}\u{5f84}")?;
                candidate.kj_h3_sage_attention_present = flag(fields[2])?;
            }
            Some("done") if fields.len() == 2 && decode(fields[1])? == "ok" => complete = true,
            _ => return Err("\u{8fdc}\u{7aef}\u{73af}\u{5883}\u{68c0}\u{67e5}\u{8fd4}\u{56de}\u{4e86}\u{672a}\u{77e5}\u{6761}\u{76ee}".into()),
        }
    }
    if !complete {
        return Err(
            "\u{8fdc}\u{7aef}\u{73af}\u{5883}\u{68c0}\u{67e5}\u{672a}\u{5b8c}\u{6210}".into(),
        );
    }
    for candidate in candidates.values_mut() {
        candidate
            .h3_source_files
            .sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        for variant in &mut candidate.model_variants {
            variant
                .files
                .sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        }
        candidate.model_variants.sort_by(|a, b| a.id.cmp(&b.id));
    }
    for candidate in candidates.values_mut() {
        let sources = candidate
            .h3_source_files
            .iter()
            .map(|file| file.relative_path.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        if sources.len() != H3_SOURCE_FILES.len()
            || H3_SOURCE_FILES
                .iter()
                .any(|expected| !sources.contains(expected))
            || candidate.h3_source_files.len() != H3_SOURCE_FILES.len()
        {
            return Err("\u{8fdc}\u{7aef}H3\u{6e90}\u{7801}\u{68c0}\u{67e5}\u{7ed3}\u{679c}\u{4e0d}\u{5b8c}\u{6574}".into());
        }
        for (variant_id, expected) in [("fl2va", FL2VA_MODEL_FILES), ("ref2va", REF2VA_MODEL_FILES)]
        {
            let variant = candidate
                .model_variants
                .iter()
                .find(|variant| variant.id == variant_id)
                .ok_or("\u{8fdc}\u{7aef}H3\u{6a21}\u{578b}\u{68c0}\u{67e5}\u{7ed3}\u{679c}\u{4e0d}\u{5b8c}\u{6574}")?;
            let files = variant
                .files
                .iter()
                .map(|file| (file.relative_path.as_str(), file.expected_size_bytes))
                .collect::<std::collections::BTreeSet<_>>();
            if variant.files.len() != expected.len()
                || files.len() != expected.len()
                || expected.iter().any(|file| !files.contains(file))
                || variant
                    .files
                    .iter()
                    .any(|file| !file.present && file.size_bytes != 0)
            {
                return Err("\u{8fdc}\u{7aef}H3\u{6a21}\u{578b}\u{68c0}\u{67e5}\u{7ed3}\u{679c}\u{4e0d}\u{5b8c}\u{6574}".into());
            }
        }
        if candidate.model_variants.len() != 2 {
            return Err("\u{8fdc}\u{7aef}H3\u{6a21}\u{578b}\u{68c0}\u{67e5}\u{7ed3}\u{679c}\u{4e0d}\u{5b8c}\u{6574}".into());
        }
    }
    let gpus = parse_gpus(&gpu_lines);
    let total_vram_mib = gpus.iter().filter_map(|gpu| gpu.vram_mib).sum();
    Ok(AutoDlRemoteProbe {
        os: os.unwrap_or_else(|| "鏈煡Linux绯荤粺".into()),
        gpus,
        total_vram_mib,
        ram_total_mib,
        python,
        disks,
        comfyui_candidates: candidates.into_values().collect(),
    })
}

fn parse_gpus(value: &str) -> Vec<RemoteGpu> {
    value
        .lines()
        .filter_map(|line| {
            let values: Vec<_> = line.split(',').map(str::trim).collect();
            let name = values.first()?.to_string();
            if name.is_empty() {
                return None;
            }
            Some(RemoteGpu {
                name,
                vram_mib: values.get(1).and_then(|v| v.parse().ok()),
                driver_version: values
                    .get(2)
                    .filter(|v| !v.is_empty())
                    .map(|v| (*v).to_string()),
            })
        })
        .collect()
}

fn parse_disks(value: &str) -> Vec<RemoteDisk> {
    value
        .lines()
        .filter_map(|line| {
            let values: Vec<_> = line.split_whitespace().collect();
            if values.len() < 6 {
                return None;
            }
            Some(RemoteDisk {
                filesystem: values[0].into(),
                total_bytes: values[1].parse().ok(),
                used_bytes: values[2].parse().ok(),
                available_bytes: values[3].parse().ok(),
                mount_point: values[5..].join(" "),
            })
        })
        .collect()
}

#[tauri::command]
pub async fn autodl_remote_probe(config: SshTunnelConfig) -> Result<AutoDlRemoteProbe, String> {
    let ssh = ssh_tunnel::system_ssh_path()?;
    let plan = build_probe_launch_plan(&config, ssh)?;
    let raw = tokio::task::spawn_blocking(move || run_probe(plan))
        .await
        .map_err(|_| "\u{8fdc}\u{7aef}\u{73af}\u{5883}\u{68c0}\u{67e5}\u{4efb}\u{52a1}\u{5f02}\u{5e38}\u{7ed3}\u{675f}")??;
    parse_probe_output(&raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn encoded(value: &str) -> String {
        BASE64.encode(value)
    }
    fn sample_config(dir: &Path) -> SshTunnelConfig {
        let identity = dir.join("id_ed25519");
        let known_hosts = dir.join("known_hosts");
        fs::write(&identity, "private").unwrap();
        fs::write(&known_hosts, "example ssh-ed25519 AAAA").unwrap();
        SshTunnelConfig {
            host: "gpu.autodl.example".into(),
            user: "root".into(),
            port: 10022,
            identity_file: identity,
            known_hosts_file: known_hosts,
            remote_comfy_port: 8188,
        }
    }

    #[test]
    fn plan_is_strict_and_remote_command_is_constant() {
        let dir = tempdir().unwrap();
        let config = sample_config(dir.path());
        let plan = build_probe_launch_plan(
            &config,
            PathBuf::from(r"C:\\Windows\\System32\\OpenSSH\\ssh.exe"),
        )
        .unwrap();
        assert!(plan.args.windows(2).any(|w| w == ["-F", "NUL"]));
        assert!(
            plan.args
                .windows(2)
                .any(|w| w == ["-o", "StrictHostKeyChecking=yes"])
        );
        assert!(
            plan.args
                .windows(2)
                .any(|w| w == ["-o", "PasswordAuthentication=no"])
        );
        assert_eq!(plan.args.last().unwrap(), REMOTE_PROBE_COMMAND);
        assert!(!REMOTE_PROBE_COMMAND.contains("gpu.autodl.example"));
        assert!(!REMOTE_PROBE_COMMAND.contains("id_ed25519"));
    }

    #[test]
    fn rejects_injected_ssh_host_without_exposing_key_path() {
        let dir = tempdir().unwrap();
        let mut config = sample_config(dir.path());
        config.host = "host; touch /tmp/pwned".into();
        let error = build_probe_launch_plan(&config, PathBuf::from("ssh.exe")).unwrap_err();
        assert_eq!(error, "SSH主机格式无效");
        assert!(!error.contains("id_ed25519"));
    }

    #[test]
    fn decodes_complete_probe_with_all_h3_inventory() {
        let path = "/root/ComfyUI";
        let mut lines = vec![
            format!("os\t{}", encoded("Ubuntu 22.04")),
            format!("gpu\t{}", encoded("NVIDIA GeForce RTX 5090, 32607, 580.10")),
            format!("ram_mib\t{}", encoded("130000")),
            format!("python\t{}", encoded("Python 3.12.3")),
            format!("disks\t{}", encoded("overlay 1000 300 700 30% /")),
            format!("comfy\t{}", encoded(path)),
        ];
        for source in H3_SOURCE_FILES {
            lines.push(format!(
                "source\t{}\t{}\t{}",
                encoded(path),
                encoded(source),
                encoded("1")
            ));
        }
        for (variant, files) in [("fl2va", FL2VA_MODEL_FILES), ("ref2va", REF2VA_MODEL_FILES)] {
            for (relative, expected) in files {
                lines.push(format!(
                    "model\t{}\t{}\t{}\t{}\t{}\t{}",
                    encoded(path),
                    encoded(variant),
                    encoded(relative),
                    encoded(&expected.to_string()),
                    encoded("1"),
                    encoded(&expected.to_string())
                ));
            }
        }
        lines.push(format!("kj_sage\t{}\t{}", encoded(path), encoded("1")));
        lines.push(format!("done\t{}", encoded("ok")));
        let report = parse_probe_output(lines.join("\n").as_bytes()).unwrap();
        assert_eq!(report.total_vram_mib, 32607);
        assert_eq!(report.comfyui_candidates.len(), 1);
        let comfy = &report.comfyui_candidates[0];
        assert_eq!(comfy.h3_source_files.len(), H3_SOURCE_FILES.len());
        assert_eq!(
            comfy
                .model_variants
                .iter()
                .find(|v| v.id == "fl2va")
                .unwrap()
                .files
                .len(),
            4
        );
        assert!(comfy.kj_h3_sage_attention_present);
    }

    #[test]
    fn rejects_partial_or_untrusted_remote_protocol() {
        assert!(parse_probe_output(format!("os\t{}", encoded("Ubuntu")).as_bytes()).is_err());
        let unsafe_source = format!(
            "comfy\t{}\nsource\t{}\t{}\t{}\ndone\t{}",
            encoded("/root/ComfyUI"),
            encoded("/root/ComfyUI"),
            encoded("../../x.py"),
            encoded("1"),
            encoded("ok")
        );
        assert!(parse_probe_output(unsafe_source.as_bytes()).is_err());
    }
}
