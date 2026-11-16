import fs from 'node:fs';
import { parseArgs } from 'node:util';
import { DashboardError, ValidationError } from './errors.js';
import { Store, KINDS } from './store.js';
import { createServer } from './server.js';

export const USAGE = `Usage: dashboard <command> [options]

Commands:
  serve [--port N]                       start the dashboard page on 127.0.0.1 (default port 4517)
  post --kind look|decide|shipped --title T [--journey ID] [--body TEXT|@file|-]
       [--default TEXT] [--source TEXT] [--attach FILE[:caption]]...
                                         add an item for the owner; prints its id
  attach ID FILE [--caption TEXT]        add a file to an item
  reply ID --text TEXT|@file|-           add an agent reply to an item
  answers [--all]                        owner replies not yet acknowledged
  ack REPLY_ID...                        mark owner replies acknowledged
  close ID [--note TEXT]                 close an item
  list [--open|--all]                    list items (default: open)
  show ID                                show one item with its thread

Every command takes --json for machine output. A look item needs --default.
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

function showItem(item) {
  const lines = [
    `#${item.id} ${item.kind} ${item.status}${item.state ? ` (${item.state})` : ''}${item.journey ? ` · journey ${item.journey}` : ''}`,
    item.title,
    `posted ${item.created_at}${item.source ? ` by ${item.source}` : ''}`,
  ];
  if (item.body) lines.push('', item.body);
  if (item.default_text) lines.push('', `Stands unless the owner says otherwise: ${item.default_text}`);
  for (const file of item.attachments) lines.push(`attachment ${file.id}: ${file.file} (${file.media_type})${file.caption ? ` — ${file.caption}` : ''}`);
  for (const reply of item.replies) {
    lines.push('', `reply ${reply.id} · ${reply.author}${reply.verdict ? ` · ${reply.verdict}` : ''} · ${reply.created_at}${reply.acked_at ? ' · acknowledged' : ''}`);
    if (reply.text) lines.push(indent(reply.text));
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
      journey: { type: 'string' },
      body: { type: 'string' },
      default: { type: 'string' },
      source: { type: 'string' },
      attach: { type: 'string', multiple: true },
    });
    if (!KINDS.includes(values.kind)) throw new UsageError(`--kind must be one of ${KINDS.join(', ')}`);
    if (!values.title) throw new UsageError('--title is required');
    const attachments = (values.attach || []).map(parseAttach);
    const defaultText = readText(values.default, 'default', io.stdin);
    const body = readText(values.body, 'body', io.stdin);
    const id = io.store().post(
      { kind: values.kind, journey: values.journey ?? null, title: values.title, body: body ?? '', defaultText: defaultText ?? null, source: values.source ?? null },
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
        .map((row) => `reply ${row.reply_id} · item ${row.item_id} (${row.kind}${row.journey ? `, ${row.journey}` : ''}) "${shortLine(row.title)}"${row.verdict ? ` · ${row.verdict}` : ''}${row.acked_at ? ' · acknowledged' : ''}${row.text ? `\n${indent(row.text)}` : ''}`)
        .join('\n\n'),
    );
  },

  ack(argv, io) {
    const { values, args } = parse(argv, {}, { positionals: 1, variadic: true });
    const ids = args.map((arg) => itemId(arg, 'reply id'));
    io.store().ack(ids);
    io.out(values.json ? JSON.stringify({ acknowledged: ids }) : `acknowledged ${ids.join(', ')}`);
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
