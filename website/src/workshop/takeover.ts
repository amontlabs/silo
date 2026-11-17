// The script takes over the page in one synchronous step: what the build drew stays until the first render is in
// the document, and both are swapped within one task, so no frame shows an empty root or two sign-in buttons.
import type { ReactNode } from 'react';
import { flushSync } from 'react-dom';
import { createRoot } from 'react-dom/client';

export function takeOver(root: HTMLElement, slot: Element | null, page: ReactNode) {
  // The account the build drew; the App's own goes after it in the slot (a portal adds to what is there).
  const drawn = slot?.hasAttribute('data-prerendered') ? [...slot.childNodes] : [];
  // React empties the root when it commits its first render; flushSync commits it before this returns.
  flushSync(() => createRoot(root).render(page));
  // Kept when the page failed and nothing replaced it: the sign-in link still works without the script.
  if (slot && slot.childNodes.length > drawn.length) for (const node of drawn) node.remove();
  root.removeAttribute('data-prerendered');
  slot?.removeAttribute('data-prerendered');
  // The address's own view is now drawn: nothing the build drew is held hidden any more (workshop/index.html).
  document.documentElement.classList.remove('ws-hold');
}
