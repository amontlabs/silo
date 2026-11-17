// The command line: a command, its positional arguments and its options.

/** Wrong use of the command: exit 2. */
export class UsageError extends Error {}

/** Refused, or errors found: exit 1. */
export class Refused extends Error {}

/**
 * @param {string[]} argv
 * @param {Record<string, 'value' | 'flag'>} options the options the command takes, besides --json
 * @returns {{ positional: string[], values: Record<string, string>, flags: Set<string> }}
 */
export function parseArgs(argv, options) {
  const all = { json: 'flag', ...options };
  const positional = [];
  /** @type {Record<string, string>} */
  const values = {};
  const flags = new Set();
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (!arg.startsWith('--')) {
      positional.push(arg);
      continue;
    }
    const eq = arg.indexOf('=');
    const name = arg.slice(2, eq === -1 ? undefined : eq);
    const kind = all[name];
    if (!kind) throw new UsageError(`unknown option --${name}`);
    if (kind === 'flag') {
      if (eq !== -1) throw new UsageError(`--${name} takes no value`);
      flags.add(name);
    } else {
      const value = eq !== -1 ? arg.slice(eq + 1) : argv[++i];
      if (value === undefined) throw new UsageError(`--${name} needs a value`);
      if (Object.hasOwn(values, name)) throw new UsageError(`--${name} given twice`);
      values[name] = value;
    }
  }
  return { positional, values, flags };
}
