// Names shared by every command: labels, credential names, defaults.

export const KIND_LABELS = {
  entry: 'workshop:entry',
  decision: 'workshop:decision',
  suggestion: 'workshop:suggestion',
};

export const VERDICTS = ['take', 'refine', 'later', 'drop'];
export const verdictLabel = (verdict) => `verdict:${verdict}`;

export const LABELS = [
  { name: KIND_LABELS.entry, color: '0e8a16', description: 'Workshop: an entry of the feature list' },
  { name: KIND_LABELS.decision, color: '1d76db', description: 'Workshop: a question for the owner' },
  { name: KIND_LABELS.suggestion, color: 'fbca04', description: 'Workshop: a feature suggested by a visitor' },
  { name: verdictLabel('take'), color: '2da44e', description: "The owner's verdict: take" },
  { name: verdictLabel('refine'), color: 'bf8700', description: "The owner's verdict: refine" },
  { name: verdictLabel('later'), color: '8c959f', description: "The owner's verdict: later" },
  { name: verdictLabel('drop'), color: 'cf222e', description: "The owner's verdict: drop" },
];

export const CREDENTIAL_NAMES = [
  'WORKSHOP_APP_ID',
  'WORKSHOP_CLIENT_ID',
  'WORKSHOP_CLIENT_SECRET',
  'WORKSHOP_PRIVATE_KEY',
];

export const KEYCHAIN_SERVICE = 'silo-workshop';
export const DEFAULT_REPO = 'amontlabs/silo';
export const DEFAULT_ORG = 'amontlabs';
export const DEFAULT_SITE = 'https://silo.amontlabs.com';
export const DEFAULT_API = 'https://api.github.com';
export const DEFAULT_WEB = 'https://github.com';
export const ENTRY_MARKER = /<!-- silo-entry: ([A-Za-z0-9._-]+) -->/;
export const entryMarker = (id) => `<!-- silo-entry: ${id} -->`;

/** Error the command reports as one plain line, exit 1. */
export class Failure extends Error {}

/** Wrong use of the command: exit 2. */
export class UsageError extends Error {}

/** GitHub refused for its rate limit; `resetAt` is a Date or null. */
export class RateLimited extends Error {
  constructor(resetAt) {
    super('GitHub rate limit reached');
    this.resetAt = resetAt;
  }
}
