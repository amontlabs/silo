// Fixtures: small plans written to a temporary directory. Tests never read the real plan/.

import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { canonicalAreas, canonicalDecisions, canonicalFeature, canonicalOrder, canonicalText } from '../model/canonical.js';
import { main } from '../cli/main.js';

export const temp = (prefix = 'plan-test-') => mkdtempSync(join(process.env.ATELIER_TMP ?? tmpdir(), prefix));
export const cleanup = (dir) => rmSync(dir, { recursive: true, force: true });

/** A valid plan: two areas, three stages (the last is gone), four features, two decisions. */
export function basePlan() {
  return {
    'PRODUCT.md': '# Product\n',
    'areas.json': [
      { id: 'engine', title: 'Engine', line: 'The core.' },
      { id: 'forks', title: 'Forks', line: 'Copies.' },
    ],
    'order.json': {
      stages: [
        { id: 'shell', title: 'Shell', line: 'One computer.', features: ['core', 'cli'] },
        { id: 'loop', title: 'The loop', line: 'Fork.', features: ['fork'] },
        { id: 'gone', title: 'Left behind', line: 'Old things.', features: ['old'] },
      ],
    },
    'decisions.json': [
      { id: 'everywhere', date: '2026-11-16', text: 'Holds for the whole product.' },
      { id: 'forks-rule', date: '2026-11-10', text: 'About forks.', about: ['forks'] },
    ],
    'features/core.json': { name: 'Core', what: 'The engine.', area: 'engine', needs: [] },
    'features/cli.json': { name: 'CLI', what: 'One command.', area: 'engine', needs: ['core'], surfaces: { cli: 'silo' } },
    'features/fork.json': { name: 'Fork', what: 'Copy a computer.', area: 'forks', needs: ['core'], suggest: { verdict: 'take', why: 'The heart.' } },
    'features/fork.md': '## Notes\n\n- A note.\n',
    'features/old.json': { name: 'Old', what: 'Left behind.', needs: [] },
  };
}

/** A fixture value as text: strings as they are, the rest in canonical form. */
function text(name, value) {
  if (typeof value === 'string') return value;
  const form = name === 'areas.json' ? canonicalAreas : name === 'order.json' ? canonicalOrder : name === 'decisions.json' ? canonicalDecisions : name.startsWith('features/') ? canonicalFeature : (v) => v;
  return canonicalText(form(value));
}

/**
 * Writes a plan into `<root>/plan`. Values that are strings are written as they are; others in canonical form.
 * @param {Record<string, unknown>} files @param {string} [root]
 */
export function writePlan(files, root = temp()) {
  const dir = join(root, 'plan');
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(join(dir, 'features'), { recursive: true });
  for (const [name, value] of Object.entries(files)) {
    if (value === undefined) continue;
    mkdirSync(join(dir, name, '..'), { recursive: true });
    writeFileSync(join(dir, name), text(name, value));
  }
  return root;
}

/** As a Map of texts, the way the reader gives files to the check. */
export function filesOf(files) {
  return new Map(Object.entries(files).filter(([, v]) => v !== undefined).map(([name, value]) => [name, text(name, value)]));
}

export function git(root, ...args) {
  return gitWith({}, root, ...args);
}

export function gitWith(env, root, ...args) {
  return execFileSync('git', ['-C', root, ...args], {
    env: { ...process.env, ...env, GIT_AUTHOR_NAME: 'Test', GIT_AUTHOR_EMAIL: 'test@example.invalid', GIT_COMMITTER_NAME: 'Test', GIT_COMMITTER_EMAIL: 'test@example.invalid', GIT_CONFIG_GLOBAL: '/dev/null', GIT_CONFIG_SYSTEM: '/dev/null' },
    stdio: ['ignore', 'pipe', 'pipe'],
  }).toString();
}

export function initRepo(root) {
  git(root, 'init', '-q', '-b', 'main');
  git(root, 'add', '-A');
  git(root, 'commit', '-q', '-m', 'Plan');
}

/** Runs the command in `cwd`; returns { code, out, err }. */
export async function run(cwd, argv, context = {}) {
  const out = [];
  const err = [];
  const code = await main(argv, { cwd, now: new Date('2026-11-20T12:00:00Z'), out: (t) => out.push(t), err: (t) => err.push(t), ...context });
  return { code, out: out.join('\n'), err: err.join('\n') };
}

/** A live state that is available, for the parts the offline one cannot show. */
export function fakeLive(state) {
  return { async read() { return { available: true, entries: {}, suggestions: [], ...state }; } };
}
