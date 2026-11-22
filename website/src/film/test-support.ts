// What the film's tests need and jsdom lacks: the home page as it is served, and SVG that can be measured.

import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const read = (file: string) => readFileSync(join(process.cwd(), file), 'utf8');

/** The home page as it is served: index.html with the film's still in it. */
export const homePage = () => read('index.html').replace('<!-- film still -->', () => read('src/film/still.html').trim());

/** Every character is 8 units wide, which is near enough for what is not layout. `npm run film-check` has the layout. */
export function measurableSvg() {
  const advance = 8;
  Object.assign(SVGElement.prototype, {
    getComputedTextLength(this: SVGElement) {
      return (this.textContent ?? '').length * advance;
    },
    getBBox(this: SVGElement) {
      const width = (this.textContent ?? '').length * advance;
      const anchor = this.getAttribute('text-anchor');
      const x = Number(this.getAttribute('x') ?? 0) - (anchor === 'middle' ? width / 2 : anchor === 'end' ? width : 0);
      return { x, y: Number(this.getAttribute('y') ?? 0) - 14, width, height: 18 };
    },
    getTotalLength: () => 100,
  });
}

/** What can be seen of `el`, as text. What is hidden or clear is left out: its position does not matter. */
export function seen(el: Element): string {
  const { style } = el as SVGElement;
  if (style?.visibility === 'hidden' || (style?.opacity && Number(style.opacity) < 0.003)) return '';
  const drawn = [...el.children].map(seen).join('');
  if (el.children.length) return drawn && `<${el.tagName} ${el.getAttribute('transform') ?? ''} ${style.opacity}>${drawn}</>`;
  if (el.tagName === 'g' || (el.tagName === 'text' && !el.textContent)) return '';
  return `<${el.tagName} ${[...el.attributes].map(({ name, value }) => `${name}=${value}`).join(' ')}>${el.textContent}</>`;
}
