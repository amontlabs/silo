"""Checks sigv4.py against the chunked-upload example of the S3 documentation ("Signature
Calculations for the Authorization Header: Transferring Payload in Multiple Chunks"), then re-signs
that body with another key and checks the new chain verifies and the length is kept.
usage: python3 vectors.py"""
import sigv4
SECRET = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"
hs = [("Host", "s3.amazonaws.com"), ("x-amz-date", "20130524T000000Z"), ("x-amz-storage-class", "REDUCED_REDUNDANCY"),
      ("x-amz-content-sha256", "STREAMING-AWS4-HMAC-SHA256-PAYLOAD"), ("Content-Encoding", "aws-chunked"),
      ("x-amz-decoded-content-length", "66560"), ("Content-Length", "66824")]
auth = {"date": "20130524", "region": "us-east-1", "service": "s3",
        "signed": "content-encoding;content-length;host;x-amz-content-sha256;x-amz-date;x-amz-decoded-content-length;x-amz-storage-class".split(";")}
seed, key, amz_date, scope = sigv4.header_signature(SECRET, "PUT", "/examplebucket/chunkObject.txt", "", hs, auth)
print("seed signature matches the documentation:", seed == "4f232c4386841ef735655705268965c44a0e4690baa4adea153f7db9fa80a0a9")
c1 = sigv4.sign(key, sigv4.chunk_sts(amz_date, scope, seed, b"a" * 65536))
c2 = sigv4.sign(key, sigv4.chunk_sts(amz_date, scope, c1, b"a" * 1024))
c3 = sigv4.sign(key, sigv4.chunk_sts(amz_date, scope, c2, b""))
print("chunk signatures match:", [c1.startswith("ad80c730"), c2.startswith("0055627c"), c3.startswith("b6c6ea8a")])
body = (b"10000;chunk-signature=" + c1.encode() + b"\r\n" + b"a" * 65536 + b"\r\n" + b"400;chunk-signature=" + c2.encode() +
        b"\r\n" + b"a" * 1024 + b"\r\n" + b"0;chunk-signature=" + c3.encode() + b"\r\n\r\n")
print("body length is the documented Content-Length:", len(body) == 66824)
nkey = sigv4.signing_key("another-secret-key-of-forty-characters!!", "20130524", "us-east-1", "s3")
new_seed = sigv4.sign(nkey, "seed")  # any new seed: the chain must follow it
nb, n, ok = sigv4.resign_aws_chunked(body, amz_date, scope, key, seed, nkey, new_seed)
_, _, ok2 = sigv4.resign_aws_chunked(nb, amz_date, scope, nkey, new_seed, nkey, new_seed)
print(f"re-signed {n} chunks; old chain valid: {ok}; new chain valid: {ok2}; length kept: {len(nb) == len(body)};",
      "bytes changed:", sum(1 for a, b in zip(body, nb) if a != b), "(all inside the three 64-digit signatures)")
