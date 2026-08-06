import { useEffect, useMemo, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import {
  Aperture, Boxes, ChevronDown, ChevronRight, CircleHelp, Clock3, Cpu,
  Download, FolderOpen, HardDrive, Image, Info, LayoutGrid, Menu,
  Moon, MoreHorizontal, Play, RotateCcw, Settings, ShieldCheck,
  Sparkles, Sun, Upload, Video, WandSparkles, X, Zap
} from 'lucide-react'
import './App.css'

type Mode = 'text' | 'frames' | 'reference'
type SystemProbe = { cpuName:string; cpuThreads:number; memoryTotalMb:number; memoryUsedMb:number; cudaAvailable:boolean; gpu?:{name:string;driverVersion:string;memoryTotalMb:number;memoryUsedMb:number;temperatureC?:number} }
type ComfyProbe = { reachable:boolean; baseUrl:string; nodeCount:number; h3RelatedNodes:string[]; latencyMs:number; message:string }

const modeContent = {
  text: { title: '文字生成视频', desc: '只需描述画面、镜头和声音，适合从零开始创作。' },
  frames: { title: '首尾帧生成', desc: '上传首帧或首尾帧，精确控制视频的开始与结束。' },
  reference: { title: '全模态参考', desc: '用图片、视频和音频共同定义角色、动作与声音。' },
}

function App() {
  const [dark, setDark] = useState(false)
  const [mode, setMode] = useState<Mode>('text')
  const [advanced, setAdvanced] = useState(false)
  const [downloading, setDownloading] = useState(false)
  const [downloaded, setDownloaded] = useState(42)
  const [generating, setGenerating] = useState(false)
  const [duration, setDuration] = useState(8)
  const [prompt, setPrompt] = useState('一艘银白色飞船掠过紫色星云，镜头缓慢推进。远处恒星闪烁，配合低沉而辽阔的环境音。')
  const [system, setSystem] = useState<SystemProbe | null>(null)
  const [engineDialog, setEngineDialog] = useState(false)
  const [comfyUrl, setComfyUrl] = useState('http://127.0.0.1:8188')
  const [probing, setProbing] = useState(false)
  const [probeResult, setProbeResult] = useState<ComfyProbe | null>(null)
  const [probeError, setProbeError] = useState('')
  const estimate = useMemo(() => duration <= 6 ? '约 8–12 分钟' : duration <= 10 ? '约 14–20 分钟' : '约 22–30 分钟', [duration])

  useEffect(() => { invoke<SystemProbe>('probe_system').then(setSystem).catch(() => {}) }, [])
  const gpu = system?.gpu
  const gpuPercent = gpu ? Math.min(100, Math.round(gpu.memoryUsedMb / gpu.memoryTotalMb * 100)) : 31

  const testComfy = async () => {
    setProbing(true); setProbeError(''); setProbeResult(null)
    try { setProbeResult(await invoke<ComfyProbe>('probe_comfyui', { baseUrl: comfyUrl })) }
    catch (error) { setProbeError(String(error)) }
    finally { setProbing(false) }
  }

  const startDownload = () => {
    setDownloading(true)
    setDownloaded((v) => v >= 100 ? 0 : v)
    const timer = setInterval(() => setDownloaded(v => {
      if (v >= 100) { clearInterval(timer); setDownloading(false); return 100 }
      return Math.min(100, v + 2)
    }), 120)
  }

  return (
    <div className={dark ? 'app dark' : 'app'}>
      <aside className="sidebar">
        <div className="brand"><div className="brand-mark"><Aperture size={20}/></div><div><strong>Langbai H3</strong><span>Studio</span></div></div>
        <nav aria-label="主导航">
          <button className="nav-item active"><WandSparkles/><span>开始创作</span></button>
          <button className="nav-item"><Clock3/><span>生成记录</span><b>3</b></button>
          <button className="nav-item"><Boxes/><span>模型中心</span></button>
          <button className="nav-item"><Zap/><span>加速插件</span></button>
          <button className="nav-item"><LayoutGrid/><span>工作流</span></button>
        </nav>
        <div className="sidebar-bottom">
          <div className="runtime-card"><div className="runtime-title"><span className="status-dot"/>{gpu ? '硬件检测完成' : '原型预览模式'}</div><small>{gpu ? `${gpu.name} · ${(gpu.memoryTotalMb/1024).toFixed(0)} GB` : 'RTX 4090 · 24 GB'}</small><div className="memory"><span style={{width:`${gpuPercent}%`}}/></div><small>{gpu ? `显存 ${(gpu.memoryUsedMb/1024).toFixed(1)} / ${(gpu.memoryTotalMb/1024).toFixed(0)} GB` : '显存 7.4 / 24 GB'}</small></div>
          <button className="nav-item"><Settings/><span>设置</span></button>
          <button className="nav-item"><CircleHelp/><span>使用帮助</span></button>
        </div>
      </aside>

      <main>
        <header className="topbar">
          <div className="mobile-brand"><Menu/><strong>Langbai H3 Studio</strong></div>
          <div className="crumb"><span>创作空间</span><ChevronRight/><strong>新建视频</strong></div>
          <div className="top-actions">
            <button className="engine" onClick={()=>setEngineDialog(true)}><span className="status-dot"/> 本地模式 <ChevronDown/></button>
            <button className="icon-button" aria-label="切换主题" onClick={() => setDark(!dark)}>{dark ? <Sun/> : <Moon/>}</button>
            <button className="icon-button" aria-label="更多选项"><MoreHorizontal/></button>
          </div>
        </header>

        <div className="workspace">
          <section className="intro">
            <div><span className="eyebrow"><Sparkles/> 简单创作，专业生成</span><h1>今天想创造什么？</h1><p>选择一种生成方式，Langbai H3 Studio 会为你的显卡自动匹配最佳设置。</p></div>
            <button className="draft-button"><RotateCcw/> 恢复上次草稿</button>
          </section>

          <div className="mode-tabs" role="tablist">
            {(Object.keys(modeContent) as Mode[]).map((key, i) => <button key={key} onClick={() => setMode(key)} className={mode === key ? 'selected':''} role="tab" aria-selected={mode === key}>
              {i === 0 ? <Sparkles/> : i === 1 ? <Image/> : <Video/>}<span><strong>{modeContent[key].title}</strong><small>{modeContent[key].desc}</small></span>{mode === key && <span className="check">✓</span>}
            </button>)}
          </div>

          <div className="columns">
            <section className="creator-card">
              {mode !== 'text' && <div className="field"><div className="label-row"><label>参考素材</label><span>{mode === 'frames' ? '支持 1–2 张图片' : '最多 12 个文件'}</span></div><button className="dropzone"><Upload/><strong>拖入图片、视频或音频</strong><small>或点击浏览本地文件</small></button></div>}
              <div className="field">
                <div className="label-row"><label htmlFor="prompt">描述你的视频</label><button><Info/> 提示词技巧</button></div>
                <div className="textarea-wrap"><textarea id="prompt" value={prompt} onChange={e=>setPrompt(e.target.value)} maxLength={2000}/><span>{prompt.length} / 2000</span></div>
                <div className="suggestions"><span>试试添加：</span><button>镜头运动</button><button>光线氛围</button><button>环境音效</button><button>对白</button></div>
              </div>

              <div className="setting-grid">
                <div className="field"><label>画面比例</label><button className="select"><span><span className="ratio-icon wide"/>16:9 · 横屏</span><ChevronDown/></button></div>
                <div className="field"><label>视频时长</label><div className="segmented">{[5,8,10,15].map(n=><button key={n} onClick={()=>setDuration(n)} className={duration===n?'active':''}>{n} 秒</button>)}</div></div>
                <div className="field"><label>生成质量</label><button className="select"><span><ShieldCheck/> 标准 · 768p</span><ChevronDown/></button></div>
                <div className="field"><label>随机种子 <Info/></label><button className="select"><span>自动随机</span><RotateCcw/></button></div>
              </div>

              <button className="advanced-toggle" onClick={()=>setAdvanced(!advanced)}><span><Settings/> 高级设置 <small>采样、卸载与加速选项</small></span><ChevronDown className={advanced?'rotated':''}/></button>
              {advanced && <div className="advanced-panel"><div><label>采样步数 <b>24</b></label><input type="range" min="12" max="50" defaultValue="24"/></div><div><label>显存策略</label><button className="select"><span>自动平衡</span><ChevronDown/></button></div><div><label>加速方案</label><button className="select"><span><Zap/> SageAttention · 已兼容</span><ChevronDown/></button></div></div>}
            </section>

            <aside className="summary-card">
              <div className="preview"><div className="preview-art"><div className="orbit one"/><div className="orbit two"/><Aperture/><span>生成预览将在这里显示</span></div><button className="expand">↗</button></div>
              <div className="summary-body"><h2>生成准备</h2><div className="summary-row"><span><Cpu/> 运行方案</span><strong>单卡优化</strong></div><div className="summary-row"><span><HardDrive/> 预计显存</span><strong className="good">约 19.6 GB</strong></div><div className="summary-row"><span><Clock3/> 预计耗时</span><strong>{estimate}</strong></div><div className="summary-row"><span><FolderOpen/> 保存到</span><button>D:\H3作品 <ChevronRight/></button></div>
                <div className="fit-notice"><ShieldCheck/><span><strong>适合当前设备</strong><small>已自动启用模型卸载与分块解码</small></span></div>
                <button className="generate" onClick={()=>setGenerating(!generating)}>{generating ? <><span className="spinner"/> 正在准备模型…</> : <><Play/> 开始生成视频</>}</button><p className="queue-note">当前队列中有 1 个任务，预计等待 3 分钟</p>
              </div>
            </aside>
          </div>

          <section className="download-card">
            <div className="model-icon"><Download/></div><div className="download-info"><div><strong>MiniMax-H3 Ref2VA · 单卡优化版</strong><span className="tag">推荐</span></div><p>支持图片、视频与音频参考 · BF16/量化混合 · 约 38.4 GB</p><div className="progress"><span style={{width:`${downloaded}%`}}/></div><small>{downloaded < 100 ? `已下载 ${(38.4*downloaded/100).toFixed(1)} GB / 38.4 GB · 42.8 MB/s · 剩余约 ${Math.ceil((100-downloaded)/6)} 分钟` : '下载完成 · 文件校验通过'}</small></div><button onClick={startDownload} disabled={downloading || downloaded===100}>{downloaded===100?'已安装':downloading?'下载中':'继续下载'}</button><button className="icon-button" aria-label="关闭"><X/></button>
          </section>
        </div>
      </main>
      {engineDialog && <div className="modal-backdrop" role="presentation" onMouseDown={e=>{if(e.target===e.currentTarget)setEngineDialog(false)}}>
        <section className="modal" role="dialog" aria-modal="true" aria-labelledby="engine-title">
          <div className="modal-head"><div><span className="eyebrow"><Cpu/> 运行环境</span><h2 id="engine-title">连接已有 ComfyUI</h2></div><button className="icon-button" onClick={()=>setEngineDialog(false)} aria-label="关闭对话框"><X/></button></div>
          <p>输入本机 ComfyUI 地址。探测只读取版本和节点能力，不修改现有环境。</p>
          <label htmlFor="comfy-url">ComfyUI 地址</label>
          <div className="url-row"><input id="comfy-url" value={comfyUrl} onChange={e=>setComfyUrl(e.target.value)} /><button onClick={testComfy} disabled={probing}>{probing?'正在检测…':'测试连接'}</button></div>
          {probeError && <div className="probe-message error"><Info/><span><strong>连接未通过</strong><small>{probeError}</small></span></div>}
          {probeResult && <div className="probe-message success"><ShieldCheck/><span><strong>{probeResult.message}</strong><small>发现 {probeResult.nodeCount} 个节点 · H3 相关 {probeResult.h3RelatedNodes.length} 个 · {probeResult.latencyMs} ms</small></span></div>}
          <div className="modal-note"><ShieldCheck/><span><strong>本地连接保护</strong><small>MVP 仅探测 127.0.0.1、localhost 或 ::1，避免意外访问不可信远端服务。</small></span></div>
        </section>
      </div>}
    </div>
  )
}

export default App
