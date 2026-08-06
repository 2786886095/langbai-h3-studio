//! Pinned community-node installation for the Studio-managed ComfyUI profile.
//!
//! Catalog evidence describes source compatibility only.  It is deliberately
//! separate from benchmark verification and never implies a performance gain.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read},
    path::{Component, Path, PathBuf},
    process::Command,
};
use zip::ZipArchive;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedNodeCatalogItem {
    pub id: String,
    pub name: String,
    pub repository: String,
    pub commit: String,
    pub archive_url: Option<String>,
    pub archive_size: Option<u64>,
    pub archive_sha256: Option<String>,
    pub license: String,
    pub category: String,
    pub evidence_level: String,
    pub evidence_url: String,
    pub required_nodes: Vec<String>,
    pub experimental: bool,
    pub installable: bool,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedNodeStatus {
    pub id: String,
    pub installed: bool,
    pub installed_commit: Option<String>,
    pub restart_required: bool,
    pub verified: bool,
    pub category: String,
    pub evidence_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallReceipt {
    id: String,
    commit: String,
    archive_sha256: String,
    installed_at_unix_ms: u128,
    verified: bool,
}

pub fn catalog() -> Vec<ManagedNodeCatalogItem> {
    vec![
        ManagedNodeCatalogItem {
            id: "digital-garbage.comfyui-funpack".into(),
            name: "ComfyUI-FunPack".into(),
            repository: "https://github.com/digital-garbage/ComfyUI-FunPack".into(),
            commit: "7af38e2e5522a6c1a253b0921ac53356799c00a6".into(),
            archive_url: Some("https://github.com/digital-garbage/ComfyUI-FunPack/archive/7af38e2e5522a6c1a253b0921ac53356799c00a6.zip".into()),
            archive_size: Some(2_719_384),
            archive_sha256: Some("5862C9A2973123316489BA6A6642405CE4934020C018393D65259BA171B124B4".into()),
            license: "GPL-3.0".into(), category: "h3-community".into(),
            evidence_level: "source-supported".into(),
            evidence_url: "https://github.com/digital-garbage/ComfyUI-FunPack/commit/46f90b4".into(),
            required_nodes: vec![], experimental: true, installable: true,
            description: "H3 社区兼容扩展；没有经过本项目性能验证。".into(),
        },
        ManagedNodeCatalogItem {
            id: "kijai.comfyui-kjnodes".into(), name: "ComfyUI-KJNodes".into(),
            repository: "https://github.com/kijai/ComfyUI-KJNodes".into(),
            commit: "8692bc8ef8beaaeee80fd52ba80477dc9e61547b".into(),
            archive_url: Some("https://github.com/kijai/ComfyUI-KJNodes/archive/8692bc8ef8beaaeee80fd52ba80477dc9e61547b.zip".into()),
            archive_size: Some(1_110_914),
            archive_sha256: Some("939D92ECA74FF7717F6C3D15945C0943B4A663E34973B14070A7E6F61748D47B".into()),
            license: "GPL-3.0".into(), category: "h3-acceleration".into(),
            evidence_level: "source-supported".into(),
            evidence_url: "https://github.com/kijai/ComfyUI-KJNodes/commit/8692bc8ef8beaaeee80fd52ba80477dc9e61547b".into(),
            required_nodes: vec!["MiniMaxH3MemoryEfficientSageAttentionPatch".into()],
            experimental: true, installable: true,
            description: "包含 H3 内存高效 SageAttention Patch；仅有源码证据，尚未完成性能验证。".into(),
        },
    ]
}

fn select(id: &str) -> Result<ManagedNodeCatalogItem, String> {
    catalog()
        .into_iter()
        .find(|x| x.id == id)
        .ok_or_else(|| "未知的社区节点目录项".into())
}

fn comfy_root(profile: &Path) -> PathBuf {
    profile.join("ComfyUI_windows_portable").join("ComfyUI")
}
fn install_name(item: &ManagedNodeCatalogItem) -> &str {
    &item.name
}
fn node_dir(profile: &Path, item: &ManagedNodeCatalogItem) -> PathBuf {
    comfy_root(profile)
        .join("custom_nodes")
        .join(install_name(item))
}
fn receipt_path(dir: &Path) -> PathBuf {
    dir.join(".langbai-managed-node.json")
}

pub fn status_for(profile: &Path, item: &ManagedNodeCatalogItem) -> ManagedNodeStatus {
    let dir = node_dir(profile, item);
    let receipt = fs::read(receipt_path(&dir))
        .ok()
        .and_then(|b| serde_json::from_slice::<InstallReceipt>(&b).ok());
    ManagedNodeStatus {
        id: item.id.clone(),
        installed: dir.is_dir() && receipt.is_some(),
        installed_commit: receipt.map(|r| r.commit),
        restart_required: dir.is_dir(),
        verified: false,
        category: item.category.clone(),
        evidence_level: item.evidence_level.clone(),
    }
}

pub fn install_archive<F>(
    profile: &Path,
    item: &ManagedNodeCatalogItem,
    archive: &Path,
    mut run_pip: F,
) -> Result<ManagedNodeStatus, String>
where
    F: FnMut(&Path, &Path) -> Result<(), String>,
{
    if !item.installable {
        return Err("该目录项尚未固定下载哈希，暂不可安装".into());
    }
    let expected = item.archive_sha256.as_deref().ok_or("目录项缺少 SHA-256")?;
    let actual = sha256_file(archive)?;
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(format!(
            "社区节点包 SHA-256 不匹配：期望 {expected}，实际 {actual}"
        ));
    }
    if fs::metadata(archive).map_err(|e| e.to_string())?.len() != item.archive_size.unwrap_or(0) {
        return Err("社区节点包大小不匹配".into());
    }
    let custom = comfy_root(profile).join("custom_nodes");
    fs::create_dir_all(&custom).map_err(|e| e.to_string())?;
    let final_dir = node_dir(profile, item);
    if final_dir.exists() {
        return Err("该社区节点已经安装".into());
    }
    let stage = custom.join(format!(".{}.installing", install_name(item)));
    if stage.exists() {
        fs::remove_dir_all(&stage).map_err(|e| e.to_string())?
    }
    fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
    let result = (|| {
        extract_repo_zip(archive, &stage, &item.commit)?;
        let requirements = stage.join("requirements.txt");
        if requirements.is_file() {
            let python = profile
                .join("ComfyUI_windows_portable")
                .join("python_embeded")
                .join("python.exe");
            if !python.is_file() {
                return Err("托管 Runtime Python 不存在".into());
            }
            run_pip(&python, &requirements)?
        }
        let receipt = InstallReceipt {
            id: item.id.clone(),
            commit: item.commit.clone(),
            archive_sha256: actual,
            installed_at_unix_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            verified: false,
        };
        fs::write(
            receipt_path(&stage),
            serde_json::to_vec_pretty(&receipt).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        fs::rename(&stage, &final_dir).map_err(|e| e.to_string())?;
        Ok(status_for(profile, item))
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&stage);
        let _ = fs::remove_dir_all(&final_dir);
    }
    result
}

pub fn uninstall_from(
    profile: &Path,
    item: &ManagedNodeCatalogItem,
) -> Result<ManagedNodeStatus, String> {
    let dir = node_dir(profile, item);
    let custom = comfy_root(profile).join("custom_nodes");
    if dir.parent() != Some(custom.as_path())
        || dir.file_name() != Some(std::ffi::OsStr::new(install_name(item)))
    {
        return Err("卸载目录不安全".into());
    }
    if !dir.exists() {
        return Ok(status_for(profile, item));
    }
    if !receipt_path(&dir).is_file() {
        return Err("目标目录不是 Studio 管理的节点，保留原目录".into());
    }
    let tomb = custom.join(format!(".{}.removing", install_name(item)));
    if tomb.exists() {
        fs::remove_dir_all(&tomb).map_err(|e| e.to_string())?
    }
    fs::rename(&dir, &tomb).map_err(|e| e.to_string())?;
    if let Err(e) = fs::remove_dir_all(&tomb) {
        let _ = fs::rename(&tomb, &dir);
        return Err(e.to_string());
    }
    Ok(status_for(profile, item))
}

fn extract_repo_zip(archive: &Path, dest: &Path, commit: &str) -> Result<(), String> {
    let mut z = ZipArchive::new(fs::File::open(archive).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    for i in 0..z.len() {
        let mut f = z.by_index(i).map_err(|e| e.to_string())?;
        if f.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            return Err("ZIP 包含符号链接".into());
        }
        let enclosed = f.enclosed_name().ok_or("ZIP 包含路径穿越")?;
        let mut parts = enclosed.components();
        let root = parts.next().ok_or("ZIP 路径为空")?;
        let root = root.as_os_str().to_string_lossy();
        if !root.ends_with(commit) {
            return Err("ZIP 根目录与固定提交不匹配".into());
        }
        let relative = parts.as_path();
        if relative.as_os_str().is_empty() {
            continue;
        }
        if relative.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return Err("ZIP 包含不安全路径".into());
        }
        let out = dest.join(relative);
        if f.is_dir() {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?
        } else {
            fs::create_dir_all(out.parent().ok_or("ZIP 输出路径无父目录")?)
                .map_err(|e| e.to_string())?;
            let mut w = fs::File::create(&out).map_err(|e| e.to_string())?;
            io::copy(&mut f, &mut w).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut f = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    let mut b = [0u8; 128 * 1024];
    loop {
        let n = f.read(&mut b).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&b[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

fn current_profile() -> Result<PathBuf, String> {
    crate::runtime_manager::RuntimeManager::new(crate::runtime_root())
        .current()
        .map_err(|e| e.to_string())?
        .map(|x| x.profile_dir)
        .ok_or_else(|| "请先安装托管 ComfyUI Runtime".into())
}

#[tauri::command]
pub fn managed_nodes_catalog() -> Vec<ManagedNodeCatalogItem> {
    catalog()
}
#[tauri::command]
pub fn managed_nodes_status() -> Result<Vec<ManagedNodeStatus>, String> {
    let p = current_profile()?;
    Ok(catalog().iter().map(|x| status_for(&p, x)).collect())
}
#[tauri::command]
pub async fn managed_nodes_install(
    app: tauri::AppHandle,
    id: String,
) -> Result<ManagedNodeStatus, String> {
    use tauri::Emitter;
    let item = select(&id)?;
    if !item.installable {
        return Err("该目录项尚未固定并验证归档哈希".into());
    }
    let profile = current_profile()?;
    let url = item.archive_url.clone().ok_or("目录项缺少下载地址")?;
    let sha = item.archive_sha256.clone().ok_or("目录项缺少 SHA-256")?;
    let req = crate::download::DownloadRequest {
        source_url: url,
        relative_path: PathBuf::from("downloads/community-nodes")
            .join(format!("{}-{}.zip", item.name, item.commit)),
        expected_sha256: sha,
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(1800))
        .user_agent("Langbai-H3-Studio")
        .build()
        .map_err(|e| e.to_string())?;
    let downloaded = crate::download::download_model(&client, &crate::runtime_root(), &req, |p| {
        let _ = app.emit("managed-node-download-progress", p);
    })
    .await?;
    install_archive(&profile, &item, &downloaded.path, |python, requirements| {
        let status = Command::new(python)
            .args(["-m", "pip", "install", "-r"])
            .arg(requirements)
            .status()
            .map_err(|e| format!("启动托管 pip 失败：{e}"))?;
        if !status.success() {
            return Err(format!("依赖安装失败，退出码 {:?}", status.code()));
        }
        if item.id == "kijai.comfyui-kjnodes" {
            let sage = Command::new(python)
                .args(["-m", "pip", "install", "sageattention>=2.2.0"])
                .status()
                .map_err(|e| format!("启动 SageAttention 依赖安装失败：{e}"))?;
            if !sage.success() {
                return Err(format!(
                    "SageAttention 依赖安装失败，退出码 {:?}；社区节点安装已回滚",
                    sage.code()
                ));
            }
        }
        Ok(())
    })
}
#[tauri::command]
pub fn managed_nodes_uninstall(id: String) -> Result<ManagedNodeStatus, String> {
    let item = select(&id)?;
    uninstall_from(&current_profile()?, &item)
}
