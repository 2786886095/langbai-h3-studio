# MiniMax H3 ComfyUI API Graph specification

Status: verified against ComfyUI commit
[`e2ab36d933356bc8cd6ecb39c655fe8be75af4e5`](https://github.com/Comfy-Org/ComfyUI/commit/e2ab36d933356bc8cd6ecb39c655fe8be75af4e5)
from PR [#15224](https://github.com/Comfy-Org/ComfyUI/pull/15224), and the two
workflow-template JSON files bundled in this repository. This document describes
the flattened object accepted as `prompt` by `POST /prompt`; the bundled official
files themselves are frontend graphs and must not be posted directly.

## Confirmed H3 node contracts

| `class_type` | Required `inputs` | Optional/dynamic `inputs` | Outputs |
|---|---|---|---|
| `MiniMaxH3ImageToVideo` | `clip`, `vae`, `prompt`, `width`, `height`, `length` | `first_frame`, `last_frame` | 0 `CONDITIONING`, 1 `LATENT` |
| `MiniMaxH3ReferenceToVideo` | `clip`, `vae`, `audio_vae`, `prompt`, `width`, `height`, `length`, `ref_image_size` | `ref_images.ref_image_0`…`8`, `ref_videos.ref_video_0`…`2`, `ref_video_audios.ref_video_audio_0`…`2`, `ref_audios.ref_audio_0`…`2` | 0 `CONDITIONING`, 1 `LATENT` |
| `EmptyMiniMaxH3LatentAV` | `width`, `height`, `length` | — | 0 `LATENT` |
| `MiniMaxH3SigmaShift` | `model`, `shift_video`, `shift_audio` | — | 0 `MODEL` |

The official T2V and Ref2VA templates do **not** use `EmptyMiniMaxH3LatentAV` or
`MiniMaxH3SigmaShift`; their conditioning nodes create the joint AV latent, and
the model is connected directly to `BasicScheduler` and `BasicGuider`.

Autogrow input names above are literal dotted API input keys. ComfyUI's V3 input
layer expands those keys into dictionaries before calling
`MiniMaxH3ReferenceToVideo.execute`.

## Shared executable sampling/decode tail

Use any stable node IDs; the names below are semantic IDs, not official IDs.
Every connection is the Comfy API form `[source_node_id, output_index]`.

| Node | `class_type` | Exact `inputs` |
|---|---|---|
| `unet` | `UNETLoader` | `unet_name`, `weight_dtype: "default"` |
| `clip` | `CLIPLoader` | `clip_name`, `type: "minimax"`, `device: "default"` |
| `video_vae` | `VAELoader` | `vae_name` |
| `audio_vae` | `VAELoader` | `vae_name` |
| `noise` | `RandomNoise` | `noise_seed` |
| `sampler` | `KSamplerSelect` | `sampler_name: "res_multistep"` |
| `scheduler` | `BasicScheduler` | `model: ["unet",0]`, `scheduler: "simple"`, `steps: 20`, `denoise: 1.0` |
| `guider` | `BasicGuider` | `model: ["unet",0]`, `conditioning: ["condition",0]` |
| `sample` | `SamplerCustomAdvanced` | `noise: ["noise",0]`, `guider: ["guider",0]`, `sampler: ["sampler",0]`, `sigmas: ["scheduler",0]`, `latent_image: ["condition",1]` |
| `decode_video` | `VAEDecode` | `samples: ["sample",0]`, `vae: ["video_vae",0]` |
| `decode_audio` | `VAEDecodeAudio` | `samples: ["sample",0]`, `vae: ["audio_vae",0]` |
| `mux` | `CreateVideo` | `images: ["decode_video",0]`, `audio: ["decode_audio",0]`, `fps: 24.0`, `bit_depth: 8` |
| `save` | `SaveVideo` | `video: ["mux",0]`, `filename_prefix: "video/MiniMax_H3"`, `format: "auto"`, `codec: "auto"` |

`SaveVideo.filename_prefix` is relative to ComfyUI's output directory. It is not
the application's arbitrary destination directory. `codec` is the flat V3
dynamic-combo selector; ComfyUI expands it to the dictionary received by the
node implementation.

## T2V / optional first-last-frame graph

Add this conditioning node to the shared tail:

```json
"condition": {
  "class_type": "MiniMaxH3ImageToVideo",
  "inputs": {
    "clip": ["clip", 0],
    "vae": ["video_vae", 0],
    "prompt": "USER_PROMPT",
    "width": 1344,
    "height": 768,
    "length": 124
  }
}
```

For FL2VA, add one or both image loaders and corresponding connections:

```json
"first_image": {"class_type":"LoadImage","inputs":{"image":"JOB/first.png"}},
"last_image":  {"class_type":"LoadImage","inputs":{"image":"JOB/last.png"}}
```

- `condition.inputs.first_frame = ["first_image", 0]`
- `condition.inputs.last_frame = ["last_image", 0]`

Pure T2V must omit both optional keys. The official model is
`minimax_h3_fl2va_pruned_int8_convrot.safetensors` for T2V/FL2VA.

## Ref2VA graph and reference loaders

The conditioning node is:

```json
"condition": {
  "class_type": "MiniMaxH3ReferenceToVideo",
  "inputs": {
    "clip": ["clip", 0],
    "vae": ["video_vae", 0],
    "audio_vae": ["audio_vae", 0],
    "prompt": "Use <Picture 1>, <Video 1> and <Audio 1> ...",
    "width": 1344,
    "height": 768,
    "length": 124,
    "ref_image_size": "match"
  }
}
```

Reference image `N`:

```json
"ref_image_N": {"class_type":"LoadImage","inputs":{"image":"JOB/ref-N.png"}}
```

Connect output 0 to `condition.inputs["ref_images.ref_image_N"]`.

Reference video `N` uses both nodes below because `LoadVideo` returns `VIDEO`,
while the H3 node requires decoded `IMAGE` frames:

```json
"ref_video_N": {"class_type":"LoadVideo","inputs":{"file":"JOB/ref-N.mp4"}},
"ref_video_parts_N": {"class_type":"GetVideoComponents","inputs":{"video":["ref_video_N",0]}}
```

- frames: `condition.inputs["ref_videos.ref_video_N"] = ["ref_video_parts_N",0]`
- paired soundtrack: `condition.inputs["ref_video_audios.ref_video_audio_N"] = ["ref_video_parts_N",1]`

Omit the paired-soundtrack key when the source has no usable audio. A standalone
audio reference uses:

```json
"ref_audio_N": {"class_type":"LoadAudio","inputs":{"audio":"JOB/ref-N.wav"}}
```

Connect output 0 to `condition.inputs["ref_audios.ref_audio_N"]`. The official
Ref2VA diffusion model is
`minimax_h3_ref2va_pruned_int8_convrot.safetensors`.

Limits confirmed by the node schema are 9 images, 3 videos and 3 standalone
audio inputs. Reference videos must contain at least 5 decoded frames. The node
truncates them to the generated frame count and then down to the nearest valid
`17k+5` length. Prompt reference ordinals are one-based even though API suffixes
are zero-based: `ref_image_0` is `<Picture 1>`.

## User-facing parameter mapping

| User value | API destination | Constraint/default from official workflow |
|---|---|---|
| prompt | `condition.prompt` | Ref2VA tags must match reference order |
| width / height | `condition.width`, `condition.height` | multiples of 32; official default 1344×768 |
| duration or frames | `condition.length` | 24 fps, normalize to `17k+5` |
| seed | `noise.noise_seed` | unsigned 64-bit |
| steps | `scheduler.steps` | official default 20 |
| scheduler | `scheduler.scheduler` | official value `simple`; template notes suggest `beta` or `normal` for reference-heavy prompts |
| sampler | `sampler.sampler_name` | official value `res_multistep` |
| diffusion model | `unet.unet_name` | mode-specific filename above |
| text encoder | `clip.clip_name` | `qwen3vl_32b_minimax_h3_nvfp4_awq.safetensors` |
| video VAE | `video_vae.vae_name` | `minimax_h3_video_vae_fp16.safetensors` |
| audio VAE | `audio_vae.vae_name` | `minimax_h3_audio_vae_fp32.safetensors` |
| reference fidelity | `condition.ref_image_size` | `match` or `max`; official default `match` |
| output name | `save.filename_prefix` | relative prefix only |

Frame normalization copied from the official templates:

```text
n = max(5, round(duration_seconds * 24))
length = n + (5 - (n mod 17)) mod 17
```

If the UI already supplies frames, apply the second line to `max(5, frames)`.

There is no mapping in these official graphs for negative prompt, CFG/guidance,
arbitrary FPS, or an attention-acceleration selector. `BasicGuider` has no CFG
input. The generated media is natively 24 fps. These application parameters must
not be silently inserted into unrelated fields.

## Capability gate

At minimum, `/object_info` must contain:

```text
UNETLoader, CLIPLoader, VAELoader, MiniMaxH3ImageToVideo (T2V)
or MiniMaxH3ReferenceToVideo (Ref2VA), RandomNoise, KSamplerSelect,
BasicScheduler, BasicGuider, SamplerCustomAdvanced, VAEDecode,
VAEDecodeAudio, CreateVideo, SaveVideo
```

Add `LoadImage` for image/keyframe input, and `LoadVideo`,
`GetVideoComponents`, or `LoadAudio` according to supplied Ref2VA assets.

## Evidence boundary

- Node IDs and input names were read from the exact ComfyUI commit above.
- Connections and official defaults were reconstructed from the bundled official
  frontend graphs, including the embedded T2V subgraph.
- The Ref2VA template currently demonstrates two image references. Video and
  audio loader wiring is derived from the exact `LoadVideo`,
  `GetVideoComponents`, `LoadAudio`, and H3 node contracts at the pinned commit;
  it should receive an integration smoke test against that ComfyUI revision.
