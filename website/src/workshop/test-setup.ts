import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/react';
import { afterEach, beforeEach, vi } from 'vitest';
import { loadConfig } from './useWorkshop';

// A failed load is tried again after 1.5 s on the page; the tests that do not look at that wait only briefly.
beforeEach(() => {
  loadConfig.retryMs = 20;
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  history.replaceState(null, '', '/workshop');
  document.cookie = 'workshop-hint=; expires=Thu, 01 Jan 1970 00:00:00 GMT; path=/';
  localStorage.clear();
  sessionStorage.clear();
});
