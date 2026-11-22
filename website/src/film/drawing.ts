// The film drawn in one of its layouts: the whole window, or on a phone the canvas through a camera with the
// place over it and the record under it.

import { VIEW, viewAt, wordsInView } from './camera';
import { PHONE_DOCK, WINDOW_DOCK, buildPlace, buildRecord, buildSidebar, type Shown } from './chrome';
import { Reel } from './reel';
import { drawShots } from './shots';
import type { Key } from './story';
import { E, G } from './svg';
import { easeOut } from './time';

/** `narrow` is the window with its smallest words drawn a unit larger (film.css), measured as they will show. */
export type Layout = 'window' | 'narrow' | 'phone';

export interface Drawing {
  reel: Reel;
  render(t: number, shown: Shown): void;
  /** The canvas as it looks now, put over it to fade as a jump crosses over. */
  ghost(): SVGGElement;
}

const part = <El extends Element>(figure: Element, selector: string) => figure.querySelector<El>(selector)!;

function drawWindow(figure: Element, go: (key: Key) => void, reduced: boolean): Drawing {
  const stage = part<SVGSVGElement>(figure, '.film-window');
  stage.replaceChildren();
  // The still is a picture. Drawn, the window holds the sidebar's buttons.
  stage.removeAttribute('aria-hidden');
  stage.setAttribute('role', 'group');
  stage.setAttribute('aria-label', "The film's places and computers");
  E('rect', { x: 0.5, y: 0.5, width: 1119, height: 479, rx: 4, class: 'f-win' }, stage);
  E('line', { x1: 1, y1: 30.5, x2: 1119, y2: 30.5, class: 'f-hair' }, stage);
  E('line', { x1: 132.5, y1: 31, x2: 132.5, y2: 479, class: 'f-hair f-divide' }, stage);
  E('line', { x1: 133, y1: 444.5, x2: 1119, y2: 444.5, class: 'f-hair' }, stage);
  // Title bar: the window's three lights and Silo's mark.
  for (let i = 0; i < 3; i++) E('circle', { cx: 16 + i * 15, cy: 15.5, r: 4.5, class: 'f-hair' }, stage);
  E('use', { href: '#silo', x: 551, y: 7, width: 17, height: 17, class: 'f-logo' }, stage);
  const side = G(stage, { class: 'f-side' });
  const canvas = G(stage, { class: 'f-canvas', transform: 'translate(132 30)', 'aria-hidden': 'true' });

  const reel = new Reel(canvas);
  drawShots(reel);
  const sidebar = buildSidebar(side, reel, go, reduced);
  const record = G(stage, { 'aria-hidden': 'true' });
  const dock = buildRecord(record, reel, WINDOW_DOCK);
  return {
    reel,
    render(t, shown) {
      for (const cue of reel.cues) cue(t);
      sidebar(t, shown);
      dock(t, shown);
      canvas.style.opacity = String(shown.jump ? easeOut(shown.jump.u) : 1);
    },
    ghost() {
      const copy = canvas.cloneNode(true) as SVGGElement;
      copy.style.opacity = '1';
      stage.insertBefore(copy, record);
      return copy;
    },
  };
}

function drawPhone(figure: Element): Drawing {
  const view = part<SVGSVGElement>(figure, '.film-view');
  const band = part<SVGSVGElement>(figure, '.film-dock');
  view.replaceChildren();
  band.replaceChildren();
  const canvas = G(view, { class: 'f-canvas' });
  const reel = new Reel(canvas, true);
  drawShots(reel);
  const words = wordsInView(canvas, reel.typed);
  const place = buildPlace(part<HTMLElement>(figure, '.film-place'), reel);

  // Past the camera's own width, a wider screen sees more of the canvas at its natural size.
  const width = Math.max(VIEW, view.clientWidth);
  view.setAttribute('viewBox', `0 8 ${width} 406`);
  band.setAttribute('viewBox', `0 0 ${width} 52`);
  const dock = buildRecord(G(band), reel, { ...PHONE_DOCK, right: width - 16 });
  return {
    reel,
    render(t, shown) {
      for (const cue of reel.cues) cue(t);
      const left = viewAt(t, width);
      canvas.setAttribute('transform', `translate(${(-left).toFixed(2)} 0)`);
      words(left, width);
      place(t, shown);
      dock(t, shown);
      canvas.style.opacity = String(shown.jump ? easeOut(shown.jump.u) : 1);
    },
    ghost() {
      const copy = canvas.cloneNode(true) as SVGGElement;
      copy.style.opacity = '1';
      view.appendChild(copy);
      return copy;
    },
  };
}

export const drawFilm = (figure: Element, layout: Layout, go: (key: Key) => void, reduced: boolean): Drawing =>
  layout === 'phone' ? drawPhone(figure) : drawWindow(figure, go, reduced);
