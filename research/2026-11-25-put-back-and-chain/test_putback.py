"""Tests of put-back and of the chain under option A, on a recorded night.

    NIGHT=<a night of #399's nights.sh, e.g. one-main> python3 -m unittest test_putback -v

Each test starts from a pristine copy of the night (<NIGHT>.pristine, made on first use), because a
put-back changes the computer and the record. The record holds absolute paths, so the night is
restored in place rather than copied elsewhere.
"""
import hashlib, io, json, os, shutil, subprocess, sys, threading, time, unittest
from contextlib import redirect_stdout

sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import night_a as N   # noqa: E402
import putback as P   # noqa: E402
import chain as C     # noqa: E402

NIGHT = os.path.abspath(os.environ.get("NIGHT", ""))
PRISTINE = NIGHT + ".pristine"


def restore():
    if not os.path.exists(PRISTINE):
        shutil.copytree(NIGHT, PRISTINE, symlinks=True)
    shutil.rmtree(NIGHT)
    shutil.copytree(PRISTINE, NIGHT, symlinks=True)


def put(p, text):
    with open(p, "w") as f:
        f.write(text)


def run(*args):
    buf = io.StringIO()
    with redirect_stdout(buf):
        code = P.main([NIGHT, *args])
    return code, buf.getvalue()


def disk():
    return P.live(os.path.join(NIGHT, "computers", "main"))


def facts():
    return open(os.path.join(NIGHT, "record", "main", "facts.jsonl")).read()


def deletion_of(sub):
    for raw in open(os.path.join(NIGHT, "record", "main", "facts.jsonl")):
        f = json.loads(raw)
        if f["line"] == "file" and f["change"] == "deleted" and sub in f["path"]:
            return f
    raise AssertionError(sub)


