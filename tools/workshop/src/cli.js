// The `workshop` command: argument parsing, output, exit codes (0 done,
// 1 failed, 2 wrong use). Everything that touches the world comes in through
// `deps`, so tests run it against a fake GitHub, a temp HOME and a fake
// Keychain.
import { spawn } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { ask, built, comment, doctor } from './commands.js';
import { appCredentials } from './credentials.js';
import { GitHub } from './github.js';
import { macKeychain } from './keychain.js';
import { DEFAULT_API, DEFAULT_ORG, DEFAULT_REPO, DEFAULT_SITE, DEFAULT_WEB, Failure, RateLimited, UsageError } from './names.js';
import { pull } from './pull.js';
import { setup, store } from './setup.js';
import { sync } from './sync.js';

const USAGE = `usage: workshop <command> [--json]

  setup [--org amontlabs] [--site https://silo.amontlabs.com] [--replace]
  store [--replace]
  doctor
  sync [--dry-run]
  pull [--write]
  ask --title T --body TEXT|@file|- [--entry ID]
  comment N --text TEXT|@file|-
  built N

See tools/workshop/README.md.`;

const COMMANDS = {
  setup: { flags: ['replace'], options: ['org', 'site'], positional: 0 },
  store: { flags: ['replace'], options: [], positional: 0 },
  doctor: { flags: [], options: [], positional: 0 },
  sync: { flags: ['dry-run'], options: [], positional: 0 },
  pull: { flags: ['write'], options: [], positional: 0 },
  ask: { flags: [], options: ['title', 'body', 'entry'], positional: 0 },
  comment: { flags: [], options: ['text'], positional: 1 },
  built: { flags: [], options: [], positional: 1 },
};

export function defaultDeps() {
  const env = process.env;
  return {
    env,
    home: homedir(),
    root: resolve(dirname(fileURLToPath(import.meta.url)), '..', '..', '..'),
    keychain: macKeychain,
    fetch: globalThis.fetch,
    api: env.WORKSHOP_GITHUB_API || DEFAULT_API,
    web: DEFAULT_WEB,
    repo: env.WORKSHOP_REPO || DEFAULT_REPO,
    stdout: process.stdout,
    stderr: process.stderr,
    readStdin: () => readFileSync(0, 'utf8'),
    openBrowser: (url) => spawn('open', [url], { stdio: 'ignore', detached: true }).unref(),
    writeDelayMs: 1000,
    now: () => new Date(),
  };
}

export function parse(argv) {
  const [command, ...rest] = argv;
  if (!command || command === 'help' || command === '--help' || command === '-h') return { help: true };
  const spec = COMMANDS[command];
  if (!spec) throw new UsageError(`unknown command ${command}`);
  const args = { command, flags: {}, options: {}, positional: [], json: false };
  for (let i = 0; i < rest.length; i += 1) {
    const arg = rest[i];
    if (arg === '--json') args.json = true;
    else if (arg === '--help' || arg === '-h') return { help: true };
    else if (arg.startsWith('--')) {
      const [name, inline] = arg.slice(2).split(/=(.*)/s);
      if (spec.flags.includes(name) && inline === undefined) args.flags[name] = true;
      else if (spec.options.includes(name)) {
        const value = inline ?? rest[++i];
        if (value === undefined) throw new UsageError(`--${name} needs a value`);
        args.options[name] = value;
      } else throw new UsageError(`${command} has no option --${name}`);
    } else args.positional.push(arg);
  }
  if (args.positional.length !== spec.positional) {
    throw new UsageError(spec.positional ? `${command} takes one issue number` : `${command} takes no arguments`);
  }
  if (spec.positional && !/^[1-9]\d*$/.test(args.positional[0])) throw new UsageError(`${args.positional[0]} is not an issue number`);
  return args;
}

function readText(value, name, deps) {
  if (value === undefined) throw new UsageError(`--${name} is required`);
  if (value === '-') return deps.readStdin();
  if (value.startsWith('@')) {
    try {
      return readFileSync(resolve(value.slice(1)), 'utf8');
    } catch {
      throw new UsageError(`cannot read ${value.slice(1)}`);
    }
  }
  return value;
}

async function connected(deps) {
  const gh = new GitHub({ api: deps.api, repo: deps.repo, fetch: deps.fetch, writeDelayMs: deps.writeDelayMs });
  await gh.authenticate(appCredentials(deps));
  return gh;
}

const when = (date) => (date === 'unknown' ? 'later' : `after ${date}`);

