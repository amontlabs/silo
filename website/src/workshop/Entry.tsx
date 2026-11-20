// One row of a list, and what it shows once open: the text, what the entry says of the plan's lists, its state, its
// notes, its history and its thread (comments, their votes) and the box; an entry with no thread yet says so and has the box too.
// Opening never moves the row or the rows above: the open frame is an overlay and the body grows below.
import { Fragment, useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { hasSessionHint } from './api';
import { DEFAULT_REPO, DEFAULT_WEB } from '../../../tools/workshop/model/index.js';
import { Markdown } from './Markdown';
import { RichText, safeHref } from './RichText';
import { changeLine, changePhrase, isDecided, relativeTime, shortDate } from './plan';
import type { Change, EntryFacts, Facts, Row, Statement } from './plan';
import { formatHash, placeOf } from './routes';
import type { Comment } from './api';
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
  const { wantFacts } = w;
  // A row with something to show from the facts asks for them as it opens, if no pointer or focus did before.
  useEffect(() => {
    if (open && row.more) wantFacts();
  }, [open, row.more, wantFacts]);

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
  // The panel grows too when what the entry says of the lists arrives.
  const factsIn = typeof w.facts === 'object';
  const lastFactsIn = useRef(factsIn);
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
    const changed = lastShown.current !== shown || lastFactsIn.current !== factsIn;
    lastShown.current = shown;
    lastFactsIn.current = factsIn;
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

/** What a vote button holds: the thumb, then the count from 1 up. */
function Thumb({ up }: { up: number }) {
  return <><span aria-hidden="true">👍</span>{up > 0 ? <span className="ws-n" aria-hidden="true">{up}</span> : null}</>;
}

function Vote({ row, ctx }: { row: Row; ctx: EntryContext }) {
  const { w } = ctx;
  if (w.waiting) return <i className="ws-ph ws-ph-vote" aria-hidden="true" />;
  if (!w.liveOk || row.up === null) return null;
  const on = row.issue !== null && w.votes[row.issue] === true;
  const busy = w.votePending.has(row.key);
  if (!w.me) return <a className="ws-vote ws-late-s" href={ctx.signIn} title="Sign in to vote" aria-label={`${row.up} votes, sign in to vote`}><Thumb up={row.up} /></a>;
  return (
    <button type="button" className={`ws-vote ws-late-s${on ? ' on' : ''}${busy ? ' ws-pending' : ''}`} aria-pressed={on} aria-busy={busy} aria-label={`${row.up} votes`} onClick={() => void w.toggleVote(row)} disabled={busy}>
      <Thumb up={row.up} />
    </button>
  );
}

function Side({ row, ctx }: { row: Row; ctx: EntryContext }) {
  const { w } = ctx;
  const showComments = w.liveOk && row.comments !== null && row.comments > 0;
  return (
    <span className="ws-side">
      {row.kind === 'suggestion' && row.author ? <span className="ws-by">{row.author}</span> : null}
      <span className="ws-votes" aria-busy={w.waiting || undefined}><Vote row={row} ctx={ctx} /></span>
      <span className="ws-talks">{showComments ? <span className="ws-late-s" aria-label={`${row.comments} comments`}>💬 {row.comments}</span> : null}</span>
    </span>
  );
}

/** The id of an entry, a plan id; null for a suggestion, which has none. */
export const planId = (row: Row): string | null => (row.kind === 'entry' ? row.key.slice('entry-'.length) : null);

/** How long "Copied" stays. */
export const COPIED_MS = 2000;

/**
 * The entry's id in the monospace face and a button that writes it to the clipboard. When the clipboard refuses, the
 * id's text is selected so the person can copy it by hand. The button keeps the width of the longer word.
 */
function CopyId({ id }: { id: string }) {
  const [copied, setCopied] = useState(false);
  const text = useRef<HTMLElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  const select = () => {
    const node = text.current;
    if (node) getSelection()?.selectAllChildren(node);
  };
  const copy = () => {
    clearTimeout(timer.current);
    const done = () => {
      setCopied(true);
      timer.current = setTimeout(() => setCopied(false), COPIED_MS);
    };
    // A missing or refusing clipboard (an insecure page, a denied permission) leaves the id selected instead.
    if (typeof navigator === 'undefined' || !navigator.clipboard) {
      setCopied(false);
      select();
      return;
    }
    navigator.clipboard.writeText(id).then(done, () => {
      setCopied(false);
      select();
    });
  };
  const now = copied ? 'Copied' : 'Copy id';
  return (
    <span className="ws-id">
      <code ref={text}>{id}</code>
      <button type="button" className="ws-copy" onClick={copy}>
        <span>{now}</span>
        <span aria-hidden="true">{copied ? 'Copy id' : 'Copied'}</span>
      </button>
      <span className="ws-sr" role="status" aria-live="polite">{copied ? 'Copied' : ''}</span>
    </span>
  );
}

function StateLine({ row }: { row: Row }) {
  const text = stateText(row);
  const tone = isDecided(row) && row.status !== 'dropped' ? 'ws-yes' : 'ws-muted';
  const id = planId(row);
  return <div className="ws-state"><span className={tone}>{text}</span>{id ? <CopyId id={id} /> : null}</div>;
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

/** A link to an entry, where it lives: a plain click opens it here, a click with a modifier key is left to the browser. */
function EntryLink({ other, ctx }: { other: Row; ctx: EntryContext }) {
  return (
    <a
      href={formatHash(placeOf(other), other.key)}
      onClick={(event) => {
        if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
        event.preventDefault();
        if (!ctx.open.has(other.key)) ctx.onToggle(other);
      }}
    >{other.name}</a>
  );
}

/** Each line links the question of each decision to where it lives. `only` picks the lines that go in one place of the open view. */
function ReplaceLines({ row, ctx, only }: { row: Row; ctx: EntryContext; only: (label: string) => boolean }) {
  const lines = replaceLines(row).filter((line) => only(line.label));
  if (!lines.length) return null;
  return (
    <>
      {lines.map((line) => (
        <p className="ws-replace" key={line.label}>
          <span>{line.label}</span>
          {line.keys.map((key) => ctx.w.rowByKey.get(key)).filter((other): other is Row => other !== undefined).map((other, index) => (
            <Fragment key={other.key}>
              {index ? ' · ' : ' '}
              <EntryLink other={other} ctx={ctx} />
            </Fragment>
          ))}
        </p>
      ))}
    </>
  );
}

/** The 👍 under a comment, the same button as a row's: signed out it asks to sign in. */
function CommentVote({ row, comment, ctx }: { row: Row; comment: Comment; ctx: EntryContext }) {
  const { w } = ctx;
  const up = comment.up ?? 0;
  if (!w.me) return <a className="ws-vote ws-cvote" href={ctx.signIn} title="Sign in to vote" aria-label={`${up} votes on this comment, sign in to vote`}><Thumb up={up} /></a>;
  const id = comment.id;
  const real = row.issue !== null && typeof id === 'number';
  const on = real && (w.commentVotes[row.issue as number] ?? []).includes(id);
  const busy = real && w.commentVotePending.has(`${row.issue}:${id}`);
  return (
    <button type="button" className={`ws-vote ws-cvote${on ? ' on' : ''}${busy ? ' ws-pending' : ''}`} aria-pressed={on} aria-busy={busy} aria-label={`${up} votes on this comment`} disabled={!real || busy} onClick={() => void w.toggleCommentVote(row, comment)}>
      <Thumb up={up} />
    </button>
  );
}

function Talk({ row, ctx, detail }: { row: Row; ctx: EntryContext; detail: Detail | undefined }) {
  const { w } = ctx;
  // Until the first answer, the box's room is held.
  if (w.waiting) {
    // Until the first answer: signed out (no session marker), the sign-in box at once and the room of the line
    // under it; otherwise the comment box's own boxes as a placeholder.
    // An entry holds the room of the shape most of them have: no comment yet, then the box.
    return (
      <div className="ws-talk" aria-busy="true">
        {row.kind === 'entry' ? <p className="ws-quiet ws-none ws-held" aria-hidden="true">No comments yet.</p> : null}
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
  const n = row.issue !== null && row.issue > 0 ? row.issue : null;
  if (n === null && (row.kind !== 'entry' || !w.liveOk)) return null;
  const repo = w.live.state?.repo ?? DEFAULT_REPO;
  const url = n === null ? null : (typeof detail === 'object' && safeHref(detail.url)) || safeHref(`${DEFAULT_WEB}/${repo}/issues/${n}`);
  const github = url ? <a className="ws-gh" href={url} target="_blank" rel="noopener noreferrer">GitHub #{n} ↗</a> : null;
  if (!w.liveOk) return <div className="ws-talk"><div className="ws-box">{github}</div></div>;
  // An entry with no thread yet has no issue to link to; the room of the link is kept, so the row taking its thread moves nothing.
  const room = n === null ? <span className="ws-gh ws-room" aria-hidden="true">GitHub #000 ↗</span> : github;
  const closed = row.closed;
  let box: ReactNode = <div className="ws-box">{room}</div>;
  // Until `me` answers, the session marker says which of the two shapes to keep the room of; without it the
  // visitor is signed out, so the panel opens at its final size.
  const signedOut = w.me === null || (w.me === undefined && !hasSessionHint());
  if (closed) {
    box = <p className="ws-quiet ws-closed">This thread is closed on GitHub.{github ? <> {github}</> : null}</p>;
  } else if (w.me || (w.me === undefined && !signedOut)) {
    box = (
      <form className="ws-box" onSubmit={(event) => void w.sendComment(row, event)}>
        <textarea aria-label="Comment" disabled={!w.me} placeholder="Add a comment" maxLength={5000} rows={1} value={w.drafts[row.key] ?? ''} onChange={(event) => w.setDrafts((current) => ({ ...current, [row.key]: event.target.value }))} />
        <button type="submit" className="ws-btn" disabled={!(w.drafts[row.key] ?? '').trim()}>Comment</button>
        {room}
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
        ) : <span className="ws-gh-alt ws-room" aria-hidden="true">or comment directly on GitHub<svg className="out"><use href="#out" /></svg></span>}
      </>
    );
  }
  // While the thread loads, each comment the count announces holds its place.
  const loading = detail === undefined || detail === 'loading';
  const ghostComments = loading ? Math.min(row.comments ?? 0, 20) : 0;
  const sending = w.sending[row.key] ?? [];
  const comments = typeof detail === 'object' ? detail.comments : [];
  const none = row.kind === 'entry' && (row.comments ?? 0) === 0 && !comments.length && !sending.length;
  return (
    <div className="ws-talk" aria-busy={ghostComments > 0 ? 'true' : undefined}>
      {Array.from({ length: ghostComments }, (_, i) => <div className="ws-c-ghost" key={`c${i}`} aria-hidden="true" />)}
      {detail === 'error' ? <p className="ws-quiet">Comments could not load.</p> : null}
      {typeof detail === 'object' && detail.more && url ? <a className="ws-gh ws-earlier" href={url} target="_blank" rel="noopener noreferrer">Earlier comments on GitHub ↗</a> : null}
      {none ? <p className="ws-quiet ws-none">No comments yet.</p> : null}
      {comments.map((comment) => (
        <div className="ws-c" key={comment.id}>
          <i aria-hidden="true">{initials(comment.author)}</i>
          <div>
            <b>{comment.author} <small>{relativeTime(comment.createdAt)}</small></b>
            <RichText text={comment.text} />
            <CommentVote row={row} comment={comment} ctx={ctx} />
          </div>
        </div>
      ))}
      {sending.map((comment) => (
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

/** The commit on GitHub, for the link of a History line. */
const commitLink = (repo: string, commit: string) => safeHref(`${DEFAULT_WEB}/${repo}/commit/${commit}`);

/** How many ghost lines hold the place of the changes while their chunk loads. */
const HISTORY_GHOSTS = 12;

/** The phrases of a change on one line: a text change opens its before and after, a replacement links the decision that replaced it. */
function ChangePhrases({ change, row, ctx, panel, open, onToggle }: { change: Change; row: Row; ctx: EntryContext; panel: string; open: boolean; onToggle: () => void }) {
  const by = change.by !== undefined ? ctx.w.rowByKey.get(`entry-${change.by}`) : undefined;
  return (
    <span className="ws-h-what">
      {change.what.map((name, index) => {
        const phrase = changePhrase(name, row.of);
        return (
          <Fragment key={name}>
            {index ? ' · ' : ''}
            {name === 'text' && change.text
              ? <button type="button" className="ws-h-btn" aria-expanded={open} aria-controls={panel} onClick={onToggle}>{phrase}</button>
              : phrase}
            {name === 'replaced' && by ? <>{' '}<EntryLink other={by} ctx={ctx} /></> : null}
          </Fragment>
        );
      })}
    </span>
  );
}

/** One line of an entry's History: the date, what changed and the commit; for a text change, its before and after below. */
function HistoryLine({ change, row, ctx, repo }: { change: Change; row: Row; ctx: EntryContext; repo: string }) {
  const [diff, setDiff] = useState(false);
  const link = commitLink(repo, change.commit);
  const panel = `${row.key}-${change.commit.slice(0, 12)}`;
  const text = change.text;
  const differs = (key: 'title' | 'body') => text !== undefined && text.before[key] !== text.after[key];
  const side = (label: string, value: NonNullable<Change['text']>['before']) => (
    <div>
      <small>{label}</small>
      {differs('title') && value.title ? <b>{value.title}</b> : null}
      {differs('body') && value.body ? <p>{value.body}</p> : null}
    </div>
  );
  return (
    <div className="ws-h">
      <i aria-hidden="true" />
      <div>
        <p>
          <small className="ws-h-date">{shortDate(change.date)}</small>
          <ChangePhrases change={change} row={row} ctx={ctx} panel={panel} open={diff} onToggle={() => setDiff((current) => !current)} />
          {link ? <a className="ws-h-link" href={link} target="_blank" rel="noopener noreferrer">commit</a> : null}
        </p>
        {diff && text ? <div className="ws-h-diff" id={panel}>{side('Before', text.before)}{side('After', text.after)}</div> : null}
      </div>
    </div>
  );
}

/** The changes of an entry, newest first; a placeholder line for each while they load. */
function HistoryList({ row, ctx }: { row: Row; ctx: EntryContext }) {
  const { w } = ctx;
  const id = planId(row);
  const changes = typeof w.histories === 'object' && id !== null ? w.histories[id] : undefined;
  const repo = w.live.state?.repo ?? DEFAULT_REPO;
  const shallow = w.plan.shallow ? <p className="ws-quiet">Earlier changes are not available in this build.</p> : null;
  if (changes) return <>{changes.map((change) => <HistoryLine key={change.commit} change={change} row={row} ctx={ctx} repo={repo} />)}{shallow}</>;
  if (w.histories === 'error') return <p className="ws-quiet">History could not load.</p>;
  return (
    <div aria-busy="true">
      {Array.from({ length: Math.min(row.history?.count ?? 1, HISTORY_GHOSTS) }, (_, i) => <div className="ws-h-ghost" key={i} aria-hidden="true" />)}
      {w.plan.shallow ? <div className="ws-h-ghost" aria-hidden="true" /> : null}
    </div>
  );
}

/** The closed History line: the newest change with its date, and how many there are when more than one. */
function historySummary(row: Row): string {
  const last = row.history;
  if (!last) return '';
  return `${changeLine(last.what, row.of)}, ${shortDate(last.date)}${last.count > 1 ? ` · ${last.count} changes` : ''}`;
}

/** The entry's notes and its history, each closed until asked for. */
function Extras({ row, ctx }: { row: Row; ctx: EntryContext }) {
  const { w } = ctx;
  const [shown, setShown] = useState(false);
  const [history, setHistory] = useState(false);
  if (!row.notes && !row.history) return null;
  const id = row.key.slice('entry-'.length);
  const notes = typeof w.notes === 'object' ? w.notes[id] : undefined;
  return (
    <div className="ws-extras">
      <div className="ws-extras-bar">
        {row.notes ? (
          <button
            type="button" className="ws-disc" aria-expanded={shown} aria-controls={`${row.key}-notes`}
            onPointerEnter={w.wantNotes} onPointerDown={w.wantNotes} onTouchStart={w.wantNotes} onFocus={w.wantNotes}
            onClick={() => { w.wantNotes(); setShown((current) => !current); }}
          >Notes</button>
        ) : null}
        {row.history ? (
          <button
            type="button" className="ws-disc ws-disc-history" aria-expanded={history} aria-controls={`${row.key}-history`}
            onPointerEnter={w.wantHistory} onPointerDown={w.wantHistory} onTouchStart={w.wantHistory} onFocus={w.wantHistory}
            onClick={() => { w.wantHistory(); setHistory((current) => !current); }}
          >History<span> · {historySummary(row)}</span></button>
        ) : null}
      </div>
      {row.notes && shown ? (
        <div className="ws-extras-in" id={`${row.key}-notes`}>
          {notes ? <Markdown text={notes} /> : w.notes === 'error' ? <p className="ws-quiet">Notes could not load.</p> : typeof w.notes === 'object' ? null : (
            <div className="ws-ph-notes" aria-busy="true">
              <i className="ws-ph" aria-hidden="true" /><i className="ws-ph" aria-hidden="true" /><i className="ws-ph" aria-hidden="true" />
            </div>
          )}
        </div>
      ) : null}
      {row.history && history ? <div className="ws-extras-in" id={`${row.key}-history`}><HistoryList row={row} ctx={ctx} /></div> : null}
    </div>
  );
}

const KINDS: Record<Statement['kind'], string> = { word: 'word', principle: 'principle', property: 'property' };

/** A statement as text: a word and its meaning, a principle and the question it asks, a property. */
function StatementText({ statement }: { statement: Statement }) {
  return (
    <span className="ws-st">
      {statement.word !== null ? <b>{statement.word}</b> : null}
      {statement.word !== null ? ' ' : ''}
      {statement.text}
      {statement.asks ? <span className="ws-st-asks"> {statement.asks}</span> : null}
    </span>
  );
}

/** One part of an open view: a small quiet label and its lines under it. */
function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="ws-fact">
      <span className="ws-fact-label">{label}</span>
      {children}
    </div>
  );
}

/** What a yes would add, change and remove: the lines of an entry with no verdict that sets or removes statements. */
function YesLines({ own, facts }: { own: EntryFacts; facts: Facts }) {
  const sets = own.sets ?? [];
  const adds = sets.filter((statement) => !facts.statements[statement.id]);
  const changes = sets.filter((statement) => facts.statements[statement.id]);
  const removes = own.removes ?? [];
  return (
    <>
      {adds.length ? (
        <Fact label="A yes would add">
          <ul className="ws-fact-lines">
            {adds.map((statement) => (
              <li key={statement.id}><span className="ws-tag">{KINDS[statement.kind]}</span><StatementText statement={statement} /></li>
            ))}
          </ul>
        </Fact>
      ) : null}
      {changes.length ? (
        <Fact label="A yes would change">
          <ul className="ws-fact-lines">
            {changes.map((statement) => (
              <li key={statement.id}>
                <span className="ws-tag">{KINDS[statement.kind]}</span><code>{statement.id}</code>
                <span className="ws-tag">Now</span><StatementText statement={facts.statements[statement.id]} />
                <span className="ws-tag">After</span><StatementText statement={statement} />
              </li>
            ))}
          </ul>
        </Fact>
      ) : null}
      {removes.length ? (
        <Fact label="A yes would remove">
          <ul className="ws-fact-lines">
            {removes.map((id) => {
              const statement = facts.statements[id];
              return (
                <li key={id}>
                  <span className="ws-tag">{statement ? KINDS[statement.kind] : ''}</span>
                  {statement?.kind === 'word' ? <StatementText statement={statement} /> : <code>{id}</code>}
                  {statement && statement.kind !== 'word' ? <><span aria-hidden="true" /><StatementText statement={statement} /></> : null}
                </li>
              );
            })}
          </ul>
        </Fact>
      ) : null}
    </>
  );
}

/** The state word of a row that another one needs; one that is not decided says nothing on its own row, and here says so. */
function needWord(row: Row): string {
  return (row.of === 'decision' ? DECISION_WORDS : WORDS)[row.status] ?? 'not decided yet';
}

function NeedsLines({ row, ctx }: { row: Row; ctx: EntryContext }) {
  const needs = row.needs.map((key) => ctx.w.rowByKey.get(key)).filter((other): other is Row => other !== undefined);
  if (!needs.length) return null;
  return (
    <Fact label="Needs">
      <ul className="ws-fact-lines ws-needs">
        {needs.map((other) => (
          <li key={other.key}><EntryLink other={other} ctx={ctx} /> <span className="ws-need-word">{needWord(other)}</span></li>
        ))}
      </ul>
    </Fact>
  );
}

/** What the open view says of the lists and of what the entry needs, in the order of the plan's model. */
function OpenFacts({ row, ctx }: { row: Row; ctx: EntryContext }) {
  const { w } = ctx;
  const id = planId(row);
  const facts = typeof w.facts === 'object' ? w.facts : null;
  const own = facts && id !== null ? facts.entries[id] : undefined;
  const waiting = row.more && !facts && w.facts !== 'error';
  const open = row.verdict === null;
  const taken = row.verdict === 'take';
  const adds = taken && own?.adds ? own.adds.map((statementId) => facts?.statements[statementId]).filter((statement): statement is Statement => statement !== undefined) : [];
  return (
    <>
      {waiting ? (
        <div className="ws-fact">
          <div className="ws-ph-notes" aria-busy="true">
            <i className="ws-ph" aria-hidden="true" /><i className="ws-ph" aria-hidden="true" /><i className="ws-ph" aria-hidden="true" />
          </div>
        </div>
      ) : null}
      {open && own?.suggest ? (
        <Fact label={`Recommended: ${own.suggest.verdict}`}>
          {own.suggest.why ? <p className="ws-fact-text">{own.suggest.why}</p> : null}
        </Fact>
      ) : null}
      {open && own && facts ? <YesLines own={own} facts={facts} /> : null}
      {adds.length ? (
        <Fact label="Added">
          <ul className="ws-fact-lines">
            {adds.map((statement) => <li key={statement.id}><span className="ws-tag">{KINDS[statement.kind]}</span><StatementText statement={statement} /></li>)}
          </ul>
        </Fact>
      ) : null}
      <ReplaceLines row={row} ctx={ctx} only={(label) => label !== 'Replaced by'} />
      <NeedsLines row={row} ctx={ctx} />
      {facts && own?.keeps && row.of === 'feature' ? (
        <Fact label="Must keep">
          <ul className="ws-fact-lines ws-keeps">
            {own.keeps.map((statementId) => <li key={statementId}>{facts.statements[statementId]?.text ?? statementId}</li>)}
          </ul>
        </Fact>
      ) : null}
      {own?.surfaces && row.of === 'feature' ? (
        <Fact label="Where it shows">
          <ul className="ws-fact-lines">
            {own.surfaces.app ? <li><span className="ws-tag">App</span><span className="ws-st">{own.surfaces.app}</span></li> : null}
            {own.surfaces.cli ? <li><span className="ws-tag">Command</span><code>{own.surfaces.cli}</code></li> : null}
          </ul>
        </Fact>
      ) : null}
    </>
  );
}

function Body({ row, ctx, detail }: { row: Row; ctx: EntryContext; detail: Detail | undefined }) {
  const { w } = ctx;
  const what = row.kind === 'entry' ? row.what : typeof detail === 'object' ? detail.body : null;
  return (
    <div className="ws-more-pad">
      {what ? <div className="ws-what"><RichText text={what} /></div> : null}
      {row.kind === 'entry' ? <OpenFacts row={row} ctx={ctx} /> : null}
      <StateLine row={row} />
      <ReplaceLines row={row} ctx={ctx} only={(label) => label === 'Replaced by'} />
      {row.kind === 'entry' ? <Extras row={row} ctx={ctx} /> : null}
      <Talk row={row} ctx={ctx} detail={detail} />
    </div>
  );
}
