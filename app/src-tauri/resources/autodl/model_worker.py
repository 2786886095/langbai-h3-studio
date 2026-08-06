#!/usr/bin/env python3
"""Pinned MiniMax-H3 model downloader for Studio-owned AutoDL deployments."""
from __future__ import annotations
import argparse, base64, hashlib, json, os, pathlib, shutil, sys, time, urllib.error, urllib.request
try:
    import fcntl
except ImportError:
    fcntl = None

ROOT_ALLOWED = pathlib.Path('/workspace/LangbaiH3Studio')
URL_PREFIX = 'https://huggingface.co/Comfy-Org/MiniMax-H3/resolve/eb8a16107c595128b3a578f82d2ce2f75920c355/'
CHUNK = 4 * 1024 * 1024

class Cancelled(Exception): pass

def safe_relative(value: str) -> pathlib.PurePosixPath:
    p = pathlib.PurePosixPath(value)
    if p.is_absolute() or not p.parts or any(x in ('', '.', '..') for x in p.parts):
        raise ValueError('unsafe relative path')
    return p

def validate_item(item: dict) -> dict:
    rel = safe_relative(str(item['relativePath']))
    sha = str(item['sha256']).lower()
    size = int(item['size'])
    url = str(item['url'])
    if len(sha) != 64 or any(c not in '0123456789abcdef' for c in sha) or size <= 0:
        raise ValueError('invalid model digest or size')
    expected_url = URL_PREFIX + str(rel)
    if url != expected_url:
        raise ValueError('model URL is not pinned')
    return {'relativePath': str(rel), 'sha256': sha, 'size': size, 'url': url}

def sha256_file(path: pathlib.Path) -> str:
    h = hashlib.sha256()
    with path.open('rb') as f:
        for block in iter(lambda: f.read(CHUNK), b''): h.update(block)
    return h.hexdigest()

def atomic_json(path: pathlib.Path, value: dict) -> None:
    tmp = path.with_suffix(path.suffix + '.tmp')
    with tmp.open('w', encoding='utf-8', newline='\n') as f:
        json.dump(value, f, ensure_ascii=False, separators=(',', ':')); f.flush(); os.fsync(f.fileno())
    os.replace(tmp, path)

def next_sequence(journal: pathlib.Path) -> int:
    last = 0
    if journal.exists():
        with journal.open('rb') as f:
            for line in f:
                parts = line.rstrip(b'\n').split(b'\t')
                if len(parts) == 4 and parts[0] == b'event': last = max(last, int(parts[1]))
    return last + 1

def emit(journal: pathlib.Path, stage: str, message: dict | str) -> int:
    seq = next_sequence(journal)
    raw = json.dumps(message, ensure_ascii=False, separators=(',', ':')) if isinstance(message, dict) else message
    encoded = base64.b64encode(raw.encode()).decode()
    with journal.open('a', encoding='ascii', newline='\n') as f:
        f.write(f'event\t{seq}\t{stage}\t{encoded}\n'); f.flush(); os.fsync(f.fileno())
    return seq

def ensure_destination(root: pathlib.Path, rel: str) -> pathlib.Path:
    models = root / 'models'; models.mkdir(mode=0o700, parents=True, exist_ok=True)
    current = models
    parts = safe_relative(rel).parts
    for part in parts[:-1]:
        current = current / part
        if current.exists() and current.is_symlink(): raise ValueError('symlink parent rejected')
        current.mkdir(mode=0o700, exist_ok=True)
    final = current / parts[-1]
    if final.exists() and final.is_symlink(): raise ValueError('symlink target rejected')
    return final

