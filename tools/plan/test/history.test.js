import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { WHAT, entryHistory, readSteps } from '../history/index.js';
import { cleanup, git, gitWith, temp } from './helpers.js';

const json = (value) => `${JSON.stringify(value, null, 2)}\n`;
const step = (n, files) => ({ commit: `c${n}`, date: `2026-11-${String(n).padStart(2, '0')}T00:00:00Z`, files });
const add = (path, value) => ({ [path]: { before: null, after: typeof value === 'string' ? value : json(value) } });
const edit = (path, before, after) => ({ [path]: { before: typeof before === 'string' ? before : json(before), after: typeof after === 'string' ? after : json(after) } });
const names = (history, id) => history.get(id).map((change) => change.what);

const P = 'engine/decisions/x.json';
const D = { asks: 'Which?', answer: 'This.' };

// entryHistory alone, with steps written by hand.

test('the first step where the file exists is `added`, and carries nothing else', () => {
  const history = entryHistory([step(1, add(P, { ...D, needs: ['a'], verdict: 'take', adds: ['b'] }))]);
  assert.deepEqual(history.get('x'), [{ commit: 'c1', date: '2026-11-01T00:00:00Z', what: ['added'] }]);
  assert.deepEqual([...history.keys()], ['x']);
});

test('each name, alone', () => {
  const base = { asks: 'Q', answer: 'A', needs: ['n'], keeps: ['k'], surfaces: { app: 'a' }, suggest: { verdict: 'take', why: 'w' } };
  const cases = {
    text: [{ ...base, asks: 'Q2' }, ['text']],
    sets: [{ ...base, sets: [{ list: 'words' }] }, ['sets']],
    needs: [{ ...base, needs: ['n', 'm'] }, ['needs']],
    keeps: [{ ...base, keeps: [] }, ['keeps']],
    surfaces: [{ ...base, surfaces: { cli: 'c' } }, ['surfaces']],
    suggest: [{ ...base, suggest: { verdict: 'drop', why: 'w' } }, ['suggest']],
    taken: [{ ...base, verdict: 'take' }, ['taken']],
    dropped: [{ ...base, verdict: 'drop' }, ['dropped']],
    replaced: [{ ...base, replaced_by: 'other' }, ['replaced']],
    started: [{ ...base, started: '2026-10-30' }, ['started']],
    built: [{ ...base, built: '2026-11-01' }, ['built']],
  };
  for (const [name, [after, what]] of Object.entries(cases)) {
    const history = entryHistory([step(1, add(P, base)), step(2, edit(P, base, after))]);
    assert.deepEqual(names(history, 'x'), [what, ['added']], name);
  }
  // Every name is covered, either above or below.
  assert.deepEqual(WHAT, ['added', 'moved', 'text', 'sets', 'needs', 'keeps', 'surfaces', 'suggest', 'notes', 'taken', 'dropped', 'cleared', 'replaced', 'started', 'built']);
  assert.equal(Object.isFrozen(WHAT), true);
});

test('cleared: from take or drop to no verdict', () => {
  for (const verdict of ['take', 'drop']) {
    const history = entryHistory([step(1, add(P, { ...D, verdict })), step(2, edit(P, { ...D, verdict }, D))]);
    assert.deepEqual(names(history, 'x')[0], ['cleared']);
  }
  // Take to drop is `dropped`, not `cleared`.
  const swap = entryHistory([step(1, add(P, { ...D, verdict: 'take' })), step(2, edit(P, { ...D, verdict: 'take' }, { ...D, verdict: 'drop' }))]);
  assert.deepEqual(names(swap, 'x')[0], ['dropped']);
});

test('`replaced` carries `by`; `text` carries the title and body before and after', () => {
  const history = entryHistory([
    step(1, add(P, D)),
    step(2, edit(P, D, { asks: 'Which one?' })),
    step(3, edit(P, { asks: 'Which one?' }, { asks: 'Which one?', replaced_by: 'y', verdict: 'take' })),
  ]);
  assert.deepEqual(history.get('x')[0], { commit: 'c3', date: '2026-11-03T00:00:00Z', what: ['taken', 'replaced'], by: 'y' });
  assert.deepEqual(history.get('x')[1].text, { before: { title: 'Which?', body: 'This.' }, after: { title: 'Which one?', body: null } });
  const feature = 'engine/features/f.json';
  const f = { name: 'Fork', what: 'Copies.' };
  const two = entryHistory([step(1, add(feature, f)), step(2, edit(feature, f, { name: 'Fork it', what: 'Copies a computer.' }))]);
  assert.deepEqual(two.get('f')[0].text, { before: { title: 'Fork', body: 'Copies.' }, after: { title: 'Fork it', body: 'Copies a computer.' } });
  // A decision's `name` is not its title.
  const other = entryHistory([step(1, add(P, D)), step(2, edit(P, D, { ...D, name: 'x', what: 'y' }))]);
  assert.equal(other.get('x').length, 1);
});