export async function main(argv, overrides = {}) {
  const deps = { ...defaultDeps(), ...overrides };
  const out = (text) => deps.stdout.write(`${text}\n`);
  const log = (text) => deps.stderr.write(`${text}\n`);
  let args;
  try {
    args = parse(argv);
    if (args.help) {
      out(USAGE);
      return 0;
    }
    const emit = (json, text) => out(args.json ? JSON.stringify(json, null, 2) : text);
    switch (args.command) {
      case 'setup': {
        const result = await setup({
          org: args.options.org ?? DEFAULT_ORG,
          site: args.options.site ?? DEFAULT_SITE,
          replace: Boolean(args.flags.replace),
          deps: { ...deps, log },
        });
        emit(result, `Created the App ${result.app}. Credentials are in the Keychain; Vercel's variables are in ${result.envFile}.\nInstall it on ${deps.repo}: ${result.installUrl}`);
        return 0;
      }
      case 'store': {
        const result = store({ replace: Boolean(args.flags.replace), deps });
        emit(result, `Stored the App credentials from ${result.from} in the Keychain.`);
        return 0;
      }
      case 'doctor': {
        const gh = new GitHub({ api: deps.api, repo: deps.repo, fetch: deps.fetch, writeDelayMs: 0 });
        const report = await doctor({ deps, gh, credentials: () => appCredentials(deps) });
        const lines = [
          ...Object.entries(report.credentials).map(([name, source]) => `${name}: ${source}`),
          `App: ${report.app ?? 'unknown'}`,
          `Installed on ${deps.repo}: ${report.installed ? 'yes' : 'no'}`,
          `Permissions: ${report.permissions ? Object.entries(report.permissions).map(([k, v]) => `${k} ${v}`).join(', ') || 'none' : 'unknown'}`,
          ...(report.labels ? Object.entries(report.labels).map(([name, ok]) => `Label ${name}: ${ok ? 'yes' : 'missing'}`) : ['Labels: unknown']),
          ...report.problems.map((problem) => `Problem: ${problem}`),
        ];
        emit({ ok: report.problems.length === 0, ...report }, lines.join('\n'));
        return report.problems.length ? 1 : 0;
      }
      case 'sync': {
        const gh = await connected(deps);
        const dryRun = Boolean(args.flags['dry-run']);
        const result = await sync({ gh, root: deps.root, dryRun, log: args.json ? () => {} : out });
        const verb = dryRun ? 'would create' : 'created';
        const summary = `${verb} ${result.created}, ${dryRun ? 'would update' : 'updated'} ${result.updated}, unchanged ${result.unchanged}${result.labels ? `, labels ${result.labels}` : ''}`;
        if (!args.json) {
          for (const d of result.duplicates) out(`duplicate: #${d.duplicate} also carries ${d.id}'s marker; #${d.kept} is used`);
          for (const skip of result.skipped) out(`skipped: ${skip}`);
        }
        if (args.json) emit(result, '');
        else out(result.stopped ? `${summary}; stopped by GitHub's rate limit, run again ${when(result.stopped)}` : summary);
        if (result.stopped) {
          if (args.json) log(`stopped by GitHub's rate limit, run again ${when(result.stopped)}`);
          return 1;
        }
        return 0;
      }
      case 'pull': {
        const gh = await connected(deps);
        const result = await pull({ gh, root: deps.root, home: deps.home, write: Boolean(args.flags.write), now: deps.now() });
        emit(result, formatPull(result));
        return 0;
      }
      case 'ask': {
        const body = readText(args.options.body, 'body', deps);
        if (args.options.title === undefined) throw new UsageError('--title is required');
        const gh = await connected(deps);
        const result = await ask({ gh, root: deps.root, title: args.options.title, body, entry: args.options.entry });
        emit(result, `Opened #${result.number}: ${result.url}`);
        return 0;
      }
      case 'comment': {
        const text = readText(args.options.text, 'text', deps);
        const gh = await connected(deps);
        const result = await comment({ gh, number: Number(args.positional[0]), text });
        emit(result, `Commented on #${result.number}: ${result.url}`);
        return 0;
      }
      case 'built': {
        const gh = await connected(deps);
        const result = await built({ gh, number: Number(args.positional[0]) });
        emit(result, result.changed ? `Closed #${result.number} as built.` : `#${result.number} was already closed as built.`);
        return 0;
      }
    }
  } catch (error) {
    if (error instanceof UsageError) {
      log(`workshop: ${error.message}`);
      log('run `workshop --help` for usage');
      return 2;
    }
    if (error instanceof Failure) {
      log(`workshop: ${error.message}`);
      return 1;
    }
    if (error instanceof RateLimited) {
      log(`workshop: GitHub's rate limit is reached; run again ${when(error.resetAt ? error.resetAt.toISOString() : 'unknown')}`);
      return 1;
    }
    log(`workshop: ${String(error?.message ?? error).split('\n')[0]}`);
    return 1;
  }
  return 1;
}

function formatPull(result) {
  const lines = [];
  lines.push(`Verdicts (${result.verdicts.length}):`);
  for (const v of result.verdicts) {
    const status = v.conflict
      ? `several verdict labels (${v.conflict.join(', ')}), not recorded`
      : v.cleared
        ? `${v.file}, cleared on GitHub (the file is not changed)`
      : `${v.file ?? 'none'} -> ${v.github}${v.confirmed ? ` by ${v.by}` : ' (not confirmed as the owner’s, not recorded)'}`;
    lines.push(`  #${v.issue} ${v.id}: ${status}`);
  }
  lines.push(`Decided on GitHub, not in DECISIONS.md (${result.decisions.length}):`);
  for (const d of result.decisions) {
    lines.push(`  #${d.issue} ${d.title}`);
    lines.push(`    ${d.outcome.author}: ${d.outcome.text.replace(/\s+/g, ' ').trim()}`);
  }
  lines.push(`Closed without an owner's outcome (${result.closedWithoutOutcome.length}):`);
  for (const d of result.closedWithoutOutcome) lines.push(`  #${d.issue} ${d.title} (closed by ${d.closedBy ?? 'unknown'})`);
  for (const d of result.duplicates) lines.push(`Duplicate: #${d.duplicate} also carries ${d.id}'s marker; #${d.kept} is used`);
  lines.push(`Suggestions (${result.suggestions.length}):`);
  for (const s of result.suggestions) lines.push(`  #${s.issue} ${s.title} (${s.up} 👍, by ${s.author})`);
  lines.push(`Comments${result.since ? ` since ${result.since}` : ''} (${result.comments.length}):`);
  for (const c of result.comments) lines.push(`  #${c.issue} ${c.author}, ${c.createdAt}: ${c.text.replace(/\s+/g, ' ').trim()}`);
  for (const skip of result.skipped) lines.push(`Skipped: ${skip}`);
  if (result.written !== null) lines.push(`Recorded ${result.written} verdict${result.written === 1 ? '' : 's'} in features.jsonl.`);
  return lines.join('\n');
}
