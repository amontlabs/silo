// A small HTML template tag: every interpolated value is escaped unless it was
// built by this tag or wrapped with raw().
class Raw {
  constructor(text) {
    this.text = text;
  }
  toString() {
    return this.text;
  }
}

export const raw = (text) => new Raw(String(text));

export function esc(value) {
  return String(value).replace(/[&<>"']/g, (char) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[char]);
}

function part(value) {
  if (value instanceof Raw) return value.text;
  if (Array.isArray(value)) return value.map(part).join('');
  if (value === null || value === undefined || value === false) return '';
  return esc(value);
}

export function h(strings, ...values) {
  let out = strings[0];
  values.forEach((value, index) => {
    out += part(value) + strings[index + 1];
  });
  return new Raw(out);
}

export const join = (list) => raw(list.map(part).join(''));
