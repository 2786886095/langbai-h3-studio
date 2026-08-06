#[path = "../src/comfy.rs"]
mod comfy;

use comfy::*;
use serde_json::json;
use std::collections::BTreeSet;

fn request() -> GenerateRequest {
    GenerateRequest {
        prompt: "海边日落".into(),
        negative_prompt: "抖动".into(),
        assets: vec![Asset::Image {
            path: "start.png".into(),
            role: AssetRole::StartFrame,
        }],
        width: 1280,
        height: 720,
        frames: 97,
        fps: 24.0,
        steps: 20,
        guidance: 6.0,
        seed: 42,
        model: "h3.safetensors".into(),
        output_directory: "D:/video".into(),
        acceleration: Some("sage_attention".into()),
    }
}

fn template() -> WorkflowTemplate {
    WorkflowTemplate::from_json(&json!({
        "id":"h3-i2v", "version":1, "title":"首帧生成", "requiredNodeTypes":["H3Sampler"],
        "workflow":{"11":{"class_type":"TextEncode","inputs":{"text":""}},"29":{"class_type":"H3Sampler","inputs":{"seed":0}},"42":{"class_type":"LoadImage","inputs":{"image":""}}},
        "bindings":[
            {"role":"prompt","node":"11","input":"text","required":true},
            {"role":"seed","node":"29","input":"seed","required":true},
            {"role":"start_frame","node":"42","input":"image","required":true}
        ]
    }).to_string()).unwrap()
}

#[test]
fn semantic_bindings_build_prompt_without_leaking_node_ids_in_plan() {
    let probe = ProbeResult {
        reachable: true,
        node_types: BTreeSet::from(["H3Sampler".into()]),
    };
    let plan = template().build_plan(&request(), &probe).unwrap();
    let ui_json = serde_json::to_string(&plan).unwrap();
    assert!(
        !ui_json.contains("\"11\"") && !ui_json.contains("\"29\"") && !ui_json.contains("\"42\"")
    );
    let body = plan.prompt_body("desktop-client");
    assert_eq!(body.prompt["11"]["inputs"]["text"], "海边日落");
    assert_eq!(body.prompt["29"]["inputs"]["seed"], 42);
    assert_eq!(body.prompt["42"]["inputs"]["image"], "start.png");
}

#[test]
fn rejects_missing_probe_capability() {
    let err = template()
        .build_plan(
            &request(),
            &ProbeResult {
                reachable: true,
                node_types: BTreeSet::new(),
            },
        )
        .unwrap_err();
    assert_eq!(
        err,
        AdapterError::MissingCapability(vec!["H3Sampler".into()])
    );
}

#[test]
fn parses_queue_history_and_progress_without_node_ids() {
    let queue = QueueSnapshot::parse(
        &json!({"queue_running":[[3,"p1",{},{}]],"queue_pending":[[4,"p2",{},{}]]}),
    )
    .unwrap();
    assert_eq!(queue.running[0].prompt_id, "p1");
    let history = parse_history(&json!({"p1":{"outputs":{"99":{"videos":[{"filename":"result.mp4","subfolder":"h3"}]}},"status":{"completed":true}}}), "p1").unwrap().unwrap();
    assert_eq!(history.outputs[0].filename, "result.mp4");
    assert!(!serde_json::to_string(&history).unwrap().contains("99"));
    assert_eq!(
        parse_progress_event(
            r#"{"type":"progress","data":{"prompt_id":"p1","value":4,"max":20,"node":"29"}}"#
        )
        .unwrap(),
        ExecutionEvent::Progress {
            prompt_id: Some("p1".into()),
            value: 4,
            max: 20
        }
    );
}

#[test]
fn parses_prompt_response_and_object_info() {
    let response =
        PromptResponse::parse(json!({"prompt_id":"abc","number":7,"node_errors":{}})).unwrap();
    assert_eq!(response.prompt_id, "abc");
    let probe = ProbeResult::from_object_info(&json!({"H3Sampler":{},"VAEDecode":{}})).unwrap();
    assert!(probe.check(&["H3Sampler".into()]).compatible);
}
