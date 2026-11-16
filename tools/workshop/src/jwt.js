// The GitHub App JWT, RS256 with node:crypto: issued 60 seconds in the past
// for clock drift, valid 9 minutes (GitHub allows at most 10).
import { createPrivateKey, createSign } from 'node:crypto';
import { Failure } from './names.js';

const b64url = (value) => Buffer.from(value).toString('base64url');

export function appJwt(appId, privateKeyPem, now = Date.now()) {
  let key;
  try {
    key = createPrivateKey(privateKeyPem);
  } catch {
    throw new Failure('WORKSHOP_PRIVATE_KEY is not a valid PEM private key');
  }
  const seconds = Math.floor(now / 1000);
  const header = b64url(JSON.stringify({ alg: 'RS256', typ: 'JWT' }));
  const payload = b64url(JSON.stringify({ iat: seconds - 60, exp: seconds + 540, iss: String(appId) }));
  const signature = createSign('RSA-SHA256').update(`${header}.${payload}`).sign(key);
  return `${header}.${payload}.${b64url(signature)}`;
}
