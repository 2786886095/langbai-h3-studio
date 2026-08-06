# Langbai H3 Studio

面向 Windows NVIDIA 单卡用户的 MiniMax-H3 视频生成桌面软件，以清晰的创作表单和智能预设替代 ComfyUI 节点画布，并保留本地模型、外接 ComfyUI、MiniMax API 与社区加速插件扩展能力。

> 当前阶段：Windows MVP 开发版。已提供 Tauri 桌面壳、硬件检测、托管 ComfyUI 安装与进程管理、H3 优化模型真实断点下载、输出目录验证、本机 ComfyUI 能力探测和 SQLite 任务记录；端到端素材上传与生成执行继续开发中。

## 已确认范围

- Windows 首发，简体中文，默认浅色并支持深色
- 重点适配 NVIDIA 16–24GB 单卡，能力以实测兼容矩阵为准
- 内置并管理独立 ComfyUI，同时可连接已有 ComfyUI
- 纯本地 H3-Base 与用户主动启用的 MiniMax API 双模式
- 文字、图片、视频、音频输入及输出路径选择
- 模型断点下载、实时速度、ETA、校验与本地模型复用
- Schema 驱动的参数解释、预设与渐进披露
- 社区加速节点通过公开 `.h3plugin` 适配协议接入
- Setup、GitHub Releases、应用内签名更新与回滚
- Studio 自有代码 Apache-2.0；捆绑依赖、模型和插件各自遵循其许可证

## 运行与构建

```powershell
cd app
npm install
npm run dev
```

构建验证：`cd app; npm run build`

Windows 桌面开发：`cd app; npm run desktop:dev`

Windows Setup 构建需从 Visual Studio x64 Developer Command Prompt 执行：`cd app; npm run desktop:build`

当前验证安装包位于 `release/v0.5.0/Langbai-H3-Studio_0.5.0_x64-setup.exe`。

## 文档

- [产品需求文档](docs/PRD.md)
- [技术架构](docs/ARCHITECTURE.md)
- [社区插件规范](docs/PLUGIN_SPEC.md)
- [设计系统](design-system/langbai-h3-studio/MASTER.md)

## 事实边界

MiniMax-H3 当前公开的 H3-Base 可进行本地 768p 音视频生成；官方 H3-Context-IR 与 H3-Regenerate-2K 未随初始开源版本发布，因此完整官方 2K 流程需要用户配置 MiniMax API。16–24GB 单卡方案依赖量化、卸载、分块和社区优化，具体组合必须通过真实硬件测试后再标记“已验证”。
