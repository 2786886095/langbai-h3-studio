#[path = "../src/download.rs"]
mod download;
#[path = "../src/runtime_manager.rs"]
mod runtime_manager;
fn runtime_root() -> std::path::PathBuf {
    std::env::temp_dir()
}
#[path = "../src/managed_nodes.rs"]
mod managed_nodes;
use managed_nodes::*;
use std::{fs, io::Write};
use zip::write::SimpleFileOptions;
fn archive(
    root: &std::path::Path,
    item: &ManagedNodeCatalogItem,
    traverse: bool,
) -> std::path::PathBuf {
    let p = root.join("node.zip");
    let mut z = zip::ZipWriter::new(fs::File::create(&p).unwrap());
    let n = if traverse {
        format!("repo-{}/../bad.py", item.commit)
    } else {
        format!("repo-{}/nodes.py", item.commit)
    };
    z.start_file(n, SimpleFileOptions::default()).unwrap();
    z.write_all(b"ok").unwrap();
    z.finish().unwrap();
    p
}
fn runtime(root: &std::path::Path) {
    fs::create_dir_all(root.join("ComfyUI_windows_portable/python_embeded")).unwrap();
    fs::write(
        root.join("ComfyUI_windows_portable/python_embeded/python.exe"),
        b"",
    )
    .unwrap()
}
#[test]
fn catalog_distinguishes_compatibility_and_acceleration() {
    let c = catalog();
    assert_eq!(c[0].category, "h3-community");
    assert!(!c[0].description.contains("加速插件"));
    assert_eq!(c[1].category, "h3-acceleration");
    assert_eq!(
        c[1].required_nodes,
        ["MiniMaxH3MemoryEfficientSageAttentionPatch"]
    );
    assert!(c[1].installable);
    assert!(
        c.iter()
            .all(|x| x.evidence_level == "source-supported" && x.experimental)
    );
}
#[test]
fn install_and_exact_uninstall_are_transactional() {
    let t = tempfile::tempdir().unwrap();
    runtime(t.path());
    let mut item = catalog().remove(0);
    let p = archive(t.path(), &item, false);
    item.archive_size = Some(fs::metadata(&p).unwrap().len());
    item.archive_sha256 = Some(file_sha(&p));
    let s = install_archive(t.path(), &item, &p, |_, _| Ok(())).unwrap();
    assert!(s.installed && s.restart_required && !s.verified);
    let s = uninstall_from(t.path(), &item).unwrap();
    assert!(!s.installed);
}
#[test]
fn traversal_archive_is_rejected_and_stage_removed() {
    let t = tempfile::tempdir().unwrap();
    runtime(t.path());
    let mut item = catalog().remove(0);
    let p = archive(t.path(), &item, true);
    item.archive_size = Some(fs::metadata(&p).unwrap().len());
    item.archive_sha256 = Some(file_sha(&p));
    assert!(install_archive(t.path(), &item, &p, |_, _| Ok(())).is_err());
    assert!(
        !t.path()
            .join("ComfyUI_windows_portable/ComfyUI/custom_nodes/ComfyUI-FunPack")
            .exists()
    );
}
#[test]
fn pip_failure_rolls_back() {
    let t = tempfile::tempdir().unwrap();
    runtime(t.path());
    let mut item = catalog().remove(0);
    let p = t.path().join("node.zip");
    let mut z = zip::ZipWriter::new(fs::File::create(&p).unwrap());
    z.start_file(
        format!("repo-{}/requirements.txt", item.commit),
        SimpleFileOptions::default(),
    )
    .unwrap();
    z.write_all(b"dep").unwrap();
    z.finish().unwrap();
    item.archive_size = Some(fs::metadata(&p).unwrap().len());
    item.archive_sha256 = Some(file_sha(&p));
    assert!(install_archive(t.path(), &item, &p, |_, _| Err("pip failed".into())).is_err());
    assert!(
        !t.path()
            .join("ComfyUI_windows_portable/ComfyUI/custom_nodes/ComfyUI-FunPack")
            .exists()
    );
}
fn file_sha(p: &std::path::Path) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(fs::read(p).unwrap()))
}
