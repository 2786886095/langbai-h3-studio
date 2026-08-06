import importlib.util, pathlib, tempfile, unittest, json
HERE=pathlib.Path(__file__).parent
spec=importlib.util.spec_from_file_location('worker',HERE/'model_worker.py'); worker=importlib.util.module_from_spec(spec); spec.loader.exec_module(worker)
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
    def test_atomic_json_leaves_no_temp(self):
        with tempfile.TemporaryDirectory() as d:
            path=pathlib.Path(d)/'status.json'; worker.atomic_json(path,{'state':'running'})
            self.assertEqual(json.loads(path.read_text())['state'],'running')
            self.assertFalse(path.with_suffix('.json.tmp').exists())
if __name__=='__main__': unittest.main()
