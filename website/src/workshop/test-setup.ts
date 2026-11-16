import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/react';
import { afterEach, vi } from 'vitest';

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  history.replaceState(null, '', '/workshop');
  document.cookie = 'workshop-hint=; expires=Thu, 01 Jan 1970 00:00:00 GMT; path=/';
  localStorage.clear();
});
