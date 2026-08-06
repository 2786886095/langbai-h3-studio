use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleFile {
    pub relative_path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelBundle {
    pub id: String,
    pub name: String,
    pub variant: String,
    pub revision: String,
    pub license: String,
    pub license_url: String,
    pub recommended_vram_gb: u32,
    pub recommended_ram_gb: u32,
    pub files: Vec<BundleFile>,
}

impl ModelBundle {
    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|file| file.size).sum()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.revision.len() != 40 || !self.revision.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("模型 revision 必须固定为 40 位 commit hash".into());
        }
        if self.files.is_empty() {
            return Err("模型清单没有文件".into());
        }
        for file in &self.files {
            if file.size == 0
                || file.sha256.len() != 64
                || !file.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(format!("模型文件校验信息无效：{}", file.relative_path));
            }
            let path = std::path::Path::new(&file.relative_path);
            if path.is_absolute()
                || path.components().any(|c| {
                    matches!(
                        c,
                        std::path::Component::ParentDir
                            | std::path::Component::RootDir
                            | std::path::Component::Prefix(_)
                    )
                })
            {
                return Err("模型相对路径不安全".into());
            }
        }
        Ok(())
    }

    pub fn download_url(&self, file: &BundleFile) -> String {
        format!(
            "https://huggingface.co/Comfy-Org/MiniMax-H3/resolve/{}/{}",
            self.revision, file.relative_path
        )
    }
}

pub fn builtins() -> Result<Vec<ModelBundle>, String> {
    let values = [
        include_str!("../resources/models/h3-t2v-int8.json"),
        include_str!("../resources/models/h3-ref2va-int8.json"),
    ];
    values
        .into_iter()
        .map(|source| {
            let bundle: ModelBundle =
                serde_json::from_str(source).map_err(|e| format!("模型清单解析失败：{e}"))?;
            bundle.validate()?;
            Ok(bundle)
        })
        .collect()
}

pub fn select(id: &str) -> Result<ModelBundle, String> {
    builtins()?
        .into_iter()
        .find(|item| item.id == id)
        .ok_or_else(|| "未知模型清单".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_manifests_are_pinned_and_valid() {
        for b in builtins().unwrap() {
            assert_eq!(b.revision.len(), 40);
            assert!(b.total_size() > 40_000_000_000);
            for f in &b.files {
                assert!(b.download_url(f).contains(&b.revision));
            }
        }
    }
    #[test]
    fn rejects_unsafe_paths() {
        let mut b = builtins().unwrap().remove(0);
        b.files[0].relative_path = "../escape".into();
        assert!(b.validate().is_err());
    }
}
