import csv, json, tempfile, unittest
from pathlib import Path
from merge_service_baselines import merge, read_capture

class MergeTests(unittest.TestCase):
    def setUp(self): self.tmp = tempfile.TemporaryDirectory(); self.root = Path(self.tmp.name)
    def tearDown(self): self.tmp.cleanup()
    def capture(self, filename, env, services, build="19045", edition="Core"):
        path = self.root / filename
        path.write_text(json.dumps({"schema_version":1,"source":"measured","environment":env,
          "os":{"product_name":"Windows","display_version":"22H2","build":build,"edition_id":edition},
          "services":services}), encoding="utf-8")
        return path
    def test_merge_empty_cell_delayed_trigger_and_varies(self):
        a = self.capture("a.json","win10-home",[{"name":"Alpha","start":2,"delayed_autostart":1,"start_label":"Automatic (Delayed)","trigger_present":True,"source":"measured"}])
        b = self.capture("b.json","win11-pro",[{"name":"Alpha","start":3,"delayed_autostart":0,"start_label":"Manual","trigger_present":False,"source":"measured"}, {"name":"Beta","start":4,"start_label":"Disabled","source":"measured"}], "26100","Professional")
        out = self.root/"out"; n, obs = merge([a,b],out)
        self.assertEqual((n,obs),(2,3))
        with (out/"service-matrix.csv").open(encoding="utf-8-sig",newline="") as f: rows={r["service"]:r for r in csv.DictReader(f)}
        self.assertEqual(rows["Alpha"]["win10-home"],"Automatic (Delayed) (Trigger)")
        self.assertEqual(rows["Alpha"]["win11-pro"],"Manual")
        self.assertEqual(rows["Alpha"]["varies_between_envs"],"true")
        self.assertEqual(rows["Beta"]["win10-home"],"")
        self.assertEqual(json.loads(rows["Alpha"]["source"]), {"win10-home":"measured","win11-pro":"measured"})
    def test_rejects_session_instance(self):
        p=self.capture("bad.json","win10-home",[{"name":"CDPUserSvc_5c1e7","start":3}])
        with self.assertRaisesRegex(ValueError,"instância dinâmica"): read_capture(p)
    def test_rejects_non_measured_source_and_duplicate_names(self):
        p=self.capture("bad.json","win10-home",[{"name":"X","start":3}])
        d=json.loads(p.read_text()); d["source"]="from_memory"; p.write_text(json.dumps(d))
        with self.assertRaisesRegex(ValueError,"source deve ser 'measured'"): read_capture(p)
        d["source"]="measured"; d["services"]=[{"name":"X","start":3},{"name":"X","start":4}]; p.write_text(json.dumps(d))
        with self.assertRaisesRegex(ValueError,"nome duplicado"): read_capture(p)
    def test_rejects_duplicate_environment(self):
        a=self.capture("a.json","same",[{"name":"X","start":3}]); b=self.capture("b.json","same",[{"name":"Y","start":3}])
        with self.assertRaisesRegex(ValueError,"environment duplicado"): merge([a,b],self.root/"out")

if __name__ == "__main__": unittest.main()