test('names come in a fixed order', () => {
  const before = { name: 'F', what: 'W', needs: ['a'], keeps: ['k'], surfaces: { app: '1' }, suggest: { verdict: 'take', why: 'w' }, sets: [] };
  const after = { name: 'G', what: 'W', needs: ['b'], keeps: [], surfaces: { app: '2' }, suggest: { verdict: 'drop', why: 'w' }, verdict: 'take', built: '2026-11-02', replaced_by: 'z' };
  const f = 'engine/features/f.json';
  const history = entryHistory([step(1, add(f, before)), step(2, { ...edit(f, before, after), ...add('engine/features/f.md', 'notes\n') })]);
  assert.deepEqual(history.get('f')[0].what, ['text', 'needs', 'keeps', 'surfaces', 'suggest', 'notes', 'taken', 'replaced', 'built']);
  const dropped = entryHistory([step(1, add(f, before)), step(2, edit(f, before, { ...before, verdict: 'drop' }))]);
  assert.deepEqual(dropped.get('f')[0].what, ['dropped']);
});

test('sets, removes and replaces count only while the entry has no verdict; a take does not report them', () => {
  const open = { ...D, sets: [{ list: 'words', word: 'w' }], removes: ['r'], replaces: ['o'] };
  const taken = { ...D, adds: ['w'], replaces: ['o'], verdict: 'take' };
  const history = entryHistory([
    step(1, add(P, D)),
    step(2, edit(P, D, open)),
    step(3, edit(P, open, { ...open, removes: ['s'] })),
    step(4, edit(P, { ...open, removes: ['s'] }, taken)),
    step(5, edit(P, taken, { ...taken, replaces: ['o', 'p'] })),
  ]);
  assert.deepEqual(names(history, 'x'), [['taken'], ['sets'], ['sets'], ['added']]);
});

test('a step that changes only `adds`, `suggested_in`, the order of keys or white space gives no change', () => {
  const taken = { asks: 'Q', answer: 'A', adds: ['a'], suggested_in: 'x', verdict: 'take' };
  const history = entryHistory([
    step(1, add(P, taken)),
    step(2, edit(P, taken, { ...taken, adds: ['a', 'b'] })),
    step(3, edit(P, { ...taken, adds: ['a', 'b'] }, { ...taken, adds: ['a', 'b'], suggested_in: 'y' })),
    step(4, edit(P, { ...taken, adds: ['a', 'b'], suggested_in: 'y' }, JSON.stringify({ verdict: 'take', suggested_in: 'y', adds: ['a', 'b'], answer: 'A', asks: 'Q' }))),
  ]);
  assert.deepEqual(names(history, 'x'), [['added']]);
  // An entry with no change at all is not in the map; one that is only `adds` after `added` still is.
  assert.deepEqual([...entryHistory([step(1, edit(P, D, { ...D, adds: ['z'] }))]).keys()], []);
});

test('values are compared as parsed JSON, not as text', () => {
  const pretty = json({ asks: 'Q', needs: ['a', 'b'] });
  const compact = '{"needs":["a","b"],"asks":"Q"}';
  assert.deepEqual([...entryHistory([step(1, add(P, pretty)), step(2, edit(P, pretty, compact))]).get('x').map((c) => c.what)], [['added']]);
  // An array is ordered.
  const swapped = entryHistory([step(1, add(P, { asks: 'Q', needs: ['a', 'b'] })), step(2, edit(P, { asks: 'Q', needs: ['a', 'b'] }, { asks: 'Q', needs: ['b', 'a'] }))]);
  assert.deepEqual(names(swapped, 'x')[0], ['needs']);
});

