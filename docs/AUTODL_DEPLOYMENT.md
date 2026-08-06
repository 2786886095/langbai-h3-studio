# AutoDL 隔离部署协议

## 目标

Langbai H3 Studio 运行在 Windows，本协议负责准备 AutoDL/Linux 侧的 ComfyUI 与 MiniMax-H3 隔离目录。远端 ComfyUI 只监听 `127.0.0.1`，Studio 通过 Windows OpenSSH 隧道连接，避免把 8188 暴露到公网。

## 固定目录

```text
/workspace/LangbaiH3Studio/ComfyUI
/workspace/LangbaiH3Studio/models
/workspace/LangbaiH3Studio/state/deployments/<deployment-id>
```

Studio 不覆盖市场镜像已有的 ComfyUI、Python 或模型目录。已存在的远端 ComfyUI 可以继续通过“连接已有 ComfyUI”使用。

## 用户流程

1. 从 AutoDL 控制台复制 SSH 登录命令。
2. 选择已核验服务器指纹的 `known_hosts` 和 SSH 私钥。
3. 检查 GPU、显存、内存、磁盘、Python、ComfyUI、H3 源码、模型与 KJNodes 状态。
4. 选择 FL2VA、Ref2VA 或两者，并生成去重后的空间预检计划。
5. 确认后创建隔离目录和部署日志。
6. 启动后台模型下载，查看文件名、进度、下载速度和 ETA。
7. 取消时保留 `.part`；重新开始时使用 HTTP Range 续传。
8. 连接远端 ComfyUI，并在本机选择视频保存目录。

## 执行与安全约束

- SSH 远端命令固定为 `sh -s -- langbai-h3-deploy-v1`。
- 脚本由 Studio 内置，执行前计算 SHA-256，通过标准输入传输，不拼接用户提供的 Shell 片段。
- 路径固定在 `/workspace/LangbaiH3Studio`，部署编号由 Studio 生成并验证。
- 模型来自固定 Hugging Face 仓库、固定 revision 和固定文件清单。
- 每个文件使用 `.part`、Range 续传、期望大小、SHA-256 和原子替换。
- 下载使用跨进程文件锁，避免两个 Studio 实例同时写入同一个目标。
- 回滚只删除部署日志中由 Studio 创建且仍位于固定根目录内的路径。

## 模型去重

FL2VA 与 Ref2VA 共享 Qwen 编码器和 VAE。预检计划按目标路径去重，共享文件只下载一份；Studio 不以符号链接修改市场模板。连接其他 ComfyUI 时，可由用户在该环境中单独配置 `extra_model_paths.yaml`。

## 当前部署状态文件

```text
state/deployments/<id>/
  manifest.tsv
  journal.tsv
  worker.lock
  cancel.requested
  model-worker.stdout.log
  model-worker.stderr.log
```

`journal.tsv` 使用递增序号、阶段和 Base64 编码消息，Windows 端轮询同一记录来恢复准备、下载、完成、失败和取消状态。格式以当前 Rust 实现和测试为准。

## 失败恢复

- Studio 关闭不会终止已启动的远端后台下载。
- 网络中断后保留 `.part`，再次开始可继续下载。
- 取消通过部署专属标记传递，不会杀死其他部署进程。
- 校验失败的文件不会替换正式模型文件。
- 回滚前必须再次确认部署编号与固定根目录。

## 已确认边界

当前代码已覆盖环境探测、空间预检、隔离目录准备、后台模型下载、进度恢复、取消与回滚。它还没有在真实 AutoDL 市场模板 `253/678`、RTX 5090 或 8GB 显卡上完成 MiniMax-H3 端到端生成基准；市场镜像的预装内容也不是本项目控制范围。
