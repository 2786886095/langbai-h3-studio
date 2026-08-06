use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};
const COMFY: &str = "ComfyUI_windows_portable/ComfyUI";
const PROTECTED: [&str; 4] = ["models", "input", "output", "user"];
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct H3PatchManifest {
    pub id: String,
    pub commit: String,
    pub sha256: String,
    pub archive_format: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PatchReceipt {
    pub patch_id: String,
    pub commit: String,
    pub profile_path: PathBuf,
    pub entries: Vec<PatchReceiptEntry>,
    pub receipt_path: PathBuf,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PatchReceiptEntry {
    pub relative_path: PathBuf,
    pub had_original: bool,
}

pub fn install_h3_patch(
    m: &H3PatchManifest,
    archive: &Path,
    profile: &Path,
) -> Result<PatchReceipt, String> {
    validate_manifest(m)?;
    let actual = sha(archive)?;
    if !actual.eq_ignore_ascii_case(&m.sha256) {
        return Err(format!(
            "SHA-256 verification failed: expected {}, actual {actual}",
            m.sha256
        ));
    }
    let comfy = profile.join(COMFY);
    if !comfy.is_dir() {
        return Err("managed ComfyUI directory is missing".into());
    }
    let patch_root = profile.join(".h3-patches").join(&m.id);
    let staging = patch_root.join("staging");
    let backup = patch_root.join("backup");
    remove(&patch_root)?;
    fs::create_dir_all(&staging).map_err(err)?;
    fs::create_dir_all(&backup).map_err(err)?;
    if let Err(e) = extract(archive, &staging, &m.commit) {
        let _ = remove(&patch_root);
        return Err(e);
    }
    let files = files(&staging)?;
    if files.is_empty() {
        let _ = remove(&patch_root);
        return Err("patch has no source files".into());
    }
    let mut entries = Vec::new();
    for rel in files {
        let src = staging.join(&rel);
        let target = comfy.join(&rel);
        let existed = target.is_file();
        let tmp = target.with_extension("h3patch.tmp");
        let prep = (|| -> Result<(), String> {
            if let Some(p) = target.parent() {
                fs::create_dir_all(p).map_err(err)?
            }
            fs::copy(&src, &tmp).map_err(err)?;
            if existed {
                let b = backup.join(&rel);
                if let Some(p) = b.parent() {
                    fs::create_dir_all(p).map_err(err)?
                }
                fs::copy(&target, b).map_err(err)?;
            }
            Ok(())
        })();
        if let Err(e) = prep {
            let _ = fs::remove_file(tmp);
            let _ = restore(&comfy, &backup, &entries);
            let _ = remove(&patch_root);
            return Err(e);
        }
        entries.push(PatchReceiptEntry {
            relative_path: rel.clone(),
            had_original: existed,
        });
        let commit = (|| -> Result<(), String> {
            if target.exists() {
                fs::remove_file(&target).map_err(err)?
            }
            fs::rename(&tmp, &target).map_err(err)
        })();
        if let Err(e) = commit {
            let _ = fs::remove_file(tmp);
            let _ = restore(&comfy, &backup, &entries);
            let _ = remove(&patch_root);
            return Err(e);
        }
    }
    remove(&staging)?;
    let receipt_path = patch_root.join("receipt.json");
    let receipt = PatchReceipt {
        patch_id: m.id.clone(),
        commit: m.commit.clone(),
        profile_path: profile.to_path_buf(),
        entries,
        receipt_path: receipt_path.clone(),
    };
    let data = serde_json::to_vec_pretty(&receipt).map_err(err)?;
    if let Err(e) = fs::write(&receipt_path, data) {
        let _ = restore(&comfy, &backup, &receipt.entries);
        let _ = remove(&patch_root);
        return Err(err(e));
    }
    Ok(receipt)
}
pub fn rollback_h3_patch(receipt_path: &Path) -> Result<(), String> {
    let r: PatchReceipt =
        serde_json::from_slice(&fs::read(receipt_path).map_err(err)?).map_err(err)?;
    let expected = r
        .profile_path
        .join(".h3-patches")
        .join(&r.patch_id)
        .join("receipt.json");
    if expected != receipt_path {
        return Err("receipt path does not match profile".into());
    }
    let root = r.profile_path.join(COMFY);
    let dir = receipt_path.parent().ok_or("invalid receipt path")?;
    restore(&root, &dir.join("backup"), &r.entries)?;
    remove(dir)
}
fn extract(path: &Path, out: &Path, commit: &str) -> Result<(), String> {
    let mut z = zip::ZipArchive::new(File::open(path).map_err(err)?).map_err(err)?;
    let expected = format!("ComfyUI-{commit}");
    for i in 0..z.len() {
        let mut e = z.by_index(i).map_err(err)?;
        let enclosed = e
            .enclosed_name()
            .ok_or_else(|| format!("path traversal in ZIP: {}", e.name()))?
            .to_path_buf();
        let mut cs = enclosed.components();
        let top = match cs.next() {
            Some(Component::Normal(v)) => v.to_string_lossy(),
            _ => return Err("invalid ZIP path".into()),
        };
        if top != expected {
            return Err(format!("unexpected top-level prefix: {top}"));
        }
        let rel: PathBuf = cs.collect();
        if rel.as_os_str().is_empty() {
            continue;
        }
        if is_protected(&rel) {
            continue;
        }
        validate_rel(&rel)?;
        let dst = out.join(rel);
        if e.is_dir() {
            fs::create_dir_all(dst).map_err(err)?
        } else {
            if let Some(p) = dst.parent() {
                fs::create_dir_all(p).map_err(err)?
            }
            let mut f = File::create(dst).map_err(err)?;
            std::io::copy(&mut e, &mut f).map_err(err)?;
            f.flush().map_err(err)?
        }
    }
    Ok(())
}
fn validate_rel(p: &Path) -> Result<(), String> {
    if p.is_absolute() || p.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(format!("unsafe patch path: {}", p.display()));
    }
    if is_protected(p) {
        let first = p.components().next().unwrap().as_os_str().to_string_lossy();
        return Err(format!("protected directory: {first}"));
    }
    Ok(())
}
fn is_protected(p: &Path) -> bool {
    let first = p
        .components()
        .next()
        .and_then(|c| match c {
            Component::Normal(v) => v.to_str(),
            _ => None,
        })
        .unwrap_or("");
    PROTECTED.iter().any(|x| first.eq_ignore_ascii_case(x))
}
fn files(root: &Path) -> Result<Vec<PathBuf>, String> {
    fn walk(root: &Path, d: &Path, o: &mut Vec<PathBuf>) -> Result<(), String> {
        for e in fs::read_dir(d).map_err(err)? {
            let e = e.map_err(err)?;
            if e.file_type().map_err(err)?.is_dir() {
                walk(root, &e.path(), o)?
            } else {
                o.push(e.path().strip_prefix(root).map_err(err)?.to_path_buf())
            }
        }
        Ok(())
    }
    let mut o = vec![];
    walk(root, root, &mut o)?;
    o.sort();
    Ok(o)
}
fn restore(root: &Path, backup: &Path, entries: &[PatchReceiptEntry]) -> Result<(), String> {
    for e in entries.iter().rev() {
        validate_rel(&e.relative_path)?;
        let t = root.join(&e.relative_path);
        if e.had_original {
            let b = backup.join(&e.relative_path);
            if !b.is_file() {
                return Err(format!("backup missing: {}", e.relative_path.display()));
            }
            if let Some(p) = t.parent() {
                fs::create_dir_all(p).map_err(err)?
            }
            fs::copy(b, t).map_err(err)?;
        } else if t.exists() {
            fs::remove_file(t).map_err(err)?
        }
    }
    Ok(())
}
fn validate_manifest(m: &H3PatchManifest) -> Result<(), String> {
    if m.id.is_empty()
        || m.commit.is_empty()
        || m.id.contains('/')
        || m.id.contains(char::from(92))
        || m.commit.contains('/')
        || m.commit.contains(char::from(92))
    {
        return Err("invalid patch id or commit".into());
    }
    if m.archive_format != "zip" {
        return Err("only zip patches are supported".into());
    }
    if m.sha256.len() != 64 || !m.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid SHA-256".into());
    }
    Ok(())
}
fn sha(p: &Path) -> Result<String, String> {
    let mut f = File::open(p).map_err(err)?;
    let mut h = Sha256::new();
    let mut b = [0; 65536];
    loop {
        let n = f.read(&mut b).map_err(err)?;
        if n == 0 {
            break;
        }
        h.update(&b[..n])
    }
    Ok(format!("{:x}", h.finalize()))
}
fn remove(p: &Path) -> Result<(), String> {
    if !p.exists() {
        return Ok(());
    }
    if p.is_dir() {
        fs::remove_dir_all(p)
    } else {
        fs::remove_file(p)
    }
    .map_err(err)
}
fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}
