import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { DashboardError, ConflictError, NotFoundError, ValidationError } from './errors.js';
import { DOC_FILES, docChanges, headCommit, readDoc } from './docs.js';
import { renderPage } from './views.js';

const HERE = path.dirname(fileURLToPath(import.meta.url));
export const DEFAULT_REPO = path.resolve(HERE, '../../..');
const PUBLIC_DIR = path.resolve(HERE, '../public');
const MERMAID_DIR = path.resolve(HERE, '../node_modules/mermaid/dist');
const MAX_BODY = 1_000_000;

const SECURITY_HEADERS = {
  'X-Content-Type-Options': 'nosniff',
  'Referrer-Policy': 'no-referrer',
  'X-Frame-Options': 'DENY',
  'Cross-Origin-Resource-Policy': 'same-origin',
};

const PAGE_CSP = [
  "default-src 'self'",
  "script-src 'self'",
  "style-src 'self' 'unsafe-inline'",
  "img-src 'self' data:",
  "media-src 'self'",
  "connect-src 'self'",
  "worker-src 'self' blob:",
  "object-src 'none'",
  "base-uri 'none'",
  "form-action 'none'",
  "frame-ancestors 'none'",
].join('; ');

const STATIC_FILES = {
  '/app.css': ['app.css', 'text/css; charset=utf-8'],
  '/app.js': ['app.js', 'text/javascript; charset=utf-8'],
};

function send(res, status, body, headers = {}) {
  res.writeHead(status, { ...SECURITY_HEADERS, 'Cache-Control': 'no-store', ...headers });
  res.end(body);
}

function sendJson(res, status, value) {
  send(res, status, JSON.stringify(value), { 'Content-Type': 'application/json; charset=utf-8' });
}

function readBody(req) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    let size = 0;
    req.on('data', (chunk) => {
      size += chunk.length;
      if (size > MAX_BODY) {
        reject(new ValidationError('request body is too large'));
        req.destroy();
      } else chunks.push(chunk);
    });
    req.on('end', () => resolve(Buffer.concat(chunks).toString('utf8')));
    req.on('error', reject);
  });
}

async function jsonBody(req) {
  const type = (req.headers['content-type'] || '').split(';')[0].trim().toLowerCase();
  if (type !== 'application/json') throw new ValidationError('send application/json');
  const text = await readBody(req);
  if (!text.trim()) return {};
  try {
    const value = JSON.parse(text);
    if (value === null || typeof value !== 'object' || Array.isArray(value)) throw new Error('not an object');
    return value;
  } catch {
    throw new ValidationError('the request body is not a JSON object');
  }
}

// "bytes=a-b" for a file of `size` bytes: { start, end } or null when unsatisfiable. undefined: no usable range.
export function parseRange(header, size) {
  const match = /^bytes=(\d*)-(\d*)$/.exec((header || '').trim());
  if (!match || (match[1] === '' && match[2] === '')) return undefined;
  let start;
  let end;
  if (match[1] === '') {
    const length = Number(match[2]);
    if (length === 0) return null;
    start = Math.max(0, size - length);
    end = size - 1;
  } else {
    start = Number(match[1]);
    end = match[2] === '' ? size - 1 : Math.min(Number(match[2]), size - 1);
  }
  if (start >= size || start > end) return null;
  return { start, end };
}

function serveFile(req, res, file, headers) {
  let stat;
  try {
    stat = fs.statSync(file);
  } catch {
    return send(res, 404, 'Not found');
  }
  if (!stat.isFile()) return send(res, 404, 'Not found');
  const base = { ...SECURITY_HEADERS, 'Cache-Control': 'no-store', 'Accept-Ranges': 'bytes', ...headers };
  const range = parseRange(req.headers.range, stat.size);
  if (range === null) {
    res.writeHead(416, { ...base, 'Content-Range': `bytes */${stat.size}` });
    return res.end();
  }
  const status = range ? 206 : 200;
  const start = range ? range.start : 0;
  const end = range ? range.end : stat.size - 1;
  res.writeHead(status, {
    ...base,
    'Content-Length': stat.size === 0 ? 0 : end - start + 1,
    ...(range ? { 'Content-Range': `bytes ${start}-${end}/${stat.size}` } : {}),
  });
  if (req.method === 'HEAD' || stat.size === 0) return res.end();
  const stream = fs.createReadStream(file, { start, end });
  stream.on('error', () => res.destroy());
  stream.pipe(res);
}

