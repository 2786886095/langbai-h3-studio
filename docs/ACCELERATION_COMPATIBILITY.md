# H3 加速与低显存兼容性

更新日期：2026-08-06

## 已确认

1. H3 PR `#15224` 的官方工作流使用标准 `UNETLoader`、`BasicScheduler`、`BasicGuider`、`SamplerCustomAdvanced` 及 H3 条件节点，没有 SageAttention、Block Swap、GGUF 或第三方加速节点。
2. 当前 ComfyUI 已具有 Dynamic VRAM 和权重动态卸载机制。Studio 的“自动动态显存”保持上游默认行为。
3. `--lowvram` 与 `--novram` 是 ComfyUI 原生命令行档位。Studio 只通过固定枚举生成参数，不接受用户拼接任意启动命令。
4. 目前未找到经过 H3 PR 对应提交和真实 H3 权重验证的 SageAttention、Block Swap 或 GGUF 社区实现。因此首批正式内置档位使用上游原生显存管理，不把相邻视频模型的加速结论套用到 H3。

## Studio 档位

| 档位 | 启动参数 | 使用建议 | 状态 |
|---|---|---|---|
| 自动动态显存 | 上游默认 | 16–24GB 首选 | 待真实矩阵 |
| 保守低显存 | `--lowvram --reserve-vram 1.5` | 12–16GB 实验 | 待真实矩阵 |
| 最小显存 | `--novram --reserve-vram 1.5` | 仅用于诊断，预计很慢 | 待真实矩阵 |

档位只改变 Runtime 内存策略，不修改 H3 工作流语义、模型精度或输出参数。

## 社区插件准入

社区加速适配包只有同时满足以下条件才标记“兼容”：

- `.h3plugin` 包通过声明式文件和 SHA-256 校验；
- 当前 Studio 版本和 Windows/NVIDIA 目标匹配；
- `/object_info` 包含插件声明的全部节点；
- 与已启用插件没有冲突；
- 用固定 H3 模型、工作流和硬件记录完成输出、耗时和峰值显存。

在只有安装成功、没有生成报告时，状态只能是“社区”或“本地”，不能标为“已验证”。

## 上游来源

- [ComfyUI](https://github.com/Comfy-Org/ComfyUI)
- [ComfyUI Dynamic VRAM announcement](https://github.com/Comfy-Org/ComfyUI/discussions/12699)
- [MiniMax-H3 ComfyUI PR #15224](https://github.com/Comfy-Org/ComfyUI/pull/15224)
