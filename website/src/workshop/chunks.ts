// The parts of the page loaded later (notes, history, what the entries say of the lists). Each deployment names
// its chunks after their content, and the site serves only the newest, so a page opened before a deployment asks
// for chunks that no longer exist. A reload is then the only way to them.

/** A chunk is gone because the site was deployed again after this page opened. */
export class SiteChanged extends Error {
  constructor() {
    super('The Workshop has changed since this page opened.');
  }
}

/** Whether the page the site serves now loads another script than `script`, the one this page runs. */
async function siteChanged(script: string): Promise<boolean> {
  try {
    const response = await fetch(location.pathname, { cache: 'no-store' });
    if (!response.ok) return false;
    return !(await response.text()).includes(script);
  } catch {
    return false;
  }
}

/** `load`, whose failure becomes `SiteChanged` when the site now serves another build than `script`'s. */
export function chunk<T>(load: () => Promise<T>, script: string): () => Promise<T> {
  return () => load().catch(async (error: unknown) => {
    throw (await siteChanged(script)) ? new SiteChanged() : error;
  });
}
