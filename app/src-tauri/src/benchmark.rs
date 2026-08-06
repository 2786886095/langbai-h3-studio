use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CompatibilityReport {
    pub schema_version: u32,
    pub report_id: String,
    pub created_at: u64,
    pub studio_version: String,
    #[serde(default = "default_execution_target")]
    pub execution_target: String,
    pub gpu_name: String,
    pub driver_version: String,
    pub vram_total_mb: u64,
    pub peak_vram_used_mb: u64,
    pub ram_total_mb: u64,
    pub peak_ram_used_mb: u64,
    #[serde(default = "default_sampling_seconds")]
    pub resource_sampling_seconds: u32,
    pub runtime_version: String,
    pub h3_patch_commit: String,
    pub generation_mode: String,
    pub width: u32,
    pub height: u32,
    pub duration_seconds: f32,
    pub steps: u32,
    pub model_file: String,
    #[serde(default)]
    pub enabled_plugins: Vec<String>,
    pub elapsed_seconds: f64,
    pub outcome: String,
    #[serde(default)]
    pub error_summary: String,
    #[serde(default)]
    pub output_file: String,
}

fn default_execution_target() -> String { "local".into() }
fn default_sampling_seconds() -> u32 { 3 }

impl CompatibilityReport {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("兼容性报告 schemaVersion 必须为 1".into());
        }
        if self.report_id.is_empty()
            || !self
                .report_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        {
            return Err("兼容性报告 ID 无效".into());
        }
        if self.gpu_name.trim().is_empty()
            || self.runtime_version.trim().is_empty()
            || self.model_file.trim().is_empty()
        {
            return Err("兼容性报告缺少运行环境信息".into());
        }
        if self.width < 32 || self.height < 32 || self.width % 32 != 0 || self.height % 32 != 0 {
            return Err("兼容性报告分辨率无效".into());
        }
        if !self.duration_seconds.is_finite()
            || self.duration_seconds <= 0.0
            || !self.elapsed_seconds.is_finite()
            || self.elapsed_seconds < 0.0
        {
            return Err("兼容性报告时间数据无效".into());
        }
        if !matches!(self.outcome.as_str(), "completed" | "failed" | "canceled") {
            return Err("兼容性报告结果无效".into());
        }
        if self.peak_vram_used_mb > self.vram_total_mb || self.peak_ram_used_mb > self.ram_total_mb
        {
            return Err("兼容性报告峰值资源数据无效".into());
        }
        if self.output_file.contains("..") {
            return Err("兼容性报告输出路径无效".into());
        }
        Ok(())
    }
}

pub fn save_report(root: &Path, report: &CompatibilityReport) -> Result<PathBuf, String> {
    report.validate()?;
    fs::create_dir_all(root).map_err(|e| format!("创建兼容性报告目录失败：{e}"))?;
    let destination = root.join(format!("{}.json", report.report_id));
    let temporary = destination.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(report)
        .map_err(|e| format!("序列化兼容性报告失败：{e}"))?;
    fs::write(&temporary, bytes).map_err(|e| format!("写入兼容性报告失败：{e}"))?;
    fs::rename(&temporary, &destination).map_err(|e| format!("提交兼容性报告失败：{e}"))?;
    Ok(destination)
}

