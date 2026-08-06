//! 可恢复的模型下载核心。
//!
//! 最终文件、`.part` 和 `.part.json` 始终位于同一目录，因此完成时的
//! `rename` 不会跨卷。调用方可把 [`DownloadProgress`] 直接作为 Tauri 事件载荷。

use reqwest::{
    Client, StatusCode,
    header::{CONTENT_LENGTH, CONTENT_RANGE, ETAG, IF_RANGE, LAST_MODIFIED, RANGE},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadPhase {
    Preparing,
    Downloading,
    Verifying,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub phase: DownloadPhase,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub progress_percent: Option<f64>,
    pub bytes_per_second: f64,
    pub eta_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRequest {
    pub source_url: String,
    /// 相对于 `model_root` 的文件名，可包含普通子目录。
    pub relative_path: PathBuf,
    pub expected_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResumeSidecar {
    version: u8,
    source_url: String,
    expected_sha256: String,
    total_bytes: Option<u64>,
    etag: Option<String>,
    last_modified: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadResult {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub sha256: String,
}

pub type DownloadError = String;

/// 下载模型并在完成后校验 SHA-256。
///
/// HTTP(S) 使用 `Range: bytes=N-` 续传；测试和离线导入也支持 `file://` URL。
pub async fn download_model<F>(
    client: &Client,
    model_root: &Path,
    request: &DownloadRequest,
    mut emit: F,
) -> Result<DownloadResult, DownloadError>
where
    F: FnMut(DownloadProgress),
{
    validate_sha256(&request.expected_sha256)?;
    let final_path = safe_destination(model_root, &request.relative_path)?;
    if final_path.is_file() {
        let existing_sha = sha256_file(&final_path)?;
        if existing_sha.eq_ignore_ascii_case(&request.expected_sha256) {
            let size = fs::metadata(&final_path)
                .map_err(|e| format!("读取已有模型失败：{e}"))?
                .len();
            emit(progress(DownloadPhase::Completed, size, Some(size), 0.0));
            return Ok(DownloadResult {
                path: final_path,
                size_bytes: size,
                sha256: existing_sha,
            });
        }
    }
    let part_path = append_suffix(&final_path, ".part")?;
    let sidecar_path = append_suffix(&part_path, ".json")?;
    emit(progress(DownloadPhase::Preparing, 0, None, 0.0));

    let mut resume_at = fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0);
    let old_sidecar = read_sidecar(&sidecar_path);
    if old_sidecar.as_ref().is_some_and(|s| {
        s.source_url != request.source_url
            || !s
                .expected_sha256
                .eq_ignore_ascii_case(&request.expected_sha256)
    }) {
        remove_if_exists(&part_path)?;
        remove_if_exists(&sidecar_path)?;
        resume_at = 0;
    }

    let parsed =
        url::Url::parse(&request.source_url).map_err(|e| format!("下载地址格式错误：{e}"))?;
    let (total, etag, last_modified) = match parsed.scheme() {
        "file" => {
            let source = parsed
                .to_file_path()
                .map_err(|_| "file URL 不是有效的本地路径".to_string())?;
            let total = fs::metadata(&source)
                .map_err(|e| format!("读取源文件失败：{e}"))?
                .len();
            if resume_at > total {
                remove_if_exists(&part_path)?;
                resume_at = 0;
            }
            write_sidecar(
                &sidecar_path,
                &ResumeSidecar {
                    version: 1,
                    source_url: request.source_url.clone(),
                    expected_sha256: request.expected_sha256.to_ascii_lowercase(),
                    total_bytes: Some(total),
                    etag: None,
                    last_modified: None,
                },
            )?;
            copy_local_range(&source, &part_path, resume_at, total, &mut emit)?;
            (Some(total), None, None)
        }
        "http" | "https" => {
            let mut request_builder = client
                .get(parsed)
                .header(RANGE, format!("bytes={resume_at}-"));
            if resume_at > 0 {
                if let Some(validator) = old_sidecar
                    .as_ref()
                    .and_then(|s| s.etag.as_ref().or(s.last_modified.as_ref()))
                {
                    request_builder = request_builder.header(IF_RANGE, validator);
                }
            }
            let response = request_builder
                .send()
                .await
                .map_err(|e| format!("连接下载地址失败：{e}"))?;

            let status = response.status();
            if status == StatusCode::RANGE_NOT_SATISFIABLE && resume_at > 0 {
                // 已完整下载的 .part 会在后续哈希校验中得到确认。
            } else if !status.is_success() {
                return Err(format!("下载服务器返回 HTTP {status}"));
            }

            let accepted_resume = status == StatusCode::PARTIAL_CONTENT
                && response
                    .headers()
                    .get(CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok())
                    .and_then(parse_content_range)
                    .is_some_and(|(start, _)| start == resume_at);
            if resume_at > 0 && !accepted_resume && status != StatusCode::RANGE_NOT_SATISFIABLE {
                resume_at = 0;
                remove_if_exists(&part_path)?;
            }

            let headers = response.headers();
            let response_len = headers
                .get(CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok());
            let total = headers
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_content_range)
                .and_then(|(_, total)| total)
                .or_else(|| response_len.map(|n| n + resume_at));
            let etag = header_string(headers, ETAG);
            let last_modified = header_string(headers, LAST_MODIFIED);
            write_sidecar(
                &sidecar_path,
                &ResumeSidecar {
                    version: 1,
                    source_url: request.source_url.clone(),
                    expected_sha256: request.expected_sha256.to_ascii_lowercase(),
                    total_bytes: total,
                    etag: etag.clone(),
                    last_modified: last_modified.clone(),
                },
            )?;
            if status != StatusCode::RANGE_NOT_SATISFIABLE {
                write_http_body(response, &part_path, resume_at, total, &mut emit).await?;
            }
            (total, etag, last_modified)
        }
        scheme => return Err(format!("不支持的下载协议：{scheme}")),
    };
    let _ = (total, etag, last_modified);

    let size = fs::metadata(&part_path)
        .map_err(|e| format!("读取临时文件失败：{e}"))?
        .len();
    emit(progress(DownloadPhase::Verifying, size, Some(size), 0.0));
    let actual = sha256_file(&part_path)?;
    if !actual.eq_ignore_ascii_case(&request.expected_sha256) {
        return Err(format!(
            "SHA-256 校验失败：期望 {}，实际 {actual}",
            request.expected_sha256
        ));
    }
    // part 与 final 位于相同目录，Windows/Unix 上均保持同卷原子替换语义。
    if final_path.exists() {
        fs::remove_file(&final_path).map_err(|e| format!("移除旧模型失败：{e}"))?;
    }
    fs::rename(&part_path, &final_path).map_err(|e| format!("完成模型文件失败：{e}"))?;
    remove_if_exists(&sidecar_path)?;
    emit(progress(DownloadPhase::Completed, size, Some(size), 0.0));
    Ok(DownloadResult {
        path: final_path,
        size_bytes: size,
        sha256: actual,
    })
}

async fn write_http_body<F>(
    mut response: reqwest::Response,
    part: &Path,
    offset: u64,
    total: Option<u64>,
    emit: &mut F,
) -> Result<(), DownloadError>
where
    F: FnMut(DownloadProgress),
{
    let mut output = open_part(part, offset)?;
    let started = Instant::now();
    let mut downloaded = offset;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("读取下载流失败：{e}"))?
    {
        output
            .write_all(&chunk)
            .map_err(|e| format!("写入临时文件失败：{e}"))?;
        downloaded += chunk.len() as u64;
        let speed = speed(downloaded - offset, started.elapsed());
        emit(progress(
            DownloadPhase::Downloading,
            downloaded,
            total,
            speed,
        ));
    }
    output
        .sync_all()
        .map_err(|e| format!("同步临时文件失败：{e}"))
}

fn copy_local_range<F>(
    source: &Path,
    part: &Path,
    offset: u64,
    total: u64,
    emit: &mut F,
) -> Result<(), DownloadError>
where
    F: FnMut(DownloadProgress),
{
    let mut input = File::open(source).map_err(|e| format!("打开源文件失败：{e}"))?;
    input
        .seek(SeekFrom::Start(offset))
        .map_err(|e| format!("定位源文件失败：{e}"))?;
    let mut output = open_part(part, offset)?;
    let started = Instant::now();
    let mut downloaded = offset;
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let count = input
            .read(&mut buffer)
            .map_err(|e| format!("读取源文件失败：{e}"))?;
        if count == 0 {
            break;
        }
        output
            .write_all(&buffer[..count])
            .map_err(|e| format!("写入临时文件失败：{e}"))?;
        downloaded += count as u64;
        emit(progress(
            DownloadPhase::Downloading,
            downloaded,
            Some(total),
            speed(downloaded - offset, started.elapsed()),
        ));
    }
    output
        .sync_all()
        .map_err(|e| format!("同步临时文件失败：{e}"))
}

