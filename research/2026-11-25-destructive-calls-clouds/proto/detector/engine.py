"""#383's engine (#353's detector with two body conditions) plus what this folder adds. Placeholder
until the rules are written; see the README."""
import os, sys
P383 = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "..",
                                     "2026-11-25-destructive-calls-platforms", "proto", "detector"))
if P383 not in sys.path:
    sys.path.insert(0, P383)
import importlib.util
_spec = importlib.util.spec_from_file_location("engine383", os.path.join(P383, "engine.py"))
engine383 = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(engine383)


class Detector(engine383.Detector):
    pass
