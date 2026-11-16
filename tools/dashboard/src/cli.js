import fs from 'node:fs';
import { parseArgs } from 'node:util';
import { DashboardError, ValidationError } from './errors.js';
import { MAX_OPTIONS, POST_KINDS, Store } from './store.js';
import { DEFAULT_REPO, createServer } from './server.js';
import { FEATURE_ID, readFeatures } from './features.js';

export const USAGE = `Usage: dashboard <command> [options]

Commands:
  serve [--port N]                       start the dashboard page on 127.0.0.1 (default port 4517)
  post --kind look|decide|shipped --title T [--entry ID] [--body TEXT|@file|-]
       [--default TEXT] [--source TEXT] [--attach FILE[:caption]]...
       [--option "Label::Detail"]... [--recommend N]
                                         add an item for the owner; prints its id
  attach ID FILE [--caption TEXT]        add a file to an item
  reply ID --text TEXT|@file|-           add an agent reply to an item
  answers [--all]                        owner replies not yet acknowledged (answers,
                                         picked options, and the owner's notes)
  ack REPLY_ID...                        mark owner replies acknowledged
  verdicts [--all]                       the owner's verdicts on the feature list not yet
                                         acknowledged (--all: every current verdict)
  verdicts ack FEATURE_ID@ID...          acknowledge them, up to the verdict id listed
  close ID [--note TEXT]                 close an item
  list [--open|--all]                    list items (default: open)
  show ID                                show one item with its thread

Every command takes --json for machine output. A look item needs --default.
--entry names the feature list entry the item is about (its id in
docs/product/features.jsonl); the card links to it in the Stack.
A decide item may offer 2 to ${MAX_OPTIONS} options: --option "Label::Detail" (the detail is
optional; label up to 80 characters, detail up to 300), --recommend N marks
option N (from 1) as recommended. The owner picks one or answers in words.
Data lives in $SILO_DASHBOARD_HOME (default ~/.local/state/silo-dashboard).`;

class UsageError extends ValidationError {}

function parse(argv, options, { positionals = 0, variadic = false } = {}) {
  let parsed;
  try {
    parsed = parseArgs({ args: argv, options: { json: { type: 'boolean' }, ...options }, allowPositionals: true });
  } catch (error) {
    throw new UsageError(error.message.replace(/\. To specify a positional.*$/, ''));
  }
  const { values, positionals: rest } = parsed;
  if (variadic ? rest.length < positionals : rest.length !== positionals) {
    throw new UsageError(`expected ${variadic ? 'at least ' : ''}${positionals} argument${positionals === 1 ? '' : 's'}, got ${rest.length}`);
  }
  return { values, args: rest };
}

function readText(value, label, stdin) {
  if (value === undefined) return undefined;
  if (value === '-') return stdin();
  if (value.startsWith('@')) {
    const file = value.slice(1);
    try {
      return fs.readFileSync(file, 'utf8');
    } catch {
      throw new DashboardError(`cannot read ${label} file: ${file}`);
    }
  }
  return value;
}

function itemId(text, label = 'item id') {
  if (!/^\d+$/.test(text)) throw new UsageError(`${label} must be a number, got "${text}"`);
  return Number(text);
}

function isFile(candidate) {
  try {
    return fs.statSync(candidate).isFile();
  } catch {
    return false;
  }
}

// "FILE" or "FILE:caption". The file may itself contain a colon: the first split that names a file wins.
function parseAttach(spec) {
  if (isFile(spec)) return { path: spec, caption: null };
  for (let at = spec.indexOf(':'); at !== -1; at = spec.indexOf(':', at + 1)) {
    if (at > 0 && isFile(spec.slice(0, at))) return { path: spec.slice(0, at), caption: spec.slice(at + 1) || null };
  }
  throw new DashboardError(`file not found: ${spec.includes(':') ? spec.slice(0, spec.indexOf(':')) : spec}`);
}

// "Label::Detail" or "Label".
function parseOption(spec) {
  const at = spec.indexOf('::');
  return at === -1 ? { label: spec, detail: null } : { label: spec.slice(0, at), detail: spec.slice(at + 2) || null };
}

