# 更新记录

## 开发中

### 新增

- 固定到 Comfy-Org MiniMax-H3 提交版本的 FL2VA / Ref2VA 优化模型清单
- 模型中心真实多文件下载、断点续传、实时速度、ETA 与逐文件 SHA-256 校验
- MiniMax H3 Community License 显式确认与本机确认记录
- 已校验文件直接复用，避免重复下载共享的文本编码器和 VAE
- Windows 原生图片、视频、音频多选和目录选择器
- 素材类型、大小、首尾帧角色展示及逐项移除
- 后端素材元数据校验与流式 ComfyUI 上传，避免大型视频经过前端 IPC 或整体载入内存
- 类型化生成状态查询，将 ComfyUI history/queue 归一为排队、运行、完成、失败或未知状态
- 固定记录 ComfyUI H3 预览实现提交、PR、源码包大小及 SHA-256
- 运行环境连接检测改为验证三个 H3 必需节点，不再把基础 ComfyUI 可连接误报为 H3 就绪

### 兼容性说明

- 经源码核对，ComfyUI v0.30.0 基线不包含 H3 节点；当前 H3 支持位于上游 PR #15224，后续由可回滚补丁安装器接入

### 修正

- 将此前演示性质的 38.4 GB 下载条替换为真实约 42.5 GB 清单数据
- 明确 24 GB 显存、64 GB 内存为建议值而非兼容保证

## 0.5.0 - 2026-08-06

### 新增

- ComfyUI v0.30.0 官方 NVIDIA CUDA 13 与 CUDA 12.6 Runtime 清单
- 官方 GitHub 下载地址、文件大小与固定 SHA-256
- Rust 原生安全 7z 解压支持，无需用户安装 7-Zip
- Runtime 断点下载→校验→解压→结构验证→激活完整安装链
- 托管 Runtime 进程真实启动、状态与停止命令
- 新手安装界面、版本说明、真实下载速度、进度与 ETA

### 验证

- 35 项 Rust 测试全部通过
- 前端生产构建通过
- 两份官方 Runtime 清单来自 Comfy-Org v0.30.0 GitHub Release

## 0.4.0 - 2026-08-06

### 新增

- 声明式托管 Runtime ZIP 安装器、SHA-256 校验与 Zip Slip 防护
- Runtime staging 安装、结构验证、激活及进程启动/停止/状态命令
- 官方 Comfy-Org MiniMax-H3 T2V/R2V 原始模板参考资源与固定哈希
- 项目语义工作流适配器注册、能力检查和来源区分
- GitHub Release/自有 Manifest 更新解析与 stable/pre-release 通道
- 更新包 Asset 选择、SHA-256 和 Ed25519 验证接口
- `.part` 更新路径及独立 Updater 原子计划

### 验证

- Rust 单元与集成测试共 35 项通过
- Runtime 安装覆盖成功、错误哈希和路径穿越
- 官方模板参考资源的固定 SHA-256 已验证
- TypeScript/Vite 生产构建通过

## 0.3.0 - 2026-08-06

### 新增

- 托管 ComfyUI 版本化 staging/versions Runtime Profile
- `current.json` 事务切换、失败恢复和上一版本保留
- 仅绑定 127.0.0.1 随机端口的启动计划
- `extra_model_paths.yaml` 安全生成
- 真实 ComfyUI `/prompt`、`/queue`、`/history`、`/interrupt` HTTP 传输层
- multipart 输入素材上传核心
- IPv4/IPv6 回环地址保护及中文网络错误
- 生成记录 SQLite 列表界面
- 输出路径创建与写权限验证界面

### 验证

- Rust 单元与集成测试共 24 项通过
- 本地 mock ComfyUI 覆盖提交、队列、历史、中断、上传、HTTP 失败和超时
- TypeScript/Vite 生产构建通过

## 0.2.0 - 2026-08-06

### 新增

- HTTP Range/If-Range 模型断点续传核心
- `.part` 与 sidecar、实时速度/ETA、SHA-256 校验和同卷原子安装
- FL2VA/Ref2VA 本地模型只读扫描、结构完整性和文件大小报告
- Schema 驱动的 ComfyUI 语义工作流编译与节点能力检查
- 队列、历史和 WebSocket 进度事件解析类型
- SQLite 任务创建、更新、列表及重启持久化
- 创作页提交真实本地任务；模型中心提供本地目录扫描界面

### 验证

- Rust 单元与集成测试共 15 项通过
- 前端 TypeScript/Vite 生产构建通过

## 0.1.0 - 2026-08-06

### 已实现

- Windows Tauri 2 桌面应用壳与 NSIS Setup
- 简体中文创作界面，浅色/深色主题
- 文字、首尾帧、全模态参考原型交互
- NVIDIA GPU、驱动、显存、CPU 和内存检测
- 本机 ComfyUI 地址校验与 `/object_info` 节点探测
- 输出目录创建及写权限验证命令
- MiniMax-H3 模型下载进度交互原型
- PRD、架构文档和 `.h3plugin` 规范草案

### 待实现

- 托管 ComfyUI Runtime 安装、启动和版本回滚
- 真实模型断点下载、哈希校验和本地模型扫描
- H3 工作流编译、提交、WebSocket 进度与任务恢复
- MiniMax API、Credential Manager 与更新签名
- 代表性社区加速插件的实机兼容验证
