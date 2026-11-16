import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { BIN, makeRepo, runCli, tempDir } from './helpers.js';

const json = (result) => {
  assert.equal(result.code, 0, result.stderr);
  return JSON.parse(result.stdout);
};

test('the bin is executable with a node shebang', () => {
  assert.match(fs.readFileSync(BIN, 'utf8'), /^#!\/usr\/bin\/env node\n/);
  assert.ok(fs.statSync(BIN).mode & 0o111);
});

test('usage: no command, unknown command and bad options exit 2; --help exits 0', async () => {
  const home = tempDir();
  const none = await runCli([], { home });
  assert.equal(none.code, 2);
  assert.match(none.stderr, /Usage: dashboard/);
  const unknown = await runCli(['frobnicate'], { home });
  assert.equal(unknown.code, 2);
  assert.match(unknown.stderr, /unknown command "frobnicate"/);
  assert.equal((await runCli(['list', '--bogus'], { home })).code, 2);
  assert.equal((await runCli(['list', '--open', '--all'], { home })).code, 2);
  assert.equal((await runCli(['show'], { home })).code, 2);
  assert.equal((await runCli(['show', 'abc'], { home })).code, 2);
  const help = await runCli(['--help'], { home });
  assert.equal(help.code, 0);
  assert.match(help.stdout, /answers \[--all\]/);
});

test('post: every option, body from a file and from stdin, attachments with captions', async () => {
  const home = tempDir();
  const files = tempDir();
  fs.writeFileSync(path.join(files, 'body.md'), 'Body from **file**');
  fs.writeFileSync(path.join(files, 'shot.png'), 'png');
  fs.writeFileSync(path.join(files, 'log:1.txt'), 'colon in name');

  const look = await runCli(
    ['post', '--kind', 'look', '--title', 'Sidebar order', '--journey', 'first-run', '--body', `@${path.join(files, 'body.md')}`, '--default', 'Alphabetical', '--source', 'feat/sidebar', '--attach', `${path.join(files, 'shot.png')}:The sidebar`, '--attach', path.join(files, 'log:1.txt')],
    { home },
  );
  assert.equal(look.code, 0, look.stderr);
  assert.equal(look.stdout.trim(), '1');

  const decide = json(await runCli(['post', '--kind', 'decide', '--title', 'Name', '--body', '-', '--json'], { home, input: 'From stdin' }));
  assert.deepEqual(decide, { id: 2 });

  const item = json(await runCli(['show', '1', '--json'], { home }));
  assert.equal(item.body, 'Body from **file**');
  assert.equal(item.default_text, 'Alphabetical');
  assert.equal(item.source, 'feat/sidebar');
  assert.equal(item.journey, 'first-run');
  assert.deepEqual(item.attachments.map((a) => [a.file, a.caption]), [['shot.png', 'The sidebar'], ['log_1.txt', null]]);
  assert.equal(json(await runCli(['show', '2', '--json'], { home })).body, 'From stdin');
  const text = await runCli(['show', '1'], { home });
  assert.match(text.stdout, /#1 look open \(waiting\) · journey first-run\nSidebar order/);
  assert.match(text.stdout, /Stands unless the owner says otherwise: Alphabetical/);
});

test('post errors: look without a default and bad kinds exit 2; a missing attachment exits 1 and posts nothing', async () => {
  const home = tempDir();
  const look = await runCli(['post', '--kind', 'look', '--title', 'No default'], { home });
  assert.equal(look.code, 2);
  assert.match(look.stderr, /needs a default/);
  assert.equal((await runCli(['post', '--kind', 'other', '--title', 'x'], { home })).code, 2);
  assert.equal((await runCli(['post', '--kind', 'decide'], { home })).code, 2);
  const missing = await runCli(['post', '--kind', 'decide', '--title', 'x', '--attach', '/no/such/file.png'], { home });
  assert.equal(missing.code, 1);
  assert.match(missing.stderr, /file not found/);
  const body = await runCli(['post', '--kind', 'decide', '--title', 'x', '--body', '@/no/such/body.md'], { home });
  assert.equal(body.code, 1);
  assert.deepEqual(json(await runCli(['list', '--all', '--json'], { home })), []);
});

test('attach, reply, answers, ack, close and list', async () => {
  const home = tempDir();
  const files = tempDir();
  fs.writeFileSync(path.join(files, 'clip.mp4'), 'mp4');
  fs.writeFileSync(path.join(files, 'reply.md'), 'Reply from a file');
  await runCli(['post', '--kind', 'look', '--title', 'Look', '--default', 'Keep'], { home });
  await runCli(['post', '--kind', 'decide', '--title', 'Decide'], { home });
  await runCli(['post', '--kind', 'shipped', '--title', 'Shipped'], { home });

  const first = json(await runCli(['attach', '3', path.join(files, 'clip.mp4'), '--caption', 'Demo', '--json'], { home }));
  assert.deepEqual([first.file, first.media_type, first.caption], ['clip.mp4', 'video/mp4', 'Demo']);
  const second = await runCli(['attach', '3', path.join(files, 'clip.mp4')], { home });
  assert.match(second.stdout, /attached clip-2\.mp4 to item 3/);
  assert.equal((await runCli(['attach', '3', path.join(files, 'gone.mp4')], { home })).code, 1);
  assert.equal((await runCli(['attach', '99', path.join(files, 'clip.mp4')], { home })).code, 1);
  assert.equal((await runCli(['attach', '3'], { home })).code, 2);

  assert.equal((await runCli(['reply', '2', '--text', `@${path.join(files, 'reply.md')}`], { home })).code, 0);
  assert.equal((await runCli(['reply', '2', '--text', '-'], { home, input: 'stdin reply' })).code, 0);
  assert.equal((await runCli(['reply', '2'], { home })).code, 2);
  assert.equal((await runCli(['reply', '42', '--text', 'x'], { home })).code, 1);
  const thread = json(await runCli(['show', '2', '--json'], { home })).replies;
  assert.deepEqual(thread.map((r) => [r.author, r.text]), [['agent', 'Reply from a file'], ['agent', 'stdin reply']]);

  // The owner answers through the store, as the page does.
  const home2 = new DatabaseSync(path.join(home, 'dashboard.db'));
  home2.prepare("INSERT INTO replies (item_id, author, verdict, text, created_at) VALUES (1, 'owner', 'disagree', 'Sort by use', ?)").run(new Date().toISOString());
  home2.prepare("INSERT INTO replies (item_id, author, verdict, text, created_at) VALUES (2, 'owner', NULL, 'Call it Silo', ?)").run(new Date().toISOString());
  home2.close();

  const answers = json(await runCli(['answers', '--json'], { home }));
  assert.deepEqual(
    answers.map((a) => [a.item_id, a.title, a.verdict, a.text]),
    [[1, 'Look', 'disagree', 'Sort by use'], [2, 'Decide', null, 'Call it Silo']],
  );
  const plain = await runCli(['answers'], { home });
  assert.match(plain.stdout, /reply \d+ · item 1 \(look\) "Look" · disagree\n  Sort by use/);

  assert.equal((await runCli(['ack', '999'], { home })).code, 1);
  assert.equal((await runCli(['ack'], { home })).code, 2);
  assert.equal((await runCli(['ack', String(thread[0].id)], { home })).code, 1, 'agent replies are not acknowledged');
  assert.deepEqual(json(await runCli(['ack', String(answers[0].reply_id), '--json'], { home })), { acknowledged: [answers[0].reply_id] });
  assert.deepEqual(json(await runCli(['answers', '--json'], { home })).map((a) => a.item_id), [2]);
  assert.equal(json(await runCli(['answers', '--all', '--json'], { home })).length, 2);

  assert.match((await runCli(['close', '1', '--note', 'Done in #12'], { home })).stdout, /closed item 1/);
  assert.equal((await runCli(['close', '1'], { home })).code, 1);
  assert.equal((await runCli(['close', '77'], { home })).code, 1);
  assert.equal(json(await runCli(['show', '1', '--json'], { home })).close_note, 'Done in #12');

  const open = json(await runCli(['list', '--json'], { home }));
  assert.deepEqual(open.map((item) => [item.id, item.state]), [[2, 'answered'], [3, 'waiting']]);
  assert.deepEqual(json(await runCli(['list', '--open', '--json'], { home })).map((item) => item.id), [2, 3]);
  assert.deepEqual(json(await runCli(['list', '--all', '--json'], { home })).map((item) => item.state), ['closed', 'answered', 'waiting']);
  assert.match((await runCli(['list'], { home })).stdout, /#3 {2}shipped waiting {2}- +Shipped/);
  assert.match((await runCli(['list'], { home: tempDir() })).stdout, /No open items/);
  assert.equal((await runCli(['show', '99'], { home })).code, 1);
});

test('a store with a newer schema is refused with exit 1', async () => {
  const home = tempDir();
  const db = new DatabaseSync(path.join(home, 'dashboard.db'));
  db.exec('PRAGMA user_version = 99');
  db.close();
  const result = await runCli(['list'], { home });
  assert.equal(result.code, 1);
  assert.match(result.stderr, /schema version 99/);
});

function serve(args, home, env = {}) {
  const child = spawn(process.execPath, [BIN, 'serve', ...args], { env: { ...process.env, SILO_DASHBOARD_HOME: home, ...env }, stdio: ['ignore', 'pipe', 'pipe'] });
  let stdout = '';
  let stderr = '';
  child.stdout.on('data', (chunk) => (stdout += chunk));
  child.stderr.on('data', (chunk) => (stderr += chunk));
  const exited = new Promise((resolve) => child.on('close', (code) => resolve({ code, stdout, stderr })));
  const ready = new Promise((resolve) => {
    child.stdout.on('data', () => {
      if (stdout.includes('\n')) resolve(stdout);
    });
    exited.then(() => resolve(stdout));
  });
  return { child, ready, exited };
}

test('serve: prints the URL, serves the page on 127.0.0.1 and stops on SIGTERM', async () => {
  const home = tempDir();
  const running = serve(['--port', '0', '--json'], home, { SILO_DASHBOARD_REPO: makeRepo() });
  const info = JSON.parse((await running.ready).split('\n')[0]);
  assert.match(info.url, /^http:\/\/127\.0\.0\.1:\d+\/$/);
  const response = await fetch(info.url);
  assert.equal(response.status, 200);
  assert.match(await response.text(), /Silo gives every agent its own computer/);
  running.child.kill('SIGTERM');
  assert.equal((await running.exited).code, 0);
});

test('serve: a taken port and a bad port are clear errors', async () => {
  const blocker = http.createServer();
  await new Promise((resolve) => blocker.listen(0, '127.0.0.1', resolve));
  const taken = blocker.address().port;
  try {
    const result = await serve(['--port', String(taken)], tempDir()).exited;
    assert.equal(result.code, 1);
    assert.match(result.stderr, new RegExp(`port ${taken} is already in use`));
  } finally {
    await new Promise((resolve) => blocker.close(resolve));
  }
  assert.equal((await serve(['--port', 'abc'], tempDir()).exited).code, 2);
  assert.equal((await serve(['--port', '70000'], tempDir()).exited).code, 2);
});