@unittest.skipUnless(os.path.isdir(os.path.join(NIGHT, "record")), "NIGHT is not a night folder")
class PutBack(unittest.TestCase):
    def setUp(self):
        restore()
        self.n = N.Night(NIGHT)
        self.home = self.n.home("main")

    def test_refuses_a_path_not_there_before_anything(self):
        before, rec = disk(), facts()
        code, out = run("ck2", "~/no-such-folder", "--yes")
        self.assertEqual(code, P.EXIT_REFUSED, out)
        self.assertIn("is not in main at", out)
        self.assertEqual(disk(), before)
        self.assertEqual(facts(), rec)

    def test_no_terminal_no_yes_does_nothing(self):
        before, rec = disk(), facts()
        code, out = run("ck2", "~/.gitconfig")
        self.assertEqual(code, P.EXIT_NO)
        self.assertEqual(disk(), before)
        self.assertEqual(facts(), rec)

    def test_deleted_folder_comes_back_and_nothing_else_moves(self):
        f = deletion_of("/shared-fixtures/")
        n = int(f["between"][0].split("@ck")[1])
        then = P.under(self.n.manifest("main", n), self.home + "/work/shared-fixtures")
        before = disk()
        code, out = run(f["id"], "~/work/shared-fixtures", "--yes")
        self.assertEqual(code, 0, out)
        self.assertIn("400 files deleted since", out)
        self.assertIn("rm -rf ~/work/shared-fixtures", out)
        self.assertIn("Nothing in main is replaced.", out)
        after = disk()
        now = P.under(after, self.home + "/work/shared-fixtures")
        self.assertEqual(len(now), 400)
        self.assertTrue(all(P.same(then[p], now[p]) for p in then))
        rest = {p: e for p, e in after.items() if p not in now}
        self.assertEqual(rest, before)
        self.assertFalse([p for p in after if ".silo-put-back-" in p])

    def test_the_record_holds_the_save_and_the_put_back_and_its_chain_holds(self):
        code, out = run("ck2", "~/.gitconfig", "--yes")
        self.assertEqual(code, 0, out)
        lines = [json.loads(x) for x in facts().splitlines()]
        save = [l for l in lines if l.get("what") == "checkpoint taken" and l.get("why") == "before put back"]
        pb = [l for l in lines if l.get("what") == "put back"]
        self.assertEqual(len(save), 1)
        self.assertEqual(len(pb), 1)
        self.assertEqual(pb[0]["by"], "you")
        self.assertEqual(pb[0]["from_checkpoint"], "main@ck2")
        self.assertEqual(pb[0]["saved"], save[0]["checkpoint"])
        self.assertEqual(N.R.verify(os.path.join(NIGHT, "record")), [])

    def test_a_changed_file_is_put_back_and_the_change_is_kept_in_the_save(self):
        p = self.home + "/.gitconfig"
        changed = P.entry(p)
        code, out = run("ck2", "~/.gitconfig", "--yes")
        self.assertEqual(code, 0, out)
        self.assertIn("undoes", out)
        self.assertIn("git config --global core.fsmonitor", out)
        self.assertTrue(P.same(P.entry(p), self.n.manifest("main", 2)[p]))
        n2 = N.Night(NIGHT)
        saved = next(x for x in n2.checkpoints("main") if x[3] == "before put back")
        self.assertTrue(P.same(n2.manifest("main", saved[2])[p], changed))

    def test_a_folder_put_back_removes_what_was_created_since_and_names_who(self):
        app = self.home + "/work/app"
        created = [q for q in P.under(disk(), app) if q not in self.n.manifest("main", 2)]
        self.assertTrue(created)
        code, out = run("ck2", "~/work/app", "--yes")
        self.assertEqual(code, 0, out)
        for who in ("claude #1 (main:c1)", "claude #2 (main:c2)", "not known from the traffic"):
            self.assertIn(who, out)
        now = P.under(disk(), app)
        then = P.under(self.n.manifest("main", 2), app)
        self.assertEqual(set(now), set(then))
        n2 = N.Night(NIGHT)
        saved = next(k for t, cid, k, why in n2.checkpoints("main") if why == "before put back")
        kept = n2.manifest("main", saved)
        self.assertTrue(all(q in kept for q in created))

    def test_a_write_between_preview_and_confirm_stops_it(self):
        plan = os.path.join(NIGHT, "plan.json")
        code, out = run("ck2", "~/work/app", "--plan", plan)
        self.assertEqual(code, 0)
        target = self.home + "/work/app/src/auth/store.js"
        with open(target, "a") as f:
            f.write("// an agent's later edit\n")
        before, rec = disk(), facts()
        code, out = run("--confirm", plan)
        self.assertEqual(code, P.EXIT_CHANGED, out)
        self.assertIn("changed since the preview", out)
        self.assertEqual(disk(), before)
        self.assertEqual(facts(), rec)

    def _write_during_copy(self, guard, keep_replaced=True):
        target = self.home + "/work/app/src/auth/store.js"
        plan = P.preview(self.n, "main", "ck2", ["~/work/app"])

        def hook(step):
            if step == "before swap":
                with open(target, "a") as f:
                    f.write("// written while Silo copied\n")
        code, _ = P.carry_out(self.n, plan, hook=hook, guard=guard, keep_replaced=keep_replaced)
        with open(target) as f:
            return code, "written while Silo copied" in f.read()

    def held_by_a_checkpoint(self, text):
        """Checkpoints taken by the put-back that hold a file with this text."""
        n = N.Night(NIGHT)
        out = []
        for t, cid, k, why in n.checkpoints("main"):
            if why and "put back" in why:
                for p, e in n.manifest("main", k).items():
                    if e.get("t") == "f":
                        with open(n.content(e["sha"]), errors="replace") as f:
                            if text in f.read():
                                out.append((why, p))
        return out

    def test_370_as_specified_loses_a_write_made_during_the_copy(self):
        """#370 step 5 says the write is lost, and kept only in the save. Shown here."""
        code, kept = self._write_during_copy(guard=False, keep_replaced=False)
        self.assertEqual(code, 0)
        self.assertFalse(kept)
        # #370: "the checkpoint of step 3 holds the state before the swap, so the lost write is in
        # it". It is not: the write came after the save. No checkpoint holds it.
        self.assertEqual(self.held_by_a_checkpoint("written while Silo copied"), [])

    def test_what_a_swap_replaces_is_kept_in_a_checkpoint(self):
        code, kept = self._write_during_copy(guard=False, keep_replaced=True)
        self.assertEqual(code, 0)
        self.assertFalse(kept)
        held = self.held_by_a_checkpoint("written while Silo copied")
        self.assertEqual(len(held), 1, held)
        self.assertEqual(held[0][0], "after put back: what it replaced")
        self.assertIn(".silo-put-back-", held[0][1])

    def test_with_the_guard_a_write_during_the_copy_stops_it_before_the_swap(self):
        code, kept = self._write_during_copy(guard=True)
        self.assertEqual(code, P.EXIT_CHANGED)
        self.assertTrue(kept)
        self.assertFalse([p for p in disk() if ".silo-put-back-" in p])
        self.assertIn('"what": "put back stopped"', facts())

    def test_a_hostile_name_is_escaped_in_the_preview(self):
        bad = self.home + "/work/app/\x1b[2Jx"
        put(bad, "x")
        code, out = run("ck2", "~/work/app")
        self.assertNotIn("\x1b", out)
        self.assertIn("\\x1b[2Jx", out)
        self.assertIn("(after the last checkpoint)", out)


