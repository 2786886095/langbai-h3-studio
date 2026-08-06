import importlib.util, pathlib, tempfile, unittest, json, hashlib, threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
HERE=pathlib.Path(__file__).parent
spec=importlib.util.spec_from_file_location('worker',HERE/'model_worker.py'); worker=importlib.util.module_from_spec(spec); spec.loader.exec_module(worker)

class _FakeFcntl:
    LOCK_EX=1
    @staticmethod
    def flock(_file,_mode): return None

class _RangeHandler(BaseHTTPRequestHandler):
    payload=b''
    def log_message(self,*_args): pass
    def do_GET(self):
        start=0
        value=self.headers.get('Range')
        if value and value.startswith('bytes='): start=int(value[6:].split('-',1)[0])
        body=self.payload[start:]
        self.send_response(206 if start else 200)
        if start:self.send_header('Content-Range',f'bytes {start}-{len(self.payload)-1}/{len(self.payload)}')
        self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)

class WorkerContractTests(unittest.TestCase):
    def test_rejects_traversal_and_unpinned_url(self):
        for value in ('../x','/root/x','a/../../b','.'):
            with self.assertRaises(ValueError): worker.safe_relative(value)
        with self.assertRaises(ValueError): worker.validate_item({'relativePath':'vae/x','size':1,'sha256':'0'*64,'url':'https://example.com/x'})
    def test_accepts_pinned_item(self):
        item={'relativePath':'vae/x.safetensors','size':1,'sha256':'a'*64,'url':worker.URL_PREFIX+'vae/x.safetensors'}
        self.assertEqual(worker.validate_item(item)['relativePath'],'vae/x.safetensors')
    def test_journal_sequence_is_monotonic(self):
        with tempfile.TemporaryDirectory() as d:
            journal=pathlib.Path(d)/'journal.tsv'
            self.assertEqual(worker.emit(journal,'model_download',{'speedBps':1}),1)
            self.assertEqual(worker.emit(journal,'completed','done'),2)
            self.assertEqual(worker.next_sequence(journal),3)
            self.assertEqual(len(journal.read_text().splitlines()),2)
    def test_small_range_download_and_hash_end_to_end(self):
        payload=(b'langbai-h3-worker-'*65536)
        _RangeHandler.payload=payload
        server=ThreadingHTTPServer(('127.0.0.1',0),_RangeHandler);thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
        old_root,old_prefix,old_fcntl=worker.ROOT_ALLOWED,worker.URL_PREFIX,worker.fcntl
        try:
            with tempfile.TemporaryDirectory() as d:
                root=pathlib.Path(d)/'LangbaiH3Studio';dep_id='h3-0123456789abcdefabcd';dep=root/'state'/'deployments'/dep_id;(dep/'logs').mkdir(parents=True)
                worker.ROOT_ALLOWED=root;worker.URL_PREFIX=f'http://127.0.0.1:{server.server_port}/';worker.fcntl=_FakeFcntl
                rel='vae/test.bin';sha=hashlib.sha256(payload).hexdigest();url=worker.URL_PREFIX+rel
                manifest={'downloadFiles':[{'relativePath':rel,'size':len(payload),'sha256':sha,'url':url}]}
                (dep/'manifest.json').write_text(json.dumps(manifest),encoding='utf-8');(dep/'journal.tsv').write_text('event\t1\tcompleted\tb2s=\n',encoding='ascii')
                part=root/'models'/'vae'/'test.bin.part';part.parent.mkdir(parents=True);part.write_bytes(payload[:12345])
                self.assertEqual(worker.run(root,dep_id),0)
                self.assertEqual((root/'models'/'vae'/'test.bin').read_bytes(),payload);self.assertFalse(part.exists())
                self.assertEqual(json.loads((dep/'download-status.json').read_text())['state'],'completed')
        finally:
            worker.ROOT_ALLOWED,worker.URL_PREFIX,worker.fcntl=old_root,old_prefix,old_fcntl;server.shutdown();server.server_close()

    def test_cancel_preserves_partial_file(self):
        payload=b'x'*65536;_RangeHandler.payload=payload
        server=ThreadingHTTPServer(('127.0.0.1',0),_RangeHandler);threading.Thread(target=server.serve_forever,daemon=True).start()
        old_root,old_prefix,old_fcntl=worker.ROOT_ALLOWED,worker.URL_PREFIX,worker.fcntl
        try:
            with tempfile.TemporaryDirectory() as d:
                root=pathlib.Path(d)/'LangbaiH3Studio';dep_id='h3-0123456789abcdefabcd';dep=root/'state'/'deployments'/dep_id;(dep/'logs').mkdir(parents=True)
                worker.ROOT_ALLOWED=root;worker.URL_PREFIX=f'http://127.0.0.1:{server.server_port}/';worker.fcntl=_FakeFcntl
                rel='vae/cancel.bin';manifest={'downloadFiles':[{'relativePath':rel,'size':len(payload),'sha256':hashlib.sha256(payload).hexdigest(),'url':worker.URL_PREFIX+rel}]}
                (dep/'manifest.json').write_text(json.dumps(manifest));(dep/'journal.tsv').write_text('event\t1\tcompleted\tb2s=\n');(dep/'cancel.requested').touch()
                part=root/'models'/'vae'/'cancel.bin.part';part.parent.mkdir(parents=True);part.write_bytes(b'partial')
                self.assertEqual(worker.run(root,dep_id),2);self.assertTrue(part.exists())
                self.assertEqual(json.loads((dep/'download-status.json').read_text())['state'],'cancelled')
        finally:
            worker.ROOT_ALLOWED,worker.URL_PREFIX,worker.fcntl=old_root,old_prefix,old_fcntl;server.shutdown();server.server_close()

    def test_atomic_json_leaves_no_temp(self):
        with tempfile.TemporaryDirectory() as d:
            path=pathlib.Path(d)/'status.json'; worker.atomic_json(path,{'state':'running'})
            self.assertEqual(json.loads(path.read_text())['state'],'running')
            self.assertFalse(path.with_suffix('.json.tmp').exists())
if __name__=='__main__': unittest.main()
