import { useEffect, useMemo, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
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
type ModelScan = { root:string; maxDepth:number; models:Array<{directory:string;modelType:string;integrity:string;totalSizeBytes:number;files:Array<{path:string;sizeBytes:number;kind:string}>;warnings:string[]}>; warnings:string[] }
type JobRecord = { id:string;name:string;status:string;stage:string;progress:number;backendId:string;outputPath?:string;errorSummary?:string;createdAt:number;updatedAt:number }
type RuntimeManifest = { version:string;url:string;sha256:string;archiveFormat:string;expectedFiles:string[] }
type TransferProgress = { phase:string;downloadedBytes?:number;totalBytes?:number;progressPercent?:number;bytesPerSecond?:number;etaSeconds?:number;currentFile?:string }
type ModelBundle = { id:string;name:string;variant:string;revision:string;license:string;licenseUrl:string;recommendedVramGb:number;recommendedRamGb:number;files:Array<{relativePath:string;size:number;sha256:string}> }
type ModelBundleFileEvent = { bundleId:string;index:number;count:number;relativePath:string;size:number }
type ModelDownloadEvent = { relativePath:string;progress:TransferProgress }
type H3PatchManifest = { id:string;commit:string;pullRequest:number;url:string;sha256:string;size:number;requiredNodes:string[];status:string }
type LocalAsset = { path:string;name:string;size:number;mime:string;kind:'image'|'video'|'audio';role:'start_frame'|'end_frame'|'reference'|'motion_reference'|'audio_reference';status:'selected'|'uploading'|'ready'|'error';remoteName?:string;error?:string }
type StartedGeneration = { promptId:string;queueNumber?:number;uploadedAssets:number;filenamePrefix:string;outputDirectory:string }
type GenerationPoll = { status:'queued'|'running'|'completed'|'failed'|'unknown';promptId:string;queuePosition?:number;outputs:Array<{filename:string;subfolder:string;mediaType:string}>;error?:string }

const modeContent = {
  text: { title: '文字生成视频', desc: '只需描述画面、镜头和声音，适合从零开始创作。' },
  frames: { title: '首尾帧生成', desc: '上传首帧或首尾帧，精确控制视频的开始与结束。' },
  reference: { title: '全模态参考', desc: '用图片、视频和音频共同定义角色、动作与声音。' },
}

function App() {
  const [dark, setDark] = useState(false)
  const [mode, setMode] = useState<Mode>('text')
  const [advanced, setAdvanced] = useState(false)
  const [generating, setGenerating] = useState(false)
  const [duration, setDuration] = useState(8)
  const [prompt, setPrompt] = useState('一艘银白色飞船掠过紫色星云，镜头缓慢推进。远处恒星闪烁，配合低沉而辽阔的环境音。')
  const [system, setSystem] = useState<SystemProbe | null>(null)
  const [engineDialog, setEngineDialog] = useState(false)
  const [comfyUrl, setComfyUrl] = useState('http://127.0.0.1:8188')
  const [probing, setProbing] = useState(false)
  const [probeResult, setProbeResult] = useState<ComfyProbe | null>(null)
  const [probeError, setProbeError] = useState('')
  const [modelDialog, setModelDialog] = useState(false)
  const [modelPath, setModelPath] = useState('D:\\AI模型')
  const [modelScan, setModelScan] = useState<ModelScan | null>(null)
  const [modelError, setModelError] = useState('')
  const [scanningModels, setScanningModels] = useState(false)
  const [jobMessage, setJobMessage] = useState('')
  const [historyDialog, setHistoryDialog] = useState(false)
  const [jobs, setJobs] = useState<JobRecord[]>([])
  const [historyLoading, setHistoryLoading] = useState(false)
  const [settingsDialog, setSettingsDialog] = useState(false)
  const [outputPath, setOutputPath] = useState('D:\\H3作品')
  const [pathMessage, setPathMessage] = useState('')
  const [runtimeManifests, setRuntimeManifests] = useState<RuntimeManifest[]>([])
  const [runtimeProgress, setRuntimeProgress] = useState<TransferProgress | null>(null)
  const [installingRuntime, setInstallingRuntime] = useState('')
  const [runtimeMessage, setRuntimeMessage] = useState('')
  const [modelBundles, setModelBundles] = useState<ModelBundle[]>([])
  const [selectedBundle, setSelectedBundle] = useState('h3-t2v-int8')
  const [licenseAccepted, setLicenseAccepted] = useState(false)
  const [installingModel, setInstallingModel] = useState(false)
  const [modelInstallMessage, setModelInstallMessage] = useState('')
  const [modelFile, setModelFile] = useState<ModelBundleFileEvent | null>(null)
  const [modelProgress, setModelProgress] = useState<TransferProgress | null>(null)
  const [assets, setAssets] = useState<LocalAsset[]>([])
  const [assetError, setAssetError] = useState('')
  const [h3Patch, setH3Patch] = useState<H3PatchManifest | null>(null)
  const [installingH3Patch, setInstallingH3Patch] = useState(false)
  const [h3PatchMessage, setH3PatchMessage] = useState('')
  const [h3PatchProgress, setH3PatchProgress] = useState<TransferProgress | null>(null)
  const [activePromptId, setActivePromptId] = useState('')
  const [generationPoll, setGenerationPoll] = useState<GenerationPoll | null>(null)
  const estimate = useMemo(() => duration <= 6 ? '约 8–12 分钟' : duration <= 10 ? '约 14–20 分钟' : '约 22–30 分钟', [duration])

  useEffect(() => {
    invoke<SystemProbe>('probe_system').then(setSystem).catch(() => {})
    invoke<RuntimeManifest[]>('runtime_manifests').then(setRuntimeManifests).catch(()=>{})
    invoke<ModelBundle[]>('model_bundles').then(setModelBundles).catch(()=>{})
    invoke<H3PatchManifest>('h3_preview_patch_manifest').then(setH3Patch).catch(()=>{})
    const unlisteners = Promise.all([
      listen<TransferProgress>('runtime-download-progress', event=>setRuntimeProgress(event.payload)),
      listen<TransferProgress>('runtime-install-progress', event=>setRuntimeProgress(event.payload)),
      listen<ModelBundleFileEvent>('model-bundle-file', event=>{setModelFile(event.payload);setModelProgress(null)}),
      listen<ModelDownloadEvent>('model-download-progress', event=>setModelProgress(event.payload.progress)),
      listen<TransferProgress>('h3-patch-download-progress', event=>setH3PatchProgress(event.payload)),
    ]).catch(()=>[] as Array<()=>void>)
    return ()=>{unlisteners.then(items=>items.forEach(fn=>fn()))}
  }, [])
  useEffect(()=>{
    if(!activePromptId) return
    let stopped=false
    const poll=async()=>{
      try{
        const value=await invoke<GenerationPoll>('comfy_poll_generation',{baseUrl:comfyUrl,promptId:activePromptId})
        if(stopped) return
        setGenerationPoll(value)
        if(value.status==='completed'){
          const saved:string[]=[]
          for(const asset of value.outputs.filter(item=>item.mediaType==='video')){
            saved.push(await invoke<string>('comfy_save_output',{baseUrl:comfyUrl,asset,outputDirectory:outputPath}))
          }
          setJobMessage(saved.length?`生成完成，已保存到 ${saved.join('、')}`:'生成完成，但历史记录中没有视频输出')
          setActivePromptId('');setGenerating(false)
        }else if(value.status==='failed'){setActivePromptId('');setGenerating(false)}
      }catch(error){if(!stopped)setJobMessage(`读取生成状态失败：${String(error)}`)}
    }
    poll();const timer=setInterval(poll,3000)
    return()=>{stopped=true;clearInterval(timer)}
  },[activePromptId,comfyUrl,outputPath])
  const gpu = system?.gpu
  const gpuPercent = gpu ? Math.min(100, Math.round(gpu.memoryUsedMb / gpu.memoryTotalMb * 100)) : 31
  const h3Ready = !!probeResult && (h3Patch?.requiredNodes || []).every(node=>probeResult.h3RelatedNodes.includes(node))

  const testComfy = async () => {
    setProbing(true); setProbeError(''); setProbeResult(null)
    try { setProbeResult(await invoke<ComfyProbe>('probe_comfyui', { baseUrl: comfyUrl })) }
    catch (error) { setProbeError(String(error)) }
    finally { setProbing(false) }
  }

  const scanModels = async () => {
    setScanningModels(true); setModelError(''); setModelScan(null)
    try { setModelScan(await invoke<ModelScan>('scan_local_models', { root:modelPath, maxDepth:5 })) }
    catch (error) { setModelError(String(error)) }
    finally { setScanningModels(false) }
  }

  const chooseAssets = async () => {
    setAssetError('')
    try {
      const selected = await open({
        multiple:true,
        filters:[{name:mode==='frames'?'图片素材':'图片、视频和音频',extensions:mode==='frames'?['png','jpg','jpeg','webp']:['png','jpg','jpeg','webp','mp4','webm','mov','wav','mp3','flac','m4a']}]
      })
      if (!selected) return
      const paths = Array.isArray(selected) ? selected : [selected]
      const inspected = await invoke<Array<Omit<LocalAsset,'role'|'status'>>>('inspect_input_files',{paths})
      if (mode==='frames' && (inspected.length>2 || inspected.some(item=>item.kind!=='image'))) throw new Error('首尾帧模式最多选择 2 张图片')
      const next = inspected.map((item,index):LocalAsset=>({...item,status:'selected',role:mode==='frames'?(index===0?'start_frame':'end_frame'):(item.kind==='audio'?'audio_reference':item.kind==='video'?'motion_reference':'reference')}))
      setAssets(next)
    } catch(error) { setAssetError(String(error)) }
  }

  const removeAsset = (path:string) => setAssets(items=>items.filter(item=>item.path!==path))

  const createGenerationJob = async () => {
    setGenerating(true); setJobMessage('')
    if(!prompt.trim()){setJobMessage('请先填写视频描述');setGenerating(false);return}
    if(mode==='frames'&&assets.length===0){setJobMessage('首尾帧模式至少需要选择一张图片');setGenerating(false);return}
    if(mode==='reference'&&assets.length===0){setJobMessage('全模态参考模式至少需要选择一个素材');setGenerating(false);return}
    const workflowMode=mode==='text'?'t2v':mode==='frames'?'fl2va':'ref2va'
    const request = { mode:workflowMode, prompt, width:1344, height:768, durationSeconds:duration, seed:Date.now(), steps:20, referenceImageSize:'match', outputDirectory:outputPath, baseUrl:comfyUrl, assets:assets.map(({path,mime,kind,role})=>({path,mime,kind,role})) }
    try {
      setJobMessage(assets.length?'正在上传素材并编译工作流…':'正在编译并提交工作流…')
      const started=await invoke<StartedGeneration>('start_h3_generation',{input:request})
      const job = await invoke<{id:string}>('create_job', { input:{ name:`${modeContent[mode].title} · ${new Date().toLocaleTimeString()}`, requestJson:JSON.stringify({...request,promptId:started.promptId}), backendId:started.promptId } })
      setActivePromptId(started.promptId);setGenerationPoll({status:'queued',promptId:started.promptId,queuePosition:started.queueNumber,outputs:[]})
      setJobMessage(`任务 ${job.id} 已提交 ComfyUI${started.queueNumber!==undefined?` · 队列编号 ${started.queueNumber}`:''}`)
    } catch(error) { setJobMessage(String(error));setGenerating(false) }
  }

  const openHistory = async () => {
    setHistoryDialog(true); setHistoryLoading(true)
    try { setJobs(await invoke<JobRecord[]>('list_jobs', { limit:100 })) }
    catch { setJobs([]) }
    finally { setHistoryLoading(false) }
  }

  const validatePath = async () => {
    setPathMessage('正在检查…')
    try { const value = await invoke<string>('validate_output_path', { path:outputPath }); setOutputPath(value); setPathMessage('目录可写，已保存') }
    catch (error) { setPathMessage(String(error)) }
  }

  const chooseOutputPath = async () => {
    const selected = await open({directory:true,multiple:false,title:'选择视频保存目录'})
    if (typeof selected === 'string') { setOutputPath(selected); setPathMessage('') }
  }

  const chooseModelPath = async () => {
    const selected = await open({directory:true,multiple:false,title:'选择 MiniMax-H3 模型目录'})
    if (typeof selected === 'string') { setModelPath(selected); setModelScan(null); setModelError('') }
  }

  const installRuntime = async (variant:string) => {
    setInstallingRuntime(variant);setRuntimeMessage('');setRuntimeProgress({phase:'preparing',progressPercent:0})
    try {
      const current = await invoke<{version:string}>('runtime_download_install_activate',{variant})
      setRuntimeMessage(`运行环境 ${current.version} 已安装并激活`)
    } catch(error) { setRuntimeMessage(String(error)) }
    finally { setInstallingRuntime('') }
  }

  const installH3Patch = async () => {
    setInstallingH3Patch(true);setH3PatchMessage('');setH3PatchProgress({phase:'preparing',progressPercent:0})
    try {
      await invoke('runtime_install_h3_preview_patch')
      setH3PatchProgress({phase:'completed',progressPercent:100})
      setH3PatchMessage('H3 预览补丁与 Python 依赖已安装；请重启托管 Runtime 后重新检测。')
    } catch(error) { setH3PatchMessage(String(error)) }
    finally { setInstallingH3Patch(false) }
  }

  const installModelBundle = async () => {
    setInstallingModel(true); setModelInstallMessage(''); setModelProgress({phase:'preparing',progressPercent:0})
    try {
      const installed = await invoke<{modelRoot:string;totalSize:number}>('download_h3_bundle',{bundleId:selectedBundle,licenseAccepted})
      setModelProgress({phase:'completed',progressPercent:100})
      setModelInstallMessage(`??????????? ? ${(installed.totalSize/1024/1024/1024).toFixed(1)} GiB ? ${installed.modelRoot}`)
    } catch(error) { setModelInstallMessage(String(error)) }
    finally { setInstallingModel(false) }
  }

  return (
    <div className={dark ? 'app dark' : 'app'}>
      <aside className="sidebar">
        <div className="brand"><div className="brand-mark"><Aperture size={20}/></div><div><strong>Langbai H3</strong><span>Studio</span></div></div>
        <nav aria-label="主导航">
          <button className="nav-item active"><WandSparkles/><span>开始创作</span></button>
          <button className="nav-item" onClick={openHistory}><Clock3/><span>生成记录</span><b>{jobs.length || 3}</b></button>
          <button className="nav-item" onClick={()=>setModelDialog(true)}><Boxes/><span>模型中心</span></button>
          <button className="nav-item"><Zap/><span>加速插件</span></button>
          <button className="nav-item"><LayoutGrid/><span>工作流</span></button>
        </nav>
        <div className="sidebar-bottom">
          <div className="runtime-card"><div className="runtime-title"><span className="status-dot"/>{gpu ? '硬件检测完成' : '原型预览模式'}</div><small>{gpu ? `${gpu.name} · ${(gpu.memoryTotalMb/1024).toFixed(0)} GB` : 'RTX 4090 · 24 GB'}</small><div className="memory"><span style={{width:`${gpuPercent}%`}}/></div><small>{gpu ? `显存 ${(gpu.memoryUsedMb/1024).toFixed(1)} / ${(gpu.memoryTotalMb/1024).toFixed(0)} GB` : '显存 7.4 / 24 GB'}</small></div>
          <button className="nav-item" onClick={()=>setSettingsDialog(true)}><Settings/><span>设置</span></button>
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
              {mode !== 'text' && <div className="field"><div className="label-row"><label>参考素材</label><span>{assets.length}/{mode === 'frames' ? 2 : 12}</span></div><button className="dropzone" onClick={chooseAssets}><Upload/><strong>{mode==='frames'?'选择首帧或尾帧图片':'选择图片、视频或音频'}</strong><small>使用 Windows 文件选择器，可一次多选</small></button>{assetError&&<div className="inline-error"><Info/>{assetError}</div>}{assets.length>0&&<div className="asset-list">{assets.map((asset,index)=><article key={asset.path}><div className={`asset-kind ${asset.kind}`}>{asset.kind==='image'?<Image/>:asset.kind==='video'?<Video/>:<Aperture/>}</div><div><strong>{asset.name}</strong><small>{mode==='frames'?(index===0?'首帧':'尾帧'):asset.kind==='image'?'图片参考':asset.kind==='video'?'动作参考':'声音参考'} · {(asset.size/1024/1024).toFixed(asset.size>1024*1024?1:2)} MB</small></div><button onClick={()=>removeAsset(asset.path)} aria-label={`移除 ${asset.name}`}><X/></button></article>)}</div>}</div>}
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
              {advanced && <div className="advanced-panel"><div><label>采样步数 <b>20</b></label><input type="range" min="12" max="50" defaultValue="20"/></div><div><label>显存策略</label><button className="select"><span>自动平衡（运行时检测）</span><ChevronDown/></button></div><div><label>加速方案</label><button className="select"><span><Zap/> 自动选择（安装后检测）</span><ChevronDown/></button></div></div>}
            </section>

            <aside className="summary-card">
              <div className="preview"><div className="preview-art"><div className="orbit one"/><div className="orbit two"/><Aperture/><span>生成预览将在这里显示</span></div><button className="expand">↗</button></div>
              <div className="summary-body"><h2>生成准备</h2><div className="summary-row"><span><Cpu/> 运行方案</span><strong>单卡优化</strong></div><div className="summary-row"><span><HardDrive/> 预计显存</span><strong className="good">约 19.6 GB</strong></div><div className="summary-row"><span><Clock3/> 预计耗时</span><strong>{estimate}</strong></div><div className="summary-row"><span><FolderOpen/> 保存到</span><button onClick={()=>setSettingsDialog(true)}>{outputPath} <ChevronRight/></button></div>
                <div className="fit-notice"><ShieldCheck/><span><strong>{gpu&&gpu.memoryTotalMb>=24000?'达到建议显存':'需要兼容性实测'}</strong><small>{gpu?`${(gpu.memoryTotalMb/1024).toFixed(0)}GB 显存 · 建议 24GB 显存与 64GB 内存`:'未检测到 NVIDIA 显卡信息'}</small></span></div>
                <button className="generate" onClick={createGenerationJob} disabled={generating||!h3Ready}>{generating ? <><span className="spinner"/> {generationPoll?.status==='running'?'正在生成视频…':generationPoll?.status==='queued'?'正在排队…':'正在提交素材…'}</> : <><Play/> {h3Ready?'开始生成视频':'请先连接 H3 运行环境'}</>}</button><p className="queue-note">{generationPoll?.status==='failed'?`生成失败：${generationPoll.error||'ComfyUI 执行错误'}`:jobMessage || (h3Ready?'运行环境已通过 H3 节点校验':'在“运行环境”中安装或连接 ComfyUI，并验证 H3 必需节点')}</p>
              </div>
            </aside>
          </div>

          <section className="download-card">
            <div className="model-icon"><Download/></div><div className="download-info"><div><strong>MiniMax-H3 ??????</strong><span className="tag">?????</span></div><p>??? 39.6 GiB?42.5 GB?? ?? 24GB ??? 64GB ?? ? ??????? SHA-256 ??</p><small>??????????????????????????????????????</small></div><button onClick={()=>setModelDialog(true)}>??????</button>
          </section>
        </div>
      </main>
      {engineDialog && <div className="modal-backdrop" role="presentation" onMouseDown={e=>{if(e.target===e.currentTarget)setEngineDialog(false)}}>
        <section className="modal" role="dialog" aria-modal="true" aria-labelledby="engine-title">
          <div className="modal-head"><div><span className="eyebrow"><Cpu/> 运行环境</span><h2 id="engine-title">连接已有 ComfyUI</h2></div><button className="icon-button" onClick={()=>setEngineDialog(false)} aria-label="关闭对话框"><X/></button></div>
          <p>新手可以一键安装独立运行环境；已有 ComfyUI 用户也可以直接连接本机实例。</p>
          <div className="runtime-options">{runtimeManifests.map((item,index)=><article key={item.version}><div className="model-icon"><Cpu/></div><div><strong>{index===0?'NVIDIA 新版运行环境':'NVIDIA 兼容运行环境'}</strong><small>{item.version} · 官方 ComfyUI v0.30.0 · 约 {index===0?'2.11':'2.05'} GB</small><span>{index===0?'适合 RTX 20 系及更新显卡':'CUDA 12.6，适合旧驱动或较老显卡'}</span></div><button onClick={()=>installRuntime(index===0?'nvidia':'nvidia-cu126')} disabled={!!installingRuntime}>{installingRuntime=== (index===0?'nvidia':'nvidia-cu126')?'安装中…':'下载并安装'}</button></article>)}</div>
          {runtimeProgress && <div className="runtime-progress"><div><span>{runtimeProgress.phase}</span><strong>{Math.round(runtimeProgress.progressPercent || 0)}%</strong></div><div className="progress"><span style={{width:`${runtimeProgress.progressPercent || 0}%`}}/></div><small>{runtimeProgress.bytesPerSecond?`${(runtimeProgress.bytesPerSecond/1024/1024).toFixed(1)} MB/s`:runtimeProgress.currentFile||'正在准备'} {runtimeProgress.etaSeconds?`· 剩余约 ${Math.ceil(runtimeProgress.etaSeconds/60)} 分钟`:''}</small></div>}
          {runtimeMessage && <div className={`probe-message ${runtimeMessage.includes('已安装')?'success':'error'}`}><ShieldCheck/><span><strong>{runtimeMessage}</strong><small>下载支持断点续传，安装前会验证官方 SHA-256。</small></span></div>}
          <div className="section-divider"><span>或连接已有环境</span></div>
          <p>输入本机 ComfyUI 地址。探测只读取版本和节点能力，不修改现有环境。</p>
          <label htmlFor="comfy-url">ComfyUI 地址</label>
          <div className="url-row"><input id="comfy-url" value={comfyUrl} onChange={e=>setComfyUrl(e.target.value)} /><button onClick={testComfy} disabled={probing}>{probing?'正在检测…':'测试连接'}</button></div>
          {probeError && <div className="probe-message error"><Info/><span><strong>连接未通过</strong><small>{probeError}</small></span></div>}
          {probeResult && <div className={`probe-message ${h3Ready?'success':'error'}`}><ShieldCheck/><span><strong>{h3Ready?'H3 运行节点完整':'ComfyUI 可连接，但尚不能运行 H3'}</strong><small>{h3Ready?`已验证 ${h3Patch?.requiredNodes.length||0} 个必需节点 · ${probeResult.latencyMs} ms`:`缺少 H3 必需节点；基础 Runtime v0.30.0 需要应用上游 PR #${h3Patch?.pullRequest||15224} 预览补丁`}</small></span></div>}
          {h3Patch && !h3Ready && <div className="patch-notice"><Info/><span><strong>H3 当前依赖上游预览实现</strong><small>固定提交 {h3Patch.commit.slice(0,8)} · 下载后校验 SHA-256 · 安装失败自动回滚</small></span><button onClick={installH3Patch} disabled={installingH3Patch}>{installingH3Patch?'安装中…':'安装 H3 补丁'}</button></div>}
          {h3PatchProgress && installingH3Patch && <div className="runtime-progress"><div><span>下载 H3 补丁</span><strong>{Math.round(h3PatchProgress.progressPercent||0)}%</strong></div><div className="progress"><span style={{width:`${h3PatchProgress.progressPercent||0}%`}}/></div><small>{h3PatchProgress.bytesPerSecond?`${(h3PatchProgress.bytesPerSecond/1024/1024).toFixed(1)} MB/s`:'正在准备'} {h3PatchProgress.etaSeconds?`· 剩余约 ${Math.ceil(h3PatchProgress.etaSeconds/60)} 分钟`:''}</small></div>}
          {h3PatchMessage && <div className={`probe-message ${h3PatchMessage.includes('已安装')?'success':'error'}`}><ShieldCheck/><span><strong>{h3PatchMessage.includes('已安装')?'补丁安装完成':'补丁安装未完成'}</strong><small>{h3PatchMessage}</small></span></div>}
          <div className="modal-note"><ShieldCheck/><span><strong>本地连接保护</strong><small>MVP 仅探测 127.0.0.1、localhost 或 ::1，避免意外访问不可信远端服务。</small></span></div>
        </section>
      </div>}
      {modelDialog && <div className="modal-backdrop" role="presentation" onMouseDown={e=>{if(e.target===e.currentTarget)setModelDialog(false)}}>
        <section className="modal model-modal" role="dialog" aria-modal="true" aria-labelledby="model-title">
          <div className="modal-head"><div><span className="eyebrow"><HardDrive/> 模型中心</span><h2 id="model-title">使用本地已有模型</h2></div><button className="icon-button" onClick={()=>setModelDialog(false)} aria-label="关闭对话框"><X/></button></div>
          <p>选择包含 MiniMax-H3 FL2VA 或 Ref2VA 的目录。扫描只读取文件结构，不移动或删除权重。</p>
          <div className="runtime-options">{modelBundles.map(bundle=>{const total=bundle.files.reduce((sum,file)=>sum+file.size,0);return <article key={bundle.id} className={selectedBundle===bundle.id?'selected':''}><div className="model-icon"><Download/></div><div><strong>{bundle.name}</strong><small>{bundle.variant.toUpperCase()} · {(total/1024/1024/1024).toFixed(1)} GiB · {bundle.files.length} 个文件</small><span>建议显存 {bundle.recommendedVramGb}GB / 内存 {bundle.recommendedRamGb}GB</span></div><button onClick={()=>setSelectedBundle(bundle.id)}>{selectedBundle===bundle.id?'已选择':'选择'}</button></article>})}</div>
          <label className="license-check"><input type="checkbox" checked={licenseAccepted} onChange={e=>setLicenseAccepted(e.target.checked)}/><span>我已阅读并接受 <a href={modelBundles.find(item=>item.id===selectedBundle)?.licenseUrl} target="_blank" rel="noreferrer">MiniMax H3 Community License</a></span></label>
          <button className="primary wide" onClick={installModelBundle} disabled={!licenseAccepted || installingModel || modelBundles.length===0}>{installingModel?'正在下载并校验…':'下载到托管 ComfyUI'}</button>
          {modelFile && <div className="runtime-progress"><div><span>文件 {modelFile.index+1}/{modelFile.count} · {modelFile.relativePath.split('/').pop()}</span><strong>{Math.round(modelProgress?.progressPercent||0)}%</strong></div><div className="progress"><span style={{width:`${modelProgress?.progressPercent||0}%`}}/></div><small>{modelProgress?.bytesPerSecond?`${(modelProgress.bytesPerSecond/1024/1024).toFixed(1)} MB/s`:modelProgress?.phase||'正在准备'} {modelProgress?.etaSeconds?`· 剩余约 ${Math.ceil(modelProgress.etaSeconds/60)} 分钟`:''}</small></div>}
          {modelInstallMessage && <div className={`probe-message ${modelInstallMessage.includes('完成')?'success':'error'}`}><ShieldCheck/><span><strong>{modelInstallMessage.includes('完成')?'模型可用':'安装未完成'}</strong><small>{modelInstallMessage}</small></span></div>}
          <div className="section-divider"><span>或使用本地已有模型</span></div>
          <label htmlFor="model-path">模型目录</label>
          <div className="url-row"><input id="model-path" value={modelPath} onChange={e=>setModelPath(e.target.value)} /><button onClick={chooseModelPath}>选择目录</button><button onClick={scanModels} disabled={scanningModels}>{scanningModels?'正在扫描…':'扫描'}</button></div>
          {modelError && <div className="probe-message error"><Info/><span><strong>扫描未完成</strong><small>{modelError}</small></span></div>}
          {modelScan && <div className="model-results"><div className="result-head"><strong>发现 {modelScan.models.length} 个模型</strong><small>扫描深度 {modelScan.maxDepth} 层</small></div>{modelScan.models.length===0?<div className="empty-result">目录中没有识别到 H3 模型结构</div>:modelScan.models.map((item,i)=><article key={i}><div className="model-icon"><Boxes/></div><div><strong>{item.modelType}</strong><small>{item.directory}</small><span>{item.integrity} · {(item.totalSizeBytes/1024/1024/1024).toFixed(1)} GB · {item.files.length} 个组件</span></div><button>关联</button></article>)}</div>}
          <div className="modal-note"><ShieldCheck/><span><strong>保持原文件位置</strong><small>关联后通过模型路径映射复用权重，不会复制数十 GB 文件。</small></span></div>
        </section>
      </div>}
      {historyDialog && <div className="modal-backdrop" role="presentation" onMouseDown={e=>{if(e.target===e.currentTarget)setHistoryDialog(false)}}>
        <section className="modal model-modal" role="dialog" aria-modal="true" aria-labelledby="history-title">
          <div className="modal-head"><div><span className="eyebrow"><Clock3/> 本地任务</span><h2 id="history-title">生成记录</h2></div><button className="icon-button" onClick={()=>setHistoryDialog(false)} aria-label="关闭对话框"><X/></button></div>
          <p>任务保存在本机 SQLite 数据库中，重启软件后仍可恢复参数和状态。</p>
          <div className="job-list">{historyLoading?<div className="empty-result">正在读取任务…</div>:jobs.length===0?<div className="empty-result">暂时没有本地任务记录</div>:jobs.map(job=><article key={job.id}><div className={`job-state ${job.status}`}>{Math.round(job.progress*100)}%</div><div><strong>{job.name}</strong><small>{job.stage} · {job.backendId}</small><span>{new Date(job.updatedAt*1000).toLocaleString()}</span></div><button>复用设置</button></article>)}</div>
        </section>
      </div>}
      {settingsDialog && <div className="modal-backdrop" role="presentation" onMouseDown={e=>{if(e.target===e.currentTarget)setSettingsDialog(false)}}>
        <section className="modal" role="dialog" aria-modal="true" aria-labelledby="settings-title">
          <div className="modal-head"><div><span className="eyebrow"><FolderOpen/> 输出设置</span><h2 id="settings-title">视频保存路径</h2></div><button className="icon-button" onClick={()=>setSettingsDialog(false)} aria-label="关闭对话框"><X/></button></div>
          <p>生成前会检查目录是否存在和可写。验证过程只创建并立即删除一个测试文件。</p>
          <label htmlFor="output-path">默认保存目录</label>
          <div className="url-row"><input id="output-path" value={outputPath} onChange={e=>{setOutputPath(e.target.value);setPathMessage('')}} /><button onClick={chooseOutputPath}>选择目录</button><button onClick={validatePath}>验证</button></div>
          {pathMessage && <div className={`probe-message ${pathMessage.includes('可写')?'success':'error'}`}><ShieldCheck/><span><strong>{pathMessage}</strong><small>后续任务可单独覆盖此目录。</small></span></div>}
        </section>
      </div>}
    </div>
  )
}

export default App