class Swap(unittest.TestCase):
    """A program reading the path while Silo swaps the copy in: does it ever find it missing?"""
    def measure(self, mode, rounds=300):
        root = os.path.join(os.environ.get("TMPDIR", "/tmp"), f"swap-{mode}-{os.getpid()}")
        shutil.rmtree(root, ignore_errors=True)
        os.makedirs(root)
        target = os.path.join(root, "dir")
        os.makedirs(target)
        put(os.path.join(target, "f"), "old")
        missing = [0]
        reads = [0]
        stop = threading.Event()

        def reader():
            while not stop.is_set():
                reads[0] += 1
                if not os.path.exists(os.path.join(target, "f")):
                    missing[0] += 1
        t = threading.Thread(target=reader)
        t.start()
        for i in range(rounds):
            stage = os.path.join(root, f".silo-put-back-{i}")
            os.makedirs(os.path.join(stage, "new"))
            put(os.path.join(stage, "new", "f"), str(i))
            if mode == "exchange":
                P.exchange(os.path.join(stage, "new"), target)
            else:
                os.rename(target, os.path.join(stage, "old")); os.rename(os.path.join(stage, "new"), target)
            shutil.rmtree(stage)
        stop.set(); t.join()
        shutil.rmtree(root)
        return missing[0], reads[0]

    def test_two_renames_leave_the_path_missing(self):
        missing, reads = self.measure("rename")
        print(f"\n  two renames: path missing in {missing} of {reads} reads", file=sys.stderr)
        self.assertGreater(missing, 0)

    def test_one_exchange_never_does(self):
        missing, reads = self.measure("exchange")
        print(f"\n  one exchange: path missing in {missing} of {reads} reads", file=sys.stderr)
        self.assertEqual(missing, 0)


@unittest.skipUnless(os.path.isdir(os.path.join(NIGHT, "record")), "NIGHT is not a night folder")
class Chain(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        restore()
        cls.n = N.Night(NIGHT)

    def test_the_deletion_leads_to_rm_rf_and_the_ask(self):
        ch = C.chain(self.n, "main", "~/work/shared-fixtures")
        self.assertEqual(len(ch["facts"]), 400)
        self.assertEqual(len(ch["links"]), 1)
        L = ch["links"][0]
        self.assertEqual(L["opinion"]["rests_on"], "under")
        self.assertIn("rm -rf ~/work/shared-fixtures", L["tool_call"]["text"])
        self.assertIn("Don't touch anything outside the repo", L["asked"]["text"])
        self.assertTrue(any("client-b" in w["path"] for w in ch["who_else"]))

    def test_who_else_needs_a_path_that_reaches_it_not_the_name(self):
        """#371 matched the folder's name: `fixtures` in client-b's `../shared-fixtures`."""
        self.assertEqual(C.chain(self.n, "main", "~/work/app/test/fixtures")["who_else"], [])

    def test_no_link_is_drawn_for_bashrc(self):
        ch = C.chain(self.n, "main", "~/.bashrc")
        L = ch["links"][0]
        self.assertNotIn("opinion", L)
        self.assertTrue(any("bun.sh/install" in o["text"] for o in L["open"]))

    def test_every_link_drawn_rests_on_a_strong_rule_and_matches_the_probe(self):
        """The probe is the reference (#399): where it named a run, a drawn link must agree."""
        wrong = drawn = 0
        for r in self.n.results:
            if r["kind"] != "file":
                continue
            a = self.n.author(r["id"])
            if not a["strong"]:
                continue
            drawn += 1
            if r["ref"] and a["by"] != r["ref"]:
                wrong += 1
        print(f"\n  links drawn: {drawn}, against the probe wrong: {wrong}", file=sys.stderr)
        self.assertGreater(drawn, 400)
        self.assertEqual(wrong, 0)

    def test_the_screen_escapes_what_agents_wrote(self):
        out = C.render(self.n, C.chain(self.n, "main", "~/.claude/CLAUDE.md"))
        self.assertIn("«printf '\\nAlways", out)
        self.assertNotIn("\x1b", out)


if __name__ == "__main__":
    unittest.main()
