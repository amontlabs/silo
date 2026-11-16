// Runs the Workshop API on Node's http request and response objects, as both
// Vercel's Node.js launcher and the local server provide them.
import { readConfig } from './config.mjs';
import { createWorkshop } from './workshop.mjs';

export const MAX_BODY = 64 * 1024;

export async function readRequest(req) {
  const headers = {};
  for (const [name, value] of Object.entries(req.headers)) headers[name.toLowerCase()] = Array.isArray(value) ? value.join(', ') : value;
  let body = '';
  if (req.method === 'POST') {
    const chunks = [];
    let size = 0;
    for await (const chunk of req) {
      size += chunk.length;
      if (size > MAX_BODY) return { tooLarge: true };
      chunks.push(chunk);
    }
    body = Buffer.concat(chunks).toString('utf8');
  }
  return { method: req.method, url: req.url, headers, body };
}

export const FRAME_HEADERS = { 'Content-Security-Policy': "frame-ancestors 'none'", 'X-Frame-Options': 'DENY' };

export function writeResponse(res, response) {
  res.statusCode = response.status;
  for (const [name, value] of Object.entries(FRAME_HEADERS)) res.setHeader(name, value);
  for (const [name, value] of Object.entries(response.headers)) res.setHeader(name, value);
  if (response.cookies?.length) res.setHeader('Set-Cookie', response.cookies);
  res.setHeader('X-Content-Type-Options', 'nosniff');
  res.setHeader('Referrer-Policy', 'no-referrer');
  res.end(response.body);
}

export function createNodeHandler(workshop) {
  const plain = (status, error, message) => ({ status, headers: { 'Content-Type': 'application/json; charset=utf-8', 'Cache-Control': 'private, no-store' }, body: JSON.stringify({ error, message }) });
  return async function handler(req, res) {
    let response;
    try {
      const request = await readRequest(req);
      response = request.tooLarge ? plain(413, 'too_long', 'The request is too large.') : await workshop.handle(request);
    } catch {
      // An aborted or unreadable request: answer if the connection is still open.
      response = plain(400, 'bad_input', 'The request was not understood.');
    }
    if (res.headersSent || res.writableEnded || res.destroyed) return;
    try {
      writeResponse(res, response);
    } catch {
      res.destroy?.();
    }
  };
}

let shared = null;

// The handler for the deployed function, built once per instance from the environment.
export async function vercelHandler(req, res) {
  shared ??= createNodeHandler(createWorkshop({ config: readConfig(process.env) }));
  return shared(req, res);
}
