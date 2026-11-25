"""Which test commands run the whole suite, and which run part of it.

    python3 tests/test_narrowed.py

A narrowed run that passes after a whole run that failed does not back "All tests pass.". #382
showed #377's checker reading `npm test -- test/auth` as the whole suite, so a false claim came
out matched. Exits 1 while any command below is read wrong.
"""
import os, sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "..", "2026-11-24-morning-review", "proto"))
import claims

# before the fix, the checker read a narrowed run with its FILTERED pattern alone
narrowed = getattr(claims, "narrowed", lambda c: bool(claims.FILTERED.search(c)))

NARROWED = [
    "npm test -- test/auth",
    "npm run test -- auth",
    "npm test -- -t login",
    "npm test -- --grep login",
    "yarn test auth",
    "pnpm test -- src/auth.test.ts",
    "pnpm run test auth",
    "npx jest src/auth",
    "npx vitest run auth",
    "bun test auth",
    "node --test test/auth.test.js",
    "pytest tests/test_auth.py",
    "pytest -k auth",
    "python -m pytest tests/unit/test_x.py::test_y",
    "go test -run TestAuth ./...",
    "go test ./pkg/auth",
    "cargo test auth",
    "cargo test --test integration",
    "rspec spec/models/user_spec.rb",
]
WHOLE = [
    "npm test",
    "npm run test",
    "npm test -- --reporter=dot",
    "npm test 2>&1 | tail -20",
    "yarn test --coverage",
    "npx jest",
    "npx vitest run",
    "bun test",
    "pytest",
    "pytest -q",
    "pytest tests/",
    "python -m pytest",
    "go test ./...",
    "cargo test",
    "cargo test --workspace",
    "make test",
]

wrong = 0
for cmd in NARROWED:
    ok = narrowed(cmd)
    wrong += not ok
    print(f"{'ok   ' if ok else 'WRONG'}  narrowed  {cmd}")
for cmd in WHOLE:
    ok = not narrowed(cmd)
    wrong += not ok
    print(f"{'ok   ' if ok else 'WRONG'}  whole     {cmd}")

# the night of #373: the whole suite fails, then a narrowed run passes
for cmd in NARROWED:
    ev = {"commands": [{"command": "npm test", "exit": 1, "t": 0}, {"command": cmd, "exit": 0, "t": 2}],
          "edits": [], "left_running": [], "connections": [], "cwd": "the repo"}
    for check in (claims.check_02, claims.check_03):
        says = check("All tests pass.", ev)[0]["says"]
        if says != "mismatch":
            wrong += 1
            print(f"WRONG  {check.__name__}: «All tests pass.» after `npm test` exit 1 then `{cmd}` exit 0 -> {says}")
print(f"{wrong} wrong")
sys.exit(1 if wrong else 0)
