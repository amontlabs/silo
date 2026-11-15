import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest

import engine_cycle_ratchet as ratchet

FILES = {
    'lib.rs': 'pub mod a;\npub mod b;\npub mod c;\npub mod d;\npub use b as bee;\n',
    'a.rs': 'use crate::b::X;\nfn f() { crate::c::go(); }\n',
    'b.rs': 'use crate::a::Y;\n#[cfg(test)]\nmod tests { use crate::d::T; }\n',
    'c.rs': 'fn go() {}\n',
    'd.rs': 'use crate::a::Y;\n',
}


def write_engine(root, files):
    src = root / ratchet.ENGINE_SRC
    for name, text in files.items():
        (src / name).parent.mkdir(parents=True, exist_ok=True)
        (src / name).write_text(text)


class EngineCycleRatchetTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.baseline = self.root / 'baseline.json'
        write_engine(self.root, FILES)
        self.addCleanup(self.tmp.cleanup)

    def run_main(self, *extra):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = ratchet.main(['--root', str(self.root), '--baseline', str(self.baseline), *extra])
        return code, out.getvalue(), err.getvalue()

    def test_scan_counts_production_edges_only(self):
        graph = ratchet.scan(self.root)
        self.assertEqual(sorted(graph['edges']['a']), ['b', 'c'])
        self.assertNotIn('d', graph['edges'].get('b', {}))
        members, pairs = ratchet.knot(graph)
        self.assertEqual(members, ['a', 'b'])
        self.assertEqual(pairs, ['a -> b', 'b -> a'])

    def test_baseline_passes_then_new_cyclic_edge_fails_naming_it(self):
        self.assertEqual(self.run_main('--update')[0], 0)
        self.assertEqual(self.run_main()[0], 0)
        write_engine(self.root, {'c.rs': 'use super::a::Z;\n'})
        code, _, err = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn('module `c` joined the knot', err)
        self.assertIn('new edge inside the knot: c -> a', err)

    def test_new_edge_between_knot_modules_fails(self):
        self.baseline.write_text(json.dumps({'modules': ['a', 'b'], 'edges': ['a -> b']}))
        code, _, err = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn('new edge inside the knot: b -> a', err)

    def test_acyclic_edge_outside_the_knot_passes(self):
        self.run_main('--update')
        write_engine(self.root, {'d.rs': 'use crate::a::Y;\nuse crate::c::W;\n'})
        self.assertEqual(self.run_main()[0], 0)

    def test_shrunk_knot_asks_for_lower_baseline_and_update_cannot_raise(self):
        self.run_main('--update')
        write_engine(self.root, {'b.rs': 'fn nothing() {}\n'})
        code, out, _ = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn('--update', out)
        self.assertEqual(self.run_main('--update')[0], 0)
        write_engine(self.root, {'b.rs': 'use crate::a::Y;\n'})
        code, _, err = self.run_main('--update')
        self.assertEqual(code, 1)
        self.assertIn('refusing to raise', err)

    def test_reach_counts_transitive_dependents(self):
        graph = ratchet.scan(self.root)
        self.assertEqual(ratchet.dependents(graph, {'c'}), {'a', 'b', 'c', 'd'})
        self.assertEqual(ratchet.dependents(graph, {'d'}), {'d'})
        text = ratchet.reach_report(graph, [str(ratchet.ENGINE_SRC / 'd.rs')])
        self.assertIn('1/4 modules', text)

    def test_committed_baseline_matches_the_engine(self):
        graph = ratchet.scan(ratchet.REPO)
        members, pairs = ratchet.knot(graph)
        baseline = json.loads(ratchet.BASELINE.read_text())
        self.assertEqual((members, pairs), (baseline['modules'], baseline['edges']))


if __name__ == '__main__':
    unittest.main()
