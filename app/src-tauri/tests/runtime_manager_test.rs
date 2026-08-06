#[path = "../src/runtime_manager.rs"]
mod runtime_manager;

use runtime_manager::*;
use std::{collections::BTreeMap, fs, path::PathBuf};

fn temp_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("h3-runtime-{name}-{}", rand::random::<u64>()))
}

#[test]
fn staged_profile_is_promoted_and_selected() {
    let root = temp_root("activate");
    let manager = RuntimeManager::new(&root);
    let staging = manager.prepare_staging("comfy-0.3.50").unwrap();
    fs::write(staging.join("main.py"), "pass").unwrap();
    let active = manager.activate_staged("comfy-0.3.50").unwrap();
    assert!(active.profile_dir.join("main.py").is_file());
    assert!(!staging.exists());
    assert_eq!(manager.current().unwrap().unwrap().version, "comfy-0.3.50");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_duplicate_update_keeps_previous_current() {
    let root = temp_root("rollback");
    let manager = RuntimeManager::new(&root);
    manager.prepare_staging("v1").unwrap();
    manager.activate_staged("v1").unwrap();
    manager.prepare_staging("v1").unwrap();
    assert!(matches!(
        manager.activate_staged("v1"),
        Err(RuntimeError::VersionAlreadyInstalled(_))
    ));
    assert_eq!(manager.current().unwrap().unwrap().version, "v1");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn launch_is_loopback_only_and_rejects_escape() {
    let root = temp_root("launch");
    let manager = RuntimeManager::new(&root);
    manager.prepare_staging("v1").unwrap();
    manager.activate_staged("v1").unwrap();
    let plan = manager
        .launch_plan("python/python.exe", "ComfyUI/main.py")
        .unwrap();
    assert!(plan.port > 0);
    assert_eq!(&plan.args[1..3], &["--listen", "127.0.0.1"]);
    assert!(plan.endpoint.starts_with("http://127.0.0.1:"));
    assert!(matches!(
        manager.launch_plan("../python.exe", "main.py"),
        Err(RuntimeError::UnsafeExecutable(_))
    ));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn model_yaml_and_lifecycle_plans_are_stable() {
    let mut models = BTreeMap::new();
    models.insert(
        "diffusion_models".into(),
        vec![PathBuf::from(r"D:\Models\diffusion")],
    );
    let yaml = render_extra_model_paths(&PathBuf::from(r"D:\Models"), &models);
    assert!(yaml.contains("base_path: \"D:/Models\""));
    assert!(yaml.contains("- \"D:/Models/diffusion\""));
    let stop = RuntimeManager::stop_plan(42);
    assert_eq!(stop.pid, 42);
    assert!(stop.force_after_timeout);
}

#[test]
fn invalid_versions_cannot_escape_runtime_root() {
    let manager = RuntimeManager::new(temp_root("version"));
    assert!(matches!(
        manager.profile_dir("../outside"),
        Err(RuntimeError::InvalidVersion)
    ));
    assert!(matches!(
        manager.profile_dir("bad/name"),
        Err(RuntimeError::InvalidVersion)
    ));
}
