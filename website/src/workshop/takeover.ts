// The script takes over the page in one synchronous step: what the build drew stays until the first render is in
// the document, and both are swapped within one task, so no frame shows an empty root or two sign-in buttons.
import type { ReactNode } from 'react';
import { flushSync } from 'react-dom';
import { createRoot } from 'react-dom/client';
import type { Root } from 'react-dom/client';

/** Renders the page into `root` and returns React's root, which a caller may unmount. */
export function takeOver(root: HTMLElement, slot: Element | null, page: ReactNode): Root {
  // The account the build drew; the App's own goes after it in the slot (a portal adds to what is there).
  const drawn = slot?.hasAttribute('data-prerendered') ? [...slot.childNodes] : [];
  // React empties the root when it commits its first render; flushSync commits it before this returns.
  const react = createRoot(root);
  flushSync(() => react.render(page));
  root.removeAttribute('data-prerendered');
  // The address's own view is now drawn: nothing the build drew is held hidden any more (workshop/index.html).
  document.documentElement.classList.remove('ws-hold');
  if (!slot) return react;
  if (slot.childNodes.length > drawn.length) {
    for (const node of drawn) node.remove();
    slot.removeAttribute('data-prerendered');
  } else if (drawn.length) {
    // The script gave the page no account (it failed to render, under the plain list): the page keeps the drawn
    // sign-in, shown with or without the session marker, as when the script never came (ws-bare, workshop.css).
    document.documentElement.classList.add('ws-bare');
  }
  return react;
}
