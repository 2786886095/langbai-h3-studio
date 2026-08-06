use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRecord {
    pub id: String,
    pub name: String,
    pub status: String,
    pub stage: String,
    pub progress: f64,
    pub request_json: String,
    pub plan_json: Option<String>,
    pub backend_id: String,
    pub output_path: Option<String>,
    pub error_summary: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewJob {
    pub name: String,
    pub request_json: String,
    pub backend_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobPatch {
    pub id: String,
    pub status: String,
    pub stage: String,
    pub progress: f64,
    pub plan_json: Option<String>,
    pub output_path: Option<String>,
    pub error_summary: Option<String>,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn database_path() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("LangbaiH3Studio").join("studio.db")
}

fn open(path: &Path) -> Result<Connection, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建数据目录失败：{e}"))?;
    }
    let connection = Connection::open(path).map_err(|e| format!("打开任务数据库失败：{e}"))?;
    connection
        .execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS jobs (
               id TEXT PRIMARY KEY,
               name TEXT NOT NULL,
               status TEXT NOT NULL,
               stage TEXT NOT NULL,
               progress REAL NOT NULL,
               request_json TEXT NOT NULL,
               plan_json TEXT,
               backend_id TEXT NOT NULL,
               output_path TEXT,
               error_summary TEXT,
               created_at INTEGER NOT NULL,
               updated_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS jobs_updated_at ON jobs(updated_at DESC);",
        )
        .map_err(|e| format!("初始化任务数据库失败：{e}"))?;
    Ok(connection)
}

fn create_at(path: &Path, input: NewJob) -> Result<JobRecord, String> {
    serde_json::from_str::<serde_json::Value>(&input.request_json)
        .map_err(|_| "任务请求不是有效 JSON".to_string())?;
    let timestamp = now();
    let id = format!("job-{timestamp}-{:08x}", rand::random::<u32>());
    let record = JobRecord {
        id,
        name: input.name.trim().chars().take(120).collect(),
        status: "queued".into(),
        stage: "等待中".into(),
        progress: 0.0,
        request_json: input.request_json,
        plan_json: None,
        backend_id: input.backend_id,
        output_path: None,
        error_summary: None,
        created_at: timestamp,
        updated_at: timestamp,
    };
    let connection = open(path)?;
    connection
        .execute(
            "INSERT INTO jobs VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                record.id,
                record.name,
                record.status,
                record.stage,
                record.progress,
                record.request_json,
                record.plan_json,
                record.backend_id,
                record.output_path,
                record.error_summary,
                record.created_at,
                record.updated_at
            ],
        )
        .map_err(|e| format!("保存任务失败：{e}"))?;
    Ok(record)
}

fn list_at(path: &Path, limit: u32) -> Result<Vec<JobRecord>, String> {
    let connection = open(path)?;
    let mut statement = connection
        .prepare("SELECT id,name,status,stage,progress,request_json,plan_json,backend_id,output_path,error_summary,created_at,updated_at FROM jobs ORDER BY updated_at DESC LIMIT ?1")
        .map_err(|e| format!("查询任务失败：{e}"))?;
    let rows = statement
        .query_map([limit.clamp(1, 500)], |row| {
            Ok(JobRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                status: row.get(2)?,
                stage: row.get(3)?,
                progress: row.get(4)?,
                request_json: row.get(5)?,
                plan_json: row.get(6)?,
                backend_id: row.get(7)?,
                output_path: row.get(8)?,
                error_summary: row.get(9)?,
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
            })
        })
        .map_err(|e| format!("读取任务失败：{e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("解析任务失败：{e}"))
}

fn patch_at(path: &Path, patch: JobPatch) -> Result<(), String> {
    if !(0.0..=1.0).contains(&patch.progress) {
        return Err("任务进度必须在 0 到 1 之间".into());
    }
    let connection = open(path)?;
    let changed = connection.execute(
        "UPDATE jobs SET status=?2,stage=?3,progress=?4,plan_json=?5,output_path=?6,error_summary=?7,updated_at=?8 WHERE id=?1",
        params![patch.id,patch.status,patch.stage,patch.progress,patch.plan_json,patch.output_path,patch.error_summary,now()],
    ).map_err(|e| format!("更新任务失败：{e}"))?;
    if changed == 0 {
        return Err("任务不存在".into());
    }
    Ok(())
}

#[tauri::command]
pub fn create_job(input: NewJob) -> Result<JobRecord, String> {
    create_at(&database_path(), input)
}

#[tauri::command]
pub fn list_jobs(limit: Option<u32>) -> Result<Vec<JobRecord>, String> {
    list_at(&database_path(), limit.unwrap_or(100))
}

#[tauri::command]
pub fn update_job(patch: JobPatch) -> Result<(), String> {
    patch_at(&database_path(), patch)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> PathBuf {
        std::env::temp_dir().join(format!("langbai-job-test-{}.db", rand::random::<u64>()))
    }

    #[test]
    fn creates_lists_and_updates_jobs() {
        let path = temp_db();
        let created = create_at(
            &path,
            NewJob {
                name: "测试任务".into(),
                request_json: "{\"mode\":\"text_to_av\"}".into(),
                backend_id: "managed-comfy".into(),
            },
        )
        .unwrap();
        assert_eq!(list_at(&path, 10).unwrap().len(), 1);
        patch_at(
            &path,
            JobPatch {
                id: created.id,
                status: "running".into(),
                stage: "推理".into(),
                progress: 0.25,
                plan_json: None,
                output_path: None,
                error_summary: None,
            },
        )
        .unwrap();
        assert_eq!(list_at(&path, 10).unwrap()[0].progress, 0.25);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_invalid_request_json() {
        let result = create_at(
            &temp_db(),
            NewJob {
                name: "坏任务".into(),
                request_json: "not-json".into(),
                backend_id: "test".into(),
            },
        );
        assert!(result.is_err());
    }
}