function attachmentHeaders(attachment) {
  const inline = /^(image|video|audio)\//.test(attachment.media_type);
  const encoded = encodeURIComponent(attachment.file).replace(/['()*]/g, (c) => `%${c.charCodeAt(0).toString(16).toUpperCase()}`);
  const plain = attachment.file.replace(/[^A-Za-z0-9._-]/g, '_');
  return {
    'Content-Type': attachment.media_type,
    'Content-Disposition': `${inline ? 'inline' : 'attachment'}; filename="${plain}"; filename*=UTF-8''${encoded}`,
    'Content-Security-Policy': "sandbox; default-src 'none'; style-src 'unsafe-inline'",
  };
}

function ownOrigins(port) {
  return { hosts: new Set([`127.0.0.1:${port}`, `localhost:${port}`]), origins: new Set([`http://127.0.0.1:${port}`, `http://localhost:${port}`]) };
}

export function createServer({ store, repoRoot = process.env.SILO_DASHBOARD_REPO || DEFAULT_REPO }) {
  const server = http.createServer(async (req, res) => {
    try {
      await handle(req, res);
    } catch (error) {
      if (error instanceof DashboardError) return sendJson(res, error.status, { error: error.message });
      process.stderr.write(`dashboard: ${error.stack || error}\n`);
      return sendJson(res, 500, { error: 'internal error' });
    }
  });

  const pageView = () => {
    const docs = {};
    for (const [name, file] of Object.entries(DOC_FILES)) docs[name] = readDoc(repoRoot, file);
    return { items: store.itemsWithState(), docs, changes: docChanges(repoRoot, store.getSeen('docs_commit'), docs) };
  };

  async function handle(req, res) {
    const port = server.address().port;
    const allowed = ownOrigins(port);
    // A request addressed to any other name (DNS rebinding) or sent from another origin gets nothing.
    const host = (req.headers.host || '').toLowerCase();
    if (!allowed.hosts.has(host)) return sendJson(res, 403, { error: 'unexpected Host header' });
    const method = req.method;
    if (method !== 'GET' && method !== 'HEAD') {
      const origin = req.headers.origin;
      if (origin !== undefined && !allowed.origins.has(origin.toLowerCase())) return sendJson(res, 403, { error: 'unexpected Origin header' });
      if (req.headers['sec-fetch-site'] && !['same-origin', 'none'].includes(req.headers['sec-fetch-site'])) {
        return sendJson(res, 403, { error: 'cross-site request refused' });
      }
    }
    const url = new URL(req.url, `http://${host}`);
    const pathname = url.pathname;

    if (method === 'GET' || method === 'HEAD') {
      if (pathname === '/') {
        const body = renderPage(pageView()).toString();
        return send(res, 200, method === 'HEAD' ? undefined : body, {
          'Content-Type': 'text/html; charset=utf-8',
          'Content-Security-Policy': PAGE_CSP,
        });
      }
      if (STATIC_FILES[pathname]) {
        const [name, type] = STATIC_FILES[pathname];
        return serveFile(req, res, path.join(PUBLIC_DIR, name), { 'Content-Type': type });
      }
      let match = /^\/vendor\/mermaid\/(.+)$/.exec(pathname);
      if (match) {
        let relative;
        try {
          relative = decodeURIComponent(match[1]);
        } catch {
          return send(res, 404, 'Not found');
        }
        const file = path.resolve(MERMAID_DIR, relative);
        if (!file.startsWith(MERMAID_DIR + path.sep) || !/\.m?js$/.test(file)) return send(res, 404, 'Not found');
        return serveFile(req, res, file, { 'Content-Type': 'text/javascript; charset=utf-8' });
      }
      match = /^\/attachments\/(\d+)(?:\/[^/]*)?$/.exec(pathname);
      if (match) {
        const attachment = store.getAttachment(Number(match[1]));
        const file = attachment && store.attachmentPath(attachment);
        if (!file) return send(res, 404, 'Not found');
        return serveFile(req, res, file, attachmentHeaders(attachment));
      }
      if (pathname === '/favicon.ico') return send(res, 204);
      return send(res, 404, 'Not found');
    }

    if (method === 'POST') {
      let match = /^\/api\/items\/(\d+)\/replies$/.exec(pathname);
      if (match) {
        const body = await jsonBody(req);
        const item = store.requireItem(Number(match[1]));
        const verdict = body.verdict ?? null;
        const text = typeof body.text === 'string' ? body.text : '';
        if (verdict === 'disagree' && !text.trim()) throw new ValidationError('say why you disagree');
        if (item.kind === 'decide' && verdict) throw new ValidationError('a decision is answered in words');
        const id = store.addReply(item.id, { author: 'owner', text, verdict });
        return sendJson(res, 201, { id });
      }
      match = /^\/api\/items\/(\d+)\/seen$/.exec(pathname);
      if (match) {
        await jsonBody(req);
        store.markShippedSeen(Number(match[1]));
        return sendJson(res, 200, { ok: true });
      }
      if (pathname === '/api/docs/seen') {
        await jsonBody(req);
        const head = headCommit(repoRoot);
        if (!head) throw new ConflictError('this checkout has no commit to mark as seen');
        store.setSeen('docs_commit', head);
        store.setSeen('docs_seen_at', store.now());
        return sendJson(res, 200, { commit: head });
      }
      throw new NotFoundError('no such endpoint');
    }
    return send(res, 405, 'Method not allowed', { Allow: 'GET, HEAD, POST' });
  }

  return server;
}