function shortLine(text, max = 90) {
  const line = String(text).replace(/\s+/g, ' ').trim();
  return line.length > max ? `${line.slice(0, max - 1)}…` : line;
}

function indent(text) {
  return String(text)
    .split('\n')
    .map((line) => `  ${line}`)
    .join('\n');
}

// " · disagree", " · chose 2 "Label"", and the reactions older stores hold.
function replyMarks(reply) {
  let out = '';
  if (reply.verdict) out += ` · ${reply.verdict}`;
  if (reply.choice != null) out += ` · chose ${reply.choice + 1} "${reply.choice_label ?? '?'}"`;
  if (reply.reaction) out += ` · reaction ${reply.reaction}`;
  else if (reply.author === 'owner' && !reply.text && !reply.verdict && reply.choice == null) out += ' · reaction cleared';
  return out;
}

function showItem(item) {
  const lines = [
    `#${item.id} ${item.kind} ${item.status}${item.state ? ` (${item.state})` : ''}${item.journey ? ` · entry ${item.journey}` : ''}`,
    item.title,
    `posted ${item.created_at}${item.source ? ` by ${item.source}` : ''}`,
  ];
  if (item.body) lines.push('', item.body);
  if (item.anchor) lines.push(`note on ${item.anchor} ("${item.anchor_text}")`);
  if (item.default_text) lines.push('', `Stands unless the owner says otherwise: ${item.default_text}`);
  if (item.options) {
    lines.push('', 'Options:');
    item.options.forEach((option, index) => lines.push(`  ${index + 1}. ${option.label}${option.recommended ? ' (recommended)' : ''}${option.detail ? ` — ${option.detail}` : ''}`));
  }
  for (const file of item.attachments) lines.push(`attachment ${file.id}: ${file.file} (${file.media_type})${file.caption ? ` — ${file.caption}` : ''}`);
  for (const reply of item.replies) {
    lines.push('', `reply ${reply.id} · ${reply.author}${replyMarks(reply)} · ${reply.created_at}${reply.acked_at ? ' · acknowledged' : ''}`);
    if (reply.text && reply.choice == null) lines.push(indent(reply.text));
  }
  if (item.status === 'closed') lines.push('', `closed ${item.closed_at}${item.close_note ? `: ${item.close_note}` : ''}`);
  return lines.join('\n');
}

