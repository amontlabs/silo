#!/usr/bin/env python3
"""Runs the commands of commands.tsv for one CLI through ./timed, with that CLI's environment
(fake tokens only) and, per command, the made-up GraphQL values in canned/overlays.json.

Usage: capture.py <cli> [name ...]
The Supabase CLI checks the shape of its token: set SUPABASE_TEST_TOKEN to a made-up one, sbp_ and
40 hex digits.
"""
import json, os, shlex, subprocess, sys, time

HERE = os.path.dirname(os.path.abspath(__file__))
ENV = {
    "railway": ["RAILWAY_TOKEN=test-token-123"],
    "fly": ["FLY_API_TOKEN=FlyV1 fm2_test-token-123", "FLY_NO_UPDATE_CHECK=1", "FLY_METRICS_TOKEN=-"],
    "vercel": ["VERCEL_TOKEN=testtoken123", "VERCEL_TELEMETRY_DISABLED=1"],
    "wrangler": ["CLOUDFLARE_API_TOKEN=test-token-123", "CLOUDFLARE_ACCOUNT_ID=acc123", "WRANGLER_SEND_METRICS=false"],
    "supabase": ["SUPABASE_ACCESS_TOKEN=" + os.environ.get("SUPABASE_TEST_TOKEN", ""), "SUPABASE_YES=1", "DO_NOT_TRACK=1"],
}


def commands(cli):
    for line in open(os.path.join(HERE, "commands.tsv")):
        if line.startswith("#") or not line.strip():
            continue
        name, c, label, cmd = line.rstrip("\n").split("\t")
        if c == cli:
            yield name, label, cmd


def main():
    cli = sys.argv[1]
    only = set(sys.argv[2:])
    base_mock = os.path.join(HERE, "canned", cli + "-mock.base.json")
    mock = os.path.join(HERE, "canned", cli + "-mock.json")
    ov_path = os.path.join(HERE, "canned", "overlays.json")
    overlays = json.load(open(ov_path)) if os.path.exists(ov_path) else {}
    for name, label, cmd in commands(cli):
        if only and name not in only:
            continue
        if os.path.exists(base_mock):
            fx = json.load(open(base_mock))
            fx.update({k: v for k, v in overlays.get(name, {}).items() if not k.startswith("_")})
            json.dump(fx, open(mock, "w"))
            time.sleep(0.05)
        for path, text in overlays.get(name, {}).get("_files", {}).items():
            os.makedirs(os.path.dirname(path), exist_ok=True)
            open(path, "w").write(text)
        env = dict(os.environ, CLI=cli, CAPTURE_HOME="/opt/clis/home-" + cli)
        print("##", name, label, cmd, flush=True)
        env_vars = overlays.get(name, {}).get("_env", ENV[cli])
        subprocess.run([os.path.join(HERE, "timed"), name] + env_vars + ["--"] + shlex.split(cmd), env=env,
                       cwd=os.environ.get("CAPTURE_CWD", "/opt/clis/work-" + cli))


if __name__ == "__main__":
    os.makedirs("/opt/clis/work-" + sys.argv[1], exist_ok=True)
    main()
