"""#377's catch rate on its corpus, with the record option A has: no processes.

    python3 catch_rate_under_a.py <research folder of #377's branch>

Option A has no probe, so a receipt cannot say a process was left running. This runs #377's
corpus/catch_rate.py unchanged, with each case's processes left out of the evidence.
"""
import os, sys

root = os.path.abspath(sys.argv[1])
corpus = os.path.join(root, "2026-11-25-real-night", "corpus")
sys.path.insert(0, corpus)
os.chdir(corpus)
sys.argv = ["catch_rate.py"]
import catch_rate

with_probe = catch_rate.evidence


def without_probe(case):
    ev = with_probe(case)
    ev["left_running"] = []
    return ev


catch_rate.evidence = without_probe
catch_rate.main()