const commands = {
  async serve(argv, io) {
    const { values } = parse(argv, { port: { type: 'string' } });
    let port = 4517;
    if (values.port !== undefined) {
      if (!/^\d+$/.test(values.port) || Number(values.port) > 65535) throw new UsageError(`--port must be 0-65535, got "${values.port}"`);
      port = Number(values.port);
    }
    const store = io.store();
    const server = createServer({ store });
    await new Promise((resolve, reject) => {
      server.once('error', (error) => {
        if (error.code === 'EADDRINUSE') reject(new DashboardError(`port ${port} is already in use; pick another with --port N`));
        else reject(error);
      });
      server.listen(port, '127.0.0.1', resolve);
    });
    const actual = server.address().port;
    const url = `http://127.0.0.1:${actual}/`;
    io.out(values.json ? JSON.stringify({ url, port: actual, home: store.home }) : `Silo owner dashboard: ${url}\nData: ${store.home}`);
    await new Promise((resolve) => {
      const stop = () => {
        server.close(() => resolve());
        server.closeAllConnections();
      };
      process.once('SIGINT', stop);
      process.once('SIGTERM', stop);
    });
  },

  post(argv, io) {
    const { values } = parse(argv, {
      kind: { type: 'string' },
      title: { type: 'string' },
      entry: { type: 'string' },
      journey: { type: 'string' },
      body: { type: 'string' },
      default: { type: 'string' },
      source: { type: 'string' },
      attach: { type: 'string', multiple: true },
      option: { type: 'string', multiple: true },
      recommend: { type: 'string' },
    });
    if (!POST_KINDS.includes(values.kind)) throw new UsageError(`--kind must be one of ${POST_KINDS.join(', ')}`);
    if (!values.title) throw new UsageError('--title is required');
    // --journey is the earlier name of --entry.
    if (values.entry !== undefined && values.journey !== undefined) throw new UsageError('use --entry, not both --entry and --journey');
    const entry = values.entry ?? values.journey ?? null;
    const options = (values.option || []).map(parseOption);
    if (options.length && values.kind !== 'decide') throw new UsageError('--option is for --kind decide');
    if (values.recommend !== undefined) {
      if (!options.length) throw new UsageError('--recommend needs --option');
      if (!/^\d+$/.test(values.recommend) || Number(values.recommend) < 1 || Number(values.recommend) > options.length) {
        throw new UsageError(`--recommend must be an option number from 1 to ${options.length}`);
      }
      options[Number(values.recommend) - 1].recommended = true;
    }
    const attachments = (values.attach || []).map(parseAttach);
    const defaultText = readText(values.default, 'default', io.stdin);
    const body = readText(values.body, 'body', io.stdin);
    const id = io.store().post(
      { kind: values.kind, entry, title: values.title, body: body ?? '', defaultText: defaultText ?? null, source: values.source ?? null, options: options.length ? options : null },
      attachments,
    );
    io.out(values.json ? JSON.stringify({ id }) : String(id));
  },

  attach(argv, io) {
    const { values, args } = parse(argv, { caption: { type: 'string' } }, { positionals: 2 });
    const id = itemId(args[0]);
    const attachment = io.store().addAttachment(id, args[1], values.caption ?? null);
    io.out(values.json ? JSON.stringify(attachment) : `attached ${attachment.file} to item ${id}`);
  },

  reply(argv, io) {
    const { values, args } = parse(argv, { text: { type: 'string' } }, { positionals: 1 });
    if (values.text === undefined) throw new UsageError('--text is required');
    const text = readText(values.text, 'text', io.stdin);
    const id = io.store().addReply(itemId(args[0]), { author: 'agent', text });
    io.out(values.json ? JSON.stringify({ id }) : String(id));
  },

  answers(argv, io) {
    const { values } = parse(argv, { all: { type: 'boolean' } });
    const rows = io.store().ownerReplies({ all: values.all });
    if (values.json) return io.out(JSON.stringify(rows));
    if (!rows.length) return io.out(values.all ? 'No owner replies.' : 'No new owner replies.');
    return io.out(
      rows
        .map((row) => {
          const what = row.kind === 'note' ? `note on ${row.anchor} "${shortLine(row.anchor_text ?? row.title)}"` : `(${row.kind}${row.journey ? `, ${row.journey}` : ''}) "${shortLine(row.title)}"`;
          const text = row.text && row.choice == null ? `\n${indent(row.text)}` : '';
          return `reply ${row.reply_id} · item ${row.item_id} ${what}${replyMarks({ ...row, author: 'owner' })}${row.acked_at ? ' · acknowledged' : ''}${text}`;
        })
        .join('\n\n'),
    );
  },

  ack(argv, io) {
    const { values, args } = parse(argv, {}, { positionals: 1, variadic: true });
    const ids = args.map((arg) => itemId(arg, 'reply id'));
    io.store().ack(ids);
    io.out(values.json ? JSON.stringify({ acknowledged: ids }) : `acknowledged ${ids.join(', ')}`);
  },

  verdicts(argv, io) {
    const { values, args } = parse(argv, { all: { type: 'boolean' } }, { positionals: 0, variadic: true });
    if (args.length) {
      if (args[0] !== 'ack') throw new UsageError(`unknown verdicts command "${args[0]}"`);
      if (values.all) throw new UsageError('--all is for listing verdicts');
      if (args.length < 2) throw new UsageError('verdicts ack needs at least one feature id');
      const requests = args.slice(1).map((arg) => {
        const match = /^([^@]+)@(\d+)$/.exec(arg);
        if (!match || !FEATURE_ID.test(match[1])) {
          throw new UsageError(`"${arg}" is not FEATURE_ID@ID: acknowledge each verdict as 'dashboard verdicts' lists it (for example engine-service@12)`);
        }
        return { feature: match[1], upTo: Number(match[2]) };
      });
      const done = io.store().ackVerdicts(requests);
      return io.out(values.json ? JSON.stringify({ acknowledged: done }) : `acknowledged ${done.join(', ')}`);
    }
    const names = new Map((readFeatures(io.repo())?.features ?? []).map((feature) => [feature.id, feature.name]));
    const rows = io
      .store()
      .verdictsToRead({ all: values.all })
      .map((row) => ({
        id: row.id,
        ack: `${row.feature_id}@${row.id}`,
        feature_id: row.feature_id,
        name: row.name ?? names.get(row.feature_id) ?? null,
        in_list: names.has(row.feature_id),
        verdict: row.verdict,
        note: row.note,
        created_at: row.created_at,
        acked_at: row.acked_at,
      }));
    if (values.json) return io.out(JSON.stringify(rows));
    if (!rows.length) return io.out(values.all ? 'No verdicts.' : 'No new verdicts.');
    return io.out(
      rows
        .map((row) => {
          const name = row.name ? ` "${shortLine(row.name)}"` : '';
          const gone = row.in_list ? '' : ' (no longer in the feature list)';
          const head = `${row.ack} · ${row.verdict ?? 'cleared'}${name}${gone} · ${row.created_at}${row.acked_at ? ' · acknowledged' : ''}`;
          return row.note ? `${head}\n${indent(row.note)}` : head;
        })
        .join('\n\n'),
    );
  },

  close(argv, io) {
    const { values, args } = parse(argv, { note: { type: 'string' } }, { positionals: 1 });
    const id = itemId(args[0]);
    io.store().closeItem(id, values.note ?? null);
    io.out(values.json ? JSON.stringify({ id, status: 'closed' }) : `closed item ${id}`);
  },

  list(argv, io) {
    const { values } = parse(argv, { open: { type: 'boolean' }, all: { type: 'boolean' } });
    if (values.open && values.all) throw new UsageError('use --open or --all, not both');
    const items = io.store().itemsWithState().filter((item) => values.all || item.status === 'open');
    if (values.json) return io.out(JSON.stringify(items));
    if (!items.length) return io.out(values.all ? 'No items.' : 'No open items.');
    return io.out(items.map((item) => `#${item.id}  ${item.kind.padEnd(7)} ${item.state.padEnd(8)} ${(item.journey || '-').padEnd(16)} ${shortLine(item.title)}`).join('\n'));
  },

  show(argv, io) {
    const { values, args } = parse(argv, {}, { positionals: 1 });
    const id = itemId(args[0]);
    const item = io.store().itemsWithState().find((entry) => entry.id === id);
    if (!item) throw new DashboardError(`no item ${id}`);
    io.out(values.json ? JSON.stringify(item) : showItem(item));
  },
};

