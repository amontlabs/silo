// One row of a list, and what it shows once open: the text, its choice while it has no verdict, what the entry says of
// the plan's lists, its state, its notes, its history and its thread (comments, their votes) and the box; an entry with
// no thread yet says so and has the box too.
// Opening never moves the row or the rows above: the open frame is an overlay and the body grows below.
import { Fragment, useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { hasSessionHint } from './api';
import { DEFAULT_REPO, repoAddress } from '../../../tools/workshop/model/index.js';
import { Markdown, plainLinks } from './Markdown';
import { inline, RichText, safeHref } from './RichText';
import { byWho, changeLine, changePhrase, isDecided, relativeTime, shortDate } from './plan';
import type { Change, Choice, ChoiceTable, EntryFacts, Facts, Row, Statement, Verdict } from './plan';
import { formatHash, placeOf } from './routes';
import type { Comment } from './api';
import { pageConfig } from './useWorkshop';
import type { Detail, Workshop } from './useWorkshop';

/** How long a panel takes to open, to close and to follow what it holds (workshop.css: `.ws-more`). */
export const GROW_MS = 260;
/** How long the closing body stays in place for its animation. */
const CLOSE_MS = GROW_MS + 40;
/** How long an opening panel waits for its details and comments, so it opens at its final size. */
export const HOLD_MS = 180;

export interface EntryContext {
  w: Workshop;
  signIn: string;
  /** Opens or closes a row; `instance` names the one drawn under a feature, when the same entry is drawn there. */
  onToggle: (row: Row, instance?: string) => void;
  /** The rows open now, by instance key (a row on its own line is its own key; all.ts `underKey`). */
  open: ReadonlySet<string>;
  /** Shows All with the entries a film shows. */
  onFilmEntries?: (film: string) => void;
}

/** A row's word: one that is not decided says nothing on its own row. */
const WORDS: Partial<Record<Row['status'], string>> = { decided: 'decided', building: 'being built', built: 'built', replaced: 'replaced', dropped: 'dropped' };

export function pipClass(row: Row): string {
  return `ws-pip ${row.status}`;
}

function initials(login: string): string {
  const parts = login.split(/[-_.]+/).filter(Boolean);
  return (parts.length > 1 ? parts[0][0] + parts[1][0] : login.slice(0, 2)).toLowerCase();
}

export function stateText(row: Row): string {
  switch (row.status) {
    case 'decided': return `${byWho(row, 'Decided')}.`;
    case 'building': return 'Being built.';
    case 'built': {
      const day = row.built ?? row.closedAt;
      return day ? `Built, ${shortDate(day)}.` : 'Built.';
    }
    case 'replaced': return 'Replaced.';
    case 'dropped': return `${byWho(row, 'Dropped')}.`;
    default: return 'Not decided.';
  }
}

export function Entry({ row, open, ctx }: { row: Row; open: boolean; ctx: EntryContext }) {
  const { w } = ctx;
  const word = WORDS[row.status] === undefined ? undefined : byWho(row, WORDS[row.status]!);
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
      <Panel row={row} open={open} ctx={ctx} id={`${row.key}-more`} />
    </li>
  );
}

export interface PanelProps {
  row: Row;
  open: boolean;
  ctx: EntryContext;
  id: string;
  /** What the panel shows first: an entry's film. */
  lead?: ReactNode;
  /** The vote button and the count of comments, in the panel, for a row that shows only its votes. */
  vote?: boolean;
}

/**
 * What a row shows once open. It opens at once and grows when its details and comments are in, or after HOLD_MS; its body stays
 * mounted while it closes, so the close animates too.
 */
