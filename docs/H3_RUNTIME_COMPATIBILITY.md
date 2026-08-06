# MiniMax-H3 Runtime 兼容性

## 已确认状态（2026-08-06）

- ComfyUI `v0.30.0` 的源码提交 `b1693ecba9f5b65f8c80ab36b195ab963ec92413` 不包含 `MiniMaxH3ImageToVideo`、`MiniMaxH3ReferenceToVideo` 或 `MiniMaxH3SigmaShift`。
- Comfy-Org 官方工作流模板指向 ComfyUI PR `#15224`。H3 节点目前存在于该 PR 的提交 `e2ab36d933356bc8cd6ecb39c655fe8be75af4e5`，尚不能把基础 v0.30.0 Runtime 单独标记为“H3 可用”。
- Studio 固定记录该提交的源码 ZIP、大小和 SHA-256。后续补丁安装器只接受这份固定清单，并在运行时通过 `/object_info` 再次验证必需节点。

## 产品行为

1. 基础 Runtime 仍用于提供隔离的 Python、CUDA/PyTorch 与 ComfyUI 环境。
2. 未发现三个 H3 必需节点时，界面显示“需要 H3 预览补丁”，不显示“环境就绪”。
3. 补丁安装必须具备下载校验、事务覆盖、依赖安装日志和回滚。
4. 上游 PR 合并并进入正式 ComfyUI Release 后，迁移到正式版本，不再默认安装预览补丁。

## 来源

- ComfyUI v0.30.0：<https://github.com/Comfy-Org/ComfyUI/tree/v0.30.0>
- H3 PR：<https://github.com/Comfy-Org/ComfyUI/pull/15224>
- 官方工作流模板：<https://github.com/Comfy-Org/workflow_templates>
