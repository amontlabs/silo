import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { docChanges, DOC_FILES, firstSentence, lineDiff, parseDecisionLine, parseDecisions, parseRestart, productLayout, readDoc, splitSections } from '../src/docs.js';
import { noteSummary, readPrivateNotes } from '../src/private.js';
import { commitAll, git, makeRepo, PRODUCT, tempDir, writeDoc } from './helpers.js';

const docsOf = (repo) => Object.fromEntries(Object.entries(DOC_FILES).map(([name, file]) => [name, readDoc(repo, file)]));

test('sections split at ## outside code fences', () => {
  const { preamble, sections } = splitSections('# T\n\nintro\n\n## A\n\n```\n## not a heading\n```\n\n## B\ntext');
  assert.equal(preamble, '# T\n\nintro');
  assert.deepEqual(sections.map((s) => s.key), ['a', 'b']);
  assert.match(sections[0].text, /## not a heading/);
});

test('readDoc reads only the known files and never leaves docs/product', () => {
  const repo = makeRepo();
  assert.match(readDoc(repo, 'PRODUCT.md'), /Silo gives/);
  assert.match(readDoc(repo, 'RESTART.md'), /Where the work stands/);
  writeDoc(repo, 'JOURNEYS.md', '# Journeys\n');
  assert.equal(readDoc(repo, 'JOURNEYS.md'), null);
  assert.equal(readDoc(repo, '../../etc/passwd'), null);
  assert.equal(readDoc(repo, 'OTHER.md'), null);
  const outside = path.join(tempDir(), 'secret.md');
  fs.writeFileSync(outside, 'secret');
  fs.rmSync(path.join(repo, 'docs/product/PRODUCT.md'));
  fs.symlinkSync(outside, path.join(repo, 'docs/product/PRODUCT.md'));
  assert.equal(readDoc(repo, 'PRODUCT.md'), null);
  fs.rmSync(path.join(repo, 'docs'), { recursive: true });
  assert.equal(readDoc(repo, 'DECISIONS.md'), null);
});

test('changes: no recorded commit marks nothing as new', () => {
  const repo = makeRepo();
  const result = docChanges(repo, null, docsOf(repo));
  assert.equal(result.state, 'never');
  assert.deepEqual(result.changes, []);
  assert.match(result.head, /^[0-9a-f]{40}$/);
});

test('changes: an unknown commit marks nothing as new, without an error', () => {
  const repo = makeRepo();
  for (const commit of ['0123456789abcdef0123456789abcdef01234567', 'not-a-commit; rm -rf /', '--help']) {
    const result = docChanges(repo, commit, docsOf(repo));
    assert.equal(result.state, 'unknown');
    assert.deepEqual(result.changes, []);
  }
});

test('changes cover PRODUCT.md, DECISIONS.md and RESTART.md, and nothing else', () => {
  const repo = makeRepo();
  const seen = git(repo, 'rev-parse', 'HEAD').trim();
  writeDoc(repo, 'PRODUCT.md', `${PRODUCT}\n## Added\n\nNew.\n`);
  writeDoc(repo, 'DECISIONS.md', '# Decisions\n\n## Product\n\n- Silo is a Mac app first.\n');
  writeDoc(repo, 'RESTART.md', '# Restart\n\n## Where the work stands\n\n1. Now: something else.\n');
  writeDoc(repo, 'JOURNEYS.md', '# Journeys\n\n## Old\n\nx\n');
  const result = docChanges(repo, seen, docsOf(repo));
  assert.deepEqual(result.changes.map((change) => [change.file, change.key, change.status]), [
    ['PRODUCT.md', 'added', 'new'],
    ['DECISIONS.md', 'product', 'changed'],
    ['DECISIONS.md', 'pricing', 'removed'],
    ['RESTART.md', 'where-the-work-stands', 'changed'],
  ]);
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
  assert.equal(docChanges(folder, null, { product: null, decisions: null, restart: null }).state, 'nogit');
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

test('where the work stands: the ordered list under its heading, each step with its state word', () => {
  const steps = parseRestart(`# Restart

## Decided

1. Now: not this list.

## Where the Work Stands

1. Done: the old tree is frozen (tag \`legacy\`).
2. DONE: the list,
   on two lines.
3. now: go down the Stack. The first 41 are the starter.
4. Next: start \`main\` again. Wait first.
5. Then: design the shell.
6. Later: something else entirely.
7. No word here: kept whole.
8. Then:glued
- a bullet, not a step

\`\`\`
9. Done: inside a fence
\`\`\`

## When a session starts

1. Done: another list.
`);
  assert.deepEqual(steps, [
    { state: 'done', text: 'the old tree is frozen (tag `legacy`).' },
    { state: 'done', text: 'the list, on two lines.' },
    { state: 'now', text: 'go down the Stack. The first 41 are the starter.' },
    { state: 'next', text: 'start `main` again. Wait first.' },
    { state: 'then', text: 'design the shell.' },
    { state: 'other', text: 'Later: something else entirely.' },
    { state: 'other', text: 'No word here: kept whole.' },
    { state: 'then', text: 'glued' },
  ]);
  assert.equal(firstSentence('start `main` again. Wait first.'), 'start `main` again.');
  assert.equal(firstSentence('keep `a. b` together. Rest.'), 'keep `a. b` together.');
  assert.equal(firstSentence('one sentence only'), 'one sentence only');
  // No file, no heading, no list: nothing.
  assert.equal(parseRestart(null), null);
  assert.equal(parseRestart('# Restart\n\n## Decided\n\n1. Done: x\n'), null);
  assert.equal(parseRestart('## Where the work stands\n\nNo list yet.\n'), null);
});

test('decisions: date · who · text, grouped by date newest first; anything else goes to Other', () => {
  const result = parseDecisions(`# Decisions

Intro paragraph · with dots · not a decision.

- 2026-11-15 · owner · Three journeys come first.
- 2026-11-16 · orchestrator, at the owner's word · Merging changes main only · not the recipe.
- 2026-11-15 · owner · Security is built properly.
- 2026-11-16 · owner · The dashboard is a folder.
- 2026-02-30 · owner · Not a real date.
- 16 Nov · owner · Wrong date shape.
- 2026-11-14 · owner
- 2026-11-14 ·  · No one decided.
- Plain line without the shape.
  continued here

\`\`\`
- 2026-11-17 · owner · inside a fence
\`\`\`

## Old

* 2026-11-10 · owner · Older, under a heading.
`);
  assert.deepEqual(result.days.map((day) => [day.date, day.items.map((item) => [item.who, item.text])]), [
    ['2026-11-16', [["orchestrator, at the owner's word", 'Merging changes main only · not the recipe.'], ['owner', 'The dashboard is a folder.']]],
    ['2026-11-15', [['owner', 'Three journeys come first.'], ['owner', 'Security is built properly.']]],
    ['2026-11-10', [['owner', 'Older, under a heading.']]],
  ]);
  assert.deepEqual(result.other, [
    '2026-02-30 · owner · Not a real date.',
    '16 Nov · owner · Wrong date shape.',
    '2026-11-14 · owner',
    '2026-11-14 ·  · No one decided.',
    'Plain line without the shape. continued here',
  ]);
  assert.equal(result.total, 10);
  assert.deepEqual(parseDecisionLine('2026-11-16 · owner · a · b'), { date: '2026-11-16', who: 'owner', text: 'a · b' });
  assert.deepEqual(parseDecisions(null), { days: [], other: [], total: 0 });
});

test('product layout: the hero is "In one sentence" (else the first paragraph); the rest are tiles, some wide', () => {
  const layout = productLayout(`# Silo

Intro paragraph.

## Who it is for

People.

## In one sentence

Silo gives an agent
its own computer.

## The loop

Loop.

## Small

Text.
`);
  assert.deepEqual(layout.hero, { key: 'in-one-sentence', text: 'Silo gives an agent\nits own computer.' });
  assert.deepEqual(layout.tiles.map((tile) => [tile.key, tile.heading, tile.text, tile.wide]), [
    ['who-it-is-for', 'Who it is for', 'People.', true],
    ['the-loop', 'The loop', 'Loop.', true],
    ['small', 'Small', 'Text.', false],
  ]);
  assert.deepEqual(productLayout('# Silo\n\n- a list\n\nFirst real paragraph\non two lines.\n\n## Rules that hold everywhere\n\nR.\n').hero, { key: null, text: 'First real paragraph\non two lines.' });
  assert.deepEqual(productLayout('## The problem\n\n```\ncode\n```\n\nBody.').hero, { key: null, text: 'Body.' });
  assert.equal(productLayout('## The problem\n\nBody.').tiles[0].wide, true);
});

test('private note tiles: title from the first # heading or the file name; summary is the first paragraph, cut', () => {
  assert.deepEqual(noteSummary({ name: 'pilot.md', text: '# Pilot **strategy**\n\n## Part\n\nProduct review, [pain points](x.md) and `pricing`.\n' }), {
    title: 'Pilot strategy',
    summary: 'Product review, pain points and pricing.',
  });
  assert.deepEqual(noteSummary({ name: 'plans.md', text: '- Plan one\n\nA line.' }), { title: 'plans', summary: 'A line.' });
  const long = noteSummary({ name: 'x.md', text: `# X\n\n${'word '.repeat(60)}` }, 40);
  assert.equal(long.summary.length, 40);
  assert.ok(long.summary.endsWith('…'));
});