export function Panel({ row, open, ctx, id, lead, vote = false }: PanelProps) {
  const { w } = ctx;
  // The body stays mounted while it closes, so the close animates too.
  const [closing, setClosing] = useState(false);
  const [wasOpen, setWasOpen] = useState(open);
  // The row shows itself open at once; its panel grows when the details and the comments are in, or after HOLD_MS.
  const detail = row.issue === null ? undefined : w.details[row.issue];
  const detailsWait = row.more && (w.facts === undefined || w.facts === 'loading');
  const commentsIn = row.issue === null || row.issue <= 0 || !w.liveOk || typeof detail === 'object' || detail === 'error';
  const ready = commentsIn && !detailsWait;
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
    if (open && row.more) wantFacts(true);
  }, [open, row.more, wantFacts]);

  // Comments that arrive while the panel is opening wait, on their placeholders, until the opening has ended.
  const [wasGrown, setWasGrown] = useState(grown);
  const [opening, setOpening] = useState<{ detail: typeof detail } | null>(null);
  const [detailsHeld, setDetailsHeld] = useState(false);
  if (wasGrown !== grown) {
    setWasGrown(grown);
    // Only a panel that opens on placeholders waits; one that opens on its comments shows every change at once.
    setOpening(grown && typeof detail !== 'object' ? { detail } : null);
    setDetailsHeld(grown && detailsWait);
  }
  useEffect(() => {
    if (!opening) return;
    const timer = setTimeout(() => setOpening(null), GROW_MS);
    return () => clearTimeout(timer);
  }, [opening]);
  const shown = opening ? opening.detail : detail;
  // The details too (the choice is several times the height of their placeholder): drawn while the panel opens, they
  // would jump it to a share of their height. They wait on the placeholder, then the panel grows to them.
  useEffect(() => {
    if (!detailsHeld) return;
    const timer = setTimeout(() => setDetailsHeld(false), GROW_MS);
    return () => clearTimeout(timer);
  }, [detailsHeld]);
  const facts = detailsHeld ? 'loading' : w.facts;

  // When the comments arrive in a panel already open, it grows from its old height to the new one instead
  // of jumping; this also covers rows that appear open (from another place, Back and Forward).
  const inner = useRef<HTMLDivElement>(null);
  const lastHeight = useRef(0);
  const lastShown = useRef(shown);
  // The panel grows too when what the entry says of the lists arrives, when Notes or History opens or closes, and when
  // their chunk arrives after its placeholder.
  const [extrasMoved, setExtrasMoved] = useState(0);
  const loaded = (chunk: unknown) => (typeof chunk === 'object' ? 'in' : String(chunk));
  const chunks = `${loaded(facts)}|${loaded(w.notes)}|${loaded(w.histories)}|${extrasMoved}`;
  const lastChunks = useRef(chunks);
  const settling = useRef<ReturnType<typeof setTimeout>>(undefined);
  // The panel's height as last laid out, kept current while it animates open (no render happens then).
  const animating = useRef(false);
  // When the panel last started to open: while it opens, its height follows what it holds without a growth of its own.
  const grewAt = useRef(-Infinity);
  const lastGrown = useRef(grown);
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
    if (grown && !lastGrown.current) grewAt.current = Date.now();
    lastGrown.current = grown;
    const unfolding = Date.now() - grewAt.current < GROW_MS;
    const changed = lastShown.current !== shown || lastChunks.current !== chunks;
    lastShown.current = shown;
    lastChunks.current = chunks;
    if (animating.current && !changed) return;
    const still = typeof matchMedia !== 'function' || matchMedia('(prefers-reduced-motion: reduce)').matches;
    // From the height on screen (mid-change, if one runs) to the panel's natural height, measured without it.
    const from = animating.current ? node.getBoundingClientRect().height : lastHeight.current;
    node.style.transition = 'none';
    node.style.height = '';
    const to = node.getBoundingClientRect().height;
    if (!animating.current) lastHeight.current = to;
    if (!changed || !grown || unfolding || still || from <= 0 || Math.abs(to - from) < 1) {
      node.style.transition = '';
      animating.current = false;
      return;
    }
    animating.current = true;
    node.style.height = `${from}px`;
    node.getBoundingClientRect();
    node.style.transition = `height ${GROW_MS}ms var(--ease), opacity .18s`;
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

  return (
    <div className={grown ? 'ws-more open' : 'ws-more'} id={id} aria-hidden={open ? undefined : true} inert={!open}>
      <div className="ws-more-in" ref={inner}>{open || closing ? <Body row={row} ctx={ctx} detail={shown} facts={facts} lead={lead} vote={vote} onExtras={() => setExtrasMoved((n) => n + 1)} /> : null}</div>
    </div>
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
  if (!w.me) return <a className="ws-vote ws-late-s" href={ctx.signIn} title="Sign in to vote" aria-label={`${row.up} votes, sign in to vote`}><Thumb up={row.up} /></a>;
  // While the person's vote is out, the page's own change to it is what shows.
  const own = w.voteOwn[row.key];
  const on = own ? own.on : row.issue !== null && w.votes[row.issue] === true;
  const up = Math.max(0, row.up + (own?.delta ?? 0));
  const busy = w.votePending.has(row.key);
  return (
    <button type="button" className={`ws-vote ws-late-s${on ? ' on' : ''}${busy ? ' ws-pending' : ''}`} aria-pressed={on} aria-busy={busy} aria-label={`${up} votes`} onClick={() => void w.toggleVote(row)} disabled={busy}>
      <Thumb up={up} />
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

/** How long a pressed Take or Drop waits for its second press. */
export const ARM_MS = 4000;
/** A second press sooner than this after the first is part of the same gesture (a double click), never the confirm. */
export const CONFIRM_AFTER_MS = 400;

/**
 * Whether the owner may take or drop the entry from the page: it has no verdict, or the agents wrote its verdict and
 * the owner's confirms or overrules it. Undoing a verdict, a replaced entry and a built one are not done here.
 */
export function offersVerdict(row: Row): boolean {
  if (row.kind !== 'entry') return false;
  return row.status === 'undecided' || (row.by === 'agents' && (row.status === 'decided' || row.status === 'dropped'));
}

const VERDICT_WORDS: Record<Verdict, { idle: string; pending: string }> = {
  take: { idle: 'Take', pending: 'Taking…' },
  drop: { idle: 'Drop', pending: 'Dropping…' },
};
const ARMED = 'Confirm';

/**
 * The owner's Take and Drop. A first press arms the button ("Confirm") and a second confirms it; an armed button goes
 * back after ARM_MS or when the focus leaves the pair. The confirm is a separate activation: a key held down repeats,
 * and Chromium clicks on each repeat of Enter, so a repeated key does nothing until it is let go, and a press sooner
 * than CONFIRM_AFTER_MS after arming is ignored. While the server writes the verdict, the pressed one says so
 * and both are disabled. Each button keeps the width of its longest word, so nothing moves as it changes. `held`
 * draws the same buttons as placeholders, inert, while `me` has not answered for the owner remembered, or the entry's
 * details have not shown yet.
 */
function VerdictButtons({ row, ctx, held = false, onSend }: { row: Row; ctx: EntryContext; held?: boolean; onSend: () => void }) {
  const { w } = ctx;
  const id = planId(row) as string;
  const pending = w.verdictPending[id];
  const [armed, setArmed] = useState<Verdict | null>(null);
  const armedAt = useRef(0);
  // A key held down on a button: its repeats, and the clicks they make, are not presses.
  const repeating = useRef(false);
  useEffect(() => {
    if (!armed) return;
    const timer = setTimeout(() => setArmed(null), ARM_MS);
    return () => clearTimeout(timer);
  }, [armed]);
  const press = (verdict: Verdict) => {
    if (pending || repeating.current) return;
    if (armed !== verdict) {
      setArmed(verdict);
      armedAt.current = Date.now();
      return;
    }
    if (Date.now() - armedAt.current < CONFIRM_AFTER_MS) return;
    setArmed(null);
    onSend();
    void w.sendVerdict(row, verdict);
  };
  if (held) {
    return (
      <span className="ws-verdict ws-held" aria-hidden="true" inert>
        {(['take', 'drop'] as const).map((verdict) => (
          <span key={verdict} className={`ws-vbtn ${verdict}`}>{[VERDICT_WORDS[verdict].idle, ARMED, VERDICT_WORDS[verdict].pending].map((word) => <span key={word}>{word}</span>)}</span>
        ))}
      </span>
    );
  }
  return (
    <span className="ws-verdict" role="group" aria-label="Your verdict" onBlur={(event) => {
      repeating.current = false;
      if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setArmed(null);
    }}>
      {(['take', 'drop'] as const).map((verdict) => {
        const words = VERDICT_WORDS[verdict];
        const now = pending === verdict ? words.pending : armed === verdict ? ARMED : words.idle;
        return (
          <button
            key={verdict} type="button" className={`ws-vbtn ${verdict}${armed === verdict ? ' armed' : ''}`}
            aria-label={armed === verdict ? `${ARMED} ${verdict}` : undefined}
            aria-busy={pending === verdict || undefined} disabled={Boolean(pending)} onClick={() => press(verdict)}
            onKeyDown={(event) => {
              repeating.current = event.repeat;
              if (event.repeat) event.preventDefault();
            }}
            onKeyUp={() => { repeating.current = false; }}
          >
            <span>{now}</span>
            {[words.idle, ARMED, words.pending].filter((word) => word !== now).map((word) => <span key={word} aria-hidden="true">{word}</span>)}
          </button>
        );
      })}
    </span>
  );
}

/**
 * The state of a row: its words, with the owner's Take and Drop at the end of their line, or of a line of their own
 * under them when both do not fit (workshop.css), and under that the id, the button that copies it and the votes. The
 * buttons come after `me` answers; for the owner remembered their room is held until then, so they move nothing. They
 * are live only once the entry's details (what a yes would change) show, since the owner decides on them too; until
 * then their room is held.
 *
 * The line whose button sent a verdict stays where it is on screen when the new state shows: the choice over it goes
 * then, several screens tall at times, and the result would leave the screen with it.
 */
function StateLine({ row, ctx, facts, children }: { row: Row; ctx: EntryContext; facts: Workshop['facts']; children?: ReactNode }) {
  const text = stateText(row);
  const tone = isDecided(row) && row.status !== 'dropped' ? 'ws-yes' : 'ws-muted';
  const id = planId(row);
  const refusal = id ? ctx.w.verdictRefusals[id] : undefined;
  const offered = offersVerdict(row);
  const detailsShown = !row.more || typeof facts === 'object';
  const line = useRef<HTMLDivElement>(null);
  const sentFrom = useRef<Row['status'] | null>(null);
  // Read while rendering: the page still shows the state before this one.
  const wasAt = sentFrom.current !== null && sentFrom.current !== row.status ? line.current?.getBoundingClientRect().top : undefined;
  useLayoutEffect(() => {
    if (wasAt === undefined || !line.current) return;
    sentFrom.current = null;
    const moved = line.current.getBoundingClientRect().top - wasAt;
    if (moved) scrollBy({ top: moved, behavior: 'instant' });
  }, [wasAt]);
  return (
    <>
      <div className="ws-state" ref={line}>
        <span className={`ws-state-text ${tone}`}>{text}</span>
        {offered && (ctx.w.mayDecide || ctx.w.verdictHeld) ? <VerdictButtons row={row} ctx={ctx} held={!ctx.w.mayDecide || !detailsShown} onSend={() => { sentFrom.current = row.status; }} /> : null}
        {id || children ? <span className="ws-state-more">{id ? <CopyId id={id} /> : null}{children}</span> : null}
      </div>
      {refusal ? <p className="ws-refused" role="alert">{refusal}</p> : null}
    </>
  );
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
  const url = n === null ? null : (typeof detail === 'object' && safeHref(detail.url)) || safeHref(`${repoAddress(repo)}/issues/${n}`);
  const github = url ? <a className="ws-gh" href={url} target="_blank" rel="noopener noreferrer">GitHub #{n} ↗</a> : null;
  if (!w.liveOk) return <div className="ws-talk"><div className="ws-box">{github}</div></div>;
  // An entry with no thread yet has no issue to link to; the room of the link is kept, so the row taking its thread moves nothing.
  const held = <span className="ws-gh ws-room" aria-hidden="true">GitHub #000 ↗</span>;
  // Once the number is known, the link keeps at least that room, so the text field keeps its width.
  const room = n === null ? held : <span className="ws-gh-slot">{github}{held}</span>;
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
            {/* The room of its 👍, in the button's box, until the comment is confirmed: it keeps its height then. */}
            <span className="ws-vote ws-cvote ws-cvote-room" aria-hidden="true"><Thumb up={0} /></span>
          </div>
        </div>
      ))}
      {box}
    </div>
  );
}

