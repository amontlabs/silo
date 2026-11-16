// One row of a list, and what it shows once open: the sentence, its state, recordings, comments and the box.
// Opening never moves the row or the rows above: the open frame is an overlay and the body grows below.
import { useEffect, useState } from 'react';
import type { ReactNode } from 'react';
import { RichText, safeHref } from './RichText';
import { awaitsConfirm, awaitsVerdict, isDecided, relativeTime, shortDate } from './plan';
import type { Row } from './plan';
import type { Workshop } from './useWorkshop';

/** How long the closing body stays in place for its animation. */
const CLOSE_MS = 240;
/** How long an opening panel waits for its comments, so it opens at its final size. */
export const HOLD_MS = 180;

export interface EntryContext {
  w: Workshop;
  signIn: string;
  onToggle: (row: Row) => void;
  mainAction: (row: Row) => void;
  /** The rows open now. */
  open: ReadonlySet<string>;
  /** Whether this row's verdict buttons act now: it was opened last and its guard has passed. */
  armed: (row: Row) => boolean;
  makeCurrent: (row: Row) => void;
}

const WORDS: Partial<Record<Row['status'], string>> = { planned: 'planned', later: 'later', building: 'building', built: 'built', dropped: 'dropped' };

export function pipClass(row: Row): string {
  if (row.kind === 'decision') return 'ws-pip undecided';
  if (row.kind === 'decided') return 'ws-pip planned';
  return `ws-pip ${row.status === 'refine' ? 'undecided' : row.status}`;
}

function initials(login: string): string {
  const parts = login.split(/[-_.]+/).filter(Boolean);
  return (parts.length > 1 ? parts[0][0] + parts[1][0] : login.slice(0, 2)).toLowerCase();
}