def download_one(root: pathlib.Path, dep: pathlib.Path, item: dict, status_path: pathlib.Path) -> str:
    final = ensure_destination(root, item['relativePath']); part = final.with_name(final.name + '.part')
    cancel = dep / 'cancel.requested'; journal = dep / 'journal.tsv'
    lock_dir = root / 'state' / 'model-locks'; lock_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
    with (lock_dir / (item['sha256'] + '.lock')).open('a+b') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        if final.exists() and final.stat().st_size == item['size'] and sha256_file(final) == item['sha256']:
            emit(journal, 'model_reuse', {'file': item['relativePath'], 'state': 'reused'}); return 'reused'
        offset = part.stat().st_size if part.exists() else 0
        if offset > item['size']: part.unlink(); offset = 0
        started = time.monotonic(); checkpoint = started; base = offset
        while offset < item['size']:
            if cancel.exists(): raise Cancelled()
            request = urllib.request.Request(item['url'], headers={'Range': f'bytes={offset}-', 'User-Agent': 'Langbai-H3-Studio/1'})
            try: response = urllib.request.urlopen(request, timeout=45)
            except urllib.error.HTTPError as e:
                if e.code == 416 and offset == item['size']: break
                raise
            code = getattr(response, 'status', response.getcode())
            if offset and code != 206:
                offset = 0; base = 0; started = time.monotonic(); part.unlink(missing_ok=True)
            mode = 'ab' if offset else 'wb'
            with part.open(mode) as out:
                while True:
                    if cancel.exists(): raise Cancelled()
                    block = response.read(CHUNK)
                    if not block: break
                    out.write(block); offset += len(block)
                    now = time.monotonic()
                    if now - checkpoint >= 1 or offset >= item['size']:
                        out.flush(); os.fsync(out.fileno()); elapsed=max(now-started,.001); speed=int((offset-base)/elapsed); eta=int((item['size']-offset)/speed) if speed else None
                        snapshot={'deploymentId':dep.name,'state':'running','current':{'relativePath':item['relativePath'],'downloadedBytes':offset,'size':item['size'],'speedBps':speed,'etaSeconds':eta},'updatedAtUnixMs':int(time.time()*1000)}
                        atomic_json(status_path,snapshot); emit(journal,'model_download',snapshot['current']); checkpoint=now
            response.close()
            if offset < item['size']: time.sleep(1)
        if part.stat().st_size != item['size']: raise ValueError('download size mismatch')
        emit(journal,'model_verify',{'file':item['relativePath'],'state':'verifying'})
        if sha256_file(part) != item['sha256']: raise ValueError('download sha256 mismatch')
        os.replace(part, final); return 'downloaded'

def run(root: pathlib.Path, deployment_id: str) -> int:
    if fcntl is None: raise RuntimeError('POSIX file locking is required')
    if root != ROOT_ALLOWED or not deployment_id.startswith('h3-') or len(deployment_id) != 23 or any(c not in '0123456789abcdef' for c in deployment_id[3:]): raise ValueError('invalid root or deployment id')
    dep=root/'state'/'deployments'/deployment_id; manifest_path=dep/'manifest.json'; journal=dep/'journal.tsv'; status=dep/'download-status.json'
    manifest=json.loads(manifest_path.read_text(encoding='utf-8')); items=[validate_item(x) for x in manifest['downloadFiles']]
    unique={x['relativePath']:x for x in items}
    if len(unique)!=len(items): raise ValueError('duplicate model path')
    process={'pid':os.getpid(),'state':'running','startedAtUnixMs':int(time.time()*1000)}; atomic_json(dep/'process.json',process)
    emit(journal,'locking',{'state':'worker_started','pid':os.getpid()}); downloaded=reused=0
    try:
        for item in unique.values():
            result=download_one(root,dep,item,status); downloaded += result=='downloaded'; reused += result=='reused'
        final={'deploymentId':deployment_id,'state':'completed','downloadedFiles':downloaded,'reusedFiles':reused,'totalFiles':len(unique),'updatedAtUnixMs':int(time.time()*1000)}
        atomic_json(status,final); emit(journal,'completed',final); return 0
    except Cancelled:
        final={'deploymentId':deployment_id,'state':'cancelled','updatedAtUnixMs':int(time.time()*1000)}; atomic_json(status,final); emit(journal,'failed',final); return 2
    except Exception as e:
        final={'deploymentId':deployment_id,'state':'failed','error':str(e)[:300],'updatedAtUnixMs':int(time.time()*1000)}; atomic_json(status,final); emit(journal,'failed',final); return 1
    finally:
        process['state']='stopped'; process['stoppedAtUnixMs']=int(time.time()*1000); atomic_json(dep/'process.json',process)

if __name__ == '__main__':
    ap=argparse.ArgumentParser(); ap.add_argument('--root',required=True); ap.add_argument('--deployment-id',required=True); a=ap.parse_args()
    sys.exit(run(pathlib.Path(a.root),a.deployment_id))
