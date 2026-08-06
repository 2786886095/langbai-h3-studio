#[path = "../src/ssh_tunnel.rs"]
mod ssh_tunnel;
use ssh_tunnel::*;
use std::{fs, path::PathBuf};
fn config(root: &std::path::Path) -> SshTunnelConfig {
    let key = root.join("id_ed25519");
    let hosts = root.join("known_hosts");
    fs::write(&key, "key").unwrap();
    fs::write(&hosts, "host ssh-ed25519 AAAA").unwrap();
    SshTunnelConfig {
        host: "gpu.example.com".into(),
        user: "studio".into(),
        port: 22,
        identity_file: key,
        known_hosts_file: hosts,
        remote_comfy_port: 8188,
    }
}
#[test]
fn command_plan_is_strict_and_loopback_only() {
    let t = tempfile::tempdir().unwrap();
    let p = build_launch_plan(
        &config(t.path()),
        PathBuf::from(r"C:\Windows\System32\OpenSSH\ssh.exe"),
        43123,
    )
    .unwrap();
    let joined = p.args.join(" ");
    for required in [
        "-F NUL",
        "-N",
        "-T",
        "BatchMode=yes",
        "PasswordAuthentication=no",
        "KbdInteractiveAuthentication=no",
        "IdentitiesOnly=yes",
        "ForwardAgent=no",
        "ClearAllForwardings=yes",
        "ExitOnForwardFailure=yes",
        "StrictHostKeyChecking=yes",
        "GlobalKnownHostsFile=NUL",
        "ServerAliveInterval=15",
        "ConnectTimeout=10",
        "127.0.0.1:43123:127.0.0.1:8188",
    ] {
        assert!(joined.contains(required), "missing {required}")
    }
    assert!(!p.args.iter().any(|a| a == "-g"));
    assert_eq!(p.endpoint, "http://127.0.0.1:43123")
}
#[test]
fn rejects_malicious_inputs() {
    let t = tempfile::tempdir().unwrap();
    let mut c = config(t.path());
    c.host = "-oProxyCommand=evil".into();
    assert!(build_launch_plan(&c, "ssh.exe".into(), 1234).is_err());
    let mut c = config(t.path());
    c.user = "a@evil".into();
    assert!(build_launch_plan(&c, "ssh.exe".into(), 1234).is_err());
    let mut c = config(t.path());
    c.port = 0;
    assert!(build_launch_plan(&c, "ssh.exe".into(), 1234).is_err());
    let mut c = config(t.path());
    c.identity_file = t.path().into();
    assert!(build_launch_plan(&c, "ssh.exe".into(), 1234).is_err())
}
#[test]
fn allocated_endpoint_is_loopback() {
    let p = allocate_loopback_port().unwrap();
    assert!(p > 0);
    let url = reqwest::Url::parse(&format!("http://127.0.0.1:{p}")).unwrap();
    assert!(url.host_str() == Some("127.0.0.1"))
}
#[test]
fn classifies_limited_errors() {
    assert_eq!(
        classify_ssh_error("Host key verification failed").0,
        "host_key_mismatch"
    );
    assert_eq!(
        classify_ssh_error("Permission denied (publickey)").0,
        "authentication_failed"
    );
    assert_eq!(
        classify_ssh_error("open failed: administratively prohibited").0,
        "forwarding_denied"
    );
    assert_eq!(
        classify_ssh_error("bind: Address already in use").0,
        "local_port_busy"
    )
}
