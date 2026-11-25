"""Ed25519 signatures beside a file, in #396's format: `<file>.sig`, one line per signature,
`<key id> <base64 signature of the file's exact bytes>`. A root file may carry several lines."""
import base64, os
from pathlib import Path
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey, Ed25519PublicKey
from cryptography.hazmat.primitives import serialization
from cryptography.exceptions import InvalidSignature


def keygen(path):
    k = Ed25519PrivateKey.generate()
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    os.write(fd, k.private_bytes(serialization.Encoding.Raw, serialization.PrivateFormat.Raw, serialization.NoEncryption()))
    os.close(fd)
    return public_of(path)


def public_of(path):
    k = Ed25519PrivateKey.from_private_bytes(Path(path).read_bytes())
    return base64.b64encode(k.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)).decode()


def sign(data, kid, keypath):
    k = Ed25519PrivateKey.from_private_bytes(Path(keypath).read_bytes())
    return f"{kid} {base64.b64encode(k.sign(data)).decode()}\n"


def signers(data, sigtext, keys):
    """The ids, among `keys` ({id: base64 public}), whose signature over `data` holds."""
    ok = set()
    for line in sigtext.splitlines():
        parts = line.split()
        if len(parts) != 2 or parts[0] not in keys:
            continue
        try:
            Ed25519PublicKey.from_public_bytes(base64.b64decode(keys[parts[0]])).verify(base64.b64decode(parts[1]), data)
            ok.add(parts[0])
        except (InvalidSignature, ValueError):
            pass
    return ok