function stateText(row: Row): string {
  switch (row.status) {
    case 'planned': return 'Planned.';
    case 'later': return 'Later.';
    case 'refine': return 'Being refined.';
    case 'building': return 'Being built now.';
    case 'built': return row.closedAt ? `Built, ${shortDate(row.closedAt)}.` : 'Built.';
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

  const isSaving = row.issue !== null && w.saving[row.issue] === true;
  const live = row.issue !== null && Boolean(w.live.state?.issues[row.issue]);
  const owed = w.canDecide && live && awaitsVerdict(row);
  const confirm = w.canDecide && live && awaitsConfirm(row);
  const word = row.kind === 'entry' || row.kind === 'suggestion'
    ? (isSaving ? 'Saving…' : owed ? 'to decide' : confirm ? 'confirm' : WORDS[row.status])
    : undefined;
  const quiet = (row.kind === 'entry' || row.kind === 'suggestion') && !isDecided(row);
  const classes = ['ws-item', `st-${row.status}`, open ? 'open' : '', quiet ? 'quiet' : '', owed || confirm ? 'owed' : ''].filter(Boolean).join(' ');
  return (
    <li className={classes} id={row.key}>
      <div className="ws-line">
        <button type="button" className="ws-row" aria-expanded={open} aria-controls={`${row.key}-more`} onClick={() => ctx.onToggle(row)} onPointerEnter={() => w.prefetch(row)} onPointerDown={() => w.prefetch(row)} onTouchStart={() => w.prefetch(row)} onFocus={() => w.prefetch(row)}>
          <i className={pipClass(row)} aria-hidden="true" />
          <span className="ws-name">{row.name}</span>
          <b className={isSaving ? 'ws-word ws-pending' : 'ws-word'}>{word ?? ''}</b>
        </button>
        <Side row={row} ctx={ctx} />
      </div>
      <div className={grown ? 'ws-more open' : 'ws-more'} id={`${row.key}-more`} aria-hidden={open ? undefined : true} inert={!open}>
        <div className="ws-more-in">{open || closing ? <Body row={row} ctx={ctx} /> : null}</div>
      </div>
    </li>
  );
}

function Vote({ row, ctx }: { row: Row; ctx: EntryContext }) {
  const { w } = ctx;
  if (!w.liveOk || row.up === null || (row.kind !== 'entry' && row.kind !== 'suggestion')) return null;
  const on = row.issue !== null && w.votes[row.issue] === true;
  const busy = row.issue !== null && w.votePending.has(row.issue);
  if (!w.me) return <a className="ws-vote" href={ctx.signIn} title="Sign in to vote" aria-label={`${row.up} votes, sign in to vote`}>👍 {row.up}</a>;
  return (
    <button type="button" className={`ws-vote${on ? ' on' : ''}${busy ? ' ws-pending' : ''}`} aria-pressed={on} aria-busy={busy} aria-label={`${row.up} votes`} onClick={() => void w.toggleVote(row)} disabled={busy || (row.issue ?? 0) <= 0}>
      👍 {row.up}
    </button>
  );
}

function Side({ row, ctx }: { row: Row; ctx: EntryContext }) {
  const { w } = ctx;
  const showComments = w.liveOk && row.comments !== null && (row.kind === 'decision' || row.kind === 'decided' || row.comments > 0);
  return (
    <span className="ws-side">
      {row.kind === 'suggestion' && row.author ? <span className="ws-by">{row.author}</span> : null}
      {row.kind === 'decided' && row.date ? <span className="ws-by">{shortDate(row.date)}</span> : null}
      <span className="ws-votes"><Vote row={row} ctx={ctx} /></span>
      <span className="ws-talks">{showComments ? <span aria-label={`${row.comments} comments`}>💬 {row.comments}</span> : null}</span>
    </span>
  );
}

function NoteBox({ row, busy, ctx }: { row: Row; busy: boolean; ctx: EntryContext }) {
  const { w } = ctx;
  if (w.refining !== row.key) return null;
  const key = `note-${row.key}`;
  return (
    <form className="ws-box" onSubmit={(event) => {
      event.preventDefault();
      const note = (w.drafts[key] ?? '').trim();
      if (note) void w.sendVerdict(row, 'refine', note);
    }}>
      <textarea aria-label="Note" placeholder="Note…" maxLength={5000} rows={2} autoFocus value={w.drafts[key] ?? ''} onChange={(event) => w.setDrafts((current) => ({ ...current, [key]: event.target.value }))} />
      <button type="submit" className="ws-btn" disabled={busy || !(w.drafts[key] ?? '').trim()}>Refine</button>
    </form>
  );
}

function VerdictButtons({ row, ctx }: { row: Row; ctx: EntryContext }) {
  const { w } = ctx;
  const busy = row.issue !== null && w.pending.has(row.issue);
  // Until the guard has passed, the buttons look as inert as they are; on a row that was not opened last,
  // a press makes it the one the verdict goes to.
  const armed = ctx.armed(row);
  const cls = (name: string) => `${name}${armed ? '' : ' ws-unarmed'}`;
  const act = (fn: () => void) => () => {
    if (!ctx.armed(row)) {
      ctx.makeCurrent(row);
      return;
    }
    fn();
  };
  if (row.carried && row.fileVerdict) {
    return (
      <>
        <div className="ws-state">
          <span>{stateText(row).replace(/\.$/, '')} · carried over</span>
          <button type="button" className={cls('ws-btn go')} disabled={busy} onClick={act(() => ctx.mainAction(row))}>Confirm <kbd>↵</kbd></button>
        </div>
        <NoteBox row={row} busy={busy} ctx={ctx} />
      </>
    );
  }
  return (
    <>
      {row.status === 'refine' ? <div className="ws-state"><span className="ws-muted">Being refined.</span></div> : null}
      <div className="ws-state">
        <button type="button" className={cls('ws-btn go')} disabled={busy} onClick={act(() => ctx.mainAction(row))}>Take <kbd>↵</kbd></button>
        <button type="button" className={cls('ws-btn')} disabled={busy} onClick={act(() => void w.sendVerdict(row, 'later'))}>Later</button>
        <button type="button" className={cls('ws-btn')} disabled={busy} onClick={act(() => void w.sendVerdict(row, 'drop'))}>Drop</button>
        <button type="button" className={cls('ws-btn')} disabled={busy} aria-expanded={w.refining === row.key} onClick={() => { ctx.makeCurrent(row); w.setRefining(w.refining === row.key ? null : row.key); }}>Refine…</button>
      </div>
      <NoteBox row={row} busy={busy} ctx={ctx} />
      {row.suggest ? <p className="ws-hint">Suggested: {row.suggest}.{row.why ? ` ${row.why}` : ''}</p> : null}
    </>
  );
}

function StateLine({ row, ctx }: { row: Row; ctx: EntryContext }) {
  const { w } = ctx;
  const hasIssue = row.issue !== null && Boolean(w.live.state?.issues[row.issue]);
  if (row.kind === 'decided') {
    return <div className="ws-state"><span className="ws-date">{row.date ? shortDate(row.date) : ''} · {row.who}</span></div>;
  }
  if (row.kind === 'decision') {
    if (row.status === 'planned') return <div className="ws-state"><span className="ws-muted">Decided{row.date ? `, ${shortDate(row.date)}` : ''}.</span></div>;
    if (w.canDecide && hasIssue && w.pending.has(row.issue as number)) return null;
    if (w.canDecide && hasIssue) {
      const busy = w.pending.has(row.issue as number);
      return (
        <form className="ws-box" onSubmit={(event) => void w.sendDecide(row, event)}>
          <textarea aria-label="Decision" placeholder="Decision…" maxLength={5000} rows={2} value={w.drafts[row.key] ?? ''} onChange={(event) => w.setDrafts((current) => ({ ...current, [row.key]: event.target.value }))} />
          <button type="submit" className="ws-btn go" disabled={busy || !(w.drafts[row.key] ?? '').trim()}>Decide</button>
        </form>
      );
    }
    return <div className="ws-state"><span className="ws-muted">Not decided yet.</span></div>;
  }
  if (row.issue !== null && w.saving[row.issue]) return <div className="ws-state"><span className="ws-muted">Saving…</span></div>;
  if (w.canDecide && hasIssue && (row.issue as number) > 0 && (awaitsVerdict(row) || (row.carried && row.fileVerdict && row.status !== 'built'))) return <VerdictButtons row={row} ctx={ctx} />;
  const text = stateText(row);
  const tone = row.status === 'building' ? 'ws-now' : isDecided(row) && row.status !== 'dropped' ? 'ws-yes' : 'ws-muted';
  return (
    <div className="ws-state">
      <span className={tone}>{row.carried && w.canDecide ? `${text.replace(/\.$/, '')} · carried over` : text}</span>
    </div>
  );
}

function Talk({ row, ctx }: { row: Row; ctx: EntryContext }) {
  const { w } = ctx;
  if (row.issue === null || row.issue <= 0) return null;
  const n = row.issue;
  const detail = w.details[n];
  const url = (typeof detail === 'object' && safeHref(detail.url)) || safeHref(`https://github.com/${w.live.state?.repo ?? 'amontlabs/silo'}/issues/${n}`);
  const github = url ? <a className="ws-gh" href={url} target="_blank" rel="noopener noreferrer">GitHub #{n} ↗</a> : null;
  if (!w.liveOk) return <div className="ws-talk"><div className="ws-box">{github}</div></div>;
  let box: ReactNode = <div className="ws-box">{github}</div>;
  if (w.me) {
    box = (
      <form className="ws-box" onSubmit={(event) => void w.sendComment(row, event)}>
        <textarea aria-label="Comment" placeholder={w.canDecide ? 'Reply…' : 'Add a comment'} maxLength={5000} rows={1} value={w.drafts[row.key] ?? ''} onChange={(event) => w.setDrafts((current) => ({ ...current, [row.key]: event.target.value }))} />
        <button type="submit" className="ws-btn" disabled={!(w.drafts[row.key] ?? '').trim()}>Comment</button>
        {github}
      </form>
    );
  } else if (w.me === null) {
    box = <div className="ws-box"><a className="ws-signin" href={ctx.signIn}>Sign in to comment</a>{github}</div>;
  }
  return (
    <div className="ws-talk">
      {/* While the comments load, each one the count announces holds its place. */}
      {(detail === undefined || detail === 'loading') && row.comments ? Array.from({ length: Math.min(row.comments, 20) }, (_, i) => <div className="ws-c-ghost" key={i} aria-hidden="true" />) : null}
      {detail === 'error' ? <p className="ws-quiet">Comments could not load.</p> : null}
      {typeof detail === 'object' && detail.more && url ? <a className="ws-gh ws-earlier" href={url} target="_blank" rel="noopener noreferrer">Earlier comments on GitHub ↗</a> : null}
      {typeof detail === 'object'
        ? detail.comments.map((comment) => (
          <div className="ws-c" key={comment.id}>
            <i aria-hidden="true">{initials(comment.author)}</i>
            <div>
              <b>{comment.author} <small>{relativeTime(comment.createdAt)}</small></b>
              <RichText text={comment.text} />
            </div>
          </div>
        ))
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

function Body({ row, ctx }: { row: Row; ctx: EntryContext }) {
  const { w } = ctx;
  const detail = row.issue === null ? undefined : w.details[row.issue];
  const what = row.kind === 'entry' || row.kind === 'decided' ? row.what : typeof detail === 'object' ? detail.body : null;
  return (
    <div className="ws-more-pad">
      {what ? <div className="ws-what"><RichText text={what} /></div> : null}
      {row.recordings.map((file) => (
        <video className="ws-recording" key={file} controls preload="metadata" src={`/workshop/recordings/${encodeURIComponent(file)}`} />
      ))}
      {w.rowNotice?.key === row.key ? <p className="ws-row-notice" role="alert">{w.rowNotice.text}</p> : null}
      <StateLine row={row} ctx={ctx} />
      <Talk row={row} ctx={ctx} />
    </div>
  );
}
