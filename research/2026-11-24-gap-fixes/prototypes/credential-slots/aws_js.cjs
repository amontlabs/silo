// The AWS SDK for JavaScript v3, signing with a placeholder secret key, through the boundary.
// usage: NODE_PATH=<dir with @aws-sdk/client-s3 and proxy-agent> node aws_js.cjs
const { S3Client, PutObjectCommand, GetObjectCommand } = require("@aws-sdk/client-s3");
const { NodeHttpHandler } = require("@smithy/node-http-handler");
const { ProxyAgent } = require("proxy-agent");
const { Readable } = require("stream");
const crypto = require("crypto");
(async () => {
  const tls = process.env.TLS === "1";
  const agent = new ProxyAgent({ getProxyForUrl: () => process.env.PX, ca: tls ? require("fs").readFileSync(process.env.CA) : undefined });
  const c = new S3Client({ region: "us-east-1", endpoint: (tls ? "https" : "http") + "://s3.test", forcePathStyle: true,
    credentials: { accessKeyId: process.env.AKID, secretAccessKey: process.env.SECRET },
    requestHandler: new NodeHttpHandler({ httpAgent: agent, httpsAgent: agent }), maxAttempts: 1 });
  const out = {};
  for (const [label, size, stream] of [["buffer", 1000, false], ["stream", 9 * 1024 * 1024, true]]) {
    const data = crypto.randomBytes(size);
    try {
      await c.send(new PutObjectCommand({ Bucket: "b", Key: label, Body: stream ? Readable.from([data.subarray(0, size / 2), data.subarray(size / 2)]) : data, ContentLength: size }));
      const got = Buffer.from(await (await c.send(new GetObjectCommand({ Bucket: "b", Key: label }))).Body.transformToByteArray());
      out[label] = got.equals(data);
    } catch (e) { out[label] = "error " + (e.name || e.message); }
  }
  console.log(JSON.stringify(out));
})();
