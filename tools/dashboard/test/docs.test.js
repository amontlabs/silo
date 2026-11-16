import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { docChanges, DOC_FILES, lineDiff, parseJourneys, readDoc, splitSections } from '../src/docs.js';
import { readPrivateNotes } from '../src/private.js';
import { commitAll, git, makeRepo, PRODUCT, tempDir, writeDoc } from './helpers.js';

const docsOf = (repo) => Object.fromEntries(Object.entries(DOC_FILES).map(([name, file]) => [name, readDoc(repo, file)]));

test('journeys: ids from the comment line or the heading, four step statuses, other [words] stay text', () => {
  const { preamble, journeys } = parseJourneys(`# Journeys

Intro text.

## First run
<!-- id: first-run -->

- [built] Install
- [partial] Start a computer
- [missing] Guided tour
- [untested] Restore from backup
- [Untested] Second untested step
- [later] Not a status
- [built]Glued text is not a step
- plain item

\`\`\`
- [built] inside a fence
\`\`\`

## Share a Workspace!

- [missing] Revoke

## Share a Workspace!

- [built] Duplicate heading
`);
  assert.match(preamble, /Intro text/);
  assert.deepEqual(journeys.map((j) => j.id), ['first-run', 'share-a-workspace', 'share-a-workspace-2']);
  assert.deepEqual(journeys[0].counts, { built: 1, partial: 1, missing: 1, untested: 2 });
  assert.deepEqual(journeys[0].steps.map((step) => step.status), ['built', 'partial', 'missing', 'untested', 'untested']);
  assert.doesNotMatch(journeys[0].body, /<!-- id/);
  assert.deepEqual(journeys[1].counts, { built: 0, partial: 0, missing: 1, untested: 0 });
  assert.equal(journeys[1].title, 'Share a Workspace!');

});

test('journeys: an empty or missing file has none', () => {
  assert.deepEqual(parseJourneys('').journeys, []);
  assert.deepEqual(parseJourneys(null).journeys, []);
});

