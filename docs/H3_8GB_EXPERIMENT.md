# MiniMax-H3 8GB 极低显存实验档

截至 2026-08-06，没有一手证据证明完整 MiniMax-H3 已在 8GB NVIDIA 显卡上稳定
完成推理。本档位是待验证目标，不代表兼容保证。

## 首轮建议

- 至少 64GB 系统内存，并保留充足 SSD / 系统管理页面文件
- T2V、608×352、约 5 秒、20 步、batch 1
- 官方 INT8 ConvRot DiT 与 NVFP4-AWQ 文本编码器
- 默认 Dynamic VRAM / NVIDIA Async Offload
- `--cpu-vae --reserve-vram 1.0`
- 不同时叠加 SageAttention、FlashAttention 和 torch.compile

默认方案仍 OOM 时，可使用“极限卸载诊断”：

```text
--novram --cpu-vae --disable-smart-memory --reserve-vram 1.0
```

这会显著增加系统内存传输，可能非常慢。页面文件只是防止提交内存不足的兜底，
不是等价于物理内存的加速方案。

## 通过门槛

同一块 8GB NVIDIA 显卡连续完成至少 3 次 T2V，包括采样、双 VAE 解码、视频保存；
记录显卡、驱动、峰值显存、峰值内存、耗时、分辨率、帧数和加速插件。达到此前，
界面始终显示“未验证极限模式”。

