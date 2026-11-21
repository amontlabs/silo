// `workshop`: the commands. Exit codes: 0 done, 1 refused or a problem found, 2 wrong use, 3 GitHub
// or the credentials to reach it could not be used.

import { spawn } from 'node:child_process';
import { homedir } from 'node:os';
import { GitHub } from '../github/client.js';
import { appCredentials } from '../github/credentials.js';
import { decline } from '../github/decline.js';
import { doctor } from '../github/doctor.js';
import { Failure, Unavailable } from '../github/errors.js';
import { macKeychain } from '../github/keychain.js';
import { DEFAULT_API, DEFAULT_ORG } from '../github/names.js';
import { setup, store } from '../github/setup.js';
import { DEFAULT_REPO, DEFAULT_SITE, DEFAULT_WEB } from '../model/names.js';
import { UsageError, parseArgs } from './args.js';

export const COMMANDS = [
  { name: 'setup', usage: 'workshop setup [--org <org>] [--site <url>] [--replace]', what: 'Creates the Workshop GitHub App.' },
  { name: 'store', usage: 'workshop store [--replace]', what: "Stores the App's credentials from vercel.env in the Keychain." },
  { name: 'doctor', usage: 'workshop doctor', what: 'Checks the credentials, the App, the labels and the entries\' issues.' },
  { name: 'decline', usage: 'workshop decline <issue> --why "…"', what: 'Drops a suggestion: closes it, with GitHub\'s own reason for closing, "not planned".' },
];

/**
 * What touches the world: GitHub, the Keychain, HOME, the browser.
 * @typedef {{ env: Record<string, string | undefined>, home: string, keychain: any, fetch: typeof fetch, api: string, web: string, repo: string,
 *   writeDelayMs: number, sleep?: (ms: number) => Promise<void>, openBrowser: (url: string) => void, setupTimeoutMs?: number }} Deps
 */
/** @typedef {{ deps: Deps, out: (text: string) => void, err: (text: string) => void }} Context */

/** @returns {Deps} */
export function defaultDeps() {
  const env = process.env;
  return {
    env,
    home: homedir(),
    keychain: macKeychain,
    fetch: globalThis.fetch,
    api: env.WORKSHOP_GITHUB_API || DEFAULT_API,
    web: DEFAULT_WEB,
    repo: env.WORKSHOP_REPO || DEFAULT_REPO,
    writeDelayMs: 1000,
    openBrowser: (url) => spawn('open', [url], { stdio: 'ignore', detached: true }).unref(),
  };
}

/** @param {string[]} argv @param {Partial<Context> & { deps?: Partial<Deps> }} [context] @returns {Promise<number>} the exit code */
export async function main(argv, context = {}) {
  /** @type {Context} */
  const ctx = {
    out: (text) => process.stdout.write(`${text}\n`),
    err: (text) => process.stderr.write(`${text}\n`),
    ...context,
    deps: { ...defaultDeps(), ...context.deps },
  };
  // --json may come before or after the command name; the command always sees it after.
  const json = argv.includes('--json');
  const [command, ...others] = argv.filter((arg) => arg !== '--json');
  const rest = json ? [...others, '--json'] : others;
  try {
    switch (command) {
      case undefined: return list(ctx, json);
      case 'setup': return await setupCommand(ctx, rest);
      case 'store': return storeCommand(ctx, rest);
      case 'doctor': return await doctorCommand(ctx, rest);
      case 'decline': return await declineCommand(ctx, rest);
      default: throw new UsageError(`unknown command "${command}"; workshop alone lists the commands`);
    }
  } catch (error) {
    const code = error instanceof UsageError ? 2 : error instanceof Unavailable ? 3 : error instanceof Failure ? 1 : null;
    // Anything else is reported by its first line only: no stack, no body that might carry a value.
    const message = code === null ? String(/** @type {any} */ (error)?.message ?? error).split('\n')[0] : /** @type {Error} */ (error).message;
    if (json) ctx.out(JSON.stringify({ ok: false, error: message }, null, 2));
    else ctx.err(`workshop: ${message}`);
    return code ?? 1;
  }
}

/** @param {Context} ctx @param {boolean} json */
function list(ctx, json) {
  if (json) {
    ctx.out(JSON.stringify({ commands: COMMANDS.map(({ name, usage, what }) => ({ name, usage, what })) }, null, 2));
    return 0;
  }
  const line = (item) => `  ${item.usage.padEnd(40)}  ${item.what}`;
  ctx.out(['workshop: the Workshop on GitHub. Every command takes --json.', '', ...COMMANDS.map(line)].join('\n'));
  return 0;
}

