// The feature list (docs/product/features.jsonl and stages.json): read live
// from the checkout, one JSON object per line, in list order. The owner's
// verdicts live in the store; this file only reads. Every function but
// readFeatures is pure, so the page, the CLI and the tests share them.
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { DOC_FOLDER } from './docs.js';

export const FEATURES_FILE = 'features.jsonl';
export const STAGES_FILE = 'stages.json';
export const FEATURE_VERDICTS = Object.freeze(['take', 'refine', 'later', 'drop']);
export const OLD_STATES = Object.freeze(['driven', 'partial', 'built', 'planned', 'idea', 'dropped']);
export const REUSE = Object.freeze(['reuse', 'rework', 'rewrite', 'unknown']);
// A feature id, as features.jsonl writes them and the API path takes them.
export const FEATURE_ID = /^[a-z0-9][a-z0-9-]{0,79}$/;
// The stage the starter ends with: the page draws its cut line above it.
export const STARTER_LAST_STAGE = 'living';
const MAX_FIELD = 2000;
const APP_HINT = /^(window|menubar|progress|tree|(side|tab|button):[^\n]{1,60})$/;

const text = (value) => (typeof value === 'string' && value.trim() ? value.trim().slice(0, MAX_FIELD) : null);
const oneOf = (value, list) => (list.includes(value) ? value : null);

// One line of features.jsonl as the dashboard uses it, or null when the line
// cannot be used (not an object, or no id, name or stage). Unknown fields are
// dropped; a field with an unknown value falls back to "nothing known".
export function cleanFeature(value) {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) return null;
  const id = typeof value.id === 'string' && FEATURE_ID.test(value.id) ? value.id : null;
  const name = text(value.name);
  const stage = typeof value.stage === 'string' && FEATURE_ID.test(value.stage) ? value.stage : null;
  if (!id || !name || !stage) return null;
  const app = typeof value.app === 'string' && APP_HINT.test(value.app.trim()) ? value.app.trim() : null;
  return {
    n: Number.isSafeInteger(value.n) ? value.n : null,
    id,
    stage,
    name,
    what: text(value.what) ?? '',
    old: oneOf(value.old, OLD_STATES),
    old_lines: Number.isSafeInteger(value.old_lines) && value.old_lines > 0 ? value.old_lines : 0,
    reuse: oneOf(value.reuse, REUSE) ?? 'unknown',
    suggest: oneOf(value.suggest, FEATURE_VERDICTS),
    why: text(value.why) ?? '',
    app,
    cli: text(value.cli),
    verdict: oneOf(value.verdict, FEATURE_VERDICTS),
  };
}

// { features (in list order: by n, then by line), skipped (lines not used) }.
// A blank line is not counted; a second line with an id already seen is.
export function parseFeatures(source) {
  const features = [];
  const seen = new Set();
  let skipped = 0;
  String(source)
    .split(/\r?\n/)
    .forEach((line, index) => {
      if (!line.trim()) return;
      let feature = null;
      try {
        feature = cleanFeature(JSON.parse(line));
      } catch {
        feature = null;
      }
      if (!feature || seen.has(feature.id)) {
        skipped += 1;
        return;
      }
      seen.add(feature.id);
      features.push({ ...feature, line: index });
    });
  features.sort((a, b) => (a.n ?? Infinity) - (b.n ?? Infinity) || a.line - b.line);
  return { features: features.map(({ line, ...feature }, index) => ({ ...feature, n: index + 1 })), skipped };
}

// [{ id, title, line }] in order. A malformed file gives []; a stage a
// feature names that the file lacks is added after the named ones, titled by
// its id; a named stage with no feature is left out.
export function parseStages(source, features) {
  let list = [];
  try {
    const value = JSON.parse(source ?? '[]');
    if (Array.isArray(value)) list = value;
  } catch {
    list = [];
  }
  const used = new Set(features.map((feature) => feature.stage));
  const stages = [];
  const named = new Set();
  for (const entry of list) {
    if (!entry || typeof entry !== 'object' || typeof entry.id !== 'string' || named.has(entry.id) || !used.has(entry.id)) continue;
    named.add(entry.id);
    stages.push({ id: entry.id, title: text(entry.title) ?? entry.id, line: text(entry.line) ?? '' });
  }
  for (const feature of features) {
    if (named.has(feature.stage)) continue;
    named.add(feature.stage);
    stages.push({ id: feature.stage, title: feature.stage, line: '' });
  }
  return stages;
}

function readFolderFile(repoRoot, name) {
  try {
    const folder = fs.realpathSync(path.join(repoRoot, DOC_FOLDER));
    const file = fs.realpathSync(path.join(folder, name));
    if (path.dirname(file) !== folder) return null;
    return fs.readFileSync(file, 'utf8');
  } catch {
    return null;
  }
}

