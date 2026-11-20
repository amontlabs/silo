// The names of the Workshop on GitHub: labels, verdicts as labels name them, ids, defaults.
// Copied from the plan's names so that this folder stands alone: issues already on GitHub keep
// these names, whatever the plan's files become.

/** A feature id, as an entry marker names it. */
export const ID_PATTERN = /^[a-z0-9][a-z0-9-]{0,62}$/;

/** A day, `YYYY-MM-DD`, UTC. */
export const DATE_PATTERN = /^\d{4}-\d{2}-\d{2}$/;

/** @typedef {'take' | 'later' | 'drop'} Verdict */
export const VERDICTS = Object.freeze(['take', 'later', 'drop']);

export const ENTRY_LABEL = 'workshop:entry';
export const SUGGESTION_LABEL = 'workshop:suggestion';
export const BUILDING_LABEL = 'workshop:building';
export const verdictLabel = (verdict) => `verdict:${verdict}`;

/** @param {string} label @returns {string | null} the verdict a label names, or null: only the three verdict labels, in any capitalisation. */
export function labelVerdict(label) {
  const match = /^verdict:(.+)$/.exec(String(label ?? '').toLowerCase());
  return match && VERDICTS.includes(match[1]) ? match[1] : null;
}

/** Every label the Workshop's issues carry, read on the issues already on GitHub. */
export const LABELS = Object.freeze([
  { name: ENTRY_LABEL, color: '0e8a16', description: 'Workshop: an entry of the plan' },
  { name: SUGGESTION_LABEL, color: 'fbca04', description: 'Workshop: a feature suggested by a visitor' },
  { name: verdictLabel('take'), color: '2da44e', description: "The owner's verdict: take" },
  { name: verdictLabel('later'), color: '8c959f', description: "The owner's verdict: later" },
  { name: verdictLabel('drop'), color: 'cf222e', description: "The owner's verdict: drop" },
  { name: BUILDING_LABEL, color: '8250df', description: 'Being built now' },
]);

/** The labels something still puts on an issue: the site labels each new suggestion. `workshop doctor` checks these. */
export const SET_LABELS = Object.freeze(LABELS.filter((label) => label.name === SUGGESTION_LABEL));

export const DEFAULT_REPO = 'amontlabs/silo';
export const DEFAULT_SITE = 'https://silo.amontlabs.com';
export const DEFAULT_WEB = 'https://github.com';
/** Where a file of the repository is read at a commit, as plain text, from any origin. */
export const DEFAULT_RAW = 'https://raw.githubusercontent.com';

/**
 * The fields a history comment can name: the keys a feature file had when the comments were
 * written, plus `notes` for its `.md`. Fixed, so that the comments already on GitHub still read.
 */
export const CHANGE_FIELDS = Object.freeze(['name', 'what', 'area', 'needs', 'surfaces', 'suggest', 'suggested_in', 'verdict', 'built', 'notes']);

/** @param {string} text @returns {boolean} a real calendar day in `YYYY-MM-DD`. */
export function isDate(text) {
  if (typeof text !== 'string' || !DATE_PATTERN.test(text)) return false;
  const [y, m, d] = text.split('-').map(Number);
  const date = new Date(Date.UTC(y, m - 1, d));
  return date.getUTCFullYear() === y && date.getUTCMonth() === m - 1 && date.getUTCDate() === d;
}
