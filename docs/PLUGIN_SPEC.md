# 社区加速插件规范（草案 v0.1）

社区 custom nodes 没有稳定统一 ABI。Studio 的 `.h3plugin` 是声明式适配包，可引用上游节点，但不等同于节点仓库。

## 包结构

```text
example.h3plugin
├── manifest.json
├── workflows/t2av.json
├── bindings/t2av.json
├── parameters.schema.json
└── benchmarks/nvidia-24gb.json
```

## Manifest 示例

```json
{
  "schemaVersion": 1,
  "id": "org.example.fast-attention",
  "name": "示例注意力加速",
  "version": "1.2.0",
  "publisher": { "name": "Example", "keyId": "KEY_ID" },
  "targets": { "studio": ">=0.1 <1", "comfyui": ">=0.8 <0.10", "os": ["windows"], "gpu": ["nvidia"] },
  "provides": ["attention.fast", "vram.low"],
  "requires": { "nodes": [{ "class": "ExampleNode", "version": ">=1" }], "models": ["minimax-h3-base"] },
  "conflicts": ["org.other.attention"],
  "artifacts": [{ "url": "URL", "sha256": "SHA256", "size": 123 }],
  "workflows": [{ "capability": "generate.text_to_av", "template": "workflows/t2av.json", "bindings": "bindings/t2av.json" }],
  "parameters": "parameters.schema.json",
  "license": "Apache-2.0"
}
```

稳定能力包括 `generate.text_to_av`、`generate.first_last_to_av`、`generate.reference_to_av`、`audio.native`、`resolution.768p`、`precision.fp8`、`offload.cpu`、`attention.sparse`、`compile.torch`、`vae.tiling`。

## 协商与信任

硬件扫描＋后端 probe＋依赖/冲突＋工作流静态检查＋`/object_info` 对照形成候选计划，按兼容性、显存可运行性、稳定级别、实测耗时排序。参数转换是受限声明式表达式，不执行任意 JS/Python。

兼容级别为“已验证、社区、本地”。首版使用 Stable/Experimental 等版本化 Runtime Profile；变更前保存 lockfile 与目录快照，健康检查失败则回滚。Studio 默认不执行 manifest 中任意安装脚本；签名和哈希可降低风险，但不构成代码沙箱。
