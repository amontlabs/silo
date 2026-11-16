import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { JSDOM } from 'jsdom';
const markup = readFileSync(new URL('../index.html', import.meta.url), 'utf8');
const script = readFileSync(new URL('../public/theme.js', import.meta.url), 'utf8');
function setup({ stored, dark = false, blocked = false } = {}) {
  const dom = new JSDOM(markup, { url: 'https://silo.test', runScripts: 'outside-only' });
  const w = dom.window;
  const media = new w.EventTarget(); media.matches = dark;
  w.matchMedia = () => media;
  if (stored) w.localStorage.setItem('silo-website-theme', stored);
  if (blocked) Object.defineProperty(w, 'localStorage', { get() { throw new Error('Blocked'); } });
  w.eval(script);
  w.document.dispatchEvent(new w.Event('DOMContentLoaded'));
  const trigger = w.document.querySelector('[data-theme-trigger]');
  return { w, media, choose(value) { trigger.click(); w.document.querySelector(`[data-theme-option="${value}"]`).click(); }, theme() { return w.document.documentElement.dataset.theme; }, close() { w.close(); } };
}
test('System is default and tracks system changes', () => {
  const t = setup();
  assert.equal(t.theme(), 'light');
  assert.equal(t.w.document.querySelector('[data-theme-option][aria-checked="true"]').dataset.themeOption, 'system');
  t.media.matches = true; t.media.dispatchEvent(new t.w.Event('change'));
  assert.equal(t.theme(), 'dark');
  assert.equal(t.w.document.documentElement.classList.contains('dark'), true);
  t.close();
});
test('Explicit selection persists, ignores system changes, and can return to System', () => {
  const t = setup({ dark: true });
  t.choose('light');
  t.media.dispatchEvent(new t.w.Event('change'));
  assert.equal(t.theme(), 'light');
  assert.equal(t.w.localStorage.getItem('silo-website-theme'), 'light');
  t.choose('system'); assert.equal(t.theme(), 'dark'); t.close();
  const saved = setup({ stored: 'dark' }); assert.equal(saved.theme(), 'dark'); saved.close();
});
test('Blocked storage and invalid preferences fall back safely', () => {
  const t = setup({ blocked: true }); t.choose('dark'); assert.equal(t.theme(), 'dark'); t.close();
  const invalid = setup({ stored: 'unknown', dark: true }); assert.equal(invalid.theme(), 'dark'); invalid.close();
});
test('Other same-origin documents synchronize through storage events', () => {
  const t = setup();
  t.w.dispatchEvent(new t.w.StorageEvent('storage', { key: 'silo-website-theme', newValue: 'dark' }));
  assert.equal(t.theme(), 'dark');
  t.w.dispatchEvent(new t.w.StorageEvent('storage', { key: null })); assert.equal(t.theme(), 'light'); t.close();
});

test('Theme menu opens at the selection, supports arrow navigation, and restores focus', () => {
  const t = setup(); const d = t.w.document;
  const trigger = d.querySelector('[data-theme-trigger]');
  const menu = d.querySelector('#theme-menu');
  trigger.click();
  assert.equal(menu.hidden, false);
  assert.equal(d.activeElement.dataset.themeOption, 'system');
  d.activeElement.dispatchEvent(new t.w.KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
  assert.equal(d.activeElement.dataset.themeOption, 'light');
  d.activeElement.click();
  assert.equal(menu.hidden, true);
  assert.equal(d.activeElement, trigger);
  trigger.click();
  d.activeElement.dispatchEvent(new t.w.KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
  assert.equal(menu.hidden, true);
  assert.equal(d.activeElement, trigger);
  trigger.click();
  d.body.dispatchEvent(new t.w.Event('pointerdown', { bubbles: true }));
  assert.equal(menu.hidden, true);
  t.close();
});
