import { execFileSync, spawn } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const BIN = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../bin/dashboard');

// Every temporary folder of a test process lives under one base, removed on exit.
const BASE = fs.mkdtempSync(path.join(os.tmpdir(), 'silo-dashboard-test-'));
process.on('exit', () => fs.rmSync(BASE, { recursive: true, force: true }));

export function tempDir(prefix = 'dash-') {
  return fs.mkdtempSync(path.join(BASE, prefix));
}

export const PRODUCT = `# Silo

Silo gives every agent its own computer.

## Why it exists

Agents need a place to work that cannot hurt the owner's machine.

## Who it is for

Developers who run agents all day.
`;

export const RESTART = `# Restart

## Where the work stands

1. Done: the old tree is frozen.
2. Done: the feature list.
3. Now: the owner goes down the **Stack**. The first entries are the starter.
4. Next: start \`main\` again. Wait for the owner first.
5. Then: design the shell. Show it first.
`;

export const DECISIONS = `# Decisions

## Product

- Silo is a Mac app first.
- Agents never see the owner's keychain.

## Pricing

- Free for one computer.
`;

export function git(repo, ...args) {
  return execFileSync('git', ['-C', repo, ...args], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
}

// A throwaway git checkout holding docs/product, with one commit.
export function makeRepo({ product = PRODUCT, decisions = DECISIONS, restart = RESTART, commit = true } = {}) {
  const repo = tempDir('dash-repo-');
  git(repo, 'init', '-q', '-b', 'main');
  git(repo, 'config', 'user.email', 'test@example.com');
  git(repo, 'config', 'user.name', 'Test');
  git(repo, 'config', 'commit.gpgsign', 'false');
  const folder = path.join(repo, 'docs', 'product');
  fs.mkdirSync(folder, { recursive: true });
  if (product !== null) fs.writeFileSync(path.join(folder, 'PRODUCT.md'), product);
  if (decisions !== null) fs.writeFileSync(path.join(folder, 'DECISIONS.md'), decisions);
  if (restart !== null) fs.writeFileSync(path.join(folder, 'RESTART.md'), restart);
  if (commit) commitAll(repo, 'docs');
  return repo;
}

export function commitAll(repo, message) {
  git(repo, 'add', '-A');
  git(repo, 'commit', '-q', '-m', message);
  return git(repo, 'rev-parse', 'HEAD').trim();
}

export function writeDoc(repo, name, text) {
  fs.mkdirSync(path.join(repo, 'docs', 'product'), { recursive: true });
  fs.writeFileSync(path.join(repo, 'docs', 'product', name), text);
}

// Runs the CLI as its own process.
export function runCli(args, { home, input, env = {} } = {}) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, [BIN, ...args], {
      // Never the real checkout: a CLI run reads docs/product from a fixture unless the test names one.
      env: { ...process.env, SILO_DASHBOARD_REPO: tempDir('dash-norepo-'), SILO_DASHBOARD_HOME: home, ...env },
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (chunk) => (stdout += chunk));
    child.stderr.on('data', (chunk) => (stderr += chunk));
    child.on('close', (code) => resolve({ code, stdout, stderr }));
    child.stdin.end(input ?? '');
  });
}

// A minimal HTTP client that can send any Host and Origin header.
export function request(port, { method = 'GET', url = '/', headers = {}, body } = {}) {
  return new Promise((resolve, reject) => {
    const req = http.request({ host: '127.0.0.1', port, method, path: url, headers: { Host: `127.0.0.1:${port}`, ...headers } }, (res) => {
      const chunks = [];
      res.on('data', (chunk) => chunks.push(chunk));
      res.on('end', () => resolve({ status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks) }));
    });
    req.on('error', reject);
    req.end(body);
  });
}

export const postJson = (port, url, value, headers = {}) =>
  request(port, { method: 'POST', url, headers: { 'Content-Type': 'application/json', ...headers }, body: JSON.stringify(value) });
