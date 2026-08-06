# Langbai H3 Studio

## v0.10.0：可爱二次元界面与可恢复 AutoDL 部署

- 银紫赛博与可爱二次元结合的简体中文界面，加入狼尊主题同人吉祥物和应用图标
- AutoDL 独立部署目录、空间预检、后台模型下载、速度／进度／ETA、断点续传、取消与精确回滚
- Windows 本地 Studio 通过严格 `known_hosts` 校验的 OpenSSH 隧道连接远端 ComfyUI，不公开 8188 端口
- 默认把视频保存到安装程序同级 `output`，也可固定自定义目录或每次生成前询问
- KJNodes H3 SageAttention 可安装并真实插入 H3 API Graph；未经 H3 验证的插件不会标记为兼容

Windows x64 Preview 安装包：[`Langbai-H3-Studio_0.10.0_x64-setup.exe`](release/v0.10.0/Langbai-H3-Studio_0.10.0_x64-setup.exe)

SHA-256：`B9E6BBAB672D8253539BEAC2FED7E868FAFE355A98DB1910DD2891713703453D`

> 当前限制：8–10GB 显存档仍是实验目标；真实 AutoDL/RTX 5090 与 8GB 生成基准尚待对应硬件验证。市场模板 `253/678` 的内部内容不是本项目控制范围，Studio 使用隔离目录避免覆盖它。

## v0.8.0：8GB 实验档与远程 RTX 5090

- 8–10GB NVIDIA 显卡提供未验证的极低显存档：CPU VAE、608×352 起步和极限卸载诊断
- 通过 Windows OpenSSH 隧道安全连接租用 RTX 5090 或其他远程 GPU 工作站
- 首批真实托管社区节点目录：KJNodes H3 SageAttention 与 FunPack H3 兼容扩展
- KJNodes H3 SageAttention 可真实插入生成 API Graph，并单独记录兼容性结果

8GB 目前是实验目标，不是稳定兼容保证。远端方案要求 ComfyUI 只监听远端
`127.0.0.1`，不应开放公网 8188。参见 [8GB 实验档](docs/H3_8GB_EXPERIMENT.md)
和 [远程 GPU 连接](docs/REMOTE_GPU.md)。

## v0.7.0 云端 Hailuo API

除本地 MiniMax-H3 / ComfyUI 外，Studio 现已提供独立的 **MiniMax 云端 Hailuo API** 模式：

- Hailuo-2.3：文字或首帧生成视频
- Hailuo-02：首尾帧生成视频
- API Key 保存到 Windows Credential Manager，应用界面不回显
- 云端任务自动轮询并将结果保存到用户选择的本地目录

云端 Hailuo API 与开源 MiniMax-H3 是两套不同的执行引擎。云端服务按 MiniMax
账户实际用量计费，本地 H3 仍由用户显卡和 ComfyUI 执行。详见
[云端 API 接入规范](docs/MINIMAX_CLOUD_API.md)。

面向 Windows NVIDIA 单卡用户的 MiniMax-H3 视频生成桌面软件，以清晰的创作表单和智能预设替代 ComfyUI 节点画布，并保留本地模型、外接 ComfyUI、MiniMax API 与社区加速插件扩展能力。

> 当前阶段：Windows MVP 开发版。已提供 Tauri 桌面壳、硬件检测、托管 ComfyUI 安装与进程管理、H3 优化模型真实断点下载、Windows 原生素材选择与流式上传、输出目录验证、本机 ComfyUI 能力探测和 SQLite 任务记录；官方 UI Graph 到可提交 API Graph 的可靠转换及完整生成执行继续开发中。

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

发布步骤：将 NSIS 产物重命名为仓库约定的连字符文件名，生成同名 `.sha256`，提交到 `release/v<版本>/`，再把这两个文件上传到同名 GitHub Preview Release。应用内更新器读取 GitHub Release JSON，并在启动安装包前强制校验对应 SHA-256。

当前 Preview 尚未进行商业代码签名，Windows 可能显示“未知发布者”；文件完整性以仓库和 GitHub Release 同时公布的 SHA-256 为准。

## 文档

- [产品需求文档](docs/PRD.md)
- [技术架构](docs/ARCHITECTURE.md)
- [社区插件规范](docs/PLUGIN_SPEC.md)
- [设计系统](design-system/langbai-h3-studio/MASTER.md)

## 事实边界

MiniMax-H3 当前公开的 H3-Base 可进行本地 768p 音视频生成；官方 H3-Context-IR 与 H3-Regenerate-2K 未随初始开源版本发布，因此完整官方 2K 流程需要用户配置 MiniMax API。16–24GB 单卡方案依赖量化、卸载、分块和社区优化，具体组合必须通过真实硬件测试后再标记“已验证”。

- [AutoDL 隔离部署](docs/AUTODL_DEPLOYMENT.md)
