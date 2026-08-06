# MiniMax 云端视频 API 接入规范

## 定位

云端模式接入的是 MiniMax 平台提供的 **Hailuo 视频生成 API**，不是开源的
MiniMax-H3 权重，也不经过本地 ComfyUI。界面、任务记录和诊断信息必须明确标记
“云端 Hailuo API”，避免将两种执行引擎混为一谈。

| 执行引擎 | 模型来源 | 计算位置 | 主要用途 |
| --- | --- | --- | --- |
| 本地 H3 | MiniMax-H3 开源权重 | 用户电脑 / 本地 ComfyUI | 本地文字、首尾帧、全模态参考生成 |
| 云端 Hailuo API | MiniMax 平台服务 | MiniMax 云端 | 无需下载权重的文字或图片生成视频 |

## 官方端点

- 创建任务：`POST https://api.minimax.io/v1/video_generation`
- 查询任务：`GET https://api.minimax.io/v1/query/video_generation?task_id=...`
- 获取文件：`GET https://api.minimax.io/v1/files/retrieve?file_id=...`
- 鉴权：`Authorization: Bearer <API_KEY>`

任务状态映射：

| 官方状态 | Studio 状态 |
| --- | --- |
| `Preparing` / `Queueing` | `queued` |
| `Processing` | `running` |
| `Success` | `completed` |
| `Fail` | `failed` |

轮询间隔默认 10 秒。只有任务成功并获得 `file_id` 后才请求文件下载地址。

## MVP 能力约束

### 文字生成视频

- 模型：`MiniMax-Hailuo-2.3`
- 768P：6 秒或 10 秒
- 1080P：6 秒
- 提示词最长 2,000 字符

### 首帧生成视频

- 模型：`MiniMax-Hailuo-2.3`
- 本地图片编码为 Data URL 后作为 `first_frame_image`
- 图片小于 20 MB，格式限 JPG、PNG、WebP
- 短边大于 300 px，宽高比位于 2:5 至 5:2

### 首尾帧生成视频

- 模型：`MiniMax-Hailuo-02`
- 分别提交 `first_frame_image` 和 `last_frame_image`
- 沿用官方首尾帧接口支持的时长和清晰度，不把本地 H3 参数直接映射到云端

全模态参考模式只属于本地 H3。云端模式不显示采样步数、显存策略、ComfyUI
加速插件等无效参数。

## 凭据与网络边界

1. API Key 只写入 Windows Credential Manager，不进入 `localStorage`、SQLite、日志、
   任务 JSON 或诊断报告。
2. 前端只能读取“已配置/未配置”状态，后端永远不返回 Key 明文。
3. API 请求固定发送到 `https://api.minimax.io`。
4. 文件下载地址必须为 HTTPS；保存前仍执行本地输出目录验证。
5. 错误消息不得包含请求头或完整响应调试转储，以防第三方响应意外回显凭据。

## 任务记录

任务记录使用 `backend = minimax-cloud`，保存以下非敏感字段：

- 任务 ID、模型、模式、清晰度、时长
- 提示词（由用户现有本地任务策略决定）
- 官方状态、错误码、文件 ID
- 最终本地保存路径

云端 API 为计费服务；提交按钮附近必须显示明确的费用提醒，并提供官方价格页入口。

## 官方资料

- [视频生成指南](https://platform.minimax.io/docs/guides/video-generation)
- [文字生成视频 API](https://platform.minimax.io/docs/api-reference/video-generation-t2v)
- [图片生成视频 API](https://platform.minimax.io/docs/api-reference/video-generation-i2v)
- [首尾帧生成视频 API](https://platform.minimax.io/docs/api-reference/video-generation-fl2v)
- [查询任务](https://platform.minimax.io/docs/api-reference/video-generation-query)
- [下载文件](https://platform.minimax.io/docs/api-reference/video-generation-download)
- [按量计费](https://platform.minimax.io/docs/guides/pricing-paygo)