// The feature list of a checkout, or null when features.jsonl is missing.
export function readFeatures(repoRoot) {
  const source = readFolderFile(repoRoot, FEATURES_FILE);
  if (source === null) return null;
  const { features, skipped } = parseFeatures(source);
  return { features, stages: parseStages(readFolderFile(repoRoot, STAGES_FILE), features), skipped };
}

// What the owner judges an entry by: a digest of what its card shows (name,
// what, old, suggest, why, in that order), SHA-256, first 16 hex characters.
// The page sends it back with a verdict; the server refuses the verdict when
// the entry's text changed since.
export function entryDigest(feature) {
  const shown = [feature.name, feature.what, feature.old, feature.suggest, feature.why];
  return createHash('sha256').update(JSON.stringify(shown)).digest('hex').slice(0, 16);
}

// feature id -> { verdict, note, source: 'store'|'file', shown, at }.
// `rows` is the store's newest verdict row per feature. The store's row wins
// over the file, also when it clears the verdict (verdict null). `shown` is
// that row's id (0 without one): what the page sends back.
export function currentVerdicts(features, rows) {
  const byFeature = new Map(rows.map((row) => [row.feature_id, row]));
  const out = new Map();
  for (const feature of features) {
    const row = byFeature.get(feature.id);
    if (row) out.set(feature.id, { verdict: row.verdict, note: row.note || '', source: 'store', shown: row.id, at: row.created_at });
    else out.set(feature.id, { verdict: feature.verdict, note: '', source: 'file', shown: 0, at: null });
  }
  return out;
}

const emptyCounts = () => ({ take: 0, refine: 0, later: 0, drop: 0, undecided: 0, total: 0 });

// { all: counts, stages: Map stage id -> counts }, counts by current verdict.
export function stackTotals(features, verdicts) {
  const all = emptyCounts();
  const stages = new Map();
  for (const feature of features) {
    if (!stages.has(feature.stage)) stages.set(feature.stage, emptyCounts());
    const key = verdicts.get(feature.id)?.verdict ?? 'undecided';
    for (const counts of [all, stages.get(feature.stage)]) {
      counts[key] += 1;
      counts.total += 1;
    }
  }
  return { all, stages };
}

export const isTaken = (verdict) => verdict === 'take' || verdict === 'refine';

// The first undecided feature at or after list position `from`, else the
// first undecided one, else null.
export function nextUndecided(features, verdicts, from = 0) {
  const undecided = (feature) => !verdicts.get(feature.id)?.verdict;
  return features.slice(from).find(undecided) ?? features.find(undecided) ?? null;
}

const PART_ORDER = { window: 0, tree: 1, menubar: 2, progress: 3, side: 4, button: 5, tab: 6 };

/**
 * What "The product so far" draws. parts: one per distinct app hint
 * ({ key, kind, label, features, taken, state }), cli: one per feature with
 * commands ({ feature, text, taken, state }). taken: a feature it belongs to
 * is taken (take or refine). state is 'current' when it belongs to `current`
 * (a feature id), else 'taken' when taken, else null (not drawn). empty:
 * nothing taken draws anything.
 */
export function schematicParts(features, verdicts, current = null) {
  const parts = new Map();
  const cli = [];
  let empty = true;
  for (const feature of features) {
    const taken = isTaken(verdicts.get(feature.id)?.verdict);
    if (taken && (feature.app || feature.cli)) empty = false;
    const state = feature.id === current ? 'current' : taken ? 'taken' : null;
    if (feature.app) {
      const [kind, ...rest] = feature.app.split(':');
      if (!parts.has(feature.app)) parts.set(feature.app, { key: feature.app, kind, label: rest.join(':') || null, features: [], taken: false, state: null });
      const part = parts.get(feature.app);
      part.features.push(feature.id);
      part.taken ||= taken;
      if (state === 'current' || (state === 'taken' && part.state === null)) part.state = state;
    }
    if (feature.cli) cli.push({ feature: feature.id, text: feature.cli, taken, state });
  }
  const list = [...parts.values()].sort((a, b) => PART_ORDER[a.kind] - PART_ORDER[b.kind]);
  return { parts: list, cli, empty };
}

// Where an app hint shows, in words.
export function appWords(app) {
  if (!app) return null;
  const [kind, ...rest] = app.split(':');
  const label = rest.join(':');
  switch (kind) {
    case 'window':
      return 'the app window, with its list of computers';
    case 'menubar':
      return 'the menu bar item';
    case 'progress':
      return 'a progress bar at the bottom of the window';
    case 'tree':
      return 'each computer listed under the one it came from';
    case 'side':
      return `a ${label} item in the sidebar`;
    case 'tab':
      return `a ${label} tab on a computer's page`;
    case 'button':
      return `a ${label} button on a computer's page`;
    default:
      return null;
  }
}
