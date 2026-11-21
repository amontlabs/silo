// The films: their line above the map or the tiles, a film's frame, the film shown first in an opened row, and a
// film's place. A film plays in a frame of its own page, /films/<id>/, which starts paused on its own play control; the
// frame loads only once its place or its row is open, with a placeholder of its exact size until it has loaded.
import { useEffect, useRef, useState } from 'react';
import type { FilmItem } from './plan';

/** The page's small icons, 14 px, drawn in the current colour. */
export const ICONS = {
  filter: <path d="M2 4h10M4 7h6M6 10h2" />,
  display: <><path d="M2 4.5h4.4M10.6 4.5H12M2 9.5h1.4M7.6 9.5H12" /><circle cx="8.5" cy="4.5" r="1.6" /><circle cx="5.5" cy="9.5" r="1.6" /></>,
  search: <><circle cx="6.2" cy="6.2" r="3.7" /><path d="M9 9l3 3" /></>,
  x: <path d="M4 4l6 6M10 4l-6 6" />,
  film: <><rect x="1.5" y="2.5" width="11" height="9" rx="1.5" /><path className="solid" d="M5.8 4.9v4.2L9.3 7z" /></>,
  chev: <path d="M5.5 3.5 9 7l-3.5 3.5" />,
};

export function Icon({ name }: { name: keyof typeof ICONS }) {
  return <svg className="wa-i" viewBox="0 0 14 14" aria-hidden="true">{ICONS[name]}</svg>;
}

/** A film is behind when an entry it shows changed in what it says after the film did. */
export const isBehind = (film: FilmItem): boolean => film.behind.length > 0;
export const FILM_TIP = 'Has a film';
export const BEHIND_TIP = 'Film is behind the plan';

/**
 * The film's page in a frame (`?in=workshop`), at the size its box gives it. The page opens paused on a picture of the
 * film and says how tall it is; the box follows. Until the page has loaded, a placeholder holds the box at the size the
 * page will take: the stage alone in a small frame, the stage with its caption, subtitle and bar at the player's size.
 */
export function FilmFrame({ film }: { film: FilmItem }) {
  const [loaded, setLoaded] = useState(false);
  const [height, setHeight] = useState<number | null>(null);
  const frame = useRef<HTMLIFrameElement>(null);
  useEffect(() => {
    const listener = (event: MessageEvent) => {
      // Only the frame this page made, on the site's own origin, speaking for this film's own page.
      if (!frame.current || event.source !== frame.current.contentWindow || event.origin !== location.origin) return;
      const data = event.data as { film?: unknown; height?: unknown } | null;
      if (data?.film !== new URL(frame.current.src, location.href).pathname) return;
      const told = data.height;
      if (typeof told === 'number' && told > 0 && told < 4000) setHeight(Math.ceil(told));
    };
    addEventListener('message', listener);
    return () => removeEventListener('message', listener);
  }, []);
  return (
    <div className={loaded ? 'wa-poster loaded' : 'wa-poster'} aria-busy={loaded ? undefined : true} style={height ? { height, paddingTop: 0 } : undefined}>
      {loaded ? null : <span className="wa-ph wa-ph-film" aria-hidden="true" />}
      <iframe ref={frame} src={`/films/${film.id}/?in=workshop`} title={film.title} onLoad={() => setLoaded(true)} allow="fullscreen" allowFullScreen />
    </div>
  );
}

/** What an opened row shows first when a film shows its entry: the film, then its title, its line and how many entries it shows. */
export function FilmBox({ film, onEntries }: { film: FilmItem; onEntries: () => void }) {
  return (
    <figure className="wa-filmbox">
      {film.playable ? <FilmFrame film={film} /> : null}
      <figcaption>
        <b>{film.title}</b>
        <span>{film.line}</span>
        <small>
          {isBehind(film) ? <><u>behind</u> · </> : null}
          <button type="button" className="ws-link" onClick={onEntries}>{film.rows.length} entries</button>
        </small>
      </figcaption>
    </figure>
  );
}

export interface FilmsLineProps {
  films: FilmItem[];
  /** The film whose place is open: its control stays pressed. */
  current: string | null;
  onOpen: (film: string) => void;
  /** The film pointed at or focused, whose parts the map marks; null when none is. */
  onPoint: (film: string | null) => void;
}

/** One control per film, in the order films/README.md gives, on the line between the title row and the map or the tiles. */
export function FilmsLine({ films, current, onOpen, onPoint }: FilmsLineProps) {
  if (!films.length) return null;
  return (
    <div className="wa-films" role="toolbar" aria-label="Films" onPointerLeave={() => onPoint(null)}>
      {films.map((film) => <FilmControl key={film.id} film={film} pressed={current === film.id} onOpen={onOpen} onPoint={onPoint} />)}
    </div>
  );
}

export function FilmControl({ film, pressed, onOpen, onPoint }: { film: FilmItem; pressed: boolean; onOpen: (film: string) => void; onPoint?: (film: string | null) => void }) {
  const behind = isBehind(film);
  return (
    <button
      type="button" className={behind ? 'wa-ctl film behind' : 'wa-ctl film'} aria-pressed={pressed}
      data-tip={behind ? BEHIND_TIP : undefined}
      aria-label={behind ? `${film.title}, ${BEHIND_TIP.toLowerCase()}` : undefined}
      data-film={film.id} onClick={() => onOpen(film.id)}
      onPointerEnter={() => onPoint?.(film.id)} onPointerLeave={() => onPoint?.(null)}
      onFocus={() => onPoint?.(film.id)} onBlur={() => onPoint?.(null)}
    >
      <Icon name="film" /><span>{film.title}</span>
    </button>
  );
}
