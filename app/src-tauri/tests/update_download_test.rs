#[path = "../src/update_manager.rs"]
mod update_manager;

use update_manager::*;

const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[test]
fn stable_uses_inline_manifest_hash() {
    let json = format!(
        r#"[
          {{"tag_name":"v0.7.0","assets":[{{"name":"Langbai-H3-Studio_0.7.0_x64-setup.exe","browser_download_url":"https://example/setup","sha256":"{HASH}"}}]}},
          {{"tag_name":"v0.8.0-rc.1","prerelease":true,"assets":[{{"name":"Langbai-H3-Studio_0.8.0_x64-setup.exe","browser_download_url":"https://example/rc","sha256":"{HASH}"}}]}}
        ]"#
    );
    let candidate = select_windows_candidate("0.6.0", &json, UpdateChannel::Stable)
        .unwrap()
        .unwrap();
    assert_eq!(candidate.version, "v0.7.0");
    assert_eq!(candidate.sha256.as_deref(), Some(HASH));
    assert_eq!(candidate.sha256_url, None);
}

#[test]
fn prerelease_uses_matching_sha256_asset() {
    let json = r#"[{"tag_name":"v0.8.0-rc.1","prerelease":true,"assets":[
      {"name":"Langbai-H3-Studio_0.8.0_x64-setup.exe","browser_download_url":"https://example/setup"},
      {"name":"Langbai-H3-Studio_0.8.0_x64-setup.exe.sha256","browser_download_url":"https://example/hash"}
    ]}]"#;
    let candidate = select_windows_candidate("0.6.0", json, UpdateChannel::PreRelease)
        .unwrap()
        .unwrap();
    assert!(candidate.pre_release);
    assert_eq!(candidate.sha256, None);
    assert_eq!(
        candidate.sha256_url.as_deref(),
        Some("https://example/hash")
    );
    assert!(
        serde_json::to_string(&candidate)
            .unwrap()
            .contains("sha256Url")
    );
}

#[test]
fn release_without_hash_is_rejected() {
    let json = r#"[{"tag_name":"v0.7.0","assets":[{"name":"Langbai-H3-Studio_0.7.0_x64-setup.exe","browser_download_url":"https://example/setup"}]}]"#;
    assert!(matches!(
        select_windows_candidate("0.6.0", json, UpdateChannel::Stable),
        Err(UpdateError::MissingHash(_))
    ));
}

#[test]
fn malicious_release_filename_is_rejected() {
    let json = format!(
        r#"[{{"tag_name":"v0.7.0","assets":[{{"name":"../Windows-x64-setup.exe","browser_download_url":"https://example/setup","sha256":"{HASH}"}}]}}]"#
    );
    assert!(matches!(
        select_windows_candidate("0.6.0", &json, UpdateChannel::Stable),
        Err(UpdateError::UnsafeFileName(_))
    ));
    assert!(safe_file_name(r"C:\\temp\\setup.exe").is_err());
}

#[test]
fn updater_plan_requires_absolute_matching_installer() {
    let candidate = UpdateCandidate {
        version: "v0.7.0".into(),
        file_name: "Langbai-H3-Studio_0.7.0_x64-setup.exe".into(),
        download_url: "https://example/setup".into(),
        sha256: Some(HASH.into()),
        sha256_url: None,
        pre_release: false,
    };
    assert!(matches!(
        updater_plan_for_candidate(&candidate, "setup.exe".into(), HASH, "app.exe".into()),
        Err(UpdateError::UnsafePath)
    ));
}
