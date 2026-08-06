#[path = "../src/h3_patch.rs"]
mod h3_patch;
use h3_patch::*;
use sha2::{Digest, Sha256};
use std::{fs, fs::File, io::Write, path::Path};
fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let f = File::create(path).unwrap();
    let mut z = zip::ZipWriter::new(f);
    let o = zip::write::SimpleFileOptions::default();
    for (n, b) in entries {
        z.start_file(*n, o).unwrap();
        z.write_all(b).unwrap();
    }
    z.finish().unwrap();
}
fn hash(p: &Path) -> String {
    format!("{:x}", Sha256::digest(fs::read(p).unwrap()))
}
fn manifest(h: String) -> H3PatchManifest {
    H3PatchManifest {
        id: "h3-test".into(),
        commit: "abc123".into(),
        sha256: h,
        archive_format: "zip".into(),
    }
}
fn profile(p: &Path) {
    fs::create_dir_all(p.join("ComfyUI_windows_portable/ComfyUI")).unwrap()
}
#[test]
fn installs_source_and_preserves_protected() {
    let t = tempfile::tempdir().unwrap();
    profile(t.path());
    let c = t.path().join("ComfyUI_windows_portable/ComfyUI");
    fs::write(c.join("main.py"), "old").unwrap();
    fs::create_dir(c.join("models")).unwrap();
    fs::write(c.join("models/keep.bin"), "keep").unwrap();
    let z = t.path().join("p.zip");
    make_zip(
        &z,
        &[
            ("ComfyUI-abc123/main.py", b"new"),
            ("ComfyUI-abc123/comfy/h3.py", b"h3"),
        ],
    );
    let r = install_h3_patch(&manifest(hash(&z)), &z, t.path()).unwrap();
    assert_eq!(fs::read_to_string(c.join("main.py")).unwrap(), "new");
    assert!(c.join("comfy/h3.py").is_file());
    assert_eq!(
        fs::read_to_string(c.join("models/keep.bin")).unwrap(),
        "keep"
    );
    assert!(r.receipt_path.is_file());
}
#[test]
fn rejects_path_traversal() {
    let t = tempfile::tempdir().unwrap();
    profile(t.path());
    let z = t.path().join("p.zip");
    make_zip(&z, &[("ComfyUI-abc123/../../escape.py", b"bad")]);
    let e = install_h3_patch(&manifest(hash(&z)), &z, t.path()).unwrap_err();
    assert!(e.contains("path traversal") || e.contains("unsafe"));
    assert!(!t.path().join("escape.py").exists());
}
#[test]
fn rejects_bad_hash() {
    let t = tempfile::tempdir().unwrap();
    profile(t.path());
    let z = t.path().join("p.zip");
    make_zip(&z, &[("ComfyUI-abc123/main.py", b"new")]);
    let e = install_h3_patch(&manifest("00".repeat(32)), &z, t.path()).unwrap_err();
    assert!(e.contains("SHA-256"));
}
#[test]
fn rollback_restores_and_removes() {
    let t = tempfile::tempdir().unwrap();
    profile(t.path());
    let c = t.path().join("ComfyUI_windows_portable/ComfyUI");
    fs::write(c.join("main.py"), "old").unwrap();
    let z = t.path().join("p.zip");
    make_zip(
        &z,
        &[
            ("ComfyUI-abc123/main.py", b"new"),
            ("ComfyUI-abc123/new.py", b"added"),
        ],
    );
    let r = install_h3_patch(&manifest(hash(&z)), &z, t.path()).unwrap();
    rollback_h3_patch(&r.receipt_path).unwrap();
    assert_eq!(fs::read_to_string(c.join("main.py")).unwrap(), "old");
    assert!(!c.join("new.py").exists());
    assert!(!r.receipt_path.exists());
}