test('a move between parts, or to the root, is `moved`', () => {
  const from = 'engine/decisions/x.json';
  const to = 'record/decisions/x.json';
  const move = { [from]: { before: json(D), after: null }, ...add(to, D) };
  const root = { [to]: { before: json(D), after: null }, ...add('decisions/x.json', D) };
  const history = entryHistory([step(1, add(from, D)), step(2, move), step(3, root)]);
  assert.deepEqual(names(history, 'x'), [['moved'], ['moved'], ['added']]);
  // Notes that move with it are not a change of notes; the move and an edit are one change.
  const withNotes = entryHistory([
    step(1, { ...add(from, D), ...add('engine/decisions/x.md', 'n\n') }),
    step(2, { [from]: { before: json(D), after: null }, 'engine/decisions/x.md': { before: 'n\n', after: null }, ...add(to, { ...D, answer: 'B' }), ...add('record/decisions/x.md', 'n\n') }),
  ]);
  assert.deepEqual(names(withNotes, 'x'), [['moved', 'text'], ['added']]);
  // Features move too, and a feature stays at its kind's path.
  const feature = entryHistory([step(1, add('a/features/f.json', { name: 'F', what: 'W' })), step(2, { 'a/features/f.json': { before: json({ name: 'F', what: 'W' }), after: null }, ...add('b/features/f.json', { name: 'F', what: 'W' }) })]);
  assert.deepEqual(names(feature, 'f'), [['moved'], ['added']]);
});

test('notes: appear, change, go', () => {
  const n = 'engine/decisions/x.md';
  const history = entryHistory([
    step(1, add(P, D)),
    step(2, add(n, 'one\n')),
    step(3, edit(n, 'one\n', 'two\n')),
    step(4, { [n]: { before: 'two\n', after: null } }),
    step(5, add(n, 'two\n')),
  ]);
  assert.deepEqual(names(history, 'x'), [['notes'], ['notes'], ['notes'], ['notes'], ['added']]);
  // Notes with the entry's file are part of `added`; notes beside nothing are nobody's.
  const first = entryHistory([step(1, { ...add(P, D), ...add(n, 'n\n') }), step(2, add('engine/decisions/other.md', 'n\n'))]);
  assert.deepEqual([...first.keys()], ['x']);
  assert.deepEqual(names(first, 'x'), [['added']]);
});

test('a file that does not parse is skipped, and the next step is compared with the last version that parsed', () => {
  const bad = '{ "asks": ';
  const history = entryHistory([
    step(1, add(P, D)),
    step(2, edit(P, D, bad)),
    step(3, edit(P, bad, { ...D, answer: 'Other.' })),
    step(4, edit(P, { ...D, answer: 'Other.' }, '[1]')),
    step(5, edit(P, '[1]', { ...D, answer: 'Other.' })),
  ]);
  assert.deepEqual(names(history, 'x'), [['text'], ['added']]);
  assert.equal(history.get('x')[0].commit, 'c3');
  // Notes changed in a skipped step show at the next one that parses.
  const n = 'engine/decisions/x.md';
  const later = entryHistory([step(1, add(P, D)), step(2, { ...edit(P, D, bad), ...add(n, 'a\n') }), step(3, edit(P, bad, D))]);
  assert.deepEqual(names(later, 'x'), [['notes'], ['added']]);
  // A file that never parses has no history; it starts when it first parses.
  const never = entryHistory([step(1, add(P, bad)), step(2, edit(P, bad, D))]);
  assert.deepEqual(never.get('x'), [{ commit: 'c2', date: '2026-11-02T00:00:00Z', what: ['added'] }]);
  assert.deepEqual([...entryHistory([step(1, add(P, bad))]).keys()], []);
});

test('a deleted file gives no change, and the same id back is `added`', () => {
  const history = entryHistory([
    step(1, add(P, D)),
    step(2, { [P]: { before: json(D), after: null } }),
    step(3, add(P, D)),
    step(4, edit(P, D, { ...D, answer: 'C' })),
  ]);
  assert.deepEqual(names(history, 'x'), [['text'], ['added'], ['added']]);
  assert.deepEqual(history.get('x').map((c) => c.commit), ['c4', 'c3', 'c1']);
});

test('paths that are not an entry file or its notes are ignored', () => {
  const ignored = {
    ...add('words.json', json([{ word: 'a', means: 'b' }])),
    ...add('engine/properties.json', '[]'),
    ...add('features/x.json', D),
    ...add('engine/decisions/.hidden.json', D),
    ...add('engine/decisions/Bad_Id.json', D),
    ...add('engine/decisions/deep/x.json', D),
    ...add('a/b/decisions/x.json', D),
    ...add('engine/features/x.txt', 'x'),
    ...add('.git/decisions/x.json', D),
    ...add('decisions/decisions/x.json', D),
  };
  assert.equal(entryHistory([step(1, ignored)]).size, 0);
  assert.equal(entryHistory([]).size, 0);
  assert.equal(entryHistory(undefined).size, 0);
});

