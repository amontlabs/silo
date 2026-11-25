"""Paramiko logs in through the boundary with the agent's key only."""
import sys, os, paramiko
os.environ["SSH_AUTH_SOCK"] = sys.argv[1]
c = paramiko.SSHClient(); c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
c.connect("127.0.0.1", int(sys.argv[2]), "deploy", allow_agent=True, look_for_keys=False, timeout=5)
_, o, _ = c.exec_command("echo ok"); print(o.read().decode().strip())
