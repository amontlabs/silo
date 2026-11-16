import assert from 'node:assert/strict';
import { chmodSync, mkdirSync, writeFileSync } from 'node:fs';
import { dirname } from 'node:path';
import { test } from 'node:test';
import { appCredentials, credentialsFile, lookup } from '../src/credentials.js';
import { appJwt } from '../src/jwt.js';
import { Failure } from '../src/names.js';
import { appKeys, verifyJwt } from './fake-github.js';
import { fakeKeychain, sandbox } from './helpers.js';

test('the App JWT is RS256, verifies with the public key and carries iss, iat and exp', () => {
  const { privateKey, publicKey } = appKeys();
  const now = Date.parse('2026-11-16T12:00:00Z');
  const jwt = appJwt('4242', privateKey, now);
  const decoded = verifyJwt(jwt, publicKey);
  assert.ok(decoded, 'signature verifies');
  assert.deepEqual(decoded.header, { alg: 'RS256', typ: 'JWT' });
  assert.equal(decoded.claims.iss, '4242');
  assert.equal(decoded.claims.iat, now / 1000 - 60);
  assert.ok(decoded.claims.exp - now / 1000 <= 600);
  const tampered = jwt.replace(/\.[^.]+\./, `.${Buffer.from(JSON.stringify({ iss: '1', iat: 0, exp: 9e9 })).toString('base64url')}.`);
  assert.equal(verifyJwt(tampered, publicKey), null);
  assert.throws(() => appJwt('1', 'not a key'), Failure);
});

function writeCredentials(home, data, mode) {
  const path = credentialsFile(home);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, JSON.stringify(data));
  chmodSync(path, mode);
  return path;
}

test('credentials come from the environment, then the Keychain, then the 0600 file', (t) => {
  const box = sandbox(t, { features: '', decisions: '' });
  writeCredentials(box.home, { WORKSHOP_APP_ID: 'file-id', WORKSHOP_PRIVATE_KEY: 'file-key', WORKSHOP_CLIENT_ID: 'file-client' }, 0o600);
  const keychain = fakeKeychain({ WORKSHOP_APP_ID: 'keychain-id', WORKSHOP_PRIVATE_KEY: 'keychain-key' });
  const env = { WORKSHOP_APP_ID: 'env-id' };
  const { values, sources } = lookup(['WORKSHOP_APP_ID', 'WORKSHOP_PRIVATE_KEY', 'WORKSHOP_CLIENT_ID', 'WORKSHOP_CLIENT_SECRET'], { env, home: box.home, keychain });
  assert.deepEqual(values, { WORKSHOP_APP_ID: 'env-id', WORKSHOP_PRIVATE_KEY: 'keychain-key', WORKSHOP_CLIENT_ID: 'file-client' });
  assert.deepEqual(sources, { WORKSHOP_APP_ID: 'environment', WORKSHOP_PRIVATE_KEY: 'keychain', WORKSHOP_CLIENT_ID: 'file', WORKSHOP_CLIENT_SECRET: null });
});

test('a credentials file readable by others is refused, and the error names no value', (t) => {
  const box = sandbox(t, { features: '', decisions: '' });
  for (const mode of [0o644, 0o640, 0o604]) {
    writeCredentials(box.home, { WORKSHOP_APP_ID: 'file-id', WORKSHOP_PRIVATE_KEY: 'very-secret-key' }, mode);
    assert.throws(
      () => appCredentials({ env: {}, home: box.home, keychain: fakeKeychain() }),
      (error) => error instanceof Failure && /chmod 600/.test(error.message) && !error.message.includes('very-secret-key'),
    );
  }
  writeCredentials(box.home, { WORKSHOP_APP_ID: 'file-id', WORKSHOP_PRIVATE_KEY: 'very-secret-key' }, 0o600);
  assert.equal(appCredentials({ env: {}, home: box.home, keychain: fakeKeychain() }).appId, 'file-id');
});

test('the file is not read at all when the environment and Keychain have everything', (t) => {
  const box = sandbox(t, { features: '', decisions: '' });
  writeCredentials(box.home, { WORKSHOP_APP_ID: 'x' }, 0o644);
  const result = appCredentials({ env: { WORKSHOP_APP_ID: '1' }, home: box.home, keychain: fakeKeychain({ WORKSHOP_PRIVATE_KEY: 'k' }) });
  assert.equal(result.appId, '1');
});

test('missing credentials are named without values', (t) => {
  const box = sandbox(t, { features: '', decisions: '' });
  assert.throws(() => appCredentials({ env: {}, home: box.home, keychain: fakeKeychain({ WORKSHOP_APP_ID: '9' }) }), /missing WORKSHOP_PRIVATE_KEY/);
});
