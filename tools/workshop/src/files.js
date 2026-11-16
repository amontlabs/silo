// The product files the tool reads and writes, under the repository root.
import { readFileSync, renameSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

export const featuresPath = (root) => join(root, 'docs', 'product', 'features.jsonl');
export const decisionsPath = (root) => join(root, 'docs', 'product', 'DECISIONS.md');

export function readText(path) {
  return readFileSync(path, 'utf8');
}

/** Replaces a file in one step, so an interrupted run never leaves half a file. */
export function writeText(path, text, mode) {
  const temp = `${path}.workshop-${process.pid}.tmp`;
  writeFileSync(temp, text, mode === undefined ? undefined : { mode });
  renameSync(temp, path);
}
