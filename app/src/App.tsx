import { useEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import {
  Aperture, Boxes, ChevronDown, ChevronRight, CircleHelp, Clock3, Cpu,
  Download, FolderOpen, HardDrive, Image, Info, LayoutGrid, Menu,
  Moon, Play, RotateCcw, Settings, ShieldCheck,
  Sparkles, Sun, Upload, Video, WandSparkles, X, Zap
} from 'lucide-react'
import './App.css'

type Mode = 'text' | 'frames' | 'reference'
type SystemProbe = { cpuName:string; cpuThreads:number; memoryTotalMb:number; memoryUsedMb:number; cudaAvailable:boolean; gpu?:{name:string;driverVersion:string;memoryTotalMb:number;memoryUsedMb:number;temperatureC?:number} }
type ComfyProbe = { reachable:boolean; baseUrl:string; nodeCount:number; h3RelatedNodes:string[]; latencyMs:number; message:string }
type ModelScan = { root:string; maxDepth:number; models:Array<{directory:string;modelType:string;integrity:string;totalSizeBytes:number;files:Array<{path:string;sizeBytes:number;kind:string}>;warnings:string[]}>; warnings:string[] }
type JobRecord = { id:string;name:string;status:string;stage:string;progress:number;requestJson:string;backendId:string;outputPath?:string;errorSummary?:string;createdAt:number;updatedAt:number }
type RuntimeManifest = { version:string;url:string;sha256:string;archiveFormat:string;expectedFiles:string[] }
type TransferProgress = { phase:string;downloadedBytes?:number;totalBytes?:number;progressPercent?:number;bytesPerSecond?:number;etaSeconds?:number;currentFile?:string }
type ModelBundle = { id:string;name:string;variant:string;revision:string;license:string;licenseUrl:string;recommendedVramGb:number;recommendedRamGb:number;files:Array<{relativePath:string;size:number;sha256:string}> }
type ModelBundleFileEvent = { bundleId:string;index:number;count:number;relativePath:string;size:number }
type ModelDownloadEvent = { relativePath:string;progress:TransferProgress }
type H3PatchManifest = { id:string;commit:string;pullRequest:number;url:string;sha256:string;size:number;requiredNodes:string[];status:string }
type LocalAsset = { path:string;name:string;size:number;mime:string;kind:'image'|'video'|'audio';role:'start_frame'|'end_frame'|'reference'|'motion_reference'|'audio_reference';status:'selected'|'uploading'|'ready'|'error';remoteName?:string;error?:string }
type StartedGeneration = { promptId:string;queueNumber?:number;uploadedAssets:number;filenamePrefix:string;outputDirectory:string }
type GenerationPoll = { status:'queued'|'running'|'completed'|'failed'|'unknown';promptId:string;queuePosition?:number;outputs:Array<{filename:string;subfolder:string;mediaType:string}>;error?:string }
type UpdateCandidate = { version:string;fileName:string;downloadUrl:string;sha256?:string;sha256Url?:string;preRelease:boolean }
type DownloadedUpdate = { version:string;installerPath:string;sha256:string }
type PluginLock = { plugins:Record<string,{version:string;enabled:boolean;sha256:string;provides:string[]}> }
type PluginInspection = { package:{manifest:{id:string;name:string;version:string;provides:string[];license?:string};packageSha256:string;files:string[]};compatibility:{compatible:boolean;missingNodes:string[];conflicts:string[];provides:string[];reasons:string[]} }
type BenchmarkState = {startedAt:number;mode:string;width:number;height:number;duration:number;steps:number;modelFile:string;gpuName:string;driverVersion:string;vramTotal:number;peakVram:number;ramTotal:number;peakRam:number}
type BenchmarkReport = {reportId:string;createdAt:number;gpuName:string;vramTotalMb:number;peakVramUsedMb:number;peakRamUsedMb:number;generationMode:string;width:number;height:number;durationSeconds:number;elapsedSeconds:number;outcome:string;enabledPlugins:string[]}
type ManagedRuntimeStatus = {running:boolean;pid?:number;endpoint?:string;startedAt?:number;exitCode?:number}
type Engine = 'local'|'cloud'
type ApiKeyStatus = {configured:boolean;maskedHint?:string}
type CloudStart = {taskId:string;status?:string}
type CloudPoll = {status:'queued'|'running'|'completed'|'failed';taskId:string;progress?:number;fileId?:string;error?:string}
type ManagedNodeItem = {id:string;name:string;repository:string;commit:string;license:string;category:string;evidenceLevel:string;evidenceUrl:string;requiredNodes:string[];experimental:boolean;installable:boolean;description:string}
type ManagedNodeState = {id:string;installed:boolean;installedCommit?:string;restartRequired:boolean;verified:boolean;category:string;evidenceLevel:string}
type SshTunnelStatus = {running:boolean;pid?:number;endpoint?:string;localPort?:number;startedAt?:number;exitCode?:number;phase:'starting'|'ready'|'stopped'|'failed';errorCode?:string;error?:string}
type AutoDlProbe = {os:string;gpus:Array<{name:string;vramMib?:number;driverVersion?:string}>;totalVramMib:number;ramTotalMib?:number;python?:string;disks:Array<{availableBytes?:number;mountPoint:string}>;comfyuiCandidates:Array<{path:string;h3SourceFiles:Array<{relativePath:string;present:boolean}>;modelVariants:Array<{id:string;files:Array<{relativePath:string;expectedSizeBytes:number;present:boolean;sizeBytes:number}>}>;kjH3SageAttentionPresent:boolean}>}
type AutoDlDeployPlan = {deploymentId:string;targetPath:string;remoteComfyPort:number;requiredBytes:number;availableBytes:number;downloadFiles:Array<{relativePath:string;size:number;sha256:string;url:string}>;rollbackSupported:boolean;warnings:string[]}
type AutoDlDeployProgress = {sequence:number;stage:string;message:string}
type AutoDlDeployPrepareResult = {plan:AutoDlDeployPlan;progress:AutoDlDeployProgress[];scriptSha256:string}
type AutoDlDownloadMessage = {state?:string;file?:string;relativePath?:string;downloadedBytes?:number;size?:number;speedBps?:number;etaSeconds?:number;pid?:number;error?:string}

const parseAutoDlDownloadMessage = (value?:string):AutoDlDownloadMessage => {try{return value?JSON.parse(value):{}}catch{return {}}}

const modeContent = {
  text: { title: '文字生成视频', desc: '只需描述画面、镜头和声音，适合从零开始创作。' },
  frames: { title: '首尾帧生成', desc: '上传首帧或首尾帧，精确控制视频的开始与结束。' },
  reference: { title: '全模态参考', desc: '用图片、视频和音频共同定义角色、动作与声音。' },
}

function readDraft(): Partial<{prompt:string;duration:number;steps:number;mode:Mode}> {
  try{return JSON.parse(localStorage.getItem('langbai-h3-draft')||'{}')}catch{return {}}
}
function readPreference(key:string,fallback:string):string { try{return localStorage.getItem(key)||fallback}catch{return fallback} }

function App() {
  const initialDraft = useRef(readDraft()).current
  const [dark, setDark] = useState(false)
  const [mode, setMode] = useState<Mode>(initialDraft.mode&&['text','frames','reference'].includes(initialDraft.mode)?initialDraft.mode:'text')
  const [advanced, setAdvanced] = useState(false)
  const [generating, setGenerating] = useState(false)
  const [duration, setDuration] = useState(initialDraft.duration||8)
  const [steps, setSteps] = useState(initialDraft.steps||20)
  const [prompt, setPrompt] = useState(initialDraft.prompt||'一艘银白色飞船掠过紫色星云，镜头缓慢推进。远处恒星闪烁，配合低沉而辽阔的环境音。')
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
  const [modelLinkMessage, setModelLinkMessage] = useState('')
  const [jobMessage, setJobMessage] = useState('')
  const [historyDialog, setHistoryDialog] = useState(false)
  const [jobs, setJobs] = useState<JobRecord[]>([])
  const [historyLoading, setHistoryLoading] = useState(false)
  const [settingsDialog, setSettingsDialog] = useState(false)
  const [outputPath, setOutputPath] = useState(()=>readPreference('langbai-h3-output-path',''))
  const [outputMode, setOutputMode] = useState<'default'|'ask'>(()=>readPreference('langbai-h3-output-mode','default')==='ask'?'ask':'default')
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
  const [activeJobId, setActiveJobId] = useState('')
  const [generationPoll, setGenerationPoll] = useState<GenerationPoll | null>(null)
  const [checkingUpdate, setCheckingUpdate] = useState(false)
  const [updateCandidate, setUpdateCandidate] = useState<UpdateCandidate | null>(null)
  const [updateProgress, setUpdateProgress] = useState<TransferProgress | null>(null)
  const [downloadedUpdate, setDownloadedUpdate] = useState<DownloadedUpdate | null>(null)
  const [updateMessage, setUpdateMessage] = useState('')
  const [pluginDialog, setPluginDialog] = useState(false)
  const [pluginLock, setPluginLock] = useState<PluginLock>({plugins:{}})
  const [pluginPackagePath, setPluginPackagePath] = useState('')
  const [pluginInspection, setPluginInspection] = useState<PluginInspection | null>(null)
  const [pluginMessage, setPluginMessage] = useState('')
  const [helpDialog, setHelpDialog] = useState(false)
  const benchmarkRef = useRef<BenchmarkState | null>(null)
  const [benchmarkReports, setBenchmarkReports] = useState<BenchmarkReport[]>([])
  const [memoryProfile, setMemoryProfile] = useState<'auto'|'conservative'|'minimum'|'eight_gb'>('auto')
  const [managedStatus, setManagedStatus] = useState<ManagedRuntimeStatus | null>(null)
  const [managedMessage, setManagedMessage] = useState('')
  const [workflowDialog, setWorkflowDialog] = useState(false)
  const [engine, setEngine] = useState<Engine>('local')
  const [apiKey, setApiKey] = useState('')
  const [apiKeyStatus, setApiKeyStatus] = useState<ApiKeyStatus>({configured:false})
  const [apiKeyMessage, setApiKeyMessage] = useState('')
  const [cloudResolution, setCloudResolution] = useState<'768P'|'1080P'>('768P')
  const [cloudTask, setCloudTask] = useState<CloudPoll|null>(null)
  const [localResolution, setLocalResolution] = useState<'608x352'|'736x416'|'1344x768'>('1344x768')
  const [seed, setSeed] = useState(()=>Math.floor(Math.random()*Number.MAX_SAFE_INTEGER))
  const [seedLocked, setSeedLocked] = useState(false)
  const [referenceImageSize, setReferenceImageSize] = useState<'match'|'max'>('match')
  const [managedNodeCatalog, setManagedNodeCatalog] = useState<ManagedNodeItem[]>([])
  const [managedNodeStates, setManagedNodeStates] = useState<ManagedNodeState[]>([])
  const [managedNodeBusy, setManagedNodeBusy] = useState('')
  const [managedNodeProgress, setManagedNodeProgress] = useState<TransferProgress|null>(null)
  const [acceleration, setAcceleration] = useState<'native'|'kj_h3_sage_attention'>('native')
  const [sshHost, setSshHost] = useState('')
  const [sshUser, setSshUser] = useState('root')
  const [sshPort, setSshPort] = useState(22)
  const [sshRemotePort, setSshRemotePort] = useState(8188)
  const [sshIdentity, setSshIdentity] = useState('')
  const [sshKnownHosts, setSshKnownHosts] = useState('')
  const [sshStatus, setSshStatus] = useState<SshTunnelStatus|null>(null)
  const [sshMessage, setSshMessage] = useState('')
  const [sshBusy, setSshBusy] = useState(false)
  const [autodlSshCommand, setAutodlSshCommand] = useState('')
  const [autoDlProbe, setAutoDlProbe] = useState<AutoDlProbe|null>(null)
  const [autoDlProbeBusy, setAutoDlProbeBusy] = useState(false)
  const [autoDlVariants, setAutoDlVariants] = useState<Array<'fl2va'|'ref2va'>>(['fl2va'])
  const [autoDlDeployPlan, setAutoDlDeployPlan] = useState<AutoDlDeployPlan|null>(null)
  const [autoDlPlanBusy, setAutoDlPlanBusy] = useState(false)
  const [autoDlPrepareBusy, setAutoDlPrepareBusy] = useState(false)
  const [autoDlPrepareResult, setAutoDlPrepareResult] = useState<AutoDlDeployPrepareResult|null>(null)
  const [autoDlStatusBusy, setAutoDlStatusBusy] = useState(false)
  const [autoDlRollbackArmed, setAutoDlRollbackArmed] = useState(false)
  const [autoDlRollbackBusy, setAutoDlRollbackBusy] = useState(false)
  const [autoDlDownloadBusy, setAutoDlDownloadBusy] = useState(false)
  const [autoDlDownloadActive, setAutoDlDownloadActive] = useState(false)
  useEffect(() => {
    invoke<SystemProbe>('probe_system').then(value=>{setSystem(value);if(value.gpu&&value.gpu.memoryTotalMb<10000){setMemoryProfile('eight_gb');setLocalResolution('608x352')}else if(value.gpu&&value.gpu.memoryTotalMb<16000)setMemoryProfile('conservative')}).catch(() => {})
    invoke<RuntimeManifest[]>('runtime_manifests').then(setRuntimeManifests).catch(()=>{})
    invoke<ModelBundle[]>('model_bundles').then(setModelBundles).catch(()=>{})
    invoke<H3PatchManifest>('h3_preview_patch_manifest').then(setH3Patch).catch(()=>{})
    invoke<ApiKeyStatus>('minimax_api_key_status').then(setApiKeyStatus).catch(()=>{})
    const unlisteners = Promise.all([
      listen<TransferProgress>('runtime-download-progress', event=>setRuntimeProgress(event.payload)),
      listen<TransferProgress>('runtime-install-progress', event=>setRuntimeProgress(event.payload)),
      listen<ModelBundleFileEvent>('model-bundle-file', event=>{setModelFile(event.payload);setModelProgress(null)}),
      listen<ModelDownloadEvent>('model-download-progress', event=>setModelProgress(event.payload.progress)),
      listen<TransferProgress>('h3-patch-download-progress', event=>setH3PatchProgress(event.payload)),
      listen<TransferProgress>('update-download-progress', event=>setUpdateProgress(event.payload)),
      listen<TransferProgress>('managed-node-download-progress', event=>setManagedNodeProgress(event.payload)),
    ]).catch(()=>[] as Array<()=>void>)
    return ()=>{unlisteners.then(items=>items.forEach(fn=>fn()))}
  }, [])
  useEffect(()=>{try{localStorage.setItem('langbai-h3-draft',JSON.stringify({prompt,duration,steps,mode}))}catch{}},[prompt,duration,steps,mode])
  useEffect(()=>{try{localStorage.setItem('langbai-h3-output-path',outputPath)}catch{}},[outputPath])
  useEffect(()=>{try{localStorage.setItem('langbai-h3-output-mode',outputMode)}catch{}},[outputMode])
  useEffect(()=>{if(outputPath)return;invoke<string>('default_output_path').then(setOutputPath).catch(error=>setPathMessage(String(error)))},[outputPath])
  useEffect(()=>{
    if(!activePromptId) return
    let stopped=false
    const poll=async()=>{
      try{
        const hardware=await invoke<SystemProbe>('probe_system').catch(()=>null)
        if(hardware&&benchmarkRef.current){benchmarkRef.current.peakRam=Math.max(benchmarkRef.current.peakRam,hardware.memoryUsedMb);if(hardware.gpu){benchmarkRef.current.peakVram=Math.max(benchmarkRef.current.peakVram,hardware.gpu.memoryUsedMb);benchmarkRef.current.gpuName=hardware.gpu.name;benchmarkRef.current.driverVersion=hardware.gpu.driverVersion;benchmarkRef.current.vramTotal=hardware.gpu.memoryTotalMb}}
        const value=await invoke<GenerationPoll>('comfy_poll_generation',{baseUrl:comfyUrl,promptId:activePromptId})
        if(stopped) return
        setGenerationPoll(value)
        if(activeJobId&&['queued','running'].includes(value.status))await invoke('update_job',{patch:{id:activeJobId,status:value.status,stage:value.status==='running'?'正在生成':'等待 ComfyUI',progress:value.status==='running'?0.5:0.1,planJson:null,outputPath:null,errorSummary:null}}).catch(()=>{})
        if(value.status==='completed'){
          const saved:string[]=[]
          for(const asset of value.outputs.filter(item=>item.mediaType==='video')){
            saved.push(await invoke<string>('comfy_save_output',{baseUrl:comfyUrl,asset,outputDirectory:outputPath}))
          }
          const sample=benchmarkRef.current
          if(sample)await invoke('benchmark_save',{report:{schemaVersion:1,reportId:value.promptId,createdAt:Math.floor(Date.now()/1000),studioVersion:'0.9.0',gpuName:sample.gpuName||'未检测到',driverVersion:sample.driverVersion,vramTotalMb:sample.vramTotal,peakVramUsedMb:sample.peakVram,ramTotalMb:sample.ramTotal,peakRamUsedMb:sample.peakRam,runtimeVersion:comfyUrl,h3PatchCommit:h3Patch?.commit||'',generationMode:sample.mode,width:sample.width,height:sample.height,durationSeconds:sample.duration,steps:sample.steps,modelFile:sample.modelFile,enabledPlugins:Object.entries(pluginLock.plugins).filter(([,item])=>item.enabled).map(([id])=>id),elapsedSeconds:(Date.now()-sample.startedAt)/1000,outcome:'completed',errorSummary:'',outputFile:saved[0]||''}}).catch(()=>{})
          benchmarkRef.current=null
          if(activeJobId)await invoke('update_job',{patch:{id:activeJobId,status:'completed',stage:'已完成',progress:1,planJson:null,outputPath:saved[0]||null,errorSummary:null}}).catch(()=>{})
          setJobMessage(saved.length?`生成完成，已保存到 ${saved.join('、')}`:'生成完成，但历史记录中没有视频输出')
          setActivePromptId('');setActiveJobId('');setGenerating(false)
        }else if(value.status==='failed'){
          const sample=benchmarkRef.current
          if(sample)await invoke('benchmark_save',{report:{schemaVersion:1,reportId:value.promptId,createdAt:Math.floor(Date.now()/1000),studioVersion:'0.9.0',gpuName:sample.gpuName||'未检测到',driverVersion:sample.driverVersion,vramTotalMb:sample.vramTotal,peakVramUsedMb:sample.peakVram,ramTotalMb:sample.ramTotal,peakRamUsedMb:sample.peakRam,runtimeVersion:comfyUrl,h3PatchCommit:h3Patch?.commit||'',generationMode:sample.mode,width:sample.width,height:sample.height,durationSeconds:sample.duration,steps:sample.steps,modelFile:sample.modelFile,enabledPlugins:Object.entries(pluginLock.plugins).filter(([,item])=>item.enabled).map(([id])=>id),elapsedSeconds:(Date.now()-sample.startedAt)/1000,outcome:'failed',errorSummary:value.error||'ComfyUI 执行失败',outputFile:''}}).catch(()=>{})
          if(activeJobId)await invoke('update_job',{patch:{id:activeJobId,status:'failed',stage:'生成失败',progress:1,planJson:null,outputPath:null,errorSummary:value.error||'ComfyUI 执行失败'}}).catch(()=>{})
          benchmarkRef.current=null;setActivePromptId('');setActiveJobId('');setGenerating(false)
        }
      }catch(error){if(!stopped)setJobMessage(`读取生成状态失败：${String(error)}`)}
    }
    poll();const timer=setInterval(poll,3000)
    return()=>{stopped=true;clearInterval(timer)}
  },[activePromptId,activeJobId,comfyUrl,outputPath,h3Patch?.commit,pluginLock.plugins])
  useEffect(()=>{
    if(engine!=='cloud'||!cloudTask||['completed','failed'].includes(cloudTask.status))return
    let stopped=false
    const poll=async()=>{try{const next=await invoke<CloudPoll>('minimax_cloud_poll',{taskId:cloudTask.taskId});if(stopped)return;setCloudTask(next);if(activeJobId&&['queued','running'].includes(next.status))await invoke('update_job',{patch:{id:activeJobId,status:next.status,stage:next.status==='running'?'云端生成中':'云端排队中',progress:(next.progress||0)/100,planJson:null,outputPath:null,errorSummary:null}}).catch(()=>{});if(next.status==='completed'&&next.fileId){const saved=await invoke<string>('minimax_cloud_save',{fileId:next.fileId,outputDirectory:outputPath});if(activeJobId)await invoke('update_job',{patch:{id:activeJobId,status:'completed',stage:'已完成',progress:1,planJson:null,outputPath:saved,errorSummary:null}}).catch(()=>{});setJobMessage(`云端生成完成，已保存到 ${saved}`);setActiveJobId('');setGenerating(false)}else if(next.status==='failed'){if(activeJobId)await invoke('update_job',{patch:{id:activeJobId,status:'failed',stage:'云端生成失败',progress:1,planJson:null,outputPath:null,errorSummary:next.error||'服务返回失败'}}).catch(()=>{});setJobMessage(`云端生成失败：${next.error||'服务返回失败'}`);setActiveJobId('');setGenerating(false)}}catch(error){if(!stopped){setJobMessage(`读取云端任务失败：${String(error)}`);setGenerating(false)}}}
    const timer=setInterval(poll,10000);poll();return()=>{stopped=true;clearInterval(timer)}
  },[cloudTask?.taskId,cloudTask?.status,engine,outputPath,activeJobId])
  const gpu = system?.gpu
  const gpuPercent = gpu ? Math.min(100, Math.round(gpu.memoryUsedMb / gpu.memoryTotalMb * 100)) : 31
  const h3Ready = !!probeResult && (h3Patch?.requiredNodes || []).every(node=>probeResult.h3RelatedNodes.includes(node))
  const kjSageReady = !!probeResult?.h3RelatedNodes.includes('MiniMaxH3MemoryEfficientSageAttentionPatch')

  const selectEngine=(value:Engine)=>{setEngine(value);setEngineDialog(false);setJobMessage('');if(value==='cloud'&&mode==='reference'){setMode('text');setAssets([])}if(value==='cloud'&&!([6,10] as number[]).includes(duration))setDuration(6)}
  const saveApiKey=async()=>{if(!apiKey.trim()){setApiKeyMessage('请输入 API Key');return}try{await invoke('minimax_api_key_set',{apiKey:apiKey.trim()});setApiKey('');setApiKeyStatus({configured:true});setApiKeyMessage('密钥已加密保存，界面不会回显')}catch(error){setApiKeyMessage(String(error))}}
  const deleteApiKey=async()=>{try{await invoke('minimax_api_key_delete');setApiKey('');setApiKeyStatus({configured:false});setApiKeyMessage('已删除本机保存的密钥')}catch(error){setApiKeyMessage(String(error))}}

  const testComfy = async () => {
    setProbing(true); setProbeError(''); setProbeResult(null)
    try { setProbeResult(await invoke<ComfyProbe>('probe_comfyui', { baseUrl: comfyUrl })) }
    catch (error) { setProbeError(String(error)) }
    finally { setProbing(false) }
  }

  const scanModels = async () => {
    setScanningModels(true); setModelError(''); setModelLinkMessage(''); setModelScan(null)
    try { setModelScan(await invoke<ModelScan>('scan_local_models', { root:modelPath, maxDepth:5 })) }
    catch (error) { setModelError(String(error)) }
    finally { setScanningModels(false) }
  }

  const associateModels = async () => {
    setModelLinkMessage('正在生成托管 ComfyUI 模型路径映射…')
    try{const value=await invoke<{configPath:string;fileCount:number;categories:Record<string,string[]>}>('associate_local_h3_models',{root:modelPath});setModelLinkMessage(`已关联 ${value.fileCount} 个 H3 组件 · ${Object.keys(value.categories).join('、')} · 重启托管环境后生效`)}
    catch(error){setModelLinkMessage(String(error))}
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

  const appendPromptHint = (hint:string) => setPrompt(value=>`${value.trim()}${value.trim()?'，':''}${hint}`.slice(0,2000))

  const restoreDraft = () => {
    try{const value=JSON.parse(localStorage.getItem('langbai-h3-draft')||'{}');if(typeof value.prompt==='string')setPrompt(value.prompt);if(typeof value.duration==='number')setDuration(value.duration);if(typeof value.steps==='number')setSteps(value.steps);if(['text','frames','reference'].includes(value.mode))setMode(value.mode)}catch{setJobMessage('没有可恢复的草稿')}
  }

  const createGenerationJob = async () => {
    setJobMessage('')
    let resolvedOutputPath=outputPath
    if(outputMode==='ask'){
      const selected=await open({directory:true,multiple:false,title:'选择本次视频保存目录'})
      if(typeof selected!=='string'){setJobMessage('已取消本次生成');return}
      resolvedOutputPath=selected
    }
    try{resolvedOutputPath=await invoke<string>('validate_output_path',{path:resolvedOutputPath});setOutputPath(resolvedOutputPath)}catch(error){setJobMessage(String(error));return}
    setGenerating(true)
    if(!prompt.trim()){setJobMessage('请先填写视频描述');setGenerating(false);return}
    if(mode==='frames'&&assets.length===0){setJobMessage('首尾帧模式至少需要选择一张图片');setGenerating(false);return}
    if(mode==='reference'&&assets.length===0){setJobMessage('全模态参考模式至少需要选择一个素材');setGenerating(false);return}
    if(engine==='cloud'){
      if(!apiKeyStatus.configured){setJobMessage('请先在运行引擎中设置 MiniMax API Key');setGenerating(false);setEngineDialog(true);return}
      if(mode==='frames'&&(assets.length<1||assets.some(item=>item.kind!=='image'))){setJobMessage('云端首帧/首尾帧模式需要 1–2 张图片');setGenerating(false);return}
      if(cloudResolution==='1080P'&&duration!==6){setJobMessage('Hailuo-2.3 的 1080P 当前仅支持 6 秒');setGenerating(false);return}
      try{setJobMessage('正在安全提交 MiniMax 云端任务…');const cloudRequest={prompt,mode:mode==='text'?'text':assets.length>1?'first_last_frames':'first_frame',model:mode==='frames'&&assets.length>1?'Hailuo-02':'Hailuo-2.3',resolution:cloudResolution,durationSeconds:duration,outputDirectory:resolvedOutputPath,assets:assets.map(({path,role})=>({path,role}))};const started=await invoke<CloudStart>('minimax_cloud_start',{input:cloudRequest});const job=await invoke<{id:string}>('create_job',{input:{name:`MiniMax 云端 · ${new Date().toLocaleTimeString()}`,requestJson:JSON.stringify({...cloudRequest,engine:'cloud',taskId:started.taskId}),backendId:started.taskId}});setActiveJobId(job.id);setCloudTask({taskId:started.taskId,status:'queued',progress:0});setJobMessage(`云端任务 ${job.id} 已提交`)}catch(error){setJobMessage(String(error));setGenerating(false)}return
    }
    const workflowMode=mode==='text'?'t2v':mode==='frames'?'fl2va':'ref2va'
    if(acceleration==='kj_h3_sage_attention'&&!kjSageReady){setJobMessage('请先安装 KJNodes、重启托管 Runtime 并重新检测 H3 SageAttention 节点');setGenerating(false);return}
    const [localWidth,localHeight]=localResolution.split('x').map(Number)
    const actualSeed=seedLocked?seed:Math.floor(Math.random()*Number.MAX_SAFE_INTEGER)
    setSeed(actualSeed)
    const request = { mode:workflowMode, prompt, width:localWidth, height:localHeight, durationSeconds:duration, seed:actualSeed, steps, referenceImageSize, acceleration, outputDirectory:resolvedOutputPath, baseUrl:comfyUrl, assets:assets.map(({path,mime,kind,role})=>({path,mime,kind,role})) }
    try {
      setJobMessage(assets.length?'正在上传素材并编译工作流…':'正在编译并提交工作流…')
      const started=await invoke<StartedGeneration>('start_h3_generation',{input:request})
      benchmarkRef.current={startedAt:Date.now(),mode:workflowMode,width:localWidth,height:localHeight,duration,steps,modelFile:workflowMode==='ref2va'?'minimax_h3_ref2va_pruned_int8_convrot.safetensors':'minimax_h3_fl2va_pruned_int8_convrot.safetensors',gpuName:system?.gpu?.name||'',driverVersion:system?.gpu?.driverVersion||'',vramTotal:system?.gpu?.memoryTotalMb||0,peakVram:system?.gpu?.memoryUsedMb||0,ramTotal:system?.memoryTotalMb||0,peakRam:system?.memoryUsedMb||0}
      const job = await invoke<{id:string}>('create_job', { input:{ name:`${modeContent[mode].title} · ${new Date().toLocaleTimeString()}`, requestJson:JSON.stringify({...request,promptId:started.promptId}), backendId:started.promptId } })
      setActiveJobId(job.id);setActivePromptId(started.promptId);setGenerationPoll({status:'queued',promptId:started.promptId,queuePosition:started.queueNumber,outputs:[]})
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

  const reuseJob = async (job:JobRecord) => {
    try{
      const value=JSON.parse(job.requestJson) as Record<string,unknown>
      if(typeof value.prompt==='string')setPrompt(value.prompt)
      if(typeof value.durationSeconds==='number')setDuration(value.durationSeconds)
      if(typeof value.steps==='number')setSteps(value.steps)
      if(typeof value.outputDirectory==='string')setOutputPath(value.outputDirectory)
      if(value.engine==='cloud'){
        setEngine('cloud');setMode(value.mode==='text'?'text':'frames')
        if(value.resolution==='768P'||value.resolution==='1080P')setCloudResolution(value.resolution)
      }else{
        setEngine('local');setMode(value.mode==='t2v'?'text':value.mode==='fl2va'?'frames':'reference')
        if(typeof value.width==='number'&&typeof value.height==='number'){
          const resolution=`${value.width}x${value.height}`
          if(['608x352','736x416','1344x768'].includes(resolution))setLocalResolution(resolution as '608x352'|'736x416'|'1344x768')
        }
        if(value.acceleration==='native'||value.acceleration==='kj_h3_sage_attention')setAcceleration(value.acceleration)
        if(typeof value.seed==='number'){setSeed(value.seed);setSeedLocked(true)}
        if(value.referenceImageSize==='match'||value.referenceImageSize==='max')setReferenceImageSize(value.referenceImageSize)
      }
      const stored=Array.isArray(value.assets)?value.assets as Array<{path:string;role:LocalAsset['role']}>:[]
      if(stored.length){const inspected=await invoke<Array<Omit<LocalAsset,'role'|'status'>>>('inspect_input_files',{paths:stored.map(item=>item.path)});setAssets(inspected.map((item,index)=>({...item,role:stored[index].role,status:'selected'})))}else setAssets([])
      setHistoryDialog(false);setJobMessage(stored.length?'已恢复参数和仍然存在的本地素材':'已恢复任务参数')
    }catch(error){setJobMessage(`恢复任务设置失败：${String(error)}`);setHistoryDialog(false)}
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

  const startManagedRuntime = async () => {
    setManagedMessage('正在启动托管 ComfyUI…')
    try{const value=await invoke<ManagedRuntimeStatus>('runtime_start',{memoryProfile});setManagedStatus(value);if(value.endpoint){setComfyUrl(value.endpoint);setManagedMessage(`托管 ComfyUI 已启动 · ${value.endpoint}`)}}
    catch(error){setManagedMessage(String(error))}
  }

  const stopManagedRuntime = async () => {
    try{const value=await invoke<ManagedRuntimeStatus>('runtime_stop');setManagedStatus(value);setManagedMessage('托管 ComfyUI 已停止')}
    catch(error){setManagedMessage(String(error))}
  }

  const chooseSshFile = async (kind:'identity'|'knownHosts') => {
    const selected=await open({multiple:false,title:kind==='identity'?'选择 SSH 私钥':'选择已核验的 known_hosts 文件'})
    if(typeof selected==='string'){if(kind==='identity')setSshIdentity(selected);else setSshKnownHosts(selected)}
  }

  const applyAutoDlSshCommand = () => {
    const match=autodlSshCommand.trim().match(/^ssh\s+(?:-p\s+(\d+)\s+)?([A-Za-z0-9._-]+)@([A-Za-z0-9.:-]+)(?:\s+-p\s+(\d+))?$/i)
    if(!match){setSshMessage('AutoDL SSH 命令格式应类似：ssh -p 10309 root@connect.example.com');return}
    const port=Number(match[1]||match[4]||22)
    if(port<1||port>65535){setSshMessage('AutoDL SSH 端口无效');return}
    setSshUser(match[2]);setSshHost(match[3]);setSshPort(port);setSshMessage('已读取 AutoDL SSH 主机、用户名和端口；请继续选择私钥与 known_hosts')
  }

  const startSshTunnel = async () => {
    setSshBusy(true);setSshMessage('正在建立加密隧道并等待远端 ComfyUI…')
    try{const value=await invoke<SshTunnelStatus>('ssh_tunnel_start',{config:{host:sshHost,user:sshUser,port:sshPort,identityFile:sshIdentity,knownHostsFile:sshKnownHosts,remoteComfyPort:sshRemotePort}});setSshStatus(value);if(value.endpoint){setComfyUrl(value.endpoint);setSshMessage(`远程 RTX 工作站已通过本地加密隧道连接 · ${value.endpoint}`);setProbeResult(null)}}
    catch(error){setSshMessage(String(error))}finally{setSshBusy(false)}
  }

  const probeAutoDl = async () => {
    setAutoDlProbeBusy(true);setAutoDlProbe(null);setSshMessage('正在只读检查 AutoDL 显卡、内存、ComfyUI、H3 节点和模型…')
    try{const value=await invoke<AutoDlProbe>('autodl_remote_probe',{config:{host:sshHost,user:sshUser,port:sshPort,identityFile:sshIdentity,knownHostsFile:sshKnownHosts,remoteComfyPort:sshRemotePort}});setAutoDlProbe(value);setSshMessage(value.comfyuiCandidates.length?'远端环境检查完成':'已连接远端，但未找到常见路径下的 ComfyUI')}
    catch(error){setSshMessage(String(error))}finally{setAutoDlProbeBusy(false)}
  }

  const preflightAutoDl = async () => {
    if(!autoDlProbe||autoDlVariants.length===0)return
    const availableBytes=Math.max(0,...autoDlProbe.disks.map(d=>d.availableBytes||0))
    setAutoDlPlanBusy(true);setAutoDlDeployPlan(null);setSshMessage('正在计算隔离部署目录、共享模型和磁盘需求…')
    try{const value=await invoke<AutoDlDeployPlan>('autodl_deploy_preflight',{input:{connection:{host:sshHost,user:sshUser,port:sshPort,identityFile:sshIdentity,knownHostsFile:sshKnownHosts,remoteComfyPort:sshRemotePort},target:'studio_managed',variants:autoDlVariants,acceleration:'native',modelStrategy:'reuse_then_download_missing',remoteComfyPort:sshRemotePort,availableBytes}});setAutoDlDeployPlan(value);setSshMessage('AutoDL 隔离部署计划已生成')}
    catch(error){setSshMessage(String(error))}finally{setAutoDlPlanBusy(false)}
  }

  const prepareAutoDl = async () => {
    if(!autoDlProbe||!autoDlDeployPlan)return
    const availableBytes=Math.max(0,...autoDlProbe.disks.map(d=>d.availableBytes||0))
    setAutoDlPrepareBusy(true);setAutoDlPrepareResult(null);setSshMessage('正在通过固定 SSH 脚本创建远端隔离目录并写入部署清单…')
    try{const value=await invoke<AutoDlDeployPrepareResult>('autodl_deploy_prepare',{input:{connection:{host:sshHost,user:sshUser,port:sshPort,identityFile:sshIdentity,knownHostsFile:sshKnownHosts,remoteComfyPort:sshRemotePort},target:'studio_managed',variants:autoDlVariants,acceleration:'native',modelStrategy:'reuse_then_download_missing',remoteComfyPort:sshRemotePort,availableBytes},deploymentId:autoDlDeployPlan.deploymentId});setAutoDlPrepareResult(value);setSshMessage('AutoDL 隔离部署目录与清单已创建')}
    catch(error){setSshMessage(String(error))}finally{setAutoDlPrepareBusy(false)}
  }

  const refreshAutoDlStatus = async () => {
    if(!autoDlPrepareResult)return
    setAutoDlStatusBusy(true);setSshMessage('正在从 AutoDL 读取持久化部署记录…')
    try{const progress=await invoke<AutoDlDeployProgress[]>('autodl_deploy_status',{config:{host:sshHost,user:sshUser,port:sshPort,identityFile:sshIdentity,knownHostsFile:sshKnownHosts,remoteComfyPort:sshRemotePort},deploymentId:autoDlPrepareResult.plan.deploymentId});setAutoDlPrepareResult({...autoDlPrepareResult,progress});setSshMessage('远端部署记录已恢复')}
    catch(error){setSshMessage(String(error))}finally{setAutoDlStatusBusy(false)}
  }

  const rollbackAutoDl = async () => {
    if(!autoDlPrepareResult)return
    if(!autoDlRollbackArmed){setAutoDlRollbackArmed(true);setSshMessage('再次点击“确认回滚”将只删除本次 Studio 隔离准备目录');return}
    setAutoDlRollbackBusy(true);setSshMessage('正在核对目录内容并执行精确回滚…')
    try{const progress=await invoke<AutoDlDeployProgress[]>('autodl_deploy_rollback',{config:{host:sshHost,user:sshUser,port:sshPort,identityFile:sshIdentity,knownHostsFile:sshKnownHosts,remoteComfyPort:sshRemotePort},deploymentId:autoDlPrepareResult.plan.deploymentId});setSshMessage(progress.at(-1)?.message||'远端隔离准备目录已回滚');setAutoDlPrepareResult(null);setAutoDlDeployPlan(null);setAutoDlRollbackArmed(false)}
    catch(error){setSshMessage(String(error));setAutoDlRollbackArmed(false)}finally{setAutoDlRollbackBusy(false)}
  }

  const startAutoDlDownload = async () => {
    if(!autoDlPrepareResult)return
    setAutoDlDownloadBusy(true);setSshMessage('正在上传固定下载 worker 并启动 AutoDL 后台任务…')
    try{const progress=await invoke<AutoDlDeployProgress[]>('autodl_model_download_start',{config:{host:sshHost,user:sshUser,port:sshPort,identityFile:sshIdentity,knownHostsFile:sshKnownHosts,remoteComfyPort:sshRemotePort},deploymentId:autoDlPrepareResult.plan.deploymentId});setAutoDlPrepareResult({...autoDlPrepareResult,progress:[...autoDlPrepareResult.progress,...progress]});setAutoDlDownloadActive(true);setSshMessage('AutoDL 模型后台下载已启动；关闭 Studio 后远端仍会继续')}
    catch(error){setSshMessage(String(error))}finally{setAutoDlDownloadBusy(false)}
  }

  const cancelAutoDlDownload = async () => {
    if(!autoDlPrepareResult)return
    setAutoDlDownloadBusy(true);setSshMessage('正在写入远端取消标记；当前分块结束后停止…')
    try{await invoke<AutoDlDeployProgress[]>('autodl_model_download_cancel',{config:{host:sshHost,user:sshUser,port:sshPort,identityFile:sshIdentity,knownHostsFile:sshKnownHosts,remoteComfyPort:sshRemotePort},deploymentId:autoDlPrepareResult.plan.deploymentId});setAutoDlDownloadActive(true);setSshMessage('取消请求已写入远端，已下载的 .part 文件会保留用于续传')}
    catch(error){setSshMessage(String(error))}finally{setAutoDlDownloadBusy(false)}
  }

  useEffect(()=>{if(!autoDlDownloadActive||!autoDlPrepareResult)return;const timer=window.setInterval(async()=>{try{const progress=await invoke<AutoDlDeployProgress[]>('autodl_deploy_status',{config:{host:sshHost,user:sshUser,port:sshPort,identityFile:sshIdentity,knownHostsFile:sshKnownHosts,remoteComfyPort:sshRemotePort},deploymentId:autoDlPrepareResult.plan.deploymentId});setAutoDlPrepareResult(current=>current?{...current,progress}:current);const last=progress.at(-1);if(last&&(last.stage==='completed'||last.stage==='failed'))setAutoDlDownloadActive(false)}catch{}},2500);return()=>window.clearInterval(timer)},[autoDlDownloadActive,autoDlPrepareResult?.plan.deploymentId,sshHost,sshUser,sshPort,sshIdentity,sshKnownHosts,sshRemotePort])

  const stopSshTunnel = async () => {
    try{const value=await invoke<SshTunnelStatus>('ssh_tunnel_stop');setSshStatus(value);setSshMessage('远程 SSH 隧道已断开');setProbeResult(null)}catch(error){setSshMessage(String(error))}
  }

  const checkUpdate = async () => {
    setCheckingUpdate(true);setUpdateMessage('');setUpdateCandidate(null);setDownloadedUpdate(null)
    try{
      const candidate=await invoke<UpdateCandidate|null>('update_check_github',{includePreRelease:true})
      setUpdateCandidate(candidate);setUpdateMessage(candidate?`发现 ${candidate.version}${candidate.preRelease?' 预览版':''}`:'当前已经是最新版本')
    }catch(error){setUpdateMessage(String(error))}finally{setCheckingUpdate(false)}
  }

  const downloadUpdate = async () => {
    if(!updateCandidate)return
    setUpdateMessage('正在下载并校验更新…');setUpdateProgress({phase:'preparing',progressPercent:0})
    try{const value=await invoke<DownloadedUpdate>('update_download_candidate',{candidate:updateCandidate});setDownloadedUpdate(value);setUpdateMessage('更新安装包已下载并通过 SHA-256 校验')}
    catch(error){setUpdateMessage(String(error))}
  }

  const launchUpdate = async () => {
    if(downloadedUpdate)await invoke('update_launch_installer',{installerPath:downloadedUpdate.installerPath})
  }

  const openPlugins = async () => {
    setPluginDialog(true);setPluginMessage('');setPluginInspection(null)
    try{
      const [lock,catalog,states]=await Promise.all([invoke<PluginLock>('plugin_list'),invoke<ManagedNodeItem[]>('managed_nodes_catalog'),invoke<ManagedNodeState[]>('managed_nodes_status')])
      setPluginLock(lock);setManagedNodeCatalog(catalog);setManagedNodeStates(states)
    }catch(error){setPluginMessage(String(error))}
  }

  const installManagedNode = async (id:string) => {
    setManagedNodeBusy(id);setManagedNodeProgress({phase:'preparing',progressPercent:0});setPluginMessage('正在下载、校验并安装固定版本社区节点…')
    try{await invoke('managed_nodes_install',{id});setManagedNodeStates(await invoke<ManagedNodeState[]>('managed_nodes_status'));setPluginMessage('社区节点已安装；请重启托管 Runtime 并重新检测节点后再启用')}
    catch(error){setPluginMessage(String(error))}finally{setManagedNodeBusy('')}
  }

  const uninstallManagedNode = async (id:string) => {
    setManagedNodeBusy(id);try{await invoke('managed_nodes_uninstall',{id});setManagedNodeStates(await invoke<ManagedNodeState[]>('managed_nodes_status'));if(id==='kijai.comfyui-kjnodes')setAcceleration('native');setPluginMessage('社区节点已卸载；重启 Runtime 后生效')}
    catch(error){setPluginMessage(String(error))}finally{setManagedNodeBusy('')}
  }

  const openSettings = async () => {
    setSettingsDialog(true)
    invoke<BenchmarkReport[]>('benchmark_list').then(setBenchmarkReports).catch(()=>setBenchmarkReports([]))
  }

  const choosePluginPackage = async () => {
    const selected=await open({multiple:false,filters:[{name:'H3 声明式插件',extensions:['h3plugin']}]})
    if(typeof selected!=='string')return
    setPluginPackagePath(selected);setPluginMessage('正在检查插件包、依赖和冲突…')
    try{const value=await invoke<PluginInspection>('plugin_inspect',{path:selected,baseUrl:comfyUrl});setPluginInspection(value);setPluginMessage(value.compatibility.compatible?'插件包验证通过':'插件当前不兼容')}
    catch(error){setPluginInspection(null);setPluginMessage(String(error))}
  }

  const installPlugin = async () => {
    if(!pluginInspection)return
    try{await invoke('plugin_install',{path:pluginPackagePath,expectedSha256:pluginInspection.package.packageSha256,baseUrl:comfyUrl});setPluginLock(await invoke<PluginLock>('plugin_list'));setPluginInspection(null);setPluginMessage('插件已安装并启用')}
    catch(error){setPluginMessage(String(error))}
  }

  const togglePlugin = async (id:string,enabled:boolean) => {
    await invoke('plugin_set_enabled',{id,enabled});setPluginLock(await invoke<PluginLock>('plugin_list'))
  }

  const uninstallPlugin = async (id:string) => {
    await invoke('plugin_uninstall',{id});setPluginLock(await invoke<PluginLock>('plugin_list'));setPluginMessage('插件已卸载')
  }

  const installModelBundle = async () => {
    setInstallingModel(true); setModelInstallMessage(''); setModelProgress({phase:'preparing',progressPercent:0})
    try {
      const installed = await invoke<{modelRoot:string;totalSize:number}>('download_h3_bundle',{bundleId:selectedBundle,licenseAccepted})
      setModelProgress({phase:'completed',progressPercent:100})
      setModelInstallMessage(`模型安装完成并通过校验 · ${(installed.totalSize/1024/1024/1024).toFixed(1)} GiB · ${installed.modelRoot}`)
    } catch(error) { setModelInstallMessage(String(error)) }
    finally { setInstallingModel(false) }
  }

  return (
    <div className={dark ? 'app dark' : 'app'}>
      <aside className="sidebar">
        <div className="brand"><div className="brand-mark"><Aperture size={20}/></div><div><strong>Langbai H3</strong><span>Studio</span></div></div>
        <nav aria-label="主导航">
          <button className="nav-item active"><WandSparkles/><span>开始创作</span></button>
          <button className="nav-item" onClick={openHistory}><Clock3/><span>生成记录</span>{jobs.length>0&&<b>{jobs.length}</b>}</button>
          <button className="nav-item" onClick={()=>setModelDialog(true)}><Boxes/><span>模型中心</span></button>
          <button className="nav-item" onClick={openPlugins}><Zap/><span>加速插件</span><b>{Object.keys(pluginLock.plugins).length||''}</b></button>
          <button className="nav-item" onClick={()=>setWorkflowDialog(true)}><LayoutGrid/><span>工作流</span></button>
        </nav>
        <div className="sidebar-bottom">
          <div className="runtime-card"><div className="runtime-title"><span className="status-dot"/>{gpu ? '硬件检测完成' : '原型预览模式'}</div><small>{gpu ? `${gpu.name} · ${(gpu.memoryTotalMb/1024).toFixed(0)} GB` : 'RTX 4090 · 24 GB'}</small><div className="memory"><span style={{width:`${gpuPercent}%`}}/></div><small>{gpu ? `显存 ${(gpu.memoryUsedMb/1024).toFixed(1)} / ${(gpu.memoryTotalMb/1024).toFixed(0)} GB` : '显存 7.4 / 24 GB'}</small></div>
          <button className="nav-item" onClick={openSettings}><Settings/><span>设置</span></button>
          <button className="nav-item" onClick={()=>setHelpDialog(true)}><CircleHelp/><span>使用帮助</span></button>
        </div>
      </aside>

      <main>
        <header className="topbar">
          <div className="mobile-brand"><Menu/><strong>Langbai H3 Studio</strong></div>
          <div className="crumb"><span>创作空间</span><ChevronRight/><strong>新建视频</strong></div>
          <div className="top-actions">
            <button className={`engine ${engine==='cloud'?'cloud':''}`} onClick={()=>setEngineDialog(true)}><span className="status-dot"/> {engine==='local'?'本地 H3':'MiniMax 云端 Hailuo API'} <ChevronDown/></button>
            <button className="icon-button" aria-label="切换主题" onClick={() => setDark(!dark)}>{dark ? <Sun/> : <Moon/>}</button>
          </div>
        </header>

        <div className="workspace">
          <section className="intro">
            <div><span className="eyebrow"><Sparkles/> 简单创作，专业生成</span><h1>今天想创造什么？</h1><p>选择一种生成方式，Langbai H3 Studio 会为你的显卡自动匹配最佳设置。</p></div>
            <button className="draft-button" onClick={restoreDraft}><RotateCcw/> 恢复上次草稿</button>
          </section>

          <div className="mode-tabs" role="tablist">
            {(Object.keys(modeContent) as Mode[]).filter(key=>engine==='local'||key!=='reference').map((key, i) => <button key={key} onClick={() => {setMode(key);setAssets([])}} className={mode === key ? 'selected':''} role="tab" aria-selected={mode === key}>
              {i === 0 ? <Sparkles/> : i === 1 ? <Image/> : <Video/>}<span><strong>{modeContent[key].title}</strong><small>{modeContent[key].desc}</small></span>{mode === key && <span className="check">✓</span>}
            </button>)}
          </div>

          <div className="columns">
            <section className="creator-card">
              {mode !== 'text' && <div className="field"><div className="label-row"><label>参考素材</label><span>{assets.length}/{mode === 'frames' ? 2 : 12}</span></div><button className="dropzone" onClick={chooseAssets}><Upload/><strong>{mode==='frames'?'选择首帧或尾帧图片':'选择图片、视频或音频'}</strong><small>使用 Windows 文件选择器，可一次多选</small></button>{assetError&&<div className="inline-error"><Info/>{assetError}</div>}{assets.length>0&&<div className="asset-list">{assets.map((asset,index)=><article key={asset.path}><div className={`asset-kind ${asset.kind}`}>{asset.kind==='image'?<Image/>:asset.kind==='video'?<Video/>:<Aperture/>}</div><div><strong>{asset.name}</strong><small>{mode==='frames'?(index===0?'首帧':'尾帧'):asset.kind==='image'?'图片参考':asset.kind==='video'?'动作参考':'声音参考'} · {(asset.size/1024/1024).toFixed(asset.size>1024*1024?1:2)} MB</small></div><button onClick={()=>removeAsset(asset.path)} aria-label={`移除 ${asset.name}`}><X/></button></article>)}</div>}</div>}
              <div className="field">
                <div className="label-row"><label htmlFor="prompt">描述你的视频</label><button onClick={()=>setHelpDialog(true)}><Info/> 参数与提示词说明</button></div>
                <div className="textarea-wrap"><textarea id="prompt" value={prompt} onChange={e=>setPrompt(e.target.value)} maxLength={2000}/><span>{prompt.length} / 2000</span></div>
                <div className="suggestions"><span>试试添加：</span><button onClick={()=>appendPromptHint('镜头缓慢推进并保持主体居中')}>镜头运动</button><button onClick={()=>appendPromptHint('电影感侧逆光与柔和体积雾')}>光线氛围</button><button onClick={()=>appendPromptHint('包含与动作同步的环境音和空间回声')}>环境音效</button><button onClick={()=>appendPromptHint('角色对白清晰自然，口型与声音同步')}>对白</button></div>
              </div>

              <div className="setting-grid">
                <div className="field"><label>画面比例</label><div className="select static-select"><span><span className="ratio-icon wide"/>16:9 · 横屏</span></div></div>
                <div className="field"><label>视频时长</label><div className={`segmented ${engine==='cloud'?'two-options':''}`}>{(engine==='cloud'?(cloudResolution==='1080P'?[6]:[6,10]):[5,8,10,15]).map(n=><button key={n} onClick={()=>setDuration(n)} className={duration===n?'active':''}>{n} 秒</button>)}</div></div>
                <div className="field"><label>生成质量</label>{engine==='cloud'?<div className="segmented two-options">{(['768P','1080P'] as const).map(value=><button key={value} onClick={()=>{setCloudResolution(value);if(value==='1080P')setDuration(6)}} className={cloudResolution===value?'active':''}>{value}</button>)}</div>:<div className="segmented resolution-options">{(['608x352','736x416','1344x768'] as const).map(value=><button key={value} onClick={()=>setLocalResolution(value)} className={localResolution===value?'active':''}>{value}</button>)}</div>}</div>
                <div className="field"><label>随机种子 <Info/></label>{engine==='cloud'?<div className="select static-select"><span>由云端服务管理</span></div>:<div className="seed-control"><input type="number" min="0" max={Number.MAX_SAFE_INTEGER} value={seed} disabled={!seedLocked} onChange={e=>setSeed(Math.max(0,Math.min(Number.MAX_SAFE_INTEGER,Number(e.target.value)||0)))}/><button onClick={()=>setSeedLocked(!seedLocked)}>{seedLocked?'锁定':'自动'}</button><button aria-label="生成新随机种子" onClick={()=>setSeed(Math.floor(Math.random()*Number.MAX_SAFE_INTEGER))}><RotateCcw/></button></div>}</div>
              </div>

              {engine==='local'&&mode==='reference'&&<div className="reference-size-setting"><div><strong>参考图处理方式</strong><small>匹配输出更省显存；最高保真会将参考图短边按 H3 上限处理，参考 token 会贯穿每个采样步。</small></div><div className="segmented two-options"><button className={referenceImageSize==='match'?'active':''} onClick={()=>setReferenceImageSize('match')}>匹配输出</button><button className={referenceImageSize==='max'?'active':''} onClick={()=>setReferenceImageSize('max')}>最高保真</button></div></div>}

              {engine==='local'&&<><button className="advanced-toggle" onClick={()=>setAdvanced(!advanced)}><span><Settings/> 高级设置 <small>采样、卸载与加速选项</small></span><ChevronDown className={advanced?'rotated':''}/></button>
              {advanced && <div className="advanced-panel"><div><label>采样步数 <b>{steps}</b></label><input type="range" min="12" max="50" value={steps} onChange={event=>setSteps(Number(event.target.value))}/></div><div><label>显存策略</label><button className="select" onClick={()=>setEngineDialog(true)}><span>{memoryProfile==='auto'?'自动动态显存':memoryProfile==='conservative'?'保守低显存':memoryProfile==='eight_gb'?'8GB 极低显存实验':'最小显存'}</span><ChevronDown/></button></div><div><label>加速方案</label><button className="select" onClick={openPlugins}><span><Zap/> {acceleration==='kj_h3_sage_attention'?'KJNodes H3 SageAttention（实验）':'官方 INT8 + NVFP4 原生方案'}</span><ChevronDown/></button></div></div>}</>}
            </section>

            <aside className="summary-card">
              <div className="preview"><div className="preview-art"><div className="orbit one"/><div className="orbit two"/><Aperture/><span>生成预览将在这里显示</span></div><button className="expand">↗</button></div>
              <div className="summary-body"><h2>生成准备</h2><div className="summary-row"><span><Cpu/> 运行方案</span><strong>{engine==='cloud'?'MiniMax 云端':memoryProfile==='auto'?'动态显存':'低显存卸载'}</strong></div><div className="summary-row"><span><HardDrive/> {engine==='cloud'?'云端模型':'峰值显存'}</span><strong>{engine==='cloud'?(mode==='frames'&&assets.length>1?'Hailuo-02':'Hailuo-2.3'):'首次生成后记录'}</strong></div><div className="summary-row"><span><Clock3/> {engine==='cloud'?'规格':'预计耗时'}</span><strong>{engine==='cloud'?`${cloudResolution} · ${duration} 秒`:'由本机实测生成'}</strong></div><div className="summary-row"><span><FolderOpen/> 保存到</span><button onClick={openSettings}>{outputPath} <ChevronRight/></button></div>
                <div className="fit-notice"><ShieldCheck/><span><strong>{engine==='cloud'?(apiKeyStatus.configured?'云端密钥已配置':'需要设置 API Key'):gpu&&gpu.memoryTotalMb>=24000?'达到建议显存':'需要兼容性实测'}</strong><small>{engine==='cloud'?'使用官方付费 API，不占用本机显存；费用由 MiniMax 账户结算':gpu?`${(gpu.memoryTotalMb/1024).toFixed(0)}GB 显存 · 建议 24GB 显存与 64GB 内存`:'未检测到 NVIDIA 显卡信息'}</small></span></div>
                <button className="generate" onClick={createGenerationJob} disabled={generating||(engine==='local'&&!h3Ready)}>{generating ? <><span className="spinner"/> {engine==='cloud'?(cloudTask?.status==='running'?'云端生成中…':'云端排队中…'):generationPoll?.status==='running'?'正在生成视频…':generationPoll?.status==='queued'?'正在排队…':'正在提交素材…'}</> : <><Play/> {engine==='cloud'?'使用 Hailuo API 生成':h3Ready?'开始生成视频':'请先连接 H3 运行环境'}</>}</button><p className="queue-note">{jobMessage || (engine==='cloud'?'云端按官方 API 实际用量计费，生成结果仍保存到你的目录':h3Ready?'运行环境已通过 H3 节点校验':'在“运行环境”中安装或连接 ComfyUI，并验证 H3 必需节点')}</p>
              </div>
            </aside>
          </div>

          {engine==='local'&&<section className="download-card">
            <div className="model-icon"><Download/></div><div className="download-info"><div><strong>MiniMax-H3 单卡优化模型</strong><span className="tag">官方工作流</span></div><p>每套约 39.6 GiB（42.5 GB）· 建议 24GB 显存和 64GB 内存 · 支持断点续传与 SHA-256 校验</p><small>模型不会自动下载。请在模型中心选择文生视频或全模态参考版本，并确认模型许可证。</small></div><button onClick={()=>setModelDialog(true)}>打开模型中心</button>
          </section>}
        </div>
      </main>
      {engineDialog && <div className="modal-backdrop" role="presentation" onMouseDown={e=>{if(e.target===e.currentTarget)setEngineDialog(false)}}>
        <section className="modal" role="dialog" aria-modal="true" aria-labelledby="engine-title">
          <div className="modal-head"><div><span className="eyebrow"><Cpu/> 生成引擎</span><h2 id="engine-title">选择视频生成方式</h2></div><button className="icon-button" onClick={()=>setEngineDialog(false)} aria-label="关闭对话框"><X/></button></div>
          <div className="engine-choices">
            <button className={engine==='local'?'selected':''} onClick={()=>setEngine('local')}><Cpu/><span><strong>本地 MiniMax-H3</strong><small>模型保存在电脑中，不产生 API 费用；需要 NVIDIA 显卡及较大内存。</small></span>{engine==='local'&&<b>已选择</b>}</button>
            <button className={engine==='cloud'?'selected':''} onClick={()=>{setEngine('cloud');if(mode==='reference'){setMode('text');setAssets([])}if(![6,10].includes(duration))setDuration(6)}}><Sparkles/><span><strong>MiniMax 云端 Hailuo API</strong><small>无需下载模型和占用显存；官方服务按实际用量付费。</small></span>{engine==='cloud'&&<b>已选择</b>}</button>
          </div>
          {engine==='cloud'&&<div className="cloud-engine-panel">
            <div className="cloud-status"><ShieldCheck/><span><strong>{apiKeyStatus.configured?'API Key 已安全保存':'尚未设置 API Key'}</strong><small>密钥只提交给本机后端安全存储，保存后不会在界面回显。</small></span></div>
            <label htmlFor="minimax-key">MiniMax API Key</label>
            <div className="url-row"><input id="minimax-key" type="password" autoComplete="off" placeholder={apiKeyStatus.configured?'输入新密钥可替换当前密钥':'请输入 API Key'} value={apiKey} onChange={e=>setApiKey(e.target.value)}/><button onClick={saveApiKey}>保存密钥</button>{apiKeyStatus.configured&&<button className="danger-button" onClick={deleteApiKey}>删除</button>}</div>
            {apiKeyMessage&&<div className={`probe-message ${apiKeyMessage.includes('已')?'success':'error'}`}><Info/><span><strong>{apiKeyMessage}</strong></span></div>}
            <div className="cloud-model-guide"><article><strong>Hailuo-2.3</strong><small>文字生视频、首帧生视频 · 768P 支持 6/10 秒 · 1080P 支持 6 秒</small></article><article><strong>Hailuo-02</strong><small>首尾帧生视频 · 用两张图片明确控制开头和结尾</small></article></div>
            <div className="modal-note warning"><Info/><span><strong>云端与本地模型不是同一个运行方案</strong><small>云端使用 MiniMax 官方 Hailuo API 和账户额度，能力、价格与可用性以官方控制台为准；本地 H3 可离线运行并支持社区加速插件。</small></span></div>
            <button className="primary wide" onClick={()=>selectEngine('cloud')}>使用云端 Hailuo API</button>
          </div>}
          <div className={engine==='local'?'engine-local':'engine-local hidden'}>
          <p>新手可以一键安装独立运行环境；已有 ComfyUI 用户也可以直接连接本机实例。</p>
          <div className="runtime-options">{runtimeManifests.map((item,index)=><article key={item.version}><div className="model-icon"><Cpu/></div><div><strong>{index===0?'NVIDIA 新版运行环境':'NVIDIA 兼容运行环境'}</strong><small>{item.version} · 官方 ComfyUI v0.30.0 · 约 {index===0?'2.11':'2.05'} GB</small><span>{index===0?'适合 RTX 20 系及更新显卡':'CUDA 12.6，适合旧驱动或较老显卡'}</span></div><button onClick={()=>installRuntime(index===0?'nvidia':'nvidia-cu126')} disabled={!!installingRuntime}>{installingRuntime=== (index===0?'nvidia':'nvidia-cu126')?'安装中…':'下载并安装'}</button></article>)}</div>
          {runtimeProgress && <div className="runtime-progress"><div><span>{runtimeProgress.phase}</span><strong>{Math.round(runtimeProgress.progressPercent || 0)}%</strong></div><div className="progress"><span style={{width:`${runtimeProgress.progressPercent || 0}%`}}/></div><small>{runtimeProgress.bytesPerSecond?`${(runtimeProgress.bytesPerSecond/1024/1024).toFixed(1)} MB/s`:runtimeProgress.currentFile||'正在准备'} {runtimeProgress.etaSeconds?`· 剩余约 ${Math.ceil(runtimeProgress.etaSeconds/60)} 分钟`:''}</small></div>}
          {runtimeMessage && <div className={`probe-message ${runtimeMessage.includes('已安装')?'success':'error'}`}><ShieldCheck/><span><strong>{runtimeMessage}</strong><small>下载支持断点续传，安装前会验证官方 SHA-256。</small></span></div>}
          <div className="section-divider"><span>托管环境启动策略</span></div>
          <div className="memory-profiles">{([{id:'auto',name:'自动动态显存',desc:'推荐 16–24GB；使用 ComfyUI 默认动态卸载'},{id:'conservative',name:'保守低显存',desc:'推荐 12–16GB 实验使用；启用 lowvram 并预留 1.5GB'},{id:'eight_gb',name:'8GB 极低显存实验',desc:'8–10GB；默认动态卸载 + CPU VAE，建议至少 64GB 内存和充足页面文件'},{id:'minimum',name:'极限卸载诊断',desc:'novram + CPU VAE + 强制卸载；默认方案仍 OOM 时再尝试，可能非常慢'}] as const).map(item=><button key={item.id} className={memoryProfile===item.id?'selected':''} onClick={()=>{setMemoryProfile(item.id);if(item.id==='eight_gb')setLocalResolution('608x352')}}><strong>{item.name}</strong><small>{item.desc}</small></button>)}</div>
          {memoryProfile==='eight_gb'&&<div className="modal-note warning"><Info/><span><strong>8GB 是实验运行目标</strong><small>官方量化模型文件总量约 42GB，权重会大量卸载到系统内存；建议先用 5 秒、较低分辨率和单一素材验证。运行成功与速度仍需按显卡、内存和驱动记录实测。</small></span></div>}
          <div className="runtime-actions"><button onClick={startManagedRuntime} disabled={managedStatus?.running}>启动托管环境</button><button onClick={stopManagedRuntime} disabled={!managedStatus?.running}>停止</button></div>
          {managedMessage&&<div className={`probe-message ${managedMessage.includes('已启动')||managedMessage.includes('已停止')?'success':'error'}`}><Cpu/><span><strong>{managedMessage}</strong><small>{managedStatus?.pid?`进程 PID ${managedStatus.pid}`:'启动参数只由内置显存档位生成'}</small></span></div>}
          <div className="section-divider"><span>租用 RTX 5090 / 远程工作站</span></div>
          <p>Studio 使用 Windows OpenSSH 把远端 127.0.0.1:8188 转发到本机随机端口。远端 ComfyUI 不需要开放公网端口。</p>
          <label>AutoDL SSH 登录命令</label><div className="url-row"><input value={autodlSshCommand} onChange={e=>setAutodlSshCommand(e.target.value)} placeholder="ssh -p 10309 root@connect.example.com"/><button onClick={applyAutoDlSshCommand}>自动填写</button></div>
          <div className="ssh-grid"><label>SSH 主机<input value={sshHost} onChange={e=>setSshHost(e.target.value)} placeholder="GPU_SERVER_IP"/></label><label>用户名<input value={sshUser} onChange={e=>setSshUser(e.target.value)} /></label><label>SSH 端口<input type="number" min="1" max="65535" value={sshPort} onChange={e=>setSshPort(Number(e.target.value))}/></label><label>远端 ComfyUI 端口<input type="number" min="1" max="65535" value={sshRemotePort} onChange={e=>setSshRemotePort(Number(e.target.value))}/></label></div>
          <label>SSH 私钥</label><div className="url-row"><input value={sshIdentity} readOnly placeholder="选择专用 SSH 私钥"/><button onClick={()=>chooseSshFile('identity')}>选择</button></div>
          <label>known_hosts</label><div className="url-row"><input value={sshKnownHosts} readOnly placeholder="选择已从租用商核验指纹的 known_hosts"/><button onClick={()=>chooseSshFile('knownHosts')}>选择</button></div>
          <button className="primary wide remote-probe-button" onClick={probeAutoDl} disabled={autoDlProbeBusy||!sshHost||!sshIdentity||!sshKnownHosts}>{autoDlProbeBusy?'正在检查远端环境…':'检查 AutoDL H3 环境'}</button>
          {autoDlProbe&&<div className="remote-probe-result"><div className="remote-summary"><strong>{autoDlProbe.gpus.map(gpu=>gpu.name).join('、')||'未检测到 NVIDIA GPU'}</strong><small>{autoDlProbe.os} · 显存 {(autoDlProbe.totalVramMib/1024).toFixed(1)}GB · 内存 {autoDlProbe.ramTotalMib?(autoDlProbe.ramTotalMib/1024).toFixed(0)+'GB':'未知'} · {autoDlProbe.python||'未找到 Python'}</small></div>{autoDlProbe.comfyuiCandidates.length===0?<div className="empty-result">常见路径下未发现 ComfyUI</div>:autoDlProbe.comfyuiCandidates.map(candidate=>{const sourcesReady=candidate.h3SourceFiles.every(file=>file.present);return <article key={candidate.path}><div><strong>{candidate.path}</strong><small>{sourcesReady?'H3 源码完整':'H3 源码不完整'} · {candidate.kjH3SageAttentionPresent?'KJ H3 Sage 已安装':'未检测到 KJ H3 Sage'}</small></div>{candidate.modelVariants.map(variant=>{const ready=variant.files.every(file=>file.present&&file.sizeBytes===file.expectedSizeBytes);return <span key={variant.id} className={ready?'ready':'missing'}>{variant.id.toUpperCase()} {ready?'模型完整':'模型缺失或大小不符'}</span>})}</article>})}</div>}
          {autoDlProbe&&<div className="autodl-preflight"><div className="section-divider"><span>??????</span></div><p>Studio ????????????????????????????? VAE?</p><div className="autodl-variant-row"><label><input type="checkbox" checked={autoDlVariants.includes('fl2va')} onChange={e=>setAutoDlVariants(v=>e.target.checked?[...new Set([...v,'fl2va' as const])]:v.filter(x=>x!=='fl2va'))}/> ?????? FL2VA</label><label><input type="checkbox" checked={autoDlVariants.includes('ref2va')} onChange={e=>setAutoDlVariants(v=>e.target.checked?[...new Set([...v,'ref2va' as const])]:v.filter(x=>x!=='ref2va'))}/> ????? Ref2VA</label></div><button className="primary wide" onClick={preflightAutoDl} disabled={autoDlPlanBusy||autoDlVariants.length===0}>{autoDlPlanBusy?'????????????':'????????'}</button>{autoDlDeployPlan&&<div className="deploy-plan-card"><div><strong>{autoDlDeployPlan.targetPath}</strong><small>???? {autoDlDeployPlan.deploymentId} ? ???? {autoDlDeployPlan.rollbackSupported?'???':'???'}</small></div><div className="deploy-metrics"><span><b>{(autoDlDeployPlan.requiredBytes/1024/1024/1024).toFixed(1)} GB</b>????</span><span><b>{autoDlDeployPlan.downloadFiles.length}</b>?????</span><span><b>{(autoDlDeployPlan.availableBytes/1024/1024/1024).toFixed(0)} GB</b>????</span></div><small className="deploy-boundary">?????????????????????????????? SHA-256?</small></div>}<button className="wide autodl-prepare-button" onClick={prepareAutoDl} disabled={!autoDlDeployPlan||autoDlPrepareBusy}>{autoDlPrepareBusy?'???????????':'???????????'}</button>{autoDlPrepareResult&&<div className="deploy-journal"><ShieldCheck/><span><strong>??????</strong><small>{autoDlPrepareResult.progress.map(item=>item.message).join(' ? ')} ? ?? {autoDlPrepareResult.scriptSha256.slice(0,12)}</small></span><button className="journal-refresh" onClick={refreshAutoDlStatus} disabled={autoDlStatusBusy}>{autoDlStatusBusy?'????':'????????'}</button><button className={autoDlRollbackArmed?'journal-rollback armed':'journal-rollback'} onClick={rollbackAutoDl} disabled={autoDlRollbackBusy}>{autoDlRollbackBusy?'????':autoDlRollbackArmed?'????':'??????'}</button></div>}{autoDlPrepareResult&&(()=>{const last=autoDlPrepareResult.progress.at(-1);const data=parseAutoDlDownloadMessage(last?.message);const percent=data.size&&data.downloadedBytes?Math.min(100,data.downloadedBytes/data.size*100):0;return <div className="autodl-download-card"><div className="download-card-head"><span><strong>??????</strong><small>{data.relativePath||data.file||'????'} ? {last?.stage||'ready'}</small></span><b>{percent.toFixed(1)}%</b></div><div className="progress"><span style={{width:`${percent}%`}}/></div><div className="download-card-stats"><span>{data.downloadedBytes?`${(data.downloadedBytes/1024/1024/1024).toFixed(2)} GB`:'?'} / {data.size?`${(data.size/1024/1024/1024).toFixed(2)} GB`:'?'}</span><span>{data.speedBps?`${(data.speedBps/1024/1024).toFixed(1)} MB/s`:'????'}</span><span>{data.etaSeconds!=null?`? ${Math.ceil(data.etaSeconds/60)} ??`:'?? ETA'}</span></div><div className="download-card-actions"><button className="primary" onClick={startAutoDlDownload} disabled={autoDlDownloadBusy||autoDlDownloadActive}>{autoDlDownloadBusy?'????':autoDlDownloadActive?'?????':'??????'}</button><button onClick={cancelAutoDlDownload} disabled={autoDlDownloadBusy||!autoDlDownloadActive}>???????</button></div><small>??? AutoDL ???????????????????? .part?????????</small></div>})()}</div>}
          <div className="runtime-actions"><button onClick={startSshTunnel} disabled={sshBusy||sshStatus?.running||!sshHost||!sshIdentity||!sshKnownHosts}>{sshBusy?'连接中…':'连接远程 GPU'}</button><button onClick={stopSshTunnel} disabled={!sshStatus?.running}>断开隧道</button></div>
          {sshMessage&&<div className={`probe-message ${sshStatus?.running||sshMessage.includes('已断开')?'success':'error'}`}><ShieldCheck/><span><strong>{sshMessage}</strong><small>{sshStatus?.running?`SSH PID ${sshStatus.pid} · 远端算力，本机保存结果`:'严格校验主机密钥；不会自动信任未知服务器'}</small></span></div>}
          <div className="modal-note warning"><Info/><span><strong>远端安全前置条件</strong><small>远端 ComfyUI 只监听 127.0.0.1，云服务器防火墙只开放 SSH；首次连接前请从租用商控制台核对主机指纹并准备 known_hosts。</small></span></div>
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
          <button className="primary wide" onClick={()=>selectEngine('local')}>使用本地 H3</button>
          </div>
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
          {modelScan && <div className="model-results"><div className="result-head"><strong>发现 {modelScan.models.length} 个模型</strong><small>扫描深度 {modelScan.maxDepth} 层</small></div>{modelScan.models.length===0?<div className="empty-result">目录中没有识别到 H3 模型结构</div>:modelScan.models.map((item,i)=><article key={i}><div className="model-icon"><Boxes/></div><div><strong>{item.modelType}</strong><small>{item.directory}</small><span>{item.integrity} · {(item.totalSizeBytes/1024/1024/1024).toFixed(1)} GB · {item.files.length} 个组件</span></div></article>)}<button className="associate-all" onClick={associateModels}>关联扫描到的模型目录</button></div>}
          {modelLinkMessage&&<div className={`probe-message ${modelLinkMessage.includes('已关联')?'success':'error'}`}><ShieldCheck/><span><strong>{modelLinkMessage.includes('已关联')?'本地模型已关联':'模型关联状态'}</strong><small>{modelLinkMessage}</small></span></div>}
          <div className="modal-note"><ShieldCheck/><span><strong>保持原文件位置</strong><small>关联后通过模型路径映射复用权重，不会复制数十 GB 文件。</small></span></div>
        </section>
      </div>}
      {pluginDialog && <div className="modal-backdrop" role="presentation" onMouseDown={e=>{if(e.target===e.currentTarget)setPluginDialog(false)}}>
        <section className="modal model-modal" role="dialog" aria-modal="true" aria-labelledby="plugin-title">
          <div className="modal-head"><div><span className="eyebrow"><Zap/> 扩展能力</span><h2 id="plugin-title">加速插件</h2></div><button className="icon-button" onClick={()=>setPluginDialog(false)} aria-label="关闭对话框"><X/></button></div>
          <p>只安装声明式 `.h3plugin` 适配包。包内脚本和二进制会被拒绝；外部 ComfyUI 节点仍由用户或 Runtime 独立管理。</p>
          <div className="section-divider"><span>托管社区节点目录</span></div>
          <div className="plugin-list managed-catalog">{managedNodeCatalog.map(item=>{const state=managedNodeStates.find(value=>value.id===item.id);const isKj=item.id==='kijai.comfyui-kjnodes';return <article key={item.id}><div className="model-icon"><Zap/></div><div><strong>{item.name}</strong><small>{item.category==='h3-acceleration'?'H3 专用显存优化':'H3 社区兼容扩展'} · {item.license} · 固定 {item.commit.slice(0,8)}</small><span>{item.description}</span><span>证据：源码支持 · 实际性能尚待本机基准验证</span></div>{state?.installed?<><button disabled={isKj&&!kjSageReady} onClick={()=>isKj&&setAcceleration(acceleration==='kj_h3_sage_attention'?'native':'kj_h3_sage_attention')}>{isKj?(kjSageReady?(acceleration==='kj_h3_sage_attention'?'已用于工作流':'启用加速'):'重启并检测'):'已安装'}</button><button className="danger-link" disabled={managedNodeBusy===item.id} onClick={()=>uninstallManagedNode(item.id)}>卸载</button></>:<button disabled={!item.installable||!!managedNodeBusy} onClick={()=>installManagedNode(item.id)}>{managedNodeBusy===item.id?'安装中…':'安装固定版本'}</button>}</article>})}</div>
          {managedNodeProgress&&managedNodeBusy&&<div className="runtime-progress"><div><span>下载社区节点</span><strong>{Math.round(managedNodeProgress.progressPercent||0)}%</strong></div><div className="progress"><span style={{width:`${managedNodeProgress.progressPercent||0}%`}}/></div><small>{managedNodeProgress.bytesPerSecond?`${(managedNodeProgress.bytesPerSecond/1024/1024).toFixed(1)} MB/s`:'正在准备'}</small></div>}
          <div className="modal-note warning"><Info/><span><strong>兼容证据不等于性能承诺</strong><small>KJNodes 的 H3 SageAttention 是实验节点，只有源码适配证据；启用后 Studio 会把它真实插入 UNETLoader 与采样器之间，并单独记录基准结果。GGUF、TeaCache 当前没有 H3 支持证据，因此不展示。</small></span></div>
          <div className="section-divider"><span>声明式工作流适配包</span></div>
          <button className="primary wide" onClick={choosePluginPackage}>导入 .h3plugin 文件</button>
          {pluginInspection&&<div className="plugin-inspection"><div><strong>{pluginInspection.package.manifest.name}</strong><span>v{pluginInspection.package.manifest.version} · {pluginInspection.package.manifest.license||'许可证未声明'}</span></div><div className="capability-tags">{pluginInspection.compatibility.provides.map(item=><span key={item}>{item}</span>)}</div>{pluginInspection.compatibility.reasons.map(item=><small key={item}>{item}</small>)}<button onClick={installPlugin} disabled={!pluginInspection.compatibility.compatible}>{pluginInspection.compatibility.compatible?'安装并启用':'当前环境不兼容'}</button></div>}
          <div className="section-divider"><span>已安装</span></div>
          <div className="plugin-list">{Object.keys(pluginLock.plugins).length===0?<div className="empty-result">尚未安装社区插件</div>:Object.entries(pluginLock.plugins).map(([id,item])=><article key={id}><div className="model-icon"><Zap/></div><div><strong>{id}</strong><small>v{item.version} · {item.provides.join('、')||'声明式适配'}</small></div><button onClick={()=>togglePlugin(id,!item.enabled)}>{item.enabled?'已启用':'已停用'}</button><button className="danger-link" onClick={()=>uninstallPlugin(id)}>卸载</button></article>)}</div>
          {pluginMessage&&<div className={`probe-message ${pluginMessage.includes('通过')||pluginMessage.includes('已安装')||pluginMessage.includes('已卸载')?'success':'error'}`}><ShieldCheck/><span><strong>{pluginMessage}</strong><small>插件状态写入当前托管 Runtime Profile 的 lock.json。</small></span></div>}
        </section>
      </div>}
      {workflowDialog && <div className="modal-backdrop" role="presentation" onMouseDown={e=>{if(e.target===e.currentTarget)setWorkflowDialog(false)}}>
        <section className="modal model-modal" role="dialog" aria-modal="true" aria-labelledby="workflow-title">
          <div className="modal-head"><div><span className="eyebrow"><LayoutGrid/> 语义工作流</span><h2 id="workflow-title">选择创作方式，不需要连接节点</h2></div><button className="icon-button" onClick={()=>setWorkflowDialog(false)} aria-label="关闭对话框"><X/></button></div>
          <p>Studio 根据素材类型和模式构建官方 H3 API Graph。节点编号、连接和模型组件由后端管理，不会暴露给创作者。</p>
          <div className="workflow-list">
            <article><div className="workflow-symbol"><Sparkles/></div><div><strong>文字生成视频</strong><small>提示词 → 原生音视频 · FL2VA INT8 模型 · 24 FPS</small><span>适合从零创作；无需参考素材</span></div><button onClick={()=>{setMode('text');setWorkflowDialog(false)}}>使用</button></article>
            <article><div className="workflow-symbol"><Image/></div><div><strong>首尾帧生成</strong><small>1–2 张图片 + 提示词 → 原生音视频 · FL2VA INT8 模型</small><span>首帧约束开场，尾帧约束结尾</span></div><button onClick={()=>{setMode('frames');setWorkflowDialog(false)}}>使用</button></article>
            <article><div className="workflow-symbol"><Video/></div><div><strong>全模态参考生成</strong><small>图片、视频、音频 + 提示词 → Ref2VA INT8 模型</small><span>最多 9 张图片、3 个视频和 3 个独立音频</span></div><button onClick={()=>{setMode('reference');setWorkflowDialog(false)}}>使用</button></article>
          </div>
          <div className={`probe-message ${h3Ready?'success':'error'}`}><ShieldCheck/><span><strong>{h3Ready?'当前 ComfyUI 已通过 H3 节点校验':'当前环境尚未通过 H3 节点校验'}</strong><small>工作流会在提交前再次按实际素材检查所有 LoadImage、LoadVideo、LoadAudio 和 H3 节点。</small></span></div>
        </section>
      </div>}
      {helpDialog && <div className="modal-backdrop" role="presentation" onMouseDown={e=>{if(e.target===e.currentTarget)setHelpDialog(false)}}>
        <section className="modal model-modal" role="dialog" aria-modal="true" aria-labelledby="help-title">
          <div className="modal-head"><div><span className="eyebrow"><CircleHelp/> 新手指南</span><h2 id="help-title">参数与提示词说明</h2></div><button className="icon-button" onClick={()=>setHelpDialog(false)} aria-label="关闭对话框"><X/></button></div>
          <p>界面只展示对 H3 官方工作流确实有效的主要参数。没有可靠节点映射的 CFG、负面提示词和任意 FPS 不会伪装成可用设置。</p>
          <div className="help-grid">
            <article><strong>视频描述</strong><p>按“主体 → 动作 → 场景 → 镜头 → 光线 → 声音”书写。H3 会同时生成画面与立体声音，可直接描述对白、环境音和音乐节奏。</p></article>
            <article><strong>生成模式</strong><p>文字模式不需要素材；首尾帧模式用 1–2 张图片约束开始和结束；全模态参考可组合图片、视频和音频。</p></article>
            <article><strong>时长</strong><p>固定按 24 FPS 生成，帧数自动对齐 H3 的 17k+5 网格。例如 5 秒会生成 124 帧。更长视频通常更慢、占用更多显存。</p></article>
            <article><strong>分辨率</strong><p>608×352 用于极低显存试跑，736×416 用于平衡测试，1344×768 为标准 768p 档。三档都会真实写入 H3 工作流。</p></article>
            <article><strong>采样步数</strong><p>默认 20 步。增加步数不一定持续提高质量，但会近似线性增加生成时间；新手建议保持默认值。</p></article>
            <article><strong>随机种子</strong><p>“自动”会在每次提交时生成新种子；切换为“锁定”可复现相同模型、素材和参数组合。任务记录保存实际值。</p></article>
            <article><strong>参考素材标签</strong><p>Ref2VA 会按顺序对应 &lt;Picture 1&gt;、&lt;Video 1&gt;、&lt;Audio 1&gt;。需要精确控制时，可在描述中引用这些标签。</p></article>
            <article><strong>显存与加速</strong><p>24GB 显存和 64GB 内存是建议值，不是兼容保证。加速插件只有通过当前 Runtime 节点与冲突检查后才会标记可用。</p></article>
          </div>
          <div className="modal-note"><ShieldCheck/><span><strong>推荐起点</strong><small>5 秒、1344×768、20 步、单一清晰主体。先验证动作与构图，再逐步增加参考素材和复杂镜头。</small></span></div>
        </section>
      </div>}
      {historyDialog && <div className="modal-backdrop" role="presentation" onMouseDown={e=>{if(e.target===e.currentTarget)setHistoryDialog(false)}}>
        <section className="modal model-modal" role="dialog" aria-modal="true" aria-labelledby="history-title">
          <div className="modal-head"><div><span className="eyebrow"><Clock3/> 本地任务</span><h2 id="history-title">生成记录</h2></div><button className="icon-button" onClick={()=>setHistoryDialog(false)} aria-label="关闭对话框"><X/></button></div>
          <p>任务保存在本机 SQLite 数据库中，重启软件后仍可恢复参数和状态。</p>
          <div className="job-list">{historyLoading?<div className="empty-result">正在读取任务…</div>:jobs.length===0?<div className="empty-result">暂时没有本地任务记录</div>:jobs.map(job=><article key={job.id}><div className={`job-state ${job.status}`}>{Math.round(job.progress*100)}%</div><div><strong>{job.name}</strong><small>{job.stage} · {job.backendId}</small><span>{new Date(job.updatedAt*1000).toLocaleString()}{job.outputPath?` · ${job.outputPath}`:''}</span></div><button onClick={()=>reuseJob(job)}>复用设置</button></article>)}</div>
        </section>
      </div>}
      {settingsDialog && <div className="modal-backdrop" role="presentation" onMouseDown={e=>{if(e.target===e.currentTarget)setSettingsDialog(false)}}>
        <section className="modal" role="dialog" aria-modal="true" aria-labelledby="settings-title">
          <div className="modal-head"><div><span className="eyebrow"><FolderOpen/> 输出设置</span><h2 id="settings-title">视频保存路径</h2></div><button className="icon-button" onClick={()=>setSettingsDialog(false)} aria-label="关闭对话框"><X/></button></div>
          <p>生成前会检查目录是否存在和可写。验证过程只创建并立即删除一个测试文件。</p>
          <div className="output-mode-picker" role="radiogroup" aria-label="视频保存方式"><button className={outputMode==='default'?'selected':''} onClick={()=>setOutputMode('default')} role="radio" aria-checked={outputMode==='default'}><strong>保存到默认目录</strong><small>首次使用为软件根目录的 output</small></button><button className={outputMode==='ask'?'selected':''} onClick={()=>setOutputMode('ask')} role="radio" aria-checked={outputMode==='ask'}><strong>每次询问</strong><small>点击生成后再选择本次保存位置</small></button></div>
          <label htmlFor="output-path">默认保存目录</label>
          <div className="url-row"><input id="output-path" value={outputPath} onChange={e=>{setOutputPath(e.target.value);setPathMessage('')}} /><button onClick={chooseOutputPath}>选择目录</button><button onClick={validatePath}>验证</button></div>
          {pathMessage && <div className={`probe-message ${pathMessage.includes('可写')?'success':'error'}`}><ShieldCheck/><span><strong>{pathMessage}</strong><small>后续任务可单独覆盖此目录。</small></span></div>}
          <div className="section-divider"><span>软件更新</span></div>
          <div className="update-row"><div><strong>GitHub Releases</strong><small>当前 v0.6.3 · 包含预览版本 · 下载支持断点续传和 SHA-256 校验</small></div><button onClick={checkUpdate} disabled={checkingUpdate}>{checkingUpdate?'检查中…':'检查更新'}</button></div>
          {updateCandidate&&!downloadedUpdate&&<div className="update-candidate"><span><strong>{updateCandidate.version}</strong><small>{updateCandidate.fileName}</small></span><button onClick={downloadUpdate}>下载更新</button></div>}
          {updateProgress&&!downloadedUpdate&&<div className="runtime-progress"><div><span>更新下载</span><strong>{Math.round(updateProgress.progressPercent||0)}%</strong></div><div className="progress"><span style={{width:`${updateProgress.progressPercent||0}%`}}/></div><small>{updateProgress.bytesPerSecond?`${(updateProgress.bytesPerSecond/1024/1024).toFixed(1)} MB/s`:'正在准备'} {updateProgress.etaSeconds?`· 剩余约 ${Math.ceil(updateProgress.etaSeconds/60)} 分钟`:''}</small></div>}
          {downloadedUpdate&&<button className="primary wide update-install" onClick={launchUpdate}>关闭软件并安装 {downloadedUpdate.version}</button>}
          {updateMessage&&<div className={`probe-message ${updateMessage.includes('通过')||updateMessage.includes('最新')?'success':updateMessage.includes('发现')?'success':'error'}`}><ShieldCheck/><span><strong>{updateMessage}</strong><small>{downloadedUpdate?`SHA-256 ${downloadedUpdate.sha256}`:'更新只从本项目 GitHub Release 获取'}</small></span></div>}
          <div className="section-divider"><span>本机兼容性记录</span></div>
          <div className="benchmark-list">{benchmarkReports.length===0?<div className="empty-result">完成一次真实生成后，这里会显示峰值显存、内存和耗时</div>:benchmarkReports.slice(0,5).map(item=><article key={item.reportId}><div className={`job-state ${item.outcome}`}>{item.outcome==='completed'?'通过':'失败'}</div><div><strong>{item.gpuName} · {item.generationMode.toUpperCase()}</strong><small>{item.width}×{item.height} · {item.durationSeconds}s · 耗时 {Math.round(item.elapsedSeconds)}s</small><span>峰值显存 {(item.peakVramUsedMb/1024).toFixed(1)}/{(item.vramTotalMb/1024).toFixed(1)} GB · 插件 {item.enabledPlugins.length}</span></div></article>)}</div>
        </section>
      </div>}
    </div>
  )
}

export default App