/** @param {Context} ctx @param {boolean} json @param {unknown} result @param {string} text */
const emit = (ctx, json, result, text) => ctx.out(json ? JSON.stringify(result, null, 2) : text);

/** @param {Context} ctx */
function setupDeps(ctx) {
  return { ...ctx.deps, log: ctx.err };
}

/** @param {Context} ctx @param {string[]} argv */
async function setupCommand(ctx, argv) {
  const { flags, positional, values } = parseArgs(argv, { org: 'value', site: 'value', replace: 'flag' });
  if (positional.length) throw new UsageError('usage: workshop setup [--org <org>] [--site <url>] [--replace]');
  const result = await setup({ org: values.org ?? DEFAULT_ORG, site: values.site ?? DEFAULT_SITE, replace: flags.has('replace'), deps: setupDeps(ctx) });
  emit(ctx, flags.has('json'), { ok: true, ...result }, `Created the App ${result.app}. Credentials are in the Keychain; Vercel's variables are in ${result.envFile}.\nInstall it on ${ctx.deps.repo}: ${result.installUrl}`);
  return 0;
}

/** @param {Context} ctx @param {string[]} argv */
function storeCommand(ctx, argv) {
  const { flags, positional } = parseArgs(argv, { replace: 'flag' });
  if (positional.length) throw new UsageError('usage: workshop store [--replace]');
  const result = store({ replace: flags.has('replace'), deps: setupDeps(ctx) });
  emit(ctx, flags.has('json'), { ok: true, ...result }, result.already ? `The Keychain already holds the App credentials of ${result.from}.` : `Stored ${result.stored.join(', ')} from ${result.from} in the Keychain.`);
  return 0;
}

/** @param {Context} ctx @param {string[]} argv */
async function doctorCommand(ctx, argv) {
  const { flags, positional } = parseArgs(argv, {});
  if (positional.length) throw new UsageError('usage: workshop doctor');
  const { deps } = ctx;
  const gh = new GitHub({ api: deps.api, repo: deps.repo, fetch: deps.fetch, writeDelayMs: 0 });
  const report = await doctor({ deps, gh, credentials: () => appCredentials(deps) });
  const lines = [
    ...Object.entries(report.credentials).map(([name, source]) => `${name}: ${source}`),
    `App: ${report.app ?? 'unknown'}`,
    `Public: ${report.public === true ? 'yes' : report.public === false ? 'no (sign-in will not work for anyone outside its organization)' : 'unknown'}`,
    `Installed on ${deps.repo}: ${report.installed ? 'yes' : 'no'}`,
    `Permissions: ${report.permissions ? Object.entries(report.permissions).map(([k, v]) => `${k} ${v}`).join(', ') || 'none' : 'unknown'}`,
    ...(report.labels ? Object.entries(report.labels).map(([name, ok]) => `Label ${name}: ${ok ? 'yes' : 'missing'}`) : ['Labels: unknown']),
    ...(report.duplicates ? [`Entries with several issues: ${Object.keys(report.duplicates).length}`] : []),
    ...report.problems.map((problem) => `Problem: ${problem}`),
  ];
  emit(ctx, flags.has('json'), { ok: report.problems.length === 0, ...report }, lines.join('\n'));
  return report.problems.length ? 1 : 0;
}

/** @param {Context} ctx @param {string[]} argv */
async function declineCommand(ctx, argv) {
  const { flags, positional, values } = parseArgs(argv, { why: 'value' });
  if (positional.length !== 1 || values.why === undefined) throw new UsageError('usage: workshop decline <issue> --why "…"');
  const text = positional[0].replace(/^#/, '');
  if (!/^[1-9]\d*$/.test(text)) throw new UsageError(`"${positional[0]}" is not an issue number`);
  const why = values.why.trim();
  if (!why) throw new UsageError('--why is empty');
  const { deps } = ctx;
  const gh = new GitHub({ api: deps.api, repo: deps.repo, fetch: deps.fetch, writeDelayMs: deps.writeDelayMs, sleep: deps.sleep });
  await gh.authenticate(appCredentials(deps));
  const result = await decline({ gh, number: Number(text), why });
  emit(ctx, flags.has('json'), { ok: true, ...result }, `Dropped #${result.number}: closed with GitHub's own reason "not planned".`);
  return 0;
}