fn open_part(path: &Path, offset: u64) -> Result<File, DownloadError> {
    let mut options = OpenOptions::new();
    options.create(true).write(true);
    if offset == 0 {
        options.truncate(true);
    } else {
        options.append(true);
    }
    options
        .open(path)
        .map_err(|e| format!("打开临时文件失败：{e}"))
}

fn progress(
    phase: DownloadPhase,
    downloaded: u64,
    total: Option<u64>,
    bytes_per_second: f64,
) -> DownloadProgress {
    DownloadProgress {
        phase,
        downloaded_bytes: downloaded,
        total_bytes: total,
        progress_percent: total
            .filter(|n| *n > 0)
            .map(|n| downloaded.min(n) as f64 * 100.0 / n as f64),
        bytes_per_second,
        eta_seconds: total.and_then(|n| {
            (bytes_per_second > 0.0 && n > downloaded)
                .then(|| ((n - downloaded) as f64 / bytes_per_second).ceil() as u64)
        }),
    }
}

fn speed(bytes: u64, elapsed: Duration) -> f64 {
    if elapsed.as_secs_f64() > 0.0 {
        bytes as f64 / elapsed.as_secs_f64()
    } else {
        0.0
    }
}

pub fn safe_destination(root: &Path, relative: &Path) -> Result<PathBuf, DownloadError> {
    if relative.as_os_str().is_empty() || relative.is_absolute() {
        return Err("模型路径必须是非空相对路径".into());
    }
    if relative
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("模型路径包含不安全的路径片段".into());
    }
    fs::create_dir_all(root).map_err(|e| format!("创建模型目录失败：{e}"))?;
    let canonical_root = fs::canonicalize(root).map_err(|e| format!("解析模型目录失败：{e}"))?;
    let destination = canonical_root.join(relative);
    let parent = destination
        .parent()
        .ok_or_else(|| "模型路径缺少父目录".to_string())?;
    fs::create_dir_all(parent).map_err(|e| format!("创建模型子目录失败：{e}"))?;
    let canonical_parent =
        fs::canonicalize(parent).map_err(|e| format!("解析模型子目录失败：{e}"))?;
    if !canonical_parent.starts_with(&canonical_root) {
        return Err("模型路径超出模型目录".into());
    }
    Ok(canonical_parent.join(
        destination
            .file_name()
            .ok_or_else(|| "模型文件名无效".to_string())?,
    ))
}

