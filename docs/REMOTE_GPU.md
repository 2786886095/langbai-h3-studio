# 远程 RTX 5090 / GPU 工作站连接

Langbai H3 Studio 通过 Windows OpenSSH 本地转发连接远端 ComfyUI。应用仍只访问
`127.0.0.1`，不需要把 ComfyUI API 暴露到公网。

## 远端准备

1. 在远端安装 MiniMax-H3、ComfyUI H3 节点及所需模型。
2. 启动 ComfyUI：`python main.py --listen 127.0.0.1 --port 8188`。
3. 云服务器安全组只开放 SSH 端口，关闭 8188 公网入站。
4. 使用专用 SSH 密钥和低权限用户。
5. 从租用商控制台等可信渠道核对 SSH 主机指纹，准备独立 `known_hosts` 文件。

## Studio 连接

在“运行引擎 → 租用 RTX 5090 / 远程工作站”中填写：

- SSH 主机、用户名及端口
- 远端 ComfyUI 端口，默认 8188
- 专用 SSH 私钥
- 已核验的 `known_hosts`

连接成功后，Studio 将远端服务映射为本机随机回环地址，然后使用与本地 ComfyUI
相同的节点探测、素材上传、任务轮询和结果保存流程。

## 安全边界

- 固定使用 `%WINDIR%\System32\OpenSSH\ssh.exe`
- `StrictHostKeyChecking=yes`
- 不加载用户 SSH config，不启用 Agent、TTY、远程命令或本地命令
- 转发仅绑定 `127.0.0.1`
- 私钥路径和内容不写入任务记录或日志
- SSH 存活但 `/object_info` 未就绪时自动终止隧道

