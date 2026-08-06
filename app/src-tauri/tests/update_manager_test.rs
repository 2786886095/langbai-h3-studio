#[path = "../src/update_manager.rs"]
mod update_manager;
use sha2::{Digest, Sha256};
use std::fs;
use update_manager::*;
#[test]
fn semver_and_channels() {
    assert!(Version::parse("1.0.0").unwrap() > Version::parse("1.0.0-rc.2").unwrap());
    assert!(Version::parse("1.0.0-rc.10").unwrap() > Version::parse("1.0.0-rc.2").unwrap());
    let r=parse_releases(r#"[{"tag_name":"v1.1.0","assets":[]},{"tag_name":"v1.2.0-rc.1","prerelease":true,"assets":[]}]"#).unwrap();
    assert_eq!(
        select_update("1.0.0", &r, UpdateChannel::Stable)
            .unwrap()
            .unwrap()
            .version,
        "v1.1.0"
    );
    assert_eq!(
        select_update("1.0.0", &r, UpdateChannel::PreRelease)
            .unwrap()
            .unwrap()
            .version,
        "v1.2.0-rc.1"
    )
}
#[test]
fn custom_manifest_and_asset() {
    let r=parse_releases(r#"{"releases":[{"version":"0.4.0","assets":[{"name":"Langbai.exe","url":"https://example.invalid/app"}]}]}"#).unwrap();
    assert_eq!(
        select_asset(&r[0], &["Langbai.exe"]).unwrap().download_url,
        "https://example.invalid/app"
    );
    assert!(select_asset(&r[0], &["other.exe"]).is_err())
}
struct Fixture;
impl Ed25519Verifier for Fixture {
    fn verify(&self, k: &[u8; 32], m: &[u8], s: &[u8; 64]) -> bool {
        k == &[7; 32] && m == b"signed fixture" && s == &[9; 64]
    }
}
#[test]
fn hash_signature_and_part_fixture() {
    let root = std::env::temp_dir().join(format!("h3-update-{}", rand::random::<u64>()));
    fs::create_dir_all(&root).unwrap();
    let f = root.join("setup.exe.part");
    fs::write(&f, b"signed fixture").unwrap();
    let hash = format!("{:x}", Sha256::digest(b"signed fixture"));
    verify_sha256(&f, &hash).unwrap();
    assert!(matches!(
        verify_sha256(&f, &"0".repeat(64)),
        Err(UpdateError::HashMismatch { .. })
    ));
    verify_ed25519(&Fixture, &[7; 32], b"signed fixture", &[9; 64]).unwrap();
    assert!(matches!(
        verify_ed25519(&Fixture, &[7; 32], b"tampered", &[9; 64]),
        Err(UpdateError::SignatureMismatch)
    ));
    assert_eq!(part_path(&root.join("setup.exe")).unwrap(), f);
    fs::remove_dir_all(root).unwrap()
}
#[test]
fn atomic_updater_plan() {
    let root = std::env::temp_dir().join(format!("h3-plan-{}", rand::random::<u64>()));
    let path = root.join("pending.json");
    let p = UpdaterPlan {
        version: "0.4.0".into(),
        verified_installer: root.join("setup.exe"),
        sha256: "a".repeat(64),
        restart_executable: root.join("app.exe"),
    };
    write_updater_plan(&path, &p).unwrap();
    assert_eq!(
        serde_json::from_slice::<UpdaterPlan>(&fs::read(&path).unwrap()).unwrap(),
        p
    );
    assert!(!path.with_extension("json.tmp").exists());
    fs::remove_dir_all(root).unwrap()
}
