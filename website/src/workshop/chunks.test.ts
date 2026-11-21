// A chunk that fails to load asks the site for the page it serves now and compares that page's build id with its own:
// another id is a change; the same id, an answer with no id, one that is not the Workshop's page, or none in time is
// a failure, and the chunk's own error stays.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { buildOf, chunk, probe, probeConfig, SiteChanged } from './chunks';

const page = (id: string | null) => `<!doctype html><html><head>${id === null ? '' : `<meta name="workshop-build" content="${id}">`}<title>Workshop</title></head><body><div id="workshop-root"></div></body></html>`;
const html = (body: string, status = 200) => new Response(body, { status, headers: { 'Content-Type': 'text/html; charset=utf-8' } });
const answering = (respond: () => Promise<Response>) => {
  const fetch = vi.fn(respond);
  vi.stubGlobal('fetch', fetch);
  return fetch;
};
const failing = () => Promise.reject(new Error('404'));

beforeEach(() => {
  probeConfig.ms = 8000;
  history.replaceState(null, '', '/workshop#part-a/core');
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('the probe', () => {
  it('answers same for a page of this build, asking this address and its query with no cache and no redirect', async () => {
    history.replaceState(null, '', '/workshop?view=all&state=undecided#part-a/core');
    const fetch = answering(async () => html(page('build-a')));
    expect(await probe('build-a')).toBe('same');
    expect(fetch).toHaveBeenCalledWith('/workshop?view=all&state=undecided', expect.objectContaining({ cache: 'no-store', redirect: 'manual' }));
  });

  it('answers failed for a redirect, wherever it leads', async () => {
    // Refused, as a browser gives it with `redirect: 'manual'`.
    answering(async () => ({ ok: false, status: 0, type: 'opaqueredirect', redirected: false, headers: new Headers(), text: async () => '' }) as unknown as Response);
    expect(await probe('build-a')).toBe('failed');
    // Followed anyway, to a page of another build.
    answering(async () => {
      const followed = html(page('build-b'));
      Object.defineProperty(followed, 'redirected', { value: true });
      return followed;
    });
    expect(await probe('build-a')).toBe('failed');
  });

  it('compares the ids exactly as written', async () => {
    answering(async () => html(page(' build-a ')));
    expect(await probe('build-a')).toBe('changed');
    answering(async () => html(page('BUILD-A')));
    expect(await probe('build-a')).toBe('changed');
  });

  it('answers changed for a page of another build', async () => {
    answering(async () => html(page('build-b')));
    expect(await probe('build-a')).toBe('changed');
  });

  it('answers failed for a page with no id, even one that names nothing of this build', async () => {
    answering(async () => html(page(null)));
    expect(await probe('build-a')).toBe('failed');
  });

  it('answers failed for a sign-in or gateway page, whether it answers 200 or 401', async () => {
    const signIn = '<!doctype html><html><head><title>Sign in</title></head><body><form action="/login"><input name="password"></form></body></html>';
    answering(async () => html(signIn));
    expect(await probe('build-a')).toBe('failed');
    answering(async () => html(signIn, 401));
    expect(await probe('build-a')).toBe('failed');
    // A page of another build that is not served as a page is not the Workshop's page either.
    answering(async () => new Response(page('build-b'), { headers: { 'Content-Type': 'text/plain' } }));
    expect(await probe('build-a')).toBe('failed');
  });

  it('answers failed when the network fails', async () => {
    answering(() => Promise.reject(new TypeError('Failed to fetch')));
    expect(await probe('build-a')).toBe('failed');
  });

  it('answers failed once its limit has passed when the answer never comes, and stops the request', async () => {
    vi.useFakeTimers();
    let signal: AbortSignal | undefined;
    const fetch = vi.fn((_url: string, init?: RequestInit) => {
      signal = init?.signal ?? undefined;
      return new Promise<Response>(() => {});
    });
    vi.stubGlobal('fetch', fetch);
    let answer: string | null = null;
    void probe('build-a').then((probed) => { answer = probed; });
    await vi.advanceTimersByTimeAsync(probeConfig.ms - 1);
    expect(answer).toBeNull();
    await vi.advanceTimersByTimeAsync(1);
    expect(answer).toBe('failed');
    expect(signal?.aborted).toBe(true);
  });

  it('answers failed on the dev server, which has no id, without asking anything', async () => {
    // The dev server and the tests have no id (vite.config.ts); the built page and script have one.
    expect(__WORKSHOP_BUILD__).toBe('');
    const fetch = answering(async () => html(page('build-b')));
    expect(await probe(__WORKSHOP_BUILD__)).toBe('failed');
    expect(fetch).not.toHaveBeenCalled();
  });

  it('reads the id from the meta of that name only', () => {
    expect(buildOf(page('build-a'))).toBe('build-a');
    expect(buildOf('<p>workshop-build build-a</p><meta name="other" content="build-a">')).toBeNull();
    expect(buildOf('<meta name="workshop-build" content="">')).toBeNull();
    expect(buildOf('<meta name="workshop-build" content=" build-a ">')).toBe(' build-a ');
  });
});

describe('a chunk that fails to load', () => {
  it('is SiteChanged when the page served now is of another build', async () => {
    answering(async () => html(page('build-b')));
    await expect(chunk(failing, 'build-a')()).rejects.toBeInstanceOf(SiteChanged);
  });

  it('keeps its own failure when the page served now is of the same build', async () => {
    const fetch = answering(async () => html(page('build-a')));
    const error = await chunk(failing, 'build-a')().catch((caught: unknown) => caught);
    expect(error).not.toBeInstanceOf(SiteChanged);
    expect(error).toEqual(new Error('404'));
    // The probe did run: it is what said the build is the same.
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it('keeps its own failure when the probe fails', async () => {
    const fetch = answering(() => Promise.reject(new TypeError('offline')));
    const error = await chunk(failing, 'build-a')().catch((caught: unknown) => caught);
    expect(error).not.toBeInstanceOf(SiteChanged);
    expect(error).toEqual(new Error('404'));
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it('asks nothing of the site when it loads', async () => {
    const fetch = answering(async () => html(page('build-b')));
    await expect(chunk(() => Promise.resolve({ core: 'In.' }), 'build-a')()).resolves.toEqual({ core: 'In.' });
    expect(fetch).not.toHaveBeenCalled();
  });
});