/** The commit on GitHub, for the link of a History line. */
const commitLink = (repo: string, commit: string) => safeHref(`${repoAddress(repo)}/commit/${commit}`);

/** How many ghost lines hold the place of the changes while their chunk loads. */
const HISTORY_GHOSTS = 12;

/** The phrases of a change on one line: a text change opens its before and after, a replacement links the decision that replaced it. */
function ChangePhrases({ change, row, ctx, panel, open, onToggle }: { change: Change; row: Row; ctx: EntryContext; panel: string; open: boolean; onToggle: () => void }) {
  const by = change.by !== undefined ? ctx.w.rowByKey.get(`entry-${change.by}`) : undefined;
  return (
    <span className="ws-h-what">
      {change.what.map((name, index) => {
        const phrase = changePhrase(name, change.agents === true);
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
  if (w.histories === 'error' || w.histories === 'changed') return <ChunkFailed what="History" state={w.histories} w={w} />;
  return (
    <div aria-busy="true">
      {Array.from({ length: Math.min(row.history?.count ?? 1, HISTORY_GHOSTS) }, (_, i) => <div className="ws-h-ghost" key={i} aria-hidden="true" />)}
      {w.plan.shallow ? <div className="ws-h-ghost" aria-hidden="true" /> : null}
    </div>
  );
}

/**
 * What shows in place of a chunk that did not load: that the site has changed since the page opened, or that it could
 * not load, and Reload, which always reaches it. While the page holds text not sent or a write being sent, the first
 * press only says so, and a second press reloads.
 */
function ChunkFailed({ what, state, w }: { what: string; state: 'error' | 'changed'; w: Workshop }) {
  const [warned, setWarned] = useState(false);
  const words = warned ? 'Text you typed is not sent yet.' : state === 'changed' ? 'The Workshop has changed since this page opened.' : `${what} could not load.`;
  const press = () => {
    if (!warned && w.unsent) setWarned(true);
    else pageConfig.reload();
  };
  return (
    <p className="ws-quiet">
      {words}{' '}
      <button type="button" className="ws-link" onClick={press}>{warned ? 'Reload anyway' : 'Reload'}</button>
    </p>
  );
}

/** The closed History line: the newest change with its date, and how many there are when more than one. */
function historySummary(row: Row): string {
  const last = row.history;
  if (!last) return '';
  return `${changeLine(last.what, last.agents)}, ${shortDate(last.date)}${last.count > 1 ? ` · ${last.count} changes` : ''}`;
}

/**
 * The entry's notes and its history, each closed until asked for; `onMove` tells the panel to grow or shrink to them.
 * While the choice shows in place, the notes are shown without it, and notes that hold nothing else have no button.
 * When the choice could not load, the notes hold it as any other of their sections.
 */
function Extras({ row, ctx, onMove }: { row: Row; ctx: EntryContext; onMove: () => void }) {
  const { w } = ctx;
  const [shown, setShown] = useState(false);
  const [history, setHistory] = useState(false);
  const inPlace = row.verdict === null && w.facts !== 'error' && w.facts !== 'changed';
  const hasNotes = row.notes && !(inPlace && row.choiceOnly);
  if (!hasNotes && !row.history) return null;
  const id = row.key.slice('entry-'.length);
  const kept = typeof w.notes === 'object' ? w.notes[id] : undefined;
  const notes = typeof kept !== 'object' ? kept : inPlace ? `${kept.before}${kept.after}` : `${kept.before}${kept.choice}${kept.after}`;
  return (
    <div className="ws-extras">
      <div className="ws-extras-bar">
        {hasNotes ? (
          <button
            type="button" className="ws-disc" aria-expanded={shown} aria-controls={`${row.key}-notes`}
            onPointerEnter={() => w.wantNotes()} onPointerDown={() => w.wantNotes()} onTouchStart={() => w.wantNotes()} onFocus={() => w.wantNotes()}
            onClick={() => { w.wantNotes(!shown); setShown((current) => !current); onMove(); }}
          >Notes</button>
        ) : null}
        {row.history ? (
          <button
            type="button" className="ws-disc ws-disc-history" aria-expanded={history} aria-controls={`${row.key}-history`}
            onPointerEnter={() => w.wantHistory()} onPointerDown={() => w.wantHistory()} onTouchStart={() => w.wantHistory()} onFocus={() => w.wantHistory()}
            onClick={() => { w.wantHistory(!history); setHistory((current) => !current); onMove(); }}
          >History<span> · {historySummary(row)}</span></button>
        ) : null}
      </div>
      {hasNotes && shown ? (
        <div className="ws-extras-in" id={`${row.key}-notes`}>
          {notes ? <Markdown text={notes} /> : w.notes === 'error' || w.notes === 'changed' ? <ChunkFailed what="Notes" state={w.notes} w={w} /> : typeof w.notes === 'object' ? null : (
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

/** Paragraphs of a choice: inline code and web links, as in notes; any other link keeps only its label. */
function ChoiceText({ text }: { text: string }) {
  return <RichText text={plainLinks(text)} className="ws-fact-text" />;
}

/** A choice of the known shape: the case, the options with what each costs, and why the recommended one. */
function ChoiceView({ choice }: { choice: ChoiceTable }) {
  const costs = choice.options.some((option) => option.cost);
  return (
    <div className="ws-choice">
      <div className="ws-fact ws-case">
        <span className="ws-fact-label">The case</span>
        <ChoiceText text={choice.case} />
      </div>
      <div className="ws-fact ws-options">
        <div className="ws-options-head">
          <span className="ws-fact-label">The options</span>
          {costs ? <span className="ws-fact-label">The cost</span> : null}
        </div>
        <ul>
          {choice.options.map((option, index) => (
            <li key={index} className={option.recommended ? 'rec' : undefined}>
              <span className="ws-option">
                <b>{inline(plainLinks(option.name), `option-${index}`)}</b>
                {option.recommended ? <span className="ws-tag">recommended</span> : null}
              </span>
              {option.what ? <p className={option.cost ? undefined : 'ws-span'}>{inline(plainLinks(option.what), `what-${index}`)}</p> : null}
              {option.cost ? <p className="ws-cost">{inline(plainLinks(option.cost), `cost-${index}`)}</p> : null}
            </li>
          ))}
        </ul>
      </div>
      <Fact label="Why this one">
        <ChoiceText text={choice.why} />
        {choice.whyMore ? <Markdown text={choice.whyMore} /> : null}
      </Fact>
    </div>
  );
}

const isTable = (choice: Choice | undefined): choice is ChoiceTable => choice !== undefined && !('text' in choice);

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

/** What the open view says of the lists and of what the entry needs, in the order of the plan's model. */
function OpenFacts({ row, ctx, state }: { row: Row; ctx: EntryContext; state: Workshop['facts'] }) {
  const { w } = ctx;
  const id = planId(row);
  const facts = typeof state === 'object' ? state : null;
  const own = facts && id !== null ? facts.entries[id] : undefined;
  const failed = row.more && (state === 'error' || state === 'changed') ? state : null;
  const waiting = row.more && !facts && !failed;
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
      {failed ? <div className="ws-fact"><ChunkFailed what="Details" state={failed} w={w} /></div> : null}
      {open && own?.suggest ? (
        <Fact label={`Recommended: ${own.suggest.verdict}`}>
          {/* A choice drawn as a table gives the reason under "Why this one". */}
          {own.suggest.why && !isTable(own.choice) ? <p className="ws-fact-text">{own.suggest.why}</p> : null}
        </Fact>
      ) : null}
      {open && own?.choice ? (
        isTable(own.choice) ? <ChoiceView choice={own.choice} /> : <div className="ws-fact"><Markdown text={own.choice.text} /></div>
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

function Body({ row, ctx, detail, facts, lead, vote, onExtras }: { row: Row; ctx: EntryContext; detail: Detail | undefined; facts: Workshop['facts']; lead?: ReactNode; vote: boolean; onExtras: () => void }) {
  const { w } = ctx;
  const what = row.kind === 'entry' ? row.what : typeof detail === 'object' ? detail.body : null;
  return (
    <div className="ws-more-pad">
      {lead}
      {what ? <div className="ws-what"><RichText text={what} /></div> : null}
      {row.kind === 'entry' ? <OpenFacts row={row} ctx={ctx} state={facts} /> : null}
      <StateLine row={row} ctx={ctx} facts={facts}>{vote ? <Side row={row} ctx={ctx} /> : null}</StateLine>
      <ReplaceLines row={row} ctx={ctx} only={(label) => label === 'Replaced by'} />
      {row.kind === 'entry' ? <Extras row={row} ctx={ctx} onMove={onExtras} /> : null}
      <Talk row={row} ctx={ctx} detail={detail} />
    </div>
  );
}
