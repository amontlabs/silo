"""Puts one object with boto3, as an agent's script would, against the boundary's stand-in S3."""
import sys, time, boto3, botocore
from botocore.config import Config
ep, key = sys.argv[1], sys.argv[2]
s3 = boto3.client("s3", endpoint_url=f"http://{ep}", config=Config(s3={"addressing_style": "path"}))
t = time.time()
try:
    s3.put_object(Bucket="prod-backups", Key=key, Body=b"x" * 1000)
    print(f"put ok after {time.time() - t:.1f} s")
except botocore.exceptions.ClientError as e:
    print(f"ClientError after {time.time() - t:.1f} s: {e}"); sys.exit(1)
except Exception as e:
    print(f"{type(e).__name__} after {time.time() - t:.1f} s: {e}"); sys.exit(2)
