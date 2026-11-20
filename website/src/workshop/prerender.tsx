// The Workshop's markup for the plain address, written into the built page so it is drawn before its script
// arrives: what the page renders with nothing remembered, nobody signed in and the load pending. Runs at build
// time, without a browser (scripts/prerender.mjs), and in the tests that compare it with the first render.
import { renderToString } from 'react-dom/server';
import { Account, App } from './App';
import { loginUrl } from './api';
import { Boundary } from './Boundary';
import { NoScript } from './NoScript';
import type { BuildData } from './plan';

export interface Prerendered {
  /** What goes into `#workshop-root`. */
  root: string;
  /** What goes into the header's account slot: sign-in, as for a visitor nobody knows. */
  account: string;
  /** What goes into the page's <noscript>: the line that says what needs JavaScript, and every part's rows as a plain list. */
  noscript: string;
}

export function prerender(data: BuildData): Prerendered {
  return {
    root: renderToString(<Boundary data={data}><App data={data} accountSlot={false} /></Boundary>),
    account: renderToString(<Account me={null} signIn={loginUrl('')} />),
    noscript: renderToString(<NoScript data={data} />),
  };
}

const ROOT = '<div id="workshop-root"></div>';
const SLOT = '<div class="ws-account-slot" id="workshop-account"></div>';
/** The page's <noscript>, which holds the line alone until the build writes the list into it. */
const NOSCRIPT = /<noscript id="workshop-noscript">[\s\S]*?<\/noscript>/g;

/**
 * The built page with the markup in place. Both containers are marked `data-prerendered` until the script has
 * taken over (see takeover.ts and workshop.css). Throws when the page no longer has the empty containers.
 */
export function inject(html: string, parts: Prerendered): string {
  if (html.split(ROOT).length !== 2 || html.split(SLOT).length !== 2 || (html.match(NOSCRIPT) ?? []).length !== 1) {
    throw new Error(`the Workshop page must hold ${ROOT}, ${SLOT} and <noscript id="workshop-noscript"> once each`);
  }
  return html
    .replace(NOSCRIPT, () => `<noscript id="workshop-noscript">${parts.noscript}</noscript>`)
    .replace(ROOT, () => `<div id="workshop-root" data-prerendered>${parts.root}</div>`)
    .replace(SLOT, () => `<div class="ws-account-slot" id="workshop-account" data-prerendered>${parts.account}</div>`);
}
