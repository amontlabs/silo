// Node's own HTTP parser (llhttp), default settings: what it parsed, as the reply's body.
const http = require('http'), crypto = require('crypto');
let n = 0;
http.createServer((req, res) => {
  const chunks = []; req.on('data', d => chunks.push(d));
  req.on('end', () => {
    const b = Buffer.concat(chunks); n++;
    const o = JSON.stringify({ parsed: { method: req.method, target: req.url, host: req.headers.host, len: b.length, sha: crypto.createHash('sha256').update(b).digest('hex').slice(0, 12) } });
    res.writeHead(200, { 'content-type': 'application/json', 'content-length': Buffer.byteLength(o) }); res.end(o);
  });
}).listen(9101, '127.0.0.1');
