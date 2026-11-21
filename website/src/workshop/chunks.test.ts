// A chunk that fails to load: a failure of the site's newer deployment becomes SiteChanged, any other stays as it is.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { chunk, SiteChanged } from './chunks';

const page = (script: string) => `<!doctype html><script type="module" src="${script}"></script>`;
const serving = (body: string | null) => vi.stubGlobal('fetch', vi.fn(() => (body === null ? Promise.reject(new Error('offline')) : Promise.resolve(new Response(body)))));
const failing = () => Promise.reject(new Error('404'));

afterEach(() => vi.unstubAllGlobals());

describe('a chunk that fails to load', () => {
  it('is SiteChanged when the page served now runs another script', async () => {
    serving(page('/assets/workshop-NEW.js'));
    await expect(chunk(failing, '/assets/workshop-OLD.js')()).rejects.toBeInstanceOf(SiteChanged);
  });

  it('keeps its own failure when the page served now runs the same script', async () => {
    serving(page('/assets/workshop-OLD.js'));
    await expect(chunk(failing, '/assets/workshop-OLD.js')()).rejects.toThrow('404');
  });

  it('keeps its own failure when the page cannot be fetched either', async () => {
    serving(null);
    await expect(chunk(failing, '/assets/workshop-OLD.js')()).rejects.toThrow('404');
  });

  it('asks nothing of the site when it loads', async () => {
    serving(page('/assets/workshop-NEW.js'));
    await expect(chunk(() => Promise.resolve({ core: 'In.' }), '/assets/workshop-OLD.js')()).resolves.toEqual({ core: 'In.' });
    expect(fetch).not.toHaveBeenCalled();
  });
});
