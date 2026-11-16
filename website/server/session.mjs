// Sealed cookies: AES-256-GCM with a key derived from WORKSHOP_SESSION_SECRET
// for each cookie purpose, so a value sealed for one cookie never opens as another.
import { createCipheriv, createDecipheriv, hkdfSync, randomBytes } from 'node:crypto';

export const SESSION_MAX_AGE = 30 * 24 * 60 * 60;
export const STATE_MAX_AGE = 10 * 60;
export const COOKIE_PATH = '/api/workshop';

function keyFor(secret, purpose) {
  return Buffer.from(hkdfSync('sha256', secret, Buffer.alloc(0), `silo-workshop ${purpose} v1`, 32));
}

export function seal(secret, purpose, value) {
  const iv = randomBytes(12);
  const cipher = createCipheriv('aes-256-gcm', keyFor(secret, purpose), iv);
  cipher.setAAD(Buffer.from(purpose));
  const body = Buffer.concat([cipher.update(JSON.stringify(value), 'utf8'), cipher.final()]);
  return Buffer.concat([iv, body, cipher.getAuthTag()]).toString('base64url');
}

export function unseal(secret, purpose, text) {
  if (typeof text !== 'string' || text.length > 8192 || !/^[A-Za-z0-9_-]+$/.test(text)) return null;
  const raw = Buffer.from(text, 'base64url');
  if (raw.length < 12 + 16 + 2) return null;
  try {
    const decipher = createDecipheriv('aes-256-gcm', keyFor(secret, purpose), raw.subarray(0, 12));
    decipher.setAAD(Buffer.from(purpose));
    decipher.setAuthTag(raw.subarray(raw.length - 16));
    const plain = Buffer.concat([decipher.update(raw.subarray(12, raw.length - 16)), decipher.final()]);
    const value = JSON.parse(plain.toString('utf8'));
    return value && typeof value === 'object' ? value : null;
  } catch {
    return null;
  }
}

export function cookieNames(secure) {
  return secure
    ? { session: '__Secure-workshop-session', state: '__Secure-workshop-signin' }
    : { session: 'workshop-session', state: 'workshop-signin' };
}

export function parseCookies(header) {
  const cookies = {};
  if (!header) return cookies;
  for (const part of header.split(';')) {
    const index = part.indexOf('=');
    if (index < 0) continue;
    const name = part.slice(0, index).trim();
    if (name && !(name in cookies)) cookies[name] = part.slice(index + 1).trim();
  }
  return cookies;
}

export function serializeCookie(name, value, { maxAge, secure }) {
  const parts = [`${name}=${value}`, `Path=${COOKIE_PATH}`, `Max-Age=${maxAge}`, 'HttpOnly', 'SameSite=Lax'];
  if (secure) parts.push('Secure');
  return parts.join('; ');
}

export function clearCookie(name, { secure }) {
  return serializeCookie(name, '', { maxAge: 0, secure });
}
