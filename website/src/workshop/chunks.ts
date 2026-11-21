// The parts of the page loaded later (notes, history, what the entries say of the lists). A chunk's name is made from
// its content, and the site serves only the newest build's files, so a page opened before a deployment that changed a
// chunk asks for one that no longer exists. A reload is then the only way to it.
//
// Whether the site changed is read from a mark made for it: the build writes one id into the page (a `meta` named
// BUILD_META) and into the script (`__WORKSHOP_BUILD__`, vite.config.ts). The dev server writes none.

/** The name of the page's `meta` that holds its build's id. */
export const BUILD_META = 'workshop-build';

/** How long the probe waits for the page the site serves now before it counts as a failure. */
export const probeConfig = { ms: 8000 };

/** A chunk is gone because the site was deployed again after this page opened. */
export class SiteChanged extends Error {
  constructor() {
    super('The Workshop has changed since this page opened.');
  }
}

/**
 * What the page the site serves now says of the build: the same as `build`, another one, or nothing to go by (no id
 * here, no answer in time, an answer that is not the Workshop's page or carries no id).
 */
export type Probe = 'same' | 'changed' | 'failed';

/** The build id a page's markup carries, exactly as written, or null when it carries none. */
export function buildOf(html: string): string | null {
  const meta = new DOMParser().parseFromString(html, 'text/html').querySelector(`meta[name="${BUILD_META}"]`);
  const id = meta?.getAttribute('content');
  return id ? id : null;
}

/**
 * Asks the site for this page's address again, its query included, and compares the build id of its answer with
 * `build`, this page's. A redirect is not followed: whatever it leads to (a sign-in, a gateway) is not this page.
 */
export async function probe(build: string): Promise<Probe> {
  // The dev server writes no id: nothing there is ever called changed.
  if (!build) return 'failed';
  const abort = new AbortController();
  let timer: ReturnType<typeof setTimeout> | undefined;
  const late = new Promise<Probe>((resolve) => {
    timer = setTimeout(() => {
      abort.abort();
      resolve('failed');
    }, probeConfig.ms);
  });
  const asked = (async (): Promise<Probe> => {
    const response = await fetch(`${location.pathname}${location.search}`, { cache: 'no-store', credentials: 'same-origin', redirect: 'manual', signal: abort.signal });
    if (!response.ok || response.redirected || response.type === 'opaqueredirect' || !(response.headers.get('Content-Type') ?? '').includes('text/html')) return 'failed';
    const id = buildOf(await response.text());
    if (id === null) return 'failed';
    return id === build ? 'same' : 'changed';
  })().catch((): Probe => 'failed');
  try {
    return await Promise.race([asked, late]);
  } finally {
    clearTimeout(timer);
  }
}

/** `load`, whose failure becomes `SiteChanged` when the site now serves another build than `build`. */
export function chunk<T>(load: () => Promise<T>, build: string): () => Promise<T> {
  return () => load().catch(async (error: unknown) => {
    throw (await probe(build)) === 'changed' ? new SiteChanged() : error;
  });
}