test('the first step is compared with the text it says the file had', () => {
  const history = entryHistory([step(5, edit(P, D, { ...D, answer: 'C' })), step(6, edit(P, { ...D, answer: 'C' }, { ...D, verdict: 'take', answer: 'C' }))]);
  assert.deepEqual(names(history, 'x'), [['taken'], ['text']]);
});

test('entries do not mix, and an id is followed wherever its file is', () => {
  const history = entryHistory([step(1, { ...add('engine/decisions/a.json', D), ...add('engine/decisions/b.json', D) }), step(2, edit('engine/decisions/b.json', D, { ...D, answer: 'B' }))]);
  assert.deepEqual(names(history, 'a'), [['added']]);
  assert.deepEqual(names(history, 'b'), [['text'], ['added']]);
});

test('the pure part imports nothing from node:', () => {
  for (const file of ['index.js', 'entries.js', 'paths.js', 'steps.js']) {
    const source = readFileSync(join(import.meta.dirname, '..', 'history', file), 'utf8');
    assert.equal(/(?:^import\s*|\bfrom\s*)['"]node:/m.test(source), false, file);
  }
});

// readSteps, on small git repositories.

const roots = [];
const repo = () => {
  const root = temp('history-test-');
  roots.push(root);
  git(root, 'init', '-q', '-b', 'main');
  return root;
};
let clock = Date.UTC(2026, 10, 1, 12, 0, 0);
function write(root, files) {
  for (const [name, content] of Object.entries(files)) {
    const path = join(root, 'plan', name);
    if (content === null) rmSync(path, { force: true });
    else {
      mkdirSync(dirname(path), { recursive: true });
      writeFileSync(path, typeof content === 'string' ? content : json(content));
    }
  }
}
/** Writes files under plan/ (null deletes) and commits them; returns the hash. */
function commit(root, files, message = 'step') {
  write(root, files);
  clock += 3600_000;
  const date = new Date(clock).toISOString().replace('.000Z', '+00:00');
  git(root, 'add', '-A');
  gitWith({ GIT_COMMITTER_DATE: date, GIT_AUTHOR_DATE: date }, root, 'commit', '-q', '--allow-empty', '-m', message);
  return git(root, 'rev-parse', 'HEAD').trim();
}
const dateOf = (n) => new Date(Date.UTC(2026, 10, 1, 12 + n, 0, 0)).toISOString().replace('.000Z', 'Z');
const run = (root) => {
  const { steps, shallow } = readSteps(root);
  return { steps, shallow, history: entryHistory(steps) };
};

test.after(() => roots.forEach(cleanup));

test('the steps are the commits that changed an entry, oldest first, with the full hash and the UTC committer date', () => {
  const root = repo();
  const a = commit(root, { 'engine/decisions/x.json': D, 'words.json': [] });
  commit(root, { 'words.json': [{ word: 'w', means: 'm' }] });
  const c = commit(root, { 'engine/decisions/x.json': { ...D, answer: 'B' } });
  const { steps, shallow, history } = run(root);
  assert.equal(shallow, false);
  assert.deepEqual(steps, [
    { commit: a, date: dateOf(1), files: { 'engine/decisions/x.json': { before: null, after: json(D) } } },
    { commit: c, date: dateOf(3), files: { 'engine/decisions/x.json': { before: json(D), after: json({ ...D, answer: 'B' }) } } },
  ]);
  assert.match(a, /^[0-9a-f]{40}$/);
  assert.deepEqual(history.get('x').map((change) => change.commit), [c, a]);
});

test('each name of the list, through git', () => {
  const root = repo();
  const f = 'engine/features/f.json';
  const base = { name: 'F', what: 'W', needs: ['a'], keeps: ['k'], surfaces: { app: '1' }, suggest: { verdict: 'take', why: 'w' } };
  // Each step adds to the one before it, so the list also shows that a step names only what it changed.
  const steps = [
    ['added', {}],
    ['text', { name: 'G' }],
    ['sets', { sets: [{ list: 'words' }] }],
    ['needs', { needs: [] }],
    ['keeps', { keeps: [] }],
    ['surfaces', { surfaces: { cli: '1' } }],
    ['suggest', { suggest: { verdict: 'drop', why: 'w' } }],
    ['taken', { verdict: 'take' }],
    ['started', { started: '2026-11-01' }],
    ['built', { built: '2026-11-02' }],
    ['replaced', { replaced_by: 'z' }],
    ['cleared', { verdict: undefined }],
    ['dropped', { verdict: 'drop' }],
  ];
  let value = base;
  const list = steps.map(([name, change]) => [(value = { ...value, ...change }), name]);
  // `sets` is not reported once the entry has a verdict, so the verdict steps come after it.
  for (const [value] of list) commit(root, { [f]: value });
  commit(root, { 'engine/features/f.md': 'notes\n' });
  const { history } = run(root);
  assert.deepEqual(history.get('f').map((change) => change.what.join('+')).reverse(), [...list.map(([, name]) => name), 'notes']);
  assert.equal(history.get('f').find((change) => change.what.includes('replaced')).by, 'z');
});

test('a move between parts, with its notes', () => {
  const root = repo();
  commit(root, { 'engine/decisions/x.json': D, 'engine/decisions/x.md': 'n\n' });
  mkdirSync(join(root, 'plan', 'record', 'decisions'), { recursive: true });
  git(root, 'mv', 'plan/engine/decisions/x.json', 'plan/record/decisions/x.json');
  git(root, 'mv', 'plan/engine/decisions/x.md', 'plan/record/decisions/x.md');
  commit(root, {});
  const { history } = run(root);
  assert.deepEqual(names(history, 'x'), [['moved'], ['added']]);
});

test('a merge is one step, compared with its first parent', () => {
  const root = repo();
  commit(root, { 'engine/decisions/x.json': D, 'engine/decisions/y.json': D });
  git(root, 'switch', '-q', '-c', 'side');
  commit(root, { 'engine/decisions/x.json': { ...D, answer: 'side' } });
  commit(root, { 'engine/decisions/x.json': { ...D, answer: 'side two' }, 'engine/decisions/y.json': { ...D, answer: 'only on side' } });
  git(root, 'switch', '-q', 'main');
  commit(root, { 'engine/decisions/z.json': D });
  gitWith({}, root, 'merge', '-q', '--no-ff', '-m', 'Merge side', 'side');
  const merge = git(root, 'rev-parse', 'HEAD').trim();
  const { steps, history } = run(root);
  // The side commits are not steps; the merge is, once, against main's tree before it.
  assert.equal(steps.length, 3);
  assert.equal(steps.at(-1).commit, merge);
  assert.deepEqual(Object.keys(steps.at(-1).files).sort(), ['engine/decisions/x.json', 'engine/decisions/y.json']);
  assert.equal(steps.at(-1).files['engine/decisions/x.json'].before, json(D));
  assert.equal(steps.at(-1).files['engine/decisions/x.json'].after, json({ ...D, answer: 'side two' }));
  assert.deepEqual(names(history, 'x'), [['text'], ['added']]);
  assert.deepEqual(history.get('x')[0].text.before, { title: 'Which?', body: 'This.' });
  assert.deepEqual(history.get('x')[0].text.after, { title: 'Which?', body: 'side two' });
  assert.deepEqual(names(history, 'z'), [['added']]);
});

test('a merge that changes nothing of main is not a step', () => {
  const root = repo();
  commit(root, { 'engine/decisions/x.json': D });
  git(root, 'switch', '-q', '-c', 'side');
  commit(root, { 'engine/decisions/x.json': { ...D, answer: 'side' } });
  git(root, 'switch', '-q', 'main');
  commit(root, { 'words.json': [] });
  gitWith({}, root, 'merge', '-q', '--no-ff', '-s', 'ours', '-m', 'Merge side, keep ours', 'side');
  assert.equal(run(root).steps.length, 1);
});

test('a file that does not parse, then fixed; a file deleted and added again', () => {
  const root = repo();
  commit(root, { 'engine/decisions/x.json': D });
  commit(root, { 'engine/decisions/x.json': '{ not json' });
  commit(root, { 'engine/decisions/x.json': { ...D, answer: 'B' } });
  commit(root, { 'engine/decisions/x.json': null });
  commit(root, { 'engine/decisions/x.json': D });
  const { history } = run(root);
  assert.deepEqual(names(history, 'x'), [['added'], ['text'], ['added']]);
});

test('a step that changes only `adds` gives no change; a take reports no `sets`', () => {
  const root = repo();
  const open = { ...D, sets: [{ list: 'words', word: 'w', means: 'm' }] };
  commit(root, { 'engine/decisions/x.json': D });
  commit(root, { 'engine/decisions/x.json': open });
  commit(root, { 'engine/decisions/x.json': { ...D, adds: ['w'], verdict: 'take' } });
  commit(root, { 'engine/decisions/x.json': { ...D, adds: ['w', 'v'], verdict: 'take' } });
  const { history } = run(root);
  assert.deepEqual(names(history, 'x'), [['taken'], ['sets'], ['added']]);
});

test('names git would quote are read whole', () => {
  const root = repo();
  const part = 'é "q"\ttab';
  commit(root, { [`${part}/decisions/x.json`]: D, [`${part}/decisions/x.md`]: 'n\n' });
  const { history, steps } = run(root);
  assert.deepEqual(Object.keys(steps[0].files).sort(), [`${part}/decisions/x.json`, `${part}/decisions/x.md`]);
  assert.deepEqual(names(history, 'x'), [['added']]);
});

test('a shallow clone: what it holds, and `shallow` is true', () => {
  const root = repo();
  commit(root, { 'engine/decisions/x.json': D });
  commit(root, { 'engine/decisions/x.json': { ...D, answer: 'B' } });
  commit(root, { 'engine/decisions/x.json': { ...D, answer: 'C' } });
  const clone = temp('history-clone-');
  roots.push(clone);
  rmSync(clone, { recursive: true });
  git(root, 'clone', '-q', '--depth', '2', `file://${root}`, clone);
  const { steps, shallow, history } = run(clone);
  assert.equal(shallow, true);
  assert.equal(steps.length, 2);
  assert.equal(names(history, 'x').length, 2);
  assert.equal(run(root).shallow, false);
});

test('it never throws on a repository it does not expect', () => {
  const empty = repo();
  assert.deepEqual(readSteps(empty), { steps: [], shallow: false });
  // No plan/ at all, in the old commits and in the new.
  writeFileSync(join(empty, 'a.txt'), 'a');
  git(empty, 'add', '-A');
  git(empty, 'commit', '-q', '-m', 'No plan');
  assert.deepEqual(readSteps(empty), { steps: [], shallow: false });
  // The plan appears later; the old commits had none.
  const first = commit(empty, { 'engine/decisions/x.json': D });
  // A detached HEAD.
  git(empty, 'checkout', '-q', '--detach', first);
  const detached = run(empty);
  assert.deepEqual(names(detached.history, 'x'), [['added']]);
  // Not a repository, and a folder that does not exist.
  const plain = temp('history-plain-');
  roots.push(plain);
  assert.deepEqual(readSteps(plain), { steps: [], shallow: false });
  assert.deepEqual(readSteps(join(plain, 'missing')), { steps: [], shallow: false });
});

test('a symbolic link is no file of the plan', () => {
  const root = repo();
  commit(root, { 'engine/decisions/x.json': D });
  mkdirSync(join(root, 'plan', 'engine', 'decisions'), { recursive: true });
  symlinkSync(join(root, 'plan', 'engine', 'decisions', 'x.json'), join(root, 'plan', 'engine', 'decisions', 'link.json'));
  commit(root, {});
  const { steps, history } = run(root);
  assert.equal(steps.length, 1);
  assert.deepEqual([...history.keys()], ['x']);
});

test('a take or a drop the agents wrote says so with `agents`; one by the owner does not, and a later owner verdict drops it', () => {
  const history = entryHistory([
    step(1, add(P, D)),
    step(2, edit(P, D, { ...D, verdict: 'take', by: 'agents' })),
    step(3, edit(P, { ...D, verdict: 'take', by: 'agents' }, { ...D, verdict: 'drop', by: 'agents' })),
    step(4, edit(P, { ...D, verdict: 'drop', by: 'agents' }, { ...D, verdict: 'take' })),
  ]);
  const [owner, dropped, taken] = history.get('x');
  assert.deepEqual(taken, { commit: 'c2', date: '2026-11-02T00:00:00Z', what: ['taken'], agents: true });
  assert.deepEqual(dropped, { commit: 'c3', date: '2026-11-03T00:00:00Z', what: ['dropped'], agents: true });
  assert.deepEqual(owner, { commit: 'c4', date: '2026-11-04T00:00:00Z', what: ['taken'] });
});
