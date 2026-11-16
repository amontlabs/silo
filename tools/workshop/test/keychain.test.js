import assert from 'node:assert/strict';
import { generateKeyPairSync } from 'node:crypto';
import { test } from 'node:test';
import { MAX_LINE, addCommand, createMacKeychain, decodeValue } from '../src/keychain.js';
import { Failure } from '../src/names.js';
import { appKeys } from './fake-github.js';

test('the stored line for a real 2048-bit PEM is one line under the limit, with the value as base64 after -w', () => {
  const pem = appKeys().privateKey;
  assert.ok(pem.length > 1600, 'a real-size key');
  const line = addCommand('WORKSHOP_PRIVATE_KEY', pem);
  assert.ok(Buffer.byteLength(line) < MAX_LINE);
  assert.equal(line.indexOf('\n'), line.length - 1, 'exactly one line');
  assert.ok(!line.includes(' -X '));
  const value = line.match(/ -w (base64:[A-Za-z0-9+/=]+)\n$/)[1];
  assert.equal(decodeValue(value), pem);
  assert.equal(line, `add-generic-password -U -s silo-workshop -a WORKSHOP_PRIVATE_KEY -l "Silo Workshop WORKSHOP_PRIVATE_KEY" -w ${value}\n`);
});

test('a value whose line would reach 4,000 bytes is refused before anything runs', () => {
  const { privateKey } = generateKeyPairSync('rsa', { modulusLength: 4096, privateKeyEncoding: { type: 'pkcs8', format: 'pem' }, publicKeyEncoding: { type: 'spki', format: 'pem' } });
  assert.throws(() => addCommand('WORKSHOP_PRIVATE_KEY', privateKey), Failure);
  let calls = 0;
  const keychain = createMacKeychain({ platform: 'darwin', exists: () => true, spawnSync: () => (calls += 1, { status: 0, stdout: '' }) });
  assert.throws(() => keychain.set('WORKSHOP_PRIVATE_KEY', privateKey), Failure);
  assert.equal(calls, 0);
  assert.throws(() => addCommand('NOT; a name', 'x'));
});

function scripted(statuses) {
  const calls = [];
  const spawnSync = (command, args, options) => {
    calls.push({ args, input: options.input });
    const next = statuses.shift();
    return typeof next === 'function' ? next(args, options) : next;
  };
  return { calls, keychain: createMacKeychain({ platform: 'darwin', exists: () => true, spawnSync }) };
}

test('only "not found" means absent; any other failure is a locked or unavailable Keychain', () => {
  const { keychain } = scripted([{ status: 44, stdout: '' }, { status: 0, stdout: 'base64:aGVsbG8=\n' }, { status: 0, stdout: 'plain\n' }, { status: 51, stdout: '' }, { status: 36, stdout: '' }, { status: null, error: new Error('spawn') }]);
  assert.equal(keychain.get('WORKSHOP_APP_ID'), null);
  assert.equal(keychain.get('WORKSHOP_APP_ID'), 'hello');
  assert.equal(keychain.get('WORKSHOP_APP_ID'), 'plain');
  for (let i = 0; i < 3; i += 1) assert.throws(() => keychain.get('WORKSHOP_APP_ID'), /the Keychain is locked or unavailable/);
});

test('set sends the line on stdin and reads the value back', () => {
  const ok = scripted([{ status: 0 }, (args) => ({ status: 0, stdout: `base64:${Buffer.from('v').toString('base64')}\n` })]);
  ok.keychain.set('WORKSHOP_APP_ID', 'v');
  assert.deepEqual(ok.calls[0].args, ['-i']);
  assert.equal(ok.calls[0].input, addCommand('WORKSHOP_APP_ID', 'v'));
  const lost = scripted([{ status: 0 }, { status: 44, stdout: '' }]);
  assert.throws(() => lost.keychain.set('WORKSHOP_APP_ID', 'v'), /did not store correctly/);
  const failed = scripted([{ status: 1 }]);
  assert.throws(() => failed.keychain.set('WORKSHOP_APP_ID', 'v'), /locked or unavailable/);
});
