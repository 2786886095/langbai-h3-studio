#[path = "../src/runtime_installer.rs"]
mod runtime_installer;

use runtime_installer::{ArchiveFormat, InstallPhase, RuntimeManifest, install_local_archive};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Write, path::Path};

fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let file = File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    for (name, contents) in entries {
        zip.start_file(*name, options).unwrap();
        zip.write_all(contents).unwrap();
    }
    zip.finish().unwrap();
}

fn sha256(path: &Path) -> String {
    format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
}

fn manifest(hash: String) -> RuntimeManifest {
    RuntimeManifest {
        version: "0.1.0".into(),
        url: "https://example.invalid/runtime.zip".into(),
        sha256: hash,
        archive_format: ArchiveFormat::Zip,
        expected_files: vec!["ComfyUI/main.py".into()],
    }
}

#[test]
fn installs_valid_runtime_into_staging_and_reports_phases() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("runtime.zip");
    write_zip(
        &archive,
        &[
            ("python/python.exe", b"stub"),
            ("ComfyUI/main.py", b"print('ok')"),
        ],
    );
    let mut phases = Vec::new();
    let result = install_local_archive(&manifest(sha256(&archive)), &archive, temp.path(), |p| {
        phases.push(p.phase)
    })
    .unwrap();
    assert!(result.staging_path.join("python").is_dir());
    assert!(result.staging_path.join("ComfyUI/main.py").is_file());
    assert_eq!(phases.first(), Some(&InstallPhase::Preparing));
    assert_eq!(phases.last(), Some(&InstallPhase::Completed));
    assert!(phases.contains(&InstallPhase::Extracting));
}

#[test]
fn rejects_hash_mismatch_without_installing() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("runtime.zip");
    write_zip(
        &archive,
        &[("python/python.exe", b"stub"), ("ComfyUI/main.py", b"ok")],
    );
    let error = install_local_archive(&manifest("00".repeat(32)), &archive, temp.path(), |_| {})
        .unwrap_err();
    assert!(error.contains("SHA-256 校验失败"));
    assert!(!temp.path().join("staging/0.1.0").exists());
}

#[test]
fn rejects_zip_slip_path() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("runtime.zip");
    write_zip(
        &archive,
        &[
            ("../escaped.txt", b"bad"),
            ("python/python.exe", b"stub"),
            ("ComfyUI/main.py", b"ok"),
        ],
    );
    let error = install_local_archive(&manifest(sha256(&archive)), &archive, temp.path(), |_| {})
        .unwrap_err();
    assert!(error.contains("不安全路径"));
    assert!(!temp.path().join("escaped.txt").exists());
}
