"""Signs a release of the price table with Ed25519, the signature Silo's boundary checks.

usage: sign.py keygen <dir> <key id>    writes <dir>/<key id>.key (private, never committed) and
                                         prints the public key for Silo's config (`price_keys`)
       sign.py sign <dir> <key id> <file>
                                         writes <file>.sig: "<key id> <base64 signature>"

The signature covers the file's exact bytes. In Silo the private key would live off the build
machine (a hardware key or a CI secret), and the public keys ship in the app.
"""
import base64, os, sys
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives import serialization


def main():
    cmd, d, kid = sys.argv[1:4]
    path = f"{d}/{kid}.key"
    if cmd == "keygen":
        k = Ed25519PrivateKey.generate()
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        os.write(fd, k.private_bytes(serialization.Encoding.Raw, serialization.PrivateFormat.Raw, serialization.NoEncryption()))
        os.close(fd)
        pub = k.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
        print(f'{{"id": "{kid}", "public": "{base64.b64encode(pub).decode()}"}}')
    elif cmd == "sign":
        f = sys.argv[4]
        k = Ed25519PrivateKey.from_private_bytes(open(path, "rb").read())
        sig = k.sign(open(f, "rb").read())
        open(f + ".sig", "w").write(f"{kid} {base64.b64encode(sig).decode()}\n")
        print(f"signed {f} with {kid}")


if __name__ == "__main__":
    main()
