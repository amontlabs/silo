// One row of a list, and what it shows once open: the text, its notes, its state and, for a row with a thread,
// the thread (comments and history lines) and the box. A row without a thread draws none of those, not even a placeholder.
// Opening never moves the row or the rows above: the open frame is an overlay and the body grows below.
import { Fragment, useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { hasSessionHint } from './api';
import { compareLink, featureFileAt } from '../../../tools/workshop/model/index.js';
import { Markdown } from './Markdown';
import { RichText, safeHref } from './RichText';
import { changesText, changeWords, isDecided, relativeTime, shortDate } from './plan';
import type { Change, Row } from './plan';
import { formatHash, placeOf } from './routes';
import type { Detail, Workshop } from './useWorkshop';

/** How long the closing body stays in place for its animation. */
const CLOSE_MS = 240;
/** How long an opening panel waits for its comments, so it opens at its final size. */
export const HOLD_MS = 180;
/** How long a panel takes to open, and to follow its comments. */
const GROW_MS = 220;

export interface EntryContext {
  w: Workshop;
  signIn: string;
  onToggle: (row: Row) => void;
  /** The rows open now. */
  open: ReadonlySet<string>;
}

const WORDS: Partial<Record<Row['status'], string>> = { planned: 'planned', built: 'built', dropped: 'dropped' };
/** A decision's word on its row: taken is decided; open or answered says nothing, as an undecided feature does. */
const DECISION_WORDS: Partial<Record<Row['status'], string>> = { planned: 'decided', replaced: 'replaced', dropped: 'dropped' };

export function pipClass(row: Row): string {
  return `ws-pip ${row.status}`;
}

function initials(login: string): string {
  const parts = login.split(/[-_.]+/).filter(Boolean);
  return (parts.length > 1 ? parts[0][0] + parts[1][0] : login.slice(0, 2)).toLowerCase();
}

export function stateText(row: Row): string {
  if (row.of === 'decision') {
    return row.status === 'planned' ? 'Decided.' : row.status === 'replaced' ? 'Replaced.' : row.status === 'dropped' ? 'Dropped.' : 'Not decided yet.';
  }
  switch (row.status) {
    case 'planned': return 'Planned.';
    case 'built': {
      const day = row.built ?? row.closedAt;
      return day ? `Built, ${shortDate(day)}.` : 'Built.';
    }
    case 'dropped': return 'Dropped.';
    default: return 'Not decided yet.';
  }
}

export function Entry({ row, open, ctx }: { row: Row; open: boolean; ctx: EntryContext }) {
  const { w } = ctx;
  // The body stays mounted while it closes, so the close animates too.
  const [closing, setClosing] = useState(false);
  const [wasOpen, setWasOpen] = useState(open);
  // The row shows itself open at once; its panel grows when the comments are in, or after HOLD_MS.
  const detail = row.issue === null ? undefined : w.details[row.issue];
  const ready = row.issue === null || row.issue <= 0 || !w.liveOk || typeof detail === 'object' || detail === 'error';
  const [held, setHeld] = useState(false);
  if (wasOpen !== open) {
    setWasOpen(open);
    setClosing(!open);
    setHeld(open && !ready);
  }
  if (held && ready) setHeld(false);
  useEffect(() => {
    if (!closing) return;
    const timer = setTimeout(() => setClosing(false), CLOSE_MS);
    return () => clearTimeout(timer);
  }, [closing]);
  useEffect(() => {
    if (!held) return;
    const timer = setTimeout(() => setHeld(false), HOLD_MS);
    return () => clearTimeout(timer);
  }, [held]);
  const grown = open && !held;

  // Comments that arrive while the panel is opening wait, on their placeholders, until the opening has ended.
  const [wasGrown, setWasGrown] = useState(grown);
  const [opening, setOpening] = useState<{ detail: typeof detail } | null>(null);
  if (wasGrown !== grown) {
    setWasGrown(grown);
    // Only a panel that opens on placeholders waits; one that opens on its comments shows every change at once.
    setOpening(grown && typeof detail !== 'object' ? { detail } : null);
  }
  useEffect(() => {
    if (!opening) return;
    const timer = setTimeout(() => setOpening(null), GROW_MS);
    return () => clearTimeout(timer);
  }, [opening]);
  const shown = opening ? opening.detail : detail;

  // When the comments arrive in a panel already open, it grows from its old height to the new one instead
  // of jumping; this also covers rows that appear open (from another place, Back and Forward).
  const inner = useRef<HTMLDivElement>(null);
  const lastHeight = useRef(0);
  const lastShown = useRef(shown);
  const settling = useRef<ReturnType<typeof setTimeout>>(undefined);
  // The panel's height as last laid out, kept current while it animates open (no render happens then).
  const animating = useRef(false);
  useEffect(() => {
    const node = inner.current;
    if (!node || typeof ResizeObserver !== 'function') return;
    const observer = new ResizeObserver(() => { if (!animating.current) lastHeight.current = node.getBoundingClientRect().height; });
    observer.observe(node);
    return () => observer.disconnect();
  }, []);
  useLayoutEffect(() => {
    const node = inner.current;
    if (!node) return;
    const changed = lastShown.current !== shown;
    lastShown.current = shown;
    if (animating.current && !changed) return;
    const still = typeof matchMedia !== 'function' || matchMedia('(prefers-reduced-motion: reduce)').matches;
    // From the height on screen (mid-change, if one runs) to the panel's natural height, measured without it.
    const from = animating.current ? node.getBoundingClientRect().height : lastHeight.current;
    node.style.transition = 'none';
    node.style.height = '';
    const to = node.getBoundingClientRect().height;
    if (!animating.current) lastHeight.current = to;
    if (!changed || !grown || still || from <= 0 || Math.abs(to - from) < 1) {
      node.style.transition = '';
      animating.current = false;
      return;
    }
    animating.current = true;
    node.style.height = `${from}px`;
    node.getBoundingClientRect();
    node.style.transition = 'height .2s cubic-bezier(.45, 0, .25, 1), opacity .18s';
    node.style.height = `${to}px`;
    const done = () => {
      node.style.height = '';
      node.style.transition = '';
      animating.current = false;
      lastHeight.current = node.getBoundingClientRect().height;
    };
    clearTimeout(settling.current);
    settling.current = setTimeout(done, GROW_MS + 40);
  });
  useEffect(() => () => clearTimeout(settling.current), []);

  const word = (row.of === 'decision' ? DECISION_WORDS : WORDS)[row.status];
  const quiet = !isDecided(row);
  const classes = ['ws-item', `st-${row.status}`, open ? 'open' : '', quiet ? 'quiet' : ''].filter(Boolean).join(' ');
  return (
    <li className={classes} id={row.key}>
      <div className="ws-line">
        <button type="button" className="ws-row" aria-expanded={open} aria-controls={`${row.key}-more`} onClick={() => ctx.onToggle(row)} onPointerEnter={() => w.prefetch(row)} onPointerDown={() => w.prefetch(row)} onTouchStart={() => w.prefetch(row)} onFocus={() => w.prefetch(row)}>
          <i className={pipClass(row)} aria-hidden="true" />
          <span className="ws-name">{row.name}</span>
          <b className="ws-word">{word ?? ''}</b>
        </button>
        <Side row={row} ctx={ctx} />
      </div>
      <div className={grown ? 'ws-more open' : 'ws-more'} id={`${row.key}-more`} aria-hidden={open ? undefined : true} inert={!open}>
        <div className="ws-more-in" ref={inner}>{open || closing ? <Body row={row} ctx={ctx} detail={shown} /> : null}</div>
      </div>
    </li>
  );
}

/** Whether a row has a thread on GitHub, or may have one once the live state answers: a suggestion, or an entry while entries have threads. */
export function threaded(row: Row, ctx: EntryContext): boolean {
  return row.kind === 'suggestion' || ctx.w.plan.threads;
}

function Vote({ row, ctx }: { row: Row; ctx: EntryContext }) {
  const { w } = ctx;
  if (!threaded(row, ctx)) return null;
  if (w.waiting) return <i className="ws-ph ws-ph-vote" aria-hidden="true" />;
  if (!w.liveOk || row.up === null) return null;
  const on = row.issue !== null && w.votes[row.issue] === true;
  const busy = row.issue !== null && w.votePending.has(row.issue);
  if (!w.me) return <a className="ws-vote ws-late-s" href={ctx.signIn} title="Sign in to vote" aria-label={`${row.up} votes, sign in to vote`}>👍 {row.up}</a>;
  return (
    <button type="button" className={`ws-vote ws-late-s${on ? ' on' : ''}${busy ? ' ws-pending' : ''}`} aria-pressed={on} aria-busy={busy} aria-label={`${row.up} votes`} onClick={() => void w.toggleVote(row)} disabled={busy || (row.issue ?? 0) <= 0}>
      👍 {row.up}
    </button>
  );
}

function Side({ row, ctx }: { row: Row; ctx: EntryContext }) {
  const { w } = ctx;
  const showComments = w.liveOk && row.comments !== null && row.comments > 0;
  return (
    <span className="ws-side">
      {row.kind === 'suggestion' && row.author ? <span className="ws-by">{row.author}</span> : null}
      <span className="ws-votes" aria-busy={(w.waiting && threaded(row, ctx)) || undefined}><Vote row={row} ctx={ctx} /></span>
      <span className="ws-talks">{showComments ? <span className="ws-late-s" aria-label={`${row.comments} comments`}>💬 {row.comments}</span> : null}</span>
    </span>
  );
}

function StateLine({ row }: { row: Row }) {
  const text = stateText(row);
  const tone = isDecided(row) && row.status !== 'dropped' ? 'ws-yes' : 'ws-muted';
  return <div className="ws-state"><span className={tone}>{text}</span></div>;
}

/**
 * The lines of a decision's open view that name other decisions: the one that replaced it, and those it replaces
 * (or would, while it has no verdict). A dropped decision names none of them.
 */
export function replaceLines(row: Row): { label: string; keys: string[] }[] {
  const lines: { label: string; keys: string[] }[] = [];
  if (row.replacedBy !== null) lines.push({ label: 'Replaced by', keys: [row.replacedBy] });
  if (row.replaces.length) lines.push({ label: row.status === 'undecided' ? 'Would replace' : 'Replaces', keys: row.replaces });
  return lines;
}

/** Each line links the question of each decision to where it lives; a plain click opens it here, a click with a modifier key is left to the browser. */
function ReplaceLines({ row, ctx }: { row: Row; ctx: EntryContext }) {
  const lines = replaceLines(row);
  if (!lines.length) return null;
  return (
    <>
      {lines.map((line) => (
        <p className="ws-replace" key={line.label}>
          <span>{line.label}</span>
          {line.keys.map((key) => ctx.w.rowByKey.get(key)).filter((other): other is Row => other !== undefined).map((other, index) => (
            <Fragment key={other.key}>
              {index ? ' · ' : ' '}
              <a
                href={formatHash(placeOf(other), other.key)}
                onClick={(event) => {
                  if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
                  event.preventDefault();
                  if (!ctx.open.has(other.key)) ctx.onToggle(other);
                }}
              >{other.name}</a>
            </Fragment>
          ))}
        </p>
      ))}
    </>
  );
}

/** Where the page reads a feature's file at a commit: raw.githubusercontent.com, or what the page names. */
function rawBase(): string | undefined {
  const named = document.querySelector<HTMLMetaElement>('meta[name="workshop-raw"]')?.content;
  return named && safeHref(named) ? named.replace(/\/+$/, '') : undefined;
}

type Texts = { name: string; what: string };
const TEXT_LIMIT = 2000;

async function textsAt(repo: string, commit: string, id: string): Promise<Texts> {
  const raw = rawBase();
  const response = await fetch(featureFileAt(commit, id, { repo, ...(raw ? { raw } : {}) }), { credentials: 'omit', cache: 'force-cache' });
  if (!response.ok) throw new Error('not available');
  const file = JSON.parse(await response.text()) as { name?: unknown; what?: unknown };
  if (typeof file.name !== 'string' || typeof file.what !== 'string') throw new Error('not available');
  return { name: file.name.slice(0, TEXT_LIMIT), what: file.what.slice(0, TEXT_LIMIT) };
}

/** A history line: when, what changed, and the change on GitHub. For the text, before and after on demand. */
function HistoryLine({ change, createdAt, row, repo }: { change: Change; createdAt: string; row: Row; repo: string }) {
  const [texts, setTexts] = useState<'closed' | 'loading' | 'error' | { before: Texts; after: Texts }>('closed');
  const words = changeWords(change);
  // A change with nothing to say (the old "later" verdict, `suggested_in`) draws no line.
  if (!words.length) return null;
  const id = row.key.startsWith('entry-') ? row.key.slice('entry-'.length) : null;
  const comparable = changesText(change) && id !== null;
  let link: string | null = null;
  try {
    link = safeHref(compareLink(change, { repo }));
  } catch {
    link = null;
  }
  const toggle = () => {
    if (texts !== 'closed') {
      setTexts('closed');
      return;
    }
    setTexts('loading');
    Promise.all([textsAt(repo, change.before, id as string), textsAt(repo, change.commit, id as string)]).then(
      ([before, after]) => setTexts({ before, after }),
      () => setTexts('error'),
    );
  };
  const open = texts !== 'closed';
  const panel = `${row.key}-${change.commit.slice(0, 12)}`;
  return (
    <div className="ws-h">
      <i aria-hidden="true" />
      <div>
        <p>
          <span className="ws-h-what">
            {words.map((word, index) => (
              <Fragment key={word}>
                {index ? ' · ' : ''}
                {word === 'Text changed' && comparable
                  ? <button type="button" className="ws-h-btn" aria-expanded={open} aria-controls={panel} onClick={toggle}>{word}</button>
                  : word}
              </Fragment>
            ))}
          </span>
          <small>{relativeTime(createdAt)}</small>
          {link ? <a className="ws-h-link" href={link} target="_blank" rel="noopener noreferrer" title="The change on GitHub">{change.commit.slice(0, 7)} ↗</a> : null}
        </p>
        {open ? (
          <div className="ws-h-diff" id={panel}>
            {texts === 'loading' ? <p className="ws-quiet">…</p> : null}
            {texts === 'error' ? <p className="ws-quiet">not available</p> : null}
            {typeof texts === 'object' ? (
              <>
                <div><small>Before</small>{texts.before.name !== texts.after.name ? <b>{texts.before.name}</b> : null}{texts.before.what !== texts.after.what ? <p>{texts.before.what}</p> : null}</div>
                <div><small>After</small>{texts.before.name !== texts.after.name ? <b>{texts.after.name}</b> : null}{texts.before.what !== texts.after.what ? <p>{texts.after.what}</p> : null}</div>
              </>
            ) : null}
          </div>
        ) : null}
      </div>
    </div>
  );
}

function Talk({ row, ctx, detail }: { row: Row; ctx: EntryContext; detail: Detail | undefined }) {
  const { w } = ctx;
  // Until the first answer, the box's room is held.
  if (w.waiting) {
    // Until the first answer: signed out (no session marker), the sign-in box at once and the room of the line
    // under it; otherwise the comment box's own boxes as a placeholder.
    return (
      <div className="ws-talk" aria-busy="true">
        {w.me === null ? (
          <>
            <div className="ws-box"><a className="ws-signin" href={ctx.signIn}>Sign in to comment</a></div>
            <span className="ws-gh-alt ws-held" aria-hidden="true">or comment directly on GitHub<svg className="out"><use href="#out" /></svg></span>
          </>
        ) : (
          <div className="ws-box ws-ph-shape" aria-hidden="true" inert>
            <textarea tabIndex={-1} rows={1} disabled />
            <span className="ws-btn">Comment</span>
          </div>
        )}
      </div>
    );
  }
  if (row.issue === null || row.issue <= 0) return null;
  const n = row.issue;
  const repo = w.live.state?.repo ?? 'amontlabs/silo';
  const url = (typeof detail === 'object' && safeHref(detail.url)) || safeHref(`https://github.com/${repo}/issues/${n}`);
  const github = url ? <a className="ws-gh" href={url} target="_blank" rel="noopener noreferrer">GitHub #{n} ↗</a> : null;
  if (!w.liveOk) return <div className="ws-talk"><div className="ws-box">{github}</div></div>;
  let box: ReactNode = <div className="ws-box">{github}</div>;
  // Until `me` answers, the session marker says which of the two shapes to keep the room of; without it the
  // visitor is signed out, so the panel opens at its final size.
  const signedOut = w.me === null || (w.me === undefined && !hasSessionHint());
  if (w.me || (w.me === undefined && !signedOut)) {
    box = (
      <form className="ws-box" onSubmit={(event) => void w.sendComment(row, event)}>
        <textarea aria-label="Comment" disabled={!w.me} placeholder="Add a comment" maxLength={5000} rows={1} value={w.drafts[row.key] ?? ''} onChange={(event) => w.setDrafts((current) => ({ ...current, [row.key]: event.target.value }))} />
        <button type="submit" className="ws-btn" disabled={!(w.drafts[row.key] ?? '').trim()}>Comment</button>
        {github}
      </form>
    );
  } else if (signedOut) {
    // One link to the issue for someone who would rather not sign in here: the line under the box.
    box = (
      <>
        <div className="ws-box"><a className="ws-signin" href={ctx.signIn}>Sign in to comment</a></div>
        {url ? (
          <a className="ws-gh-alt ws-late-s" href={url} target="_blank" rel="noopener noreferrer" title={`GitHub #${n}`}>
            or comment directly on GitHub<svg className="out" aria-hidden="true"><use href="#out" /></svg>
          </a>
        ) : null}
      </>
    );
  }
  // While the thread loads, each comment and history line the counts announce holds its place.
  const loading = detail === undefined || detail === 'loading';
  const ghostComments = loading ? Math.min(row.comments ?? 0, 20) : 0;
  const ghostUpdates = loading ? Math.min(row.updates, 20 - ghostComments) : 0;
  return (
    <div className="ws-talk" aria-busy={ghostUpdates + ghostComments > 0 ? 'true' : undefined}>
      {Array.from({ length: ghostUpdates }, (_, i) => <div className="ws-h-ghost" key={`h${i}`} aria-hidden="true" />)}
      {Array.from({ length: ghostComments }, (_, i) => <div className="ws-c-ghost" key={`c${i}`} aria-hidden="true" />)}
      {detail === 'error' ? <p className="ws-quiet">Comments could not load.</p> : null}
      {typeof detail === 'object' && detail.more && url ? <a className="ws-gh ws-earlier" href={url} target="_blank" rel="noopener noreferrer">Earlier comments on GitHub ↗</a> : null}
      {typeof detail === 'object'
        ? detail.comments.map((comment) => (comment.change && comment.bot
          ? <HistoryLine key={comment.id} change={comment.change} createdAt={comment.createdAt} row={row} repo={repo} />
          : (
            <div className="ws-c" key={comment.id}>
              <i aria-hidden="true">{initials(comment.author)}</i>
              <div>
                <b>{comment.author} <small>{relativeTime(comment.createdAt)}</small></b>
                <RichText text={comment.text} />
              </div>
            </div>
          )))
        : null}
      {(w.sending[n] ?? []).map((comment) => (
        <div className="ws-c ws-pending" key={comment.id} aria-busy="true">
          <i aria-hidden="true">{initials(comment.author)}</i>
          <div>
            <b>{comment.author} <small>Sending…</small></b>
            <RichText text={comment.text} />
          </div>
        </div>
      ))}
      {box}
    </div>
  );
}

/** The entry's notes, closed until asked for. */
function Extras({ row, ctx }: { row: Row; ctx: EntryContext }) {
  const { w } = ctx;
  const [shown, setShown] = useState(false);
  if (!row.notes) return null;
  const id = row.key.slice('entry-'.length);
  const notes = typeof w.notes === 'object' ? w.notes[id] : undefined;
  return (
    <div className="ws-extras">
      <div className="ws-extras-bar">
        <button
          type="button" className="ws-disc" aria-expanded={shown} aria-controls={`${row.key}-notes`}
          onPointerEnter={w.wantNotes} onPointerDown={w.wantNotes} onTouchStart={w.wantNotes} onFocus={w.wantNotes}
          onClick={() => { w.wantNotes(); setShown((current) => !current); }}
        >Notes</button>
      </div>
      {shown ? (
        <div className="ws-extras-in" id={`${row.key}-notes`}>
          {notes ? <Markdown text={notes} /> : w.notes === 'error' ? <p className="ws-quiet">Notes could not load.</p> : typeof w.notes === 'object' ? null : (
            <div className="ws-ph-notes" aria-busy="true">
              <i className="ws-ph" aria-hidden="true" /><i className="ws-ph" aria-hidden="true" /><i className="ws-ph" aria-hidden="true" />
            </div>
          )}
        </div>
      ) : null}
    </div>
  );
}

function Body({ row, ctx, detail }: { row: Row; ctx: EntryContext; detail: Detail | undefined }) {
  const { w } = ctx;
  const what = row.kind === 'entry' ? row.what : typeof detail === 'object' ? detail.body : null;
  return (
    <div className="ws-more-pad">
      {what ? <div className="ws-what"><RichText text={what} /></div> : null}
      {row.kind === 'entry' ? <Extras row={row} ctx={ctx} /> : null}
      <StateLine row={row} />
      <ReplaceLines row={row} ctx={ctx} />
      {threaded(row, ctx) ? <Talk row={row} ctx={ctx} detail={detail} /> : null}
    </div>
  );
}
