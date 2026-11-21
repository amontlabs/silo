// The tooltips of the marks and the controls: what an element's `data-tip` says (and its `data-kbd`, a key), shown under
// it on pointing and on keyboard focus; a name cut by an ellipsis (`data-cut`) shows whole. No tooltip on a touch.
// One tooltip for the page, placed by hand, so no row renders anything to have one.
import { useEffect, useLayoutEffect, useRef, useState } from 'react';

interface Tip {
  text: string;
  kbd: string | null;
  x: number;
  top: number;
  bottom: number;
  beside: boolean;
  right: number;
}

/** How long the pointer rests on a mark before its tooltip shows. */
const REST_MS = 240;

function tipOf(node: Element): Tip | null {
  const target = node.closest<HTMLElement>('[data-tip], [data-cut]');
  if (!target) return null;
  let text = target.dataset.tip ?? '';
  if (!text && target.dataset.cut !== undefined) {
    // Only a name the ellipsis cuts.
    if (target.scrollWidth <= target.clientWidth + 1 && target.scrollHeight <= target.clientHeight + 1) return null;
    text = target.textContent ?? '';
  }
  if (!text) return null;
  const box = target.getBoundingClientRect();
  return { text, kbd: target.dataset.kbd ?? null, x: box.left + box.width / 2, top: box.top, bottom: box.bottom, beside: target.classList.contains('wa-film'), right: box.right };
}

export function Tips() {
  const [tip, setTip] = useState<Tip | null>(null);
  const node = useRef<HTMLDivElement>(null);
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    let over: Element | null = null;
    const hide = () => {
      clearTimeout(timer);
      setTip(null);
    };
    const onOver = (event: PointerEvent) => {
      if (event.pointerType === 'touch') return;
      const target = (event.target as Element | null)?.closest?.('[data-tip], [data-cut]') ?? null;
      if (target === over) return;
      over = target;
      hide();
      if (!target) return;
      timer = setTimeout(() => setTip(tipOf(target)), REST_MS);
    };
    const onFocus = (event: FocusEvent) => {
      const target = event.target as Element | null;
      if (!target || !(target instanceof HTMLElement) || !target.matches(':focus-visible')) return;
      if (!target.matches('[data-tip], [data-cut]')) return;
      hide();
      setTip(tipOf(target));
    };
    const onKey = (event: KeyboardEvent) => { if (event.key === 'Escape') hide(); };
    document.addEventListener('pointerover', onOver);
    document.addEventListener('focusin', onFocus);
    document.addEventListener('focusout', hide);
    document.addEventListener('scroll', hide, { passive: true, capture: true });
    document.addEventListener('keydown', onKey);
    document.addEventListener('pointerdown', hide);
    return () => {
      clearTimeout(timer);
      document.removeEventListener('pointerover', onOver);
      document.removeEventListener('focusin', onFocus);
      document.removeEventListener('focusout', hide);
      document.removeEventListener('scroll', hide, { capture: true });
      document.removeEventListener('keydown', onKey);
      document.removeEventListener('pointerdown', hide);
    };
  }, []);
  // Under its element, or beside it for a film mark, and kept 8 px inside the window.
  useLayoutEffect(() => {
    const box = node.current;
    if (!box || !tip) return;
    const { offsetWidth: width, offsetHeight: height } = box;
    const left = tip.beside ? tip.right + 6 : Math.max(8, Math.min(innerWidth - width - 8, tip.x - width / 2));
    let top = tip.beside ? (tip.top + tip.bottom) / 2 - height / 2 : tip.bottom + 6;
    if (top + height > innerHeight - 8) top = tip.top - height - 6;
    box.style.transform = `translate(${Math.round(left)}px, ${Math.round(top)}px)`;
  }, [tip]);
  if (!tip) return null;
  return (
    <div className="ws-tip wa-tip" ref={node} aria-hidden="true">
      {tip.text}{tip.kbd ? <span className="wa-kbd">{tip.kbd}</span> : null}
    </div>
  );
}
