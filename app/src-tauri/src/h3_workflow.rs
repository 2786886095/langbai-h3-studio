use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum H3Mode {
    T2v,
    Fl2va,
    Ref2va,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum H3AssetKind {
    Image,
    Video,
    Audio,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum H3AssetRole {
    StartFrame,
    EndFrame,
    Reference,
    MotionReference,
    AudioReference,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UploadedAsset {
    pub remote_path: String,
    pub kind: H3AssetKind,
    pub role: H3AssetRole,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct H3WorkflowRequest {
    pub mode: H3Mode,
    pub prompt: String,
    pub width: u32,
    pub height: u32,
    pub duration_seconds: f32,
    pub seed: u64,
    pub steps: u32,
    #[serde(default = "default_ref_size")]
    pub reference_image_size: String,
    #[serde(default)]
    pub assets: Vec<UploadedAsset>,
    pub filename_prefix: String,
    #[serde(default)]
    pub acceleration: H3Acceleration,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum H3Acceleration {
    #[default]
    Native,
    KJH3SageAttention,
}

fn default_ref_size() -> String {
    "match".into()
}

pub fn normalized_frames(duration_seconds: f32) -> Result<u32, String> {
    if !duration_seconds.is_finite() || !(0.2..=15.0).contains(&duration_seconds) {
        return Err("视频时长必须在 0.2–15 秒之间".into());
    }
    let base = (duration_seconds * 24.0).round().max(5.0) as u32;
    Ok(base + (5 + 17 - base % 17) % 17)
}

fn connection(node: &str, output: u8) -> Value {
    json!([node, output])
}

fn node(class_type: &str, inputs: Value) -> Value {
    json!({"class_type":class_type,"inputs":inputs})
}

fn validate_request(request: &H3WorkflowRequest) -> Result<u32, String> {
    if request.prompt.trim().is_empty() {
        return Err("请填写视频描述".into());
    }
    if request.width < 32
        || request.height < 32
        || request.width % 32 != 0
        || request.height % 32 != 0
    {
        return Err("宽度和高度必须是不小于 32 的 32 倍数".into());
    }
    if !(1..=100).contains(&request.steps) {
        return Err("采样步数必须在 1–100 之间".into());
    }
    if request.filename_prefix.is_empty()
        || request.filename_prefix.contains("..")
        || request.filename_prefix.starts_with(['/', '\\'])
    {
        return Err("输出文件前缀无效".into());
    }
    if request.assets.iter().any(|a| {
        a.remote_path.is_empty()
            || a.remote_path.contains("..")
            || a.remote_path.starts_with(['/', '\\'])
    }) {
        return Err("ComfyUI 素材路径无效".into());
    }
    match request.mode {
        H3Mode::T2v if !request.assets.is_empty() => {
            return Err("文生视频模式不接收参考素材".into());
        }
        H3Mode::Fl2va if request.assets.is_empty() => {
            return Err("首尾帧模式至少需要一张图片".into());
        }
        H3Mode::Fl2va
            if request.assets.iter().any(|a| {
                a.kind != H3AssetKind::Image
                    || !matches!(a.role, H3AssetRole::StartFrame | H3AssetRole::EndFrame)
            }) =>
        {
            return Err("首尾帧模式只接收首帧或尾帧图片".into());
        }
        H3Mode::Ref2va if request.assets.is_empty() => {
            return Err("全模态参考模式至少需要一个素材".into());
        }
        _ => {}
    }
    normalized_frames(request.duration_seconds)
}

pub fn build_h3_prompt(request: &H3WorkflowRequest) -> Result<Value, String> {
    let frames = validate_request(request)?;
    let mut graph = Map::new();
    let unet_name = if request.mode == H3Mode::Ref2va {
        "minimax_h3_ref2va_pruned_int8_convrot.safetensors"
    } else {
        "minimax_h3_fl2va_pruned_int8_convrot.safetensors"
    };
    graph.insert(
        "unet".into(),
        node(
            "UNETLoader",
            json!({"unet_name":unet_name,"weight_dtype":"default"}),
        ),
    );
    let model_node = match request.acceleration {
        H3Acceleration::Native => "unet",
        H3Acceleration::KJH3SageAttention => {
            graph.insert(
                "h3_sage_attention".into(),
                node(
                    "MiniMaxH3MemoryEfficientSageAttentionPatch",
                    json!({"model":connection("unet",0)}),
                ),
            );
            "h3_sage_attention"
        }
    };
    graph.insert("clip".into(), node("CLIPLoader", json!({"clip_name":"qwen3vl_32b_minimax_h3_nvfp4_awq.safetensors","type":"minimax","device":"default"})));
    graph.insert(
        "video_vae".into(),
        node(
            "VAELoader",
            json!({"vae_name":"minimax_h3_video_vae_fp16.safetensors"}),
        ),
    );
    graph.insert(
        "audio_vae".into(),
        node(
            "VAELoader",
            json!({"vae_name":"minimax_h3_audio_vae_fp32.safetensors"}),
        ),
    );

    let mut condition = Map::new();
    condition.insert("clip".into(), connection("clip", 0));
    condition.insert("vae".into(), connection("video_vae", 0));
    condition.insert("prompt".into(), json!(request.prompt.trim()));
    condition.insert("width".into(), json!(request.width));
    condition.insert("height".into(), json!(request.height));
    condition.insert("length".into(), json!(frames));

    match request.mode {
        H3Mode::T2v | H3Mode::Fl2va => {
            for asset in &request.assets {
                let (id, input) = match asset.role {
                    H3AssetRole::StartFrame => ("first_image", "first_frame"),
                    H3AssetRole::EndFrame => ("last_image", "last_frame"),
                    _ => return Err("首尾帧素材角色无效".into()),
                };
                if graph.contains_key(id) {
                    return Err("首帧或尾帧不能重复".into());
                }
                graph.insert(
                    id.into(),
                    node("LoadImage", json!({"image":asset.remote_path})),
                );
                condition.insert(input.into(), connection(id, 0));
            }
            graph.insert(
                "condition".into(),
                node("MiniMaxH3ImageToVideo", Value::Object(condition)),
            );
        }
        H3Mode::Ref2va => {
            condition.insert("audio_vae".into(), connection("audio_vae", 0));
            condition.insert("ref_image_size".into(), json!(request.reference_image_size));
            let (mut images, mut videos, mut audios) = (0usize, 0usize, 0usize);
            for asset in &request.assets {
                match asset.kind {
                    H3AssetKind::Image => {
                        if images >= 9 {
                            return Err("参考图片最多 9 张".into());
                        }
                        let id = format!("ref_image_{images}");
                        graph.insert(
                            id.clone(),
                            node("LoadImage", json!({"image":asset.remote_path})),
                        );
                        condition
                            .insert(format!("ref_images.ref_image_{images}"), connection(&id, 0));
                        images += 1;
                    }
                    H3AssetKind::Video => {
                        if videos >= 3 {
                            return Err("参考视频最多 3 个".into());
                        }
                        let load = format!("ref_video_{videos}");
                        let parts = format!("ref_video_parts_{videos}");
                        graph.insert(
                            load.clone(),
                            node("LoadVideo", json!({"file":asset.remote_path})),
                        );
                        graph.insert(
                            parts.clone(),
                            node("GetVideoComponents", json!({"video":connection(&load,0)})),
                        );
                        condition.insert(
                            format!("ref_videos.ref_video_{videos}"),
                            connection(&parts, 0),
                        );
                        condition.insert(
                            format!("ref_video_audios.ref_video_audio_{videos}"),
                            connection(&parts, 1),
                        );
                        videos += 1;
                    }
                    H3AssetKind::Audio => {
                        if audios >= 3 {
                            return Err("参考音频最多 3 个".into());
                        }
                        let id = format!("ref_audio_{audios}");
                        graph.insert(
                            id.clone(),
                            node("LoadAudio", json!({"audio":asset.remote_path})),
                        );
                        condition
                            .insert(format!("ref_audios.ref_audio_{audios}"), connection(&id, 0));
                        audios += 1;
                    }
                }
            }
            graph.insert(
                "condition".into(),
                node("MiniMaxH3ReferenceToVideo", Value::Object(condition)),
            );
        }
    }

    graph.insert(
        "noise".into(),
        node("RandomNoise", json!({"noise_seed":request.seed})),
    );
    graph.insert(
        "sampler".into(),
        node("KSamplerSelect", json!({"sampler_name":"res_multistep"})),
    );
    graph.insert("scheduler".into(), node("BasicScheduler", json!({"model":connection(model_node,0),"scheduler":"simple","steps":request.steps,"denoise":1.0})));
    graph.insert(
        "guider".into(),
        node(
            "BasicGuider",
            json!({"model":connection(model_node,0),"conditioning":connection("condition",0)}),
        ),
    );
    graph.insert("sample".into(), node("SamplerCustomAdvanced", json!({"noise":connection("noise",0),"guider":connection("guider",0),"sampler":connection("sampler",0),"sigmas":connection("scheduler",0),"latent_image":connection("condition",1)})));
    graph.insert(
        "decode_video".into(),
        node(
            "VAEDecode",
            json!({"samples":connection("sample",0),"vae":connection("video_vae",0)}),
        ),
    );
    graph.insert(
        "decode_audio".into(),
        node(
            "VAEDecodeAudio",
            json!({"samples":connection("sample",0),"vae":connection("audio_vae",0)}),
        ),
    );
    graph.insert("mux".into(), node("CreateVideo", json!({"images":connection("decode_video",0),"audio":connection("decode_audio",0),"fps":24.0,"bit_depth":8})));
    graph.insert("save".into(), node("SaveVideo", json!({"video":connection("mux",0),"filename_prefix":request.filename_prefix,"format":"auto","codec":"auto"})));
    Ok(Value::Object(graph))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn base(mode: H3Mode) -> H3WorkflowRequest {
        H3WorkflowRequest {
            mode,
            prompt: "镜头缓慢推进".into(),
            width: 1344,
            height: 768,
            duration_seconds: 5.0,
            seed: 7,
            steps: 20,
            reference_image_size: "match".into(),
            assets: vec![],
            filename_prefix: "video/job-1".into(),
            acceleration: H3Acceleration::Native,
        }
    }
    #[test]
    fn frame_grid_matches_h3_contract() {
        assert_eq!(normalized_frames(5.0).unwrap(), 124);
        assert_eq!(normalized_frames(1.0).unwrap(), 39);
    }
    #[test]
    fn t2v_uses_real_h3_nodes_without_cfg() {
        let g = build_h3_prompt(&base(H3Mode::T2v)).unwrap();
        assert_eq!(g["condition"]["class_type"], "MiniMaxH3ImageToVideo");
        assert!(g["guider"]["inputs"].get("cfg").is_none());
        assert_eq!(g["mux"]["inputs"]["fps"], 24.0);
    }
    #[test]
    fn fl2va_maps_first_frame() {
        let mut r = base(H3Mode::Fl2va);
        r.assets.push(UploadedAsset {
            remote_path: "job/a.png".into(),
            kind: H3AssetKind::Image,
            role: H3AssetRole::StartFrame,
        });
        let g = build_h3_prompt(&r).unwrap();
        assert_eq!(
            g["condition"]["inputs"]["first_frame"],
            json!(["first_image", 0])
        );
    }
    #[test]
    fn ref2va_maps_dotted_autogrow_inputs() {
        let mut r = base(H3Mode::Ref2va);
        r.assets.push(UploadedAsset {
            remote_path: "job/a.mp4".into(),
            kind: H3AssetKind::Video,
            role: H3AssetRole::MotionReference,
        });
        let g = build_h3_prompt(&r).unwrap();
        assert_eq!(
            g["condition"]["inputs"]["ref_videos.ref_video_0"],
            json!(["ref_video_parts_0", 0])
        );
        assert_eq!(
            g["condition"]["inputs"]["ref_video_audios.ref_video_audio_0"],
            json!(["ref_video_parts_0", 1])
        );
    }

    #[test]
    fn h3_sage_patch_is_inserted_between_loader_and_consumers() {
        let mut r = base(H3Mode::T2v);
        r.acceleration = H3Acceleration::KJH3SageAttention;
        let g = build_h3_prompt(&r).unwrap();
        assert_eq!(
            g["h3_sage_attention"]["class_type"],
            "MiniMaxH3MemoryEfficientSageAttentionPatch"
        );
        assert_eq!(
            g["h3_sage_attention"]["inputs"]["model"],
            json!(["unet", 0])
        );
        assert_eq!(
            g["scheduler"]["inputs"]["model"],
            json!(["h3_sage_attention", 0])
        );
        assert_eq!(
            g["guider"]["inputs"]["model"],
            json!(["h3_sage_attention", 0])
        );
    }
}