pub fn list_reports(root: &Path) -> Result<Vec<CompatibilityReport>, String> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut reports = Vec::new();
    for entry in fs::read_dir(root).map_err(|e| format!("读取兼容性报告失败：{e}"))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let report: CompatibilityReport =
            serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
                .map_err(|e| format!("兼容性报告损坏：{e}"))?;
        report.validate()?;
        reports.push(report);
    }
    reports.sort_by_key(|item| std::cmp::Reverse(item.created_at));
    Ok(reports)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AnonymousBenchmarkReport {
    pub created_day: u64,
    pub studio_version: String,
    pub execution_target: String,
    pub gpu_name: String,
    pub driver_version: String,
    pub vram_total_mb: u64,
    pub peak_vram_used_mb: u64,
    pub ram_total_mb: u64,
    pub peak_ram_used_mb: u64,
    pub resource_sampling_seconds: u32,
    pub h3_patch_commit: String,
    pub generation_mode: String,
    pub width: u32,
    pub height: u32,
    pub duration_seconds: f32,
    pub steps: u32,
    pub model_file_name: String,
    pub enabled_plugins: Vec<String>,
    pub elapsed_seconds: f64,
    pub outcome: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AnonymousBenchmarkBundle {
    pub schema_version: u32,
    pub exported_at: u64,
    pub privacy_notice: String,
    pub reports: Vec<AnonymousBenchmarkReport>,
}

pub fn anonymous_bundle(
    reports: &[CompatibilityReport],
    exported_at: u64,
) -> AnonymousBenchmarkBundle {
    let reports = reports
        .iter()
        .map(|item| AnonymousBenchmarkReport {
            created_day: item.created_at - item.created_at % 86_400,
            studio_version: item.studio_version.clone(),
            execution_target: item.execution_target.clone(),
            gpu_name: item.gpu_name.clone(),
            driver_version: item.driver_version.clone(),
            vram_total_mb: item.vram_total_mb,
            peak_vram_used_mb: item.peak_vram_used_mb,
            ram_total_mb: item.ram_total_mb,
            peak_ram_used_mb: item.peak_ram_used_mb,
            resource_sampling_seconds: item.resource_sampling_seconds,
            h3_patch_commit: item.h3_patch_commit.clone(),
            generation_mode: item.generation_mode.clone(),
            width: item.width,
            height: item.height,
            duration_seconds: item.duration_seconds,
            steps: item.steps,
            model_file_name: Path::new(&item.model_file)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("未命名模型")
                .to_string(),
            enabled_plugins: item.enabled_plugins.clone(),
            elapsed_seconds: item.elapsed_seconds,
            outcome: item.outcome.clone(),
        })
        .collect();
    AnonymousBenchmarkBundle {
        schema_version: 1,
        exported_at,
        privacy_notice: "不包含提示词、输入素材、输出路径、ComfyUI 地址、任务 ID 或错误详情"
            .into(),
        reports,
    }
}

pub fn export_anonymous(
    root: &Path,
    destination: &Path,
    exported_at: u64,
) -> Result<PathBuf, String> {
    if !destination.is_absolute()
        || destination.extension().and_then(|value| value.to_str()) != Some("json")
    {
        return Err("请选择以 .json 结尾的绝对保存路径".into());
    }
    let parent = destination
        .parent()
        .ok_or_else(|| "基准报告保存目录无效".to_string())?;
    fs::create_dir_all(parent).map_err(|e| format!("创建基准报告保存目录失败：{e}"))?;
    let reports = list_reports(root)?;
    if reports.is_empty() {
        return Err("完成至少一次真实生成后才能导出基准报告".into());
    }
    let bytes = serde_json::to_vec_pretty(&anonymous_bundle(&reports, exported_at))
        .map_err(|e| format!("序列化匿名基准报告失败：{e}"))?;
    let temporary = destination.with_extension("json.tmp");
    fs::write(&temporary, bytes).map_err(|e| format!("写入匿名基准报告失败：{e}"))?;
    fs::rename(&temporary, destination).map_err(|e| format!("提交匿名基准报告失败：{e}"))?;
    Ok(destination.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> CompatibilityReport {
        CompatibilityReport {
            schema_version: 1,
            report_id: "job-1".into(),
            created_at: 90_000,
            studio_version: "0.10.1".into(),
            execution_target: "local".into(),
            gpu_name: "RTX".into(),
            driver_version: "1".into(),
            vram_total_mb: 24_000,
            peak_vram_used_mb: 20_000,
            ram_total_mb: 64_000,
            peak_ram_used_mb: 40_000,
            resource_sampling_seconds: 3,
            runtime_version: "runtime".into(),
            h3_patch_commit: "commit".into(),
            generation_mode: "t2v".into(),
            width: 1344,
            height: 768,
            duration_seconds: 5.0,
            steps: 20,
            model_file: "model.safetensors".into(),
            enabled_plugins: vec![],
            elapsed_seconds: 60.0,
            outcome: "completed".into(),
            error_summary: String::new(),
            output_file: "D:/out.mp4".into(),
        }
    }

    #[test]
    fn saves_and_lists_reports_atomically() {
        let temp = tempfile::tempdir().unwrap();
        let path = save_report(temp.path(), &report()).unwrap();
        assert!(path.is_file());
        assert_eq!(list_reports(temp.path()).unwrap().len(), 1);
    }

    #[test]
    fn rejects_impossible_peak_usage() {
        let mut value = report();
        value.peak_vram_used_mb = 25_000;
        assert!(value.validate().is_err());
    }

    #[test]
    fn anonymous_export_removes_paths_ids_urls_and_errors() {
        let source = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let mut value = report();
        value.runtime_version = "http://127.0.0.1:8188/private".into();
        value.model_file = "D:/models/private/model.safetensors".into();
        value.output_file = "D:/users/name/output/video.mp4".into();
        value.error_summary = "private stack trace".into();
        save_report(source.path(), &value).unwrap();
        let destination = output.path().join("anonymous.json");
        export_anonymous(source.path(), &destination, 123).unwrap();
        let raw = fs::read_to_string(destination).unwrap();
        assert!(raw.contains("model.safetensors"));
        for secret in ["job-1", "127.0.0.1", "D:/users", "D:/models", "private stack"] {
            assert!(!raw.contains(secret));
        }
    }
}
