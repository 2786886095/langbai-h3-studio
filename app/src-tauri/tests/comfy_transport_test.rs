#[path = "../src/comfy_transport.rs"]
mod comfy_transport;

use comfy_transport::{ComfyTransport, validate_loopback_url};
use serde_json::json;
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};

fn server(status: &str, body: &'static str) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}/", listener.local_addr().unwrap());
    let status = status.to_owned();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut buf = [0u8; 16384];
        let count = stream.read(&mut buf).unwrap();
        let request = String::from_utf8_lossy(&buf[..count]).into_owned();
        write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        request
    });
    (address, handle)
}

#[test]
fn only_accepts_loopback_base_urls() {
    assert!(validate_loopback_url("http://127.0.0.1:8188").is_ok());
    assert!(validate_loopback_url("http://[::1]:8188/").is_ok());
    assert!(validate_loopback_url("http://localhost:8188/").is_ok());
    assert!(validate_loopback_url("http://192.168.1.2:8188").is_err());
    assert!(validate_loopback_url("file:///tmp/comfy").is_err());
    assert!(validate_loopback_url("http://localhost:8188/api").is_err());
}

#[test]
fn posts_prompt_without_exposing_node_errors() {
    let (url, handle) = server(
        "200 OK",
        r#"{"prompt_id":"job-1","number":3,"node_errors":{"42":{"errors":[]}}}"#,
    );
    let transport = ComfyTransport::new(&url, Duration::from_secs(2)).unwrap();
    let receipt = tauri::async_runtime::block_on(
        transport.post_prompt(json!({"1":{"class_type":"Test"}}), "client-a"),
    )
    .unwrap();
    assert_eq!(receipt.prompt_id, "job-1");
    assert_eq!(receipt.queue_number, Some(3));
    assert_eq!(receipt.validation_error_count, 1);
    let request = handle.join().unwrap();
    assert!(request.starts_with("POST /prompt "));
    assert!(request.contains("client-a"));
}

#[test]
fn supports_queue_history_interrupt_and_upload() {
    let (url, h) = server("200 OK", r#"{"queue_running":[],"queue_pending":[]}"#);
    let t = ComfyTransport::new(&url, Duration::from_secs(2)).unwrap();
    tauri::async_runtime::block_on(t.get_queue()).unwrap();
    assert!(h.join().unwrap().starts_with("GET /queue "));

    let (url, h) = server("200 OK", r#"{"job-1":{"status":{"completed":true}}}"#);
    let t = ComfyTransport::new(&url, Duration::from_secs(2)).unwrap();
    tauri::async_runtime::block_on(t.get_history("job-1")).unwrap();
    assert!(h.join().unwrap().starts_with("GET /history/job-1 "));

    let (url, h) = server("200 OK", "{}");
    let t = ComfyTransport::new(&url, Duration::from_secs(2)).unwrap();
    tauri::async_runtime::block_on(t.interrupt()).unwrap();
    assert!(h.join().unwrap().starts_with("POST /interrupt "));

    let (url, h) = server(
        "200 OK",
        r#"{"name":"frame.png","subfolder":"refs","type":"input"}"#,
    );
    let t = ComfyTransport::new(&url, Duration::from_secs(2)).unwrap();
    let result = tauri::async_runtime::block_on(t.upload_input(
        "frame.png",
        vec![1, 2, 3],
        "image/png",
        Some("refs"),
        true,
    ))
    .unwrap();
    assert_eq!(result.name, "frame.png");
    let request = h.join().unwrap();
    assert!(request.starts_with("POST /upload/image "));
    assert!(request.contains("multipart/form-data"));
    assert!(request.contains("frame.png"));
}

#[test]
fn translates_http_and_timeout_failures_to_chinese() {
    let (url, h) = server("500 Internal Server Error", r#"{"node_id":"secret-node"}"#);
    let t = ComfyTransport::new(&url, Duration::from_secs(2)).unwrap();
    let error = tauri::async_runtime::block_on(t.get_queue()).unwrap_err();
    h.join().unwrap();
    assert!(error.contains("读取任务队列失败"));
    assert!(!error.contains("secret-node"));

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let sleeper = thread::spawn(move || {
        let _ = listener.accept();
        thread::sleep(Duration::from_millis(250));
    });
    let t = ComfyTransport::new(&url, Duration::from_millis(50)).unwrap();
    let error = tauri::async_runtime::block_on(t.get_queue()).unwrap_err();
    sleeper.join().unwrap();
    assert!(error.contains("超时"));
}
