#!/usr/bin/env python3
"""Runs the commands of commands.tsv for one CLI through ./timed, with that CLI's environment
(made-up credentials only, none shaped like a real one). As #383's capture.py.

Usage: capture.py <cli> [name ...]
"""
import os, shlex, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ENV = {
    "aws": ["AWS_ACCESS_KEY_ID=TESTKEY", "AWS_SECRET_ACCESS_KEY=testsecret", "AWS_DEFAULT_REGION=us-east-1",
            "AWS_EC2_METADATA_DISABLED=true", "AWS_PAGER=", "AWS_MAX_ATTEMPTS=1",
            "PYTHONUSERBASE=/root/.local"],  # awscli finds colorama and PyYAML there
    "gcloud": ["CLOUDSDK_AUTH_ACCESS_TOKEN_FILE=/opt/clis/gc-token", "CLOUDSDK_CORE_PROJECT=proj1",
               "CLOUDSDK_CORE_CUSTOM_CA_CERTS_FILE=/opt/clis/pki/bundle.crt", "CLOUDSDK_CORE_DISABLE_PROMPTS=1",
               "CLOUDSDK_COMPONENT_MANAGER_DISABLE_UPDATE_CHECK=1", "CLOUDSDK_CORE_DISABLE_USAGE_REPORTING=1",
               "CLOUDSDK_CONFIG=/opt/clis/home-gcloud/gcloud"],
    "az": ["IDENTITY_ENDPOINT=https://identity.capture.test/token", "IDENTITY_HEADER=made-up", "AZURE_CORE_COLLECT_TELEMETRY=0",
           "AZURE_CORE_ONLY_SHOW_ERRORS=1", "AZURE_CORE_NO_COLOR=1", "AZURE_EXTENSION_USE_DYNAMIC_INSTALL=no"],
    "gh": ["GH_TOKEN=made-up", "GH_PROMPT_DISABLED=1", "GH_NO_UPDATE_NOTIFIER=1", "GH_REPO=acme/app"],
    "kubectl": ["KUBECONFIG=/opt/clis/kubeconfig"],
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
    for name, label, cmd in commands(cli):
        if only and name not in only:
            continue
        env = dict(os.environ, CLI=cli, CAPTURE_HOME="/opt/clis/home-" + cli)
        print("##", name, label, cmd, flush=True)
        subprocess.run([os.path.join(HERE, "timed"), name] + ENV[cli] + ["--"] + shlex.split(cmd), env=env,
                       cwd="/opt/clis/work-" + cli)


if __name__ == "__main__":
    os.makedirs("/opt/clis/work-" + sys.argv[1], exist_ok=True)
    main()
