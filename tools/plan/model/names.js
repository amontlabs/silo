// Names shared by every part of the plan: patterns, verdicts, labels, the shapes' key orders.

/** A feature, area, stage or decision id; a feature's id is also its file name. */
export const ID_PATTERN = /^[a-z0-9][a-z0-9-]{0,62}$/;

/** A day, `YYYY-MM-DD`, UTC. */
export const DATE_PATTERN = /^\d{4}-\d{2}-\d{2}$/;

export const VERDICTS = Object.freeze(['take', 'later', 'drop']);

/** The last stage: old things proposed to be left behind. Only its features may have no area. */
export const GONE_STAGE = 'gone';

/** The first four stages of `order.json` are the starter; this is its last. */
export const STARTER_STAGES = 4;

export const ENTRY_LABEL = 'workshop:entry';
export const SUGGESTION_LABEL = 'workshop:suggestion';
export const BUILDING_LABEL = 'workshop:building';
export const verdictLabel = (verdict) => `verdict:${verdict}`;

/** @param {string} label @returns {string | null} the verdict a label names, or null. */
export function labelVerdict(label) {
  const match = /^verdict:(.+)$/.exec(label);
  return match && VERDICTS.includes(match[1]) ? match[1] : null;
}

export const LABELS = Object.freeze([
  { name: ENTRY_LABEL, color: '0e8a16', description: 'Workshop: an entry of the plan' },
  { name: SUGGESTION_LABEL, color: 'fbca04', description: 'Workshop: a feature suggested by a visitor' },
  { name: verdictLabel('take'), color: '2da44e', description: "The owner's verdict: take" },
  { name: verdictLabel('later'), color: '8c959f', description: "The owner's verdict: later" },
  { name: verdictLabel('drop'), color: 'cf222e', description: "The owner's verdict: drop" },
  { name: BUILDING_LABEL, color: '8250df', description: 'Being built now' },
]);

export const DEFAULT_REPO = 'amontlabs/silo';
export const DEFAULT_SITE = 'https://silo.amontlabs.com';
export const DEFAULT_WEB = 'https://github.com';
/** Where a file of the repository is read at a commit, as plain text, from any origin. */
export const DEFAULT_RAW = 'https://raw.githubusercontent.com';

/** Key orders of the canonical form, per shape. */
export const FEATURE_KEYS = Object.freeze(['name', 'what', 'area', 'needs', 'surfaces', 'suggest', 'suggested_in', 'verdict', 'built']);
export const SURFACES_KEYS = Object.freeze(['app', 'cli']);
export const SUGGEST_KEYS = Object.freeze(['verdict', 'why']);
export const AREA_KEYS = Object.freeze(['id', 'title', 'line']);
export const ORDER_KEYS = Object.freeze(['stages']);
export const STAGE_KEYS = Object.freeze(['id', 'title', 'line', 'features']);
export const DECISION_KEYS = Object.freeze(['id', 'date', 'text', 'about']);

/** The fields a history comment can name: a feature's keys, plus `notes` for its `.md`. */
export const CHANGE_FIELDS = Object.freeze([...FEATURE_KEYS, 'notes']);

/** The files of `plan/`, relative to it. */
export const PLAN_FILES = Object.freeze({
  product: 'PRODUCT.md',
  areas: 'areas.json',
  order: 'order.json',
  decisions: 'decisions.json',
  features: 'features',
});

/** @param {string} text @returns {boolean} a real calendar day in `YYYY-MM-DD`. */
export function isDate(text) {
  if (typeof text !== 'string' || !DATE_PATTERN.test(text)) return false;
  const [y, m, d] = text.split('-').map(Number);
  const date = new Date(Date.UTC(y, m - 1, d));
  return date.getUTCFullYear() === y && date.getUTCMonth() === m - 1 && date.getUTCDate() === d;
}

/** @param {Date} now @returns {string} that day, UTC. */
export function today(now) {
  return now.toISOString().slice(0, 10);
}