test('sections split at ## outside code fences', () => {
  const { preamble, sections } = splitSections('# T\n\nintro\n\n## A\n\n```\n## not a heading\n```\n\n## B\ntext');
  assert.equal(preamble, '# T\n\nintro');
  assert.deepEqual(sections.map((s) => s.key), ['a', 'b']);
  assert.match(sections[0].text, /## not a heading/);
});

test('readDoc reads only the three known files and never leaves docs/product', () => {
  const repo = makeRepo();
  assert.match(readDoc(repo, 'PRODUCT.md'), /Silo gives/);
  assert.equal(readDoc(repo, '../../etc/passwd'), null);
  assert.equal(readDoc(repo, 'OTHER.md'), null);
  const outside = path.join(tempDir(), 'secret.md');
  fs.writeFileSync(outside, 'secret');
  fs.rmSync(path.join(repo, 'docs/product/PRODUCT.md'));
  fs.symlinkSync(outside, path.join(repo, 'docs/product/PRODUCT.md'));
  assert.equal(readDoc(repo, 'PRODUCT.md'), null);
  fs.rmSync(path.join(repo, 'docs'), { recursive: true });
  assert.equal(readDoc(repo, 'JOURNEYS.md'), null);
});

test('changes: no recorded commit shows every section as new', () => {
  const repo = makeRepo();
  const result = docChanges(repo, null, docsOf(repo));
  assert.equal(result.state, 'never');
  assert.ok(result.changes.length > 0);
  assert.ok(result.changes.every((change) => change.status === 'new'));
});

test('changes: an unknown commit shows every section as new, without an error', () => {
  const repo = makeRepo();
  for (const commit of ['0123456789abcdef0123456789abcdef01234567', 'not-a-commit; rm -rf /', '--help']) {
    const result = docChanges(repo, commit, docsOf(repo));
    assert.equal(result.state, 'unknown');
    assert.ok(result.changes.every((change) => change.status === 'new'));
  }
});

test('changes across a commit: changed, new and removed sections, then uncommitted edits', () => {
  const repo = makeRepo();
  const seen = git(repo, 'rev-parse', 'HEAD').trim();
  const base = docChanges(repo, seen, docsOf(repo));
  assert.equal(base.state, 'tracked');
  assert.deepEqual(base.changes, []);

  writeDoc(
    repo,
    'PRODUCT.md',
    PRODUCT.replace('cannot hurt the owner', 'can never hurt the owner').replace(/## Who it is for[\s\S]*$/, '## Pricing\n\nFree.\n'),
  );
  commitAll(repo, 'edit product');
  const after = docChanges(repo, seen, docsOf(repo));
  const byKey = Object.fromEntries(after.changes.map((change) => [change.key, change]));
  assert.equal(byKey['why-it-exists'].status, 'changed');
  assert.match(byKey['why-it-exists'].before, /cannot hurt/);
  assert.match(byKey['why-it-exists'].after, /can never hurt/);
  assert.equal(byKey.pricing.status, 'new');
  assert.equal(byKey['who-it-is-for'].status, 'removed');
  assert.equal(after.changes.length, 3);

  // Marking HEAD as seen clears them; an uncommitted edit counts again.
  const head = after.head;
  assert.deepEqual(docChanges(repo, head, docsOf(repo)).changes, []);
  fs.appendFileSync(path.join(repo, 'docs/product/DECISIONS.md'), '- Uncommitted decision.\n');
  const dirty = docChanges(repo, head, docsOf(repo));
  assert.deepEqual(dirty.changes.map((change) => [change.file, change.key, change.status]), [['DECISIONS.md', 'pricing', 'changed']]);
});

test('changes: a file added after the seen commit counts as new', () => {
  const repo = makeRepo({ decisions: null });
  const seen = docChanges(repo, null, docsOf(repo)).head;
  writeDoc(repo, 'DECISIONS.md', '# Decisions\n\n- One.\n');
  const result = docChanges(repo, seen, docsOf(repo));
  assert.deepEqual(result.changes.map((change) => [change.file, change.status]), [['DECISIONS.md', 'new']]);
});

test('changes: a folder that is not a git checkout reports nogit', () => {
  const folder = tempDir();
  assert.equal(docChanges(folder, null, { product: null, journeys: null, decisions: null }).state, 'nogit');
});

test('line diff', () => {
  assert.deepEqual(lineDiff('a\nb\nc', 'a\nc\nd'), [
    { op: ' ', line: 'a' },
    { op: '-', line: 'b' },
    { op: ' ', line: 'c' },
    { op: '+', line: 'd' },
  ]);
  assert.deepEqual(lineDiff('', 'x'), [{ op: '+', line: 'x' }]);
});

test('private notes: recursive, grouped by folder, sorted by file name, Markdown only', () => {
  const home = tempDir();
  const root = path.join(home, 'private');
  fs.mkdirSync(path.join(root, 'b-folder', 'deep'), { recursive: true });
  fs.mkdirSync(path.join(root, 'a-folder'));
  fs.writeFileSync(path.join(root, 'zeta.md'), '# Zeta');
  fs.writeFileSync(path.join(root, 'alpha.md'), '# Alpha');
  fs.writeFileSync(path.join(root, 'image.png'), 'not markdown');
  fs.writeFileSync(path.join(root, '.hidden.md'), 'hidden');
  fs.writeFileSync(path.join(root, 'b-folder', 'two.md'), 'two');
  fs.writeFileSync(path.join(root, 'b-folder', 'one.md'), 'one');
  fs.writeFileSync(path.join(root, 'b-folder', 'deep', 'x.md'), 'x');
  fs.writeFileSync(path.join(root, 'a-folder', 'note.MD'), 'upper');
  const groups = readPrivateNotes(home);
  assert.deepEqual(
    groups.map((group) => [group.folder, group.notes.map((note) => note.path)]),
    [
      ['', ['alpha.md', 'zeta.md']],
      ['a-folder', ['a-folder/note.MD']],
      ['b-folder', ['b-folder/one.md', 'b-folder/two.md']],
      ['b-folder/deep', ['b-folder/deep/x.md']],
    ],
  );
  assert.equal(groups[0].notes[0].text, '# Alpha');
});

test('private notes: a missing or empty folder gives none; links that leave the folder are skipped', () => {
  const home = tempDir();
  assert.deepEqual(readPrivateNotes(home), []);
  fs.mkdirSync(path.join(home, 'private'));
  assert.deepEqual(readPrivateNotes(home), []);

  const outside = tempDir();
  fs.writeFileSync(path.join(outside, 'secret.md'), 'secret');
  fs.symlinkSync(path.join(outside, 'secret.md'), path.join(home, 'private', 'link.md'));
  fs.symlinkSync(outside, path.join(home, 'private', 'linked-folder'));
  fs.symlinkSync(path.join(home, 'private'), path.join(home, 'private', 'loop'));
  assert.deepEqual(readPrivateNotes(home), []);

  // A link that stays inside is followed.
  fs.writeFileSync(path.join(home, 'private', 'real.md'), 'real');
  fs.symlinkSync(path.join(home, 'private', 'real.md'), path.join(home, 'private', 'alias.md'));
  assert.deepEqual(readPrivateNotes(home)[0].notes.map((note) => note.path), ['alias.md', 'real.md']);
});
