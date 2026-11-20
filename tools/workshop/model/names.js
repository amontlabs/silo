// The names of the Workshop on GitHub: labels, ids, defaults.
// Copied from the plan's names so that this folder stands alone: issues already on GitHub keep
// these names, whatever the plan's files become.

/** A feature id, as an entry marker names it. */
export const ID_PATTERN = /^[a-z0-9][a-z0-9-]{0,62}$/;

export const ENTRY_LABEL = 'workshop:entry';
export const SUGGESTION_LABEL = 'workshop:suggestion';

/** Every label the Workshop's issues carry. */
export const LABELS = Object.freeze([
  { name: ENTRY_LABEL, color: '0e8a16', description: 'Workshop: an entry of the plan' },
  { name: SUGGESTION_LABEL, color: 'fbca04', description: 'Workshop: a feature suggested by a visitor' },
]);

/**
 * The labels something puts on an issue: the site labels each new suggestion, and opens an entry's thread
 * with the entry label. GitHub's documentation does not say that it creates a label an issue names, so
 * `workshop doctor` checks that these exist.
 */
export const SET_LABELS = LABELS;

export const DEFAULT_REPO = 'amontlabs/silo';
export const DEFAULT_SITE = 'https://silo.amontlabs.com';
export const DEFAULT_WEB = 'https://github.com';

/** The public https address of a repository on GitHub: the page's links to its commits and issues, and the build's fetch of its history, start from it. */
export const repoAddress = (repo = DEFAULT_REPO) => `${DEFAULT_WEB}/${repo}`;
