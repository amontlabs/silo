// Which issues are the Workshop's own: an entry issue is one the bot created
// whose body ends with the entry's marker line; a decision issue made by sync
// is a closed one the bot created whose body is exactly the decision's text.
// Writes to the product files re-read the file first and change only the one
// line they mean, so an edit made while the tool runs is kept.
import { appendReference, listItems } from './decisions.js';
import { featuresPath, readText, writeText, decisionsPath } from './files.js';
import { joinFeatures, parseFeatures, setField } from './jsonl.js';

const MARKER_LINE = /^<!-- silo-entry: ([A-Za-z0-9._-]+) -->$/;

/** The entry id named by the body's last non-blank line, or null. */
export function lastLineMarker(body) {
  const lines = (body ?? '').replace(/\r\n/g, '\n').trimEnd().split('\n');
  return lines.at(-1).trim().match(MARKER_LINE)?.[1] ?? null;
}

/**
 * Entry issues by number and by entry id. When two issues carry one marker,
 * the oldest (lowest number) is the entry's and the other is reported.
 */
export function entryIndex(issues, gh) {
  const byNumber = new Map();
  const byMarker = new Map();
  const duplicates = [];
  for (const issue of issues.slice().sort((a, b) => a.number - b.number)) {
    if (!gh.isOwnBot(issue.user)) continue;
    const id = lastLineMarker(issue.body);
    if (!id) continue;
    byNumber.set(issue.number, { issue, id });
    if (byMarker.has(id)) duplicates.push({ id, kept: byMarker.get(id).number, duplicate: issue.number });
    else byMarker.set(id, issue);
  }
  return { byNumber, byMarker, duplicates };
}

/**
 * Closed decision issues the bot made, by exact body. When two carry one
 * body, the oldest is used and the other is reported.
 */
export function decisionIndex(issues, gh, title = (body) => body) {
  const byBody = new Map();
  const duplicates = [];
  for (const issue of issues.slice().sort((a, b) => a.number - b.number)) {
    if (issue.state !== 'closed' || !gh.isOwnBot(issue.user)) continue;
    if (byBody.has(issue.body)) duplicates.push({ decision: title(issue.body), kept: byBody.get(issue.body).number, duplicate: issue.number });
    else byBody.set(issue.body, issue);
  }
  return { byBody, duplicates };
}

/** Markdown links with a relative target reduced to their text; absolute (scheme:) links kept. */
export function withoutRelativeLinks(text) {
  return text.replace(/\[([^\]]+)\]\((?![A-Za-z][A-Za-z0-9+.-]*:)[^)\s]*\)/g, '$1');
}

/**
 * Sets one field on the line whose id is `id`, as the file is now: 'missing'
 * when that line is gone, 'changed' when `expect` rejects the line as it is
 * now, else 'ok'.
 */
export function updateFeature(root, id, key, value, expect = () => true) {
  const path = featuresPath(root);
  const lines = parseFeatures(readText(path));
  const line = lines.find((l) => l.entry?.id === id);
  if (!line) return 'missing';
  if (!expect(line.entry)) return 'changed';
  line.raw = setField(line.raw, key, value);
  writeText(path, joinFeatures(lines));
  return 'ok';
}

/** Appends " (#N)" to the first unreferenced item with this text, as the file is now. False when there is none. */
export function referenceDecision(root, text, number) {
  const path = decisionsPath(root);
  const lines = readText(path).split('\n');
  const item = listItems(lines).find((i) => i.issue == null && i.text === text);
  if (!item) return false;
  writeText(path, appendReference(lines, item, number).join('\n'));
  return true;
}
