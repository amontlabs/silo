// The build writes one id into the Workshop's page and into its script, the same in both for one build, and another
// for a build whose only difference is a chunk (here the notes): a page left open across a deployment tells the two
// apart by it (src/workshop/chunks.ts).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { appendFile, cp, mkdtemp, readdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildSite, workshopOf } from './workshop-builds.mjs';

const website = fileURLToPath(new URL('..', import.meta.url));
const sample = join(website, '..', 'tools', 'plan', 'test', 'fixtures', 'sample');

const dir = await mkdtemp(join(tmpdir(), 'silo-build-id-'));
test.after(() => rm(dir, { recursive: true, force: true }));
const changed = join(dir, 'plan');
await cp(sample, changed, { recursive: true });
await appendFile(join(changed, 'engine', 'features', 'start-computer.md'), '\nOne more line of notes.\n');
await Promise.all([buildSite(join(dir, 'a'), sample), buildSite(join(dir, 'b'), changed)]);
const a = await workshopOf(join(dir, 'a'));
const b = await workshopOf(join(dir, 'b'));
const notesChunk = async (dist) => (await readdir(join(dist, 'assets'))).filter((name) => /notes/i.test(name));

test('a build writes one id into its page, in the head, after the prerender', () => {
  for (const build of [a, b]) {
    assert.equal(build.metas.length, 1);
    assert.match(build.id, /^[0-9a-f-]{36}$/);
    assert.ok(build.html.indexOf('name="workshop-build"') < build.html.indexOf('</head>'));
    assert.match(build.html, /<div id="workshop-root" data-prerendered>/);
  }
});

test('the script the page runs holds the same id as the page', () => {
  assert.ok(a.script?.includes(a.id), 'build A: the id is in its script');
  assert.ok(b.script?.includes(b.id), 'build B: the id is in its script');
});

test('two builds whose only difference is a chunk have two ids, and two scripts', async () => {
  assert.notDeepEqual(await notesChunk(join(dir, 'a')), await notesChunk(join(dir, 'b')), 'the notes differ');
  assert.notEqual(a.id, b.id);
  assert.notEqual(a.src, b.src);
  assert.ok(!a.script?.includes(b.id));
  assert.ok(!b.script?.includes(a.id));
});

test('the home page carries no id', async () => {
  const { readFile } = await import('node:fs/promises');
  assert.doesNotMatch(await readFile(join(dir, 'a', 'index.html'), 'utf8'), /workshop-build/);
});