/**
 * Runs the CLI and returns the exit code: 0 success, 1 error, 2 usage error.
 * io: { out(text), err(text), stdin(): string } can be overridden for tests.
 */
export async function main(argv, overrides = {}) {
  let store = null;
  const io = {
    out: (text) => process.stdout.write(`${text}\n`),
    err: (text) => process.stderr.write(`${text}\n`),
    stdin: () => fs.readFileSync(0, 'utf8'),
    ...overrides,
  };
  io.store = () => (store ||= new Store());
  io.repo ??= () => process.env.SILO_DASHBOARD_REPO || DEFAULT_REPO;
  const [name, ...rest] = argv;
  try {
    if (!name) {
      io.err(USAGE);
      return 2;
    }
    if (name === '--help' || name === '-h' || name === 'help') {
      io.out(USAGE);
      return 0;
    }
    if (!Object.hasOwn(commands, name)) throw new UsageError(`unknown command "${name}"`);
    if (rest.includes('--help') || rest.includes('-h')) {
      io.out(USAGE);
      return 0;
    }
    await commands[name](rest, io);
    return 0;
  } catch (error) {
    if (error instanceof DashboardError) {
      io.err(`dashboard: ${error.message}`);
      if (error instanceof UsageError) io.err("Run 'dashboard --help' for usage.");
      return error.exit;
    }
    io.err(`dashboard: ${error.stack || error}`);
    return 1;
  } finally {
    store?.close();
  }
}