fn append_suffix(path: &Path, suffix: &str) -> Result<PathBuf, DownloadError> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "模型文件名不是有效文本".to_string())?;
    Ok(path.with_file_name(format!("{name}{suffix}")))
}

fn validate_sha256(value: &str) -> Result<(), DownloadError> {
    if value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err("SHA-256 必须是 64 位十六进制文本".into())
    }
}

fn sha256_file(path: &Path) -> Result<String, DownloadError> {
    let mut file = File::open(path).map_err(|e| format!("打开校验文件失败：{e}"))?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|e| format!("读取校验文件失败：{e}"))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn write_sidecar(path: &Path, sidecar: &ResumeSidecar) -> Result<(), DownloadError> {
    let bytes = serde_json::to_vec_pretty(sidecar).map_err(|e| format!("编码续传信息失败：{e}"))?;
    fs::write(path, bytes).map_err(|e| format!("写入续传信息失败：{e}"))
}

fn read_sidecar(path: &Path) -> Option<ResumeSidecar> {
    fs::read(path)
        .ok()
        .and_then(|v| serde_json::from_slice(&v).ok())
}

fn remove_if_exists(path: &Path) -> Result<(), DownloadError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("清理临时文件失败：{e}")),
    }
}

fn header_string(
    headers: &reqwest::header::HeaderMap,
    name: reqwest::header::HeaderName,
) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

fn parse_content_range(value: &str) -> Option<(u64, Option<u64>)> {
    let value = value.strip_prefix("bytes ")?;
    let (range, total) = value.split_once('/')?;
    let (start, _) = range.split_once('-')?;
    Some((
        start.parse().ok()?,
        if total == "*" {
            None
        } else {
            total.parse().ok()
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_traversal_and_absolute_paths() {
        let root = std::env::temp_dir().join(format!("h3-download-path-{}", std::process::id()));
        assert!(safe_destination(&root, Path::new("../outside.bin")).is_err());
        assert!(safe_destination(&root, Path::new("C:\\outside.bin")).is_err());
        assert!(
            safe_destination(&root, Path::new("models/h3.bin"))
                .unwrap()
                .starts_with(fs::canonicalize(&root).unwrap())
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn parses_content_range_and_builds_eta() {
        assert_eq!(
            parse_content_range("bytes 100-199/1000"),
            Some((100, Some(1000)))
        );
        let p = progress(DownloadPhase::Downloading, 250, Some(1000), 250.0);
        assert_eq!(p.progress_percent, Some(25.0));
        assert_eq!(p.eta_seconds, Some(3));
    }

    #[test]
    fn hashes_file() {
        let path = std::env::temp_dir().join(format!("h3-download-hash-{}", std::process::id()));
        fs::write(&path, b"abc").unwrap();
        assert_eq!(
            sha256_file(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let _ = fs::remove_file(path);
    }
}
