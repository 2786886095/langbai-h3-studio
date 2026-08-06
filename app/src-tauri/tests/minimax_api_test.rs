#[path = "../src/minimax_api.rs"]
mod minimax_api;
use minimax_api::*;
fn req(model: &str) -> HailuoVideoRequest {
    HailuoVideoRequest {
        model: model.into(),
        prompt: "电影感海边日出".into(),
        first_frame_image: None,
        last_frame_image: None,
        duration: 6,
        resolution: "1080P".into(),
        prompt_optimizer: true,
    }
}
#[test]
fn separates_cloud_hailuo_from_local_h3() {
    assert!(validate_request(&req("MiniMax-Hailuo-2.3")).is_ok());
    assert!(validate_request(&req("MiniMax-Hailuo-02")).is_ok());
    assert!(validate_request(&req("MiniMax-H3")).is_err())
}
#[test]
fn api_origin_is_pinned() {
    assert!(
        validate_api_url(
            &reqwest::Url::parse("https://api.minimax.io/v1/video_generation").unwrap()
        )
        .is_ok()
    );
    assert!(
        validate_api_url(
            &reqwest::Url::parse("https://api.minimax.io.evil.test/v1/video_generation").unwrap()
        )
        .is_err()
    );
    assert!(
        validate_api_url(
            &reqwest::Url::parse("http://api.minimax.io/v1/video_generation").unwrap()
        )
        .is_err()
    )
}
#[test]
fn download_requires_public_https() {
    assert!(validate_download_url("https://cdn.example.test/video.mp4").is_ok());
    assert!(validate_download_url("http://cdn.example.test/video.mp4").is_err());
    assert!(validate_download_url("https://127.0.0.1/video.mp4").is_err())
}
#[test]
fn validates_constraints_and_output_name() {
    let mut r = req("MiniMax-Hailuo-2.3");
    r.duration = 10;
    assert!(validate_request(&r).is_err());
    let p = safe_output_path(std::path::Path::new("out"), Some("../bad.mp4"), "123").unwrap();
    assert_eq!(p, std::path::Path::new("out/hailuo-123.mp4"))
}
