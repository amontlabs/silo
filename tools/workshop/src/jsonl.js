// features.jsonl edited in place: a field's value is replaced, or a new field
// appended before the closing brace, without re-serializing the line, so
// every other byte (field order, spacing, escapes) stays as it was.
import { isDeepStrictEqual } from 'node:util';

function skipSpace(text, i) {
  while (i < text.length && /\s/.test(text[i])) i += 1;
  return i;
}

function endOfString(text, i) {
  i += 1;
  while (i < text.length) {
    if (text[i] === '\\') i += 2;
    else if (text[i] === '"') return i + 1;
    else i += 1;
  }
  throw new Error('unterminated string');
}

function endOfValue(text, i) {
  if (text[i] === '"') return endOfString(text, i);
  if (text[i] === '{' || text[i] === '[') {
    let depth = 0;
    while (i < text.length) {
      const c = text[i];
      if (c === '"') {
        i = endOfString(text, i);
        continue;
      }
      if (c === '{' || c === '[') depth += 1;
      if (c === '}' || c === ']') {
        depth -= 1;
        if (depth === 0) return i + 1;
      }
      i += 1;
    }
    throw new Error('unterminated value');
  }
  while (i < text.length && !/[,}\]\s]/.test(text[i])) i += 1;
  return i;
}

/** Top-level fields of a one-line JSON object: [{ key, valueStart, valueEnd }] and the closing brace index. */
function scan(text) {
  let i = skipSpace(text, 0);
  if (text[i] !== '{') throw new Error('not a JSON object');
  i += 1;
  const fields = [];
  for (;;) {
    i = skipSpace(text, i);
    if (text[i] === '}') return { fields, close: i };
    const keyEnd = endOfString(text, i);
    const key = JSON.parse(text.slice(i, keyEnd));
    i = skipSpace(text, keyEnd);
    if (text[i] !== ':') throw new Error('expected ":"');
    const valueStart = skipSpace(text, i + 1);
    const valueEnd = endOfValue(text, valueStart);
    fields.push({ key, valueStart, valueEnd });
    i = skipSpace(text, valueEnd);
    if (text[i] === ',') i += 1;
  }
}

/** The line with `key` set to `value`; the result is checked to parse to exactly the intended object. */
export function setField(line, key, value) {
  const before = JSON.parse(line);
  const { fields, close } = scan(line);
  const encoded = JSON.stringify(value);
  const field = fields.find((f) => f.key === key);
  let result;
  if (field) {
    result = line.slice(0, field.valueStart) + encoded + line.slice(field.valueEnd);
  } else {
    const colon = /":\s/.test(line) ? ': ' : ':';
    const comma = /,\s"/.test(line) ? ', ' : ',';
    const separator = fields.length ? comma : '';
    let end = close;
    while (end > 0 && /\s/.test(line[end - 1])) end -= 1;
    result = `${line.slice(0, end)}${separator}${JSON.stringify(key)}${colon}${encoded}${line.slice(end)}`;
  }
  if (!isDeepStrictEqual(JSON.parse(result), { ...before, [key]: value })) {
    throw new Error(`could not set ${key} on a line of features.jsonl`);
  }
  return result;
}

/** The file as lines with their parsed entries; blank lines kept as they are. */
export function parseFeatures(text) {
  const lines = text.split('\n');
  return lines.map((raw, index) => {
    if (!raw.trim()) return { raw, entry: null };
    try {
      return { raw, entry: JSON.parse(raw) };
    } catch {
      throw new Error(`features.jsonl line ${index + 1} is not valid JSON`);
    }
  });
}

export function joinFeatures(lines) {
  return lines.map((line) => line.raw).join('\n');
}
