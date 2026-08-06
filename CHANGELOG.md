# 更新记录

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
