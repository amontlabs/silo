// Checks that read the code of the Workshop itself.
import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { describe, expect, it } from 'vitest';

describe('state classes', () => {
  it('puts the classes of the Workshop\'s states on no other element', () => {
    // A state class is set on .ws alone (App.tsx), and workshop.css styles it there. Another element carrying the same
    // name would take the rules written for the whole Workshop, or give its own rules to the Workshop.
    const dir = join(process.cwd(), 'src/workshop');
    const app = readFileSync(join(dir, 'App.tsx'), 'utf8');
    const begin = app.indexOf('const classes = [');
    const statement = app.slice(begin, app.indexOf('].filter(Boolean)', begin));
    // A name built from a state (`v-${view}`) takes each value of that state's type, read from its useState.
    const valuesOf = (name: string) => {
      let type = app.match(new RegExp(`const \\[${name}, \\w+\\] = useState<([^>]+)>`))?.[1] ?? '';
      if (/^\w+$/.test(type)) type = app.match(new RegExp(`type ${type} = ([^;]+);`))?.[1] ?? '';
      const values = [...type.matchAll(/'([a-z][\w-]*)'/g)].map((match) => match[1]);
      expect(values, `the values of ${name}`).not.toEqual([]);
      return values;
    };
    // Every item of the list is read: a name, a name built from a state, or either one behind a condition.
    const nameOf = (text: string): string[] => {
      const plain = text.match(/^'([a-z][\w-]*)'$/);
      if (plain) return [plain[1]];
      const built = text.match(/^`([a-z][\w-]*)\$\{(\w+)\}([\w-]*)`$/);
      if (built) return valuesOf(built[2]).map((value) => built[1] + value + built[3]);
      throw new Error(`a state class this check cannot read: ${text}`);
    };
    const items = statement.slice('const classes = ['.length).split(',').map((item) => item.trim()).filter(Boolean);
    // A condition's branches are read in turn; a branch may hold a condition of its own.
    const namesIn = (text: string): string[] => {
      const branches = text.match(/^[\w.!&| ]+\?\s*(.+?)\s*:\s*(.+)$/);
      if (branches) return [branches[1], branches[2]].flatMap(namesIn);
      return text === "''" ? [] : nameOf(text);
    };
    const states = new Set(items.flatMap(namesIn));
    states.delete('ws');
    // A name with a hyphen cannot be a variable, so finding it anywhere in the code means an element may carry it.
    expect([...states].filter((name) => !name.includes('-'))).toEqual([]);
    expect(states).toContain('v-map');
    expect(states).toContain('v-all');
    expect(states).toContain('is-open');
    // The same state on purpose: the account slot in the header fades in with the person's details, like the Workshop.
    const shared = "accountSlot.classList.toggle('ws-arr-p'";
    expect(app).toContain(shared);
    const walk = (folder: string): string[] => readdirSync(folder, { withFileTypes: true }).flatMap((entry) =>
      entry.isDirectory() ? walk(join(folder, entry.name)) : [join(folder, entry.name)]);
    const site = join(process.cwd(), 'src');
    const sources = [
      ...walk(site).filter((file) => /\.(tsx?|html)$/.test(file) && !/\.test\.|^workshop\/(test-support|test-setup|fixtures|devFixture)\./.test(relative(site, file))),
      join(process.cwd(), 'index.html'),
      join(process.cwd(), 'workshop/index.html'),
    ];
    const elsewhere = sources.flatMap((file) => {
      // Comments go; a `//` inside a URL stays, since it follows a colon, not a space.
      const code = readFileSync(file, 'utf8').replace(statement, '').replace(shared, '').replace(/\/\*[\s\S]*?\*\/|(^|\s)\/\/.*$/gm, '$1');
      return [...states].filter((name) => new RegExp(`(^|[^\\w-])${name}(?![\\w-])`, 'm').test(code))
        .map((name) => `${file.slice(process.cwd().length + 1)}: ${name}`);
    });
    expect(elsewhere).toEqual([]);
  });
});
