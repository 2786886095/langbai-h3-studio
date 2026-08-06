#[path = "../src/plugin_manager.rs"]
mod plugin_manager;
use plugin_manager::*;
use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use zip::write::SimpleFileOptions;

fn pkg(root: &Path, id: &str, conflicts: &[&str], extra: Option<(&str, &str)>) -> PathBuf {
    let path = root.join(format!("{id}.h3plugin"));
    let file = fs::File::create(&path).unwrap();
    let mut z = zip::ZipWriter::new(file);
    let o = SimpleFileOptions::default();
    let manifest = serde_json::json!({"schemaVersion":1,"id":id,"name":"测试插件","version":"1.2.0","targets":{"studio":">=0.1 <1","comfyui":">=0.8","os":["windows"],"gpu":["nvidia"]},"provides":["attention.fast"],"requires":{"nodes":[{"class":"RequiredNode","version":">=1"}],"models":[]},"conflicts":conflicts,"artifacts":[{"url":"https://example.invalid/a","sha256":"a".repeat(64),"size":1}],"workflows":[{"capability":"generate.text_to_av","template":"workflows/t2av.json","bindings":"bindings/t2av.json"}],"parameters":"parameters.schema.json"});
    for (n, v) in [
        ("manifest.json", manifest.to_string()),
        ("workflows/t2av.json", "{}".into()),
        ("bindings/t2av.json", "{}".into()),
        ("parameters.schema.json", "{}".into()),
    ] {
        z.start_file(n, o).unwrap();
        z.write_all(v.as_bytes()).unwrap()
    }
    if let Some((n, v)) = extra {
        z.start_file(n, o).unwrap();
        z.write_all(v.as_bytes()).unwrap()
    }
    z.finish().unwrap();
    path
}
fn nodes() -> BTreeSet<String> {
    BTreeSet::from(["RequiredNode".into()])
}

#[test]
fn valid_package_installs_and_uninstalls() {
    let t = tempfile::tempdir().unwrap();
    let p = pkg(t.path(), "org.example.fast", &[], None);
    let i = inspect_package(&p, None).unwrap();
    assert_eq!(i.manifest.schema_version, 1);
    let m = PluginManager::new(t.path().join("runtime"));
    m.install(&p, None, "0.6.0", &nodes()).unwrap();
    assert!(
        m.root()
            .join("org.example.fast/1.2.0/manifest.json")
            .is_file()
    );
    m.uninstall("org.example.fast").unwrap();
    assert!(
        !m.read_lock()
            .unwrap()
            .plugins
            .contains_key("org.example.fast")
    );
}
#[test]
fn path_traversal_is_rejected() {
    let t = tempfile::tempdir().unwrap();
    let p = pkg(
        t.path(),
        "org.example.path",
        &[],
        Some(("../outside.json", "{}")),
    );
    assert!(inspect_package(&p, None).is_err());
}
#[test]
fn executable_or_script_is_rejected() {
    let t = tempfile::tempdir().unwrap();
    let p = pkg(
        t.path(),
        "org.example.script",
        &[],
        Some(("install.py", "print(1)")),
    );
    assert!(inspect_package(&p, None).is_err());
}
#[test]
fn enabled_conflict_blocks_install() {
    let t = tempfile::tempdir().unwrap();
    let m = PluginManager::new(t.path().join("runtime"));
    let a = pkg(t.path(), "org.example.a", &[], None);
    m.install(&a, None, "0.6.0", &nodes()).unwrap();
    let b = pkg(t.path(), "org.example.b", &["org.example.a"], None);
    assert!(matches!(
        m.install(&b, None, "0.6.0", &nodes()),
        Err(PluginError::Conflict(_))
    ));
}
#[test]
fn lock_enable_switch_is_durable() {
    let t = tempfile::tempdir().unwrap();
    let m = PluginManager::new(t.path().join("runtime"));
    let p = pkg(t.path(), "org.example.switch", &[], None);
    m.install(&p, None, "0.6.0", &nodes()).unwrap();
    m.set_enabled("org.example.switch", false).unwrap();
    assert!(!m.read_lock().unwrap().plugins["org.example.switch"].enabled);
    m.set_enabled("org.example.switch", true).unwrap();
    assert!(m.read_lock().unwrap().plugins["org.example.switch"].enabled);
}
