import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { FormEvent, ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { api, ApiError, loginUrl } from './api';
import type { Comment, IssueDetail, Me } from './api';
import { RichText, safeHref } from './RichText';
import { applyKept, keep, keptComments } from './writes';
import {
  allRows, awaitsVerdict, buildPlan, isDecided, relativeTime, shortDate, stagePhrase,
} from './plan';
import type { BuildData, LiveIssue, LiveState, Row, Verdict } from './plan';

/** A row that just opened refuses its main action for this long. */
export const OPEN_GUARD_MS = 700;
const NOTICE_MS = 8000;

function decodeHash(hash: string): string | null {
  try {
    return decodeURIComponent(hash.replace(/^#/, '')) || null;
  } catch {
    return null;
  }
}

function takeSigninFailure(): boolean {
  const params = new URLSearchParams(location.search);
  if (params.get('signin') !== 'failed') return false;
  params.delete('signin');
  const search = params.toString();
  history.replaceState(null, '', `${location.pathname}${search ? `?${search}` : ''}${location.hash}`);
  return true;
}

type Live = { status: 'loading' | 'ok' | 'unavailable'; state: LiveState | null };
type Detail = IssueDetail | 'loading' | 'error';

export interface AppProps {
  data: BuildData;
  /** The element in the site header that holds sign-in and the signed-in login. */
  accountSlot?: Element | null;
}

const WORDS: Partial<Record<Row['status'], string>> = { planned: 'planned', later: 'later', building: 'building', built: 'built' };

function dotClass(row: Row): string {
  if (row.kind === 'decision') return 'ws-dot';
  if (row.status === 'planned' || row.status === 'building' || row.status === 'built') return 'ws-dot yes';
  if (row.status === 'later') return 'ws-dot later';
  return 'ws-dot';
}

function initials(login: string): string {
  const parts = login.split(/[-_.]+/).filter(Boolean);
  return (parts.length > 1 ? parts[0][0] + parts[1][0] : login.slice(0, 2)).toLowerCase();
}

function isField(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName);
}

function verdictPatch(verdict: Verdict | null): Partial<LiveIssue> {
  return verdict === 'drop'
    ? { verdict: 'drop', state: 'closed', reason: 'not_planned' }
    : { verdict, state: 'open', reason: null, closedAt: null };
}

function stateText(row: Row): string {
  switch (row.status) {
    case 'planned': return 'Planned.';
    case 'later': return 'Later.';
    case 'refine': return 'Being refined.';
    case 'building': return 'Building.';
    case 'built': return row.closedAt ? `Built, ${shortDate(row.closedAt)}.` : 'Built.';
    case 'dropped': return 'Dropped.';
    default: return 'Not decided yet.';
  }
}

export function App({ data, accountSlot }: AppProps) {
  const [live, setLive] = useState<Live>({ status: 'loading', state: null });
  const [me, setMe] = useState<Me | null | undefined>(undefined);
  const [details, setDetails] = useState<Record<number, Detail>>({});
  const [openKey, setOpenKey] = useState<string | null>(null);
  const [tab, setTab] = useState<'plan' | 'decisions'>('plan');
  const [stageOpen, setStageOpen] = useState<Record<string, boolean>>({});
  const [droppedOpen, setDroppedOpen] = useState(false);
  const [pending, setPending] = useState<ReadonlySet<number>>(new Set());
  const [notice, setNotice] = useState<string | null>(null);
  const [rowNotice, setRowNotice] = useState<{ key: string; text: string } | null>(null);
  // Writes the server has not answered yet. Nothing here counts as done.
  const [saving, setSaving] = useState<Record<number, true>>({});
  const [sending, setSending] = useState<Record<number, Comment[]>>({});
  const [votePending, setVotePending] = useState<ReadonlySet<number>>(new Set());
  const [suggestionsSending, setSuggestionsSending] = useState<{ id: number; title: string; author: string }[]>([]);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [refining, setRefining] = useState<string | null>(null);
  const [suggesting, setSuggesting] = useState(false);
  const [suggestDraft, setSuggestDraft] = useState({ title: '', text: '' });
  const [announce, setAnnounce] = useState('');
  const [focusKey, setFocusKey] = useState<string | null>(null);
  const [votes, setVotes] = useState<Record<number, boolean>>({});
  const [signinFailed] = useState(takeSigninFailure);
  const openedAt = useRef(0);
  const openKeyRef = useRef<string | null>(null);
  const pendingRef = useRef(new Set<number>());
  const autoOpened = useRef(false);
  const wantedHash = useRef<string | null>(null);

  const liveOk = live.status === 'ok';
  const canDecide = liveOk && me?.canDecide === true;
  const plan = useMemo(() => buildPlan(data, live.state), [data, live.state]);
  const rows = useMemo(() => allRows(plan), [plan]);
  const rowByKey = useMemo(() => new Map(rows.map((row) => [row.key, row])), [rows]);
  const queue = useMemo(() => [...plan.stages.flatMap((group) => group.rows), ...plan.suggestions], [plan]);

  useEffect(() => {
    let cancelled = false;
    api.state().then(
      (state) => { if (!cancelled) setLive({ status: 'ok', state: applyKept({ ...state, issues: state.issues ?? {} }, Date.now()) }); },
      () => { if (!cancelled) setLive({ status: 'unavailable', state: null }); },
    );
    api.me().then(
      (who) => { if (!cancelled) setMe(who); },
      () => { if (!cancelled) setMe(null); },
    );
    return () => { cancelled = true; };
  }, []);

  useEffect(() => {
    if (!notice) return;
    const timer = setTimeout(() => setNotice(null), NOTICE_MS);
    return () => clearTimeout(timer);
  }, [notice]);

  const patchIssue = useCallback((n: number, patch: (issue: LiveIssue) => Partial<LiveIssue> | null) => {
    setLive((current) => {
      const issue = current.state?.issues[n];
      if (!current.state || !issue) return current;
      const change = patch(issue);
      if (!change) return current;
      return { ...current, state: { ...current.state, issues: { ...current.state.issues, [n]: { ...issue, ...change } } } };
    });
  }, []);

  const loadDetail = useCallback((n: number, force = false) => {
    if (n <= 0) return;
    setDetails((current) => (current[n] && current[n] !== 'error' && !force ? current : { ...current, [n]: current[n] && current[n] !== 'error' ? current[n] : 'loading' }));
    api.issue(n).then(
      (detail) => setDetails((current) => ({ ...current, [n]: { ...detail, comments: keptComments(n, detail.comments ?? [], Date.now()) } })),
      () => setDetails((current) => (typeof current[n] === 'object' ? current : { ...current, [n]: 'error' })),
    );
  }, []);

  const setHash = (key: string | null) => {
    const url = key ? `#${key}` : `${location.pathname}${location.search}`;
    if ((key ? `#${key}` : '') !== location.hash) history.replaceState(null, '', url);
  };

  const openRow = useCallback((key: string | null, options: { focus?: boolean } = {}) => {
    setOpenKey(key);
    openKeyRef.current = key;
    setRefining(null);
    setRowNotice((current) => (current?.key === key ? current : null));
    openedAt.current = Date.now();
    setHash(key);
    if (!key) return;
    const row = rowByKey.get(key);
    if (!row) return;
    setAnnounce(`${row.name}, open`);
    if (row.kind === 'decision' || row.kind === 'decided') setTab('decisions');
    else setTab('plan');
    if (row.status === 'dropped') setDroppedOpen(true);
    if (row.stage) setStageOpen((current) => ({ ...current, [row.stage as string]: true }));
    if (row.issue !== null && liveOk) loadDetail(row.issue);
    if (options.focus) setFocusKey(key);
  }, [rowByKey, liveOk, loadDetail]);

  const toggleRow = (row: Row) => openRow(openKey === row.key ? null : row.key);

  // Details of the open row load once the live state arrives; a signed-in person's own vote with them.
  useEffect(() => {
    const row = openKey ? rowByKey.get(openKey) : undefined;
    if (liveOk && row?.issue != null && details[row.issue] === undefined) loadDetail(row.issue);
  }, [liveOk, openKey, rowByKey, details, loadDetail]);
  // One check of the person's own vote per opening of a row, never retried: a failure leaves 👍 off.
  const signedInAs = me?.login ?? null;
  useEffect(() => {
    const row = openKey ? rowByKey.get(openKey) : undefined;
    const n = row?.issue;
    if (!liveOk || !signedInAs || n == null || n <= 0 || (row?.kind !== 'entry' && row?.kind !== 'suggestion')) return;
    api.myVote(n).then(
      (on) => setVotes((current) => (n in current ? current : { ...current, [n]: on })),
      (error) => {
        if (error instanceof ApiError && error.status === 401) signedOut();
      },
    );
    // Re-rendering, or the plan changing under the same open row, does not ask again.
  }, [openKey, liveOk, signedInAs]);

  // Deep links: `#entry-<id>`, `#issue-<n>` or `#decided-<i>` opens that row, now or once it exists.
  const openRowRef = useRef(openRow);
  openRowRef.current = openRow;
  const rowByKeyRef = useRef(rowByKey);
  rowByKeyRef.current = rowByKey;
  useEffect(() => {
    const follow = () => {
      const key = decodeHash(location.hash);
      if (!key) {
        wantedHash.current = null;
        return;
      }
      if (rowByKeyRef.current.has(key)) {
        wantedHash.current = null;
        openRowRef.current(key, { focus: true });
      } else wantedHash.current = key;
    };
    follow();
    addEventListener('hashchange', follow);
    return () => removeEventListener('hashchange', follow);
  }, []);
  useEffect(() => {
    const key = wantedHash.current;
    if (!key || !rowByKey.has(key)) return;
    wantedHash.current = null;
    openRow(key, { focus: true });
  }, [rowByKey, openRow]);

  // A link to a row that does not exist falls back to the normal start once the live state is known.
  useEffect(() => {
    if (live.status === 'loading' || !wantedHash.current || rowByKey.has(wantedHash.current)) return;
    wantedHash.current = null;
    setHash(null);
  }, [live.status, rowByKey]);

  // A person who can decide starts on the first undecided entry.
  useEffect(() => {
    if (!canDecide || autoOpened.current) return;
    autoOpened.current = true;
    if (wantedHash.current && !rowByKey.has(wantedHash.current)) {
      wantedHash.current = null;
      setHash(null);
    }
    if (openKeyRef.current || wantedHash.current) return;
    const first = queue.find((row) => awaitsVerdict(row) && live.state?.issues[row.issue as number]);
    if (first) openRow(first.key, { focus: true });
  }, [canDecide, queue, openRow, live.state, rowByKey]);

  useEffect(() => {
    if (!focusKey) return;
    const item = document.getElementById(focusKey);
    const button = item?.querySelector<HTMLElement>('.ws-row');
    button?.focus({ preventScroll: true });
    item?.scrollIntoView?.({ block: 'nearest' });
    setFocusKey(null);
  }, [focusKey]);

  const setBusy = (n: number, busy: boolean) => {
    if (busy) pendingRef.current.add(n);
    else pendingRef.current.delete(n);
    setPending(new Set(pendingRef.current));
  };

  const liveRef = useRef(live);
  liveRef.current = live;
  /** The issue as confirmed so far, for computing what a confirmed write changes. */
  const confirmedIssue = (n: number) => liveRef.current.state?.issues[n];
  const remember = (n: number, fields: Partial<LiveIssue>, comment?: Comment) => {
    if (n > 0) keep(n, fields, Date.now(), comment);
  };

  function signedOut() {
    setMe(null);
    setVotes({});
    setRefining(null);
    setNotice('Sign in again.');
  }

  const ownComment = (text: string): Comment | null => (me
    ? { id: `own-${Date.now()}-${Math.random().toString(36).slice(2)}`, author: me.login, avatar: me.avatar, bot: false, createdAt: new Date().toISOString(), text }
    : null);
  const startSending = (n: number, comment: Comment | null) => {
    if (comment) setSending((current) => ({ ...current, [n]: [...(current[n] ?? []), comment] }));
  };
  const stopSending = (n: number, comment: Comment | null) => {
    if (comment) setSending((current) => ({ ...current, [n]: (current[n] ?? []).filter((c) => c.id !== comment.id) }));
  };
  /** A comment the server confirmed joins the thread as an ordinary one. */
  const confirmComment = (n: number, comment: Comment | null) => {
    if (!comment) return;
    setDetails((current) => {
      const detail = current[n];
      return typeof detail === 'object' ? { ...current, [n]: { ...detail, comments: [...detail.comments, { ...comment, createdAt: new Date().toISOString() }] } } : current;
    });
  };

  const failed = (what: string, error: unknown) => {
    if (error instanceof ApiError && error.status === 401) {
      signedOut();
      return;
    }
    const message = error instanceof ApiError ? error.message : 'Something went wrong.';
    setNotice(`${what}: not saved. ${message}`);
  };

  const rowShown = (row: Row): boolean => {
    const onDecisions = row.kind === 'decision' || row.kind === 'decided';
    if ((tab === 'decisions') !== onDecisions) return false;
    if (row.status === 'dropped' && (row.kind === 'entry' || row.kind === 'suggestion')) return droppedOpen;
    if (row.kind === 'suggestion') return liveOk;
    if (row.kind === 'entry') {
      const index = plan.stages.findIndex((group) => group.stage.id === row.stage);
      return index !== -1 && (index <= plan.lastOpenStage || stageOpen[row.stage as string] === true);
    }
    return true;
  };

  /** The one guard every verdict passes, by key or by click. */
  const verdictReady = (row: Row): boolean => {
    const n = row.issue;
    return canDecide && n !== null && n > 0 && Boolean(live.state?.issues[n]) && !pendingRef.current.has(n)
      && openKeyRef.current === row.key && Date.now() - openedAt.current >= OPEN_GUARD_MS;
  };

  const advanceFrom = (row: Row) => {
    const start = queue.findIndex((item) => item.key === row.key);
    const ordered = start === -1 ? queue : [...queue.slice(start + 1), ...queue.slice(0, start)];
    const next = ordered.find((item) => item.key !== row.key && awaitsVerdict(item) && !pendingRef.current.has(item.issue as number));
    if (next) openRow(next.key, { focus: true });
    else openRow(null);
  };

  const sendVerdict = async (row: Row, verdict: Verdict | null, note?: string) => {
    const n = row.issue;
    if (n === null || !live.state?.issues[n] || !verdictReady(row)) return;
    if (note !== undefined) {
      setDrafts((current) => ({ ...current, [`note-${row.key}`]: '' }));
      setRefining(null);
    }
    const own = note === undefined ? null : ownComment(note);
    setBusy(n, true);
    setSaving((current) => ({ ...current, [n]: true }));
    startSending(n, own);
    setNotice(null);
    setRowNotice(null);
    // The queue moves on at once; the row shows "Saving…" until the server answers.
    if (openKeyRef.current === row.key) {
      setAnnounce('');
      advanceFrom(row);
      setAnnounce((next) => (next ? `${row.name}: saving. ${next}` : `${row.name}: saving`));
    } else setAnnounce(`${row.name}: saving`);
    try {
      await api.verdict(n, verdict, note);
      const confirmed: Partial<LiveIssue> = { ...verdictPatch(verdict), ...(own ? { comments: (confirmedIssue(n)?.comments ?? 0) + 1 } : {}) };
      patchIssue(n, () => confirmed);
      confirmComment(n, own);
      remember(n, confirmed, own ?? undefined);
      if (own) loadDetail(n, true);
    } catch (error) {
      if (error instanceof ApiError && error.status === 401) {
        signedOut();
        return;
      }
      // Back to the row that was refused, with what was open there.
      openRow(row.key, { focus: true });
      if (note !== undefined) {
        setDrafts((current) => ({ ...current, [`note-${row.key}`]: current[`note-${row.key}`] || note }));
        setRefining(row.key);
      }
      const message = error instanceof ApiError ? error.message : 'Something went wrong.';
      setRowNotice({ key: row.key, text: `${row.name}: not saved. ${message}` });
    } finally {
      setBusy(n, false);
      setSaving((current) => {
        const next = { ...current };
        delete next[n];
        return next;
      });
      stopSending(n, own);
    }
  };

  const mainAction = (row: Row) => {
    if (!verdictReady(row)) return;
    if (awaitsVerdict(row)) void sendVerdict(row, 'take');
    else if (row.carried && row.fileVerdict === 'refine') setRefining(row.key);
    else if (row.carried && row.fileVerdict) void sendVerdict(row, row.fileVerdict);
  };

  const keyHandler = useRef<(event: KeyboardEvent) => void>(() => {});
  keyHandler.current = (event: KeyboardEvent) => {
    if (event.key !== 'Enter' || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    if (!canDecide || !openKey || isField(event.target)) return;
    const row = rowByKey.get(openKey);
    if (!row || !(awaitsVerdict(row) || row.carried) || !rowShown(row)) return;
    const target = event.target as HTMLElement | null;
    const item = document.getElementById(openKey);
    // Other buttons and links in the row keep their own Enter.
    const control = target?.closest('button, a, summary');
    const onRow = target === document.body || target === document.documentElement
      || (item !== null && target !== null && item.contains(target) && (!control || control.classList.contains('ws-row')));
    if (!onRow) return;
    event.preventDefault();
    if (event.repeat) return;
    mainAction(row);
  };
  useEffect(() => {
    const listener = (event: KeyboardEvent) => keyHandler.current(event);
    document.addEventListener('keydown', listener);
    return () => document.removeEventListener('keydown', listener);
  }, []);

  const sendComment = async (row: Row, event: FormEvent) => {
    event.preventDefault();
    const n = row.issue;
    const text = (drafts[row.key] ?? '').trim();
    const own = ownComment(text);
    if (n === null || n <= 0 || !text || !own) return;
    setDrafts((current) => ({ ...current, [row.key]: '' }));
    startSending(n, own);
    setAnnounce('Sending comment');
    try {
      await api.comment(n, text);
      const comments = (confirmedIssue(n)?.comments ?? 0) + 1;
      patchIssue(n, () => ({ comments }));
      confirmComment(n, own);
      remember(n, { comments }, own);
    } catch (error) {
      setDrafts((current) => ({ ...current, [row.key]: current[row.key] ? current[row.key] : text }));
      failed('Comment', error);
    } finally {
      stopSending(n, own);
    }
  };

  const sendDecide = async (row: Row, event: FormEvent) => {
    event.preventDefault();
    const n = row.issue;
    const text = (drafts[row.key] ?? '').trim();
    const own = ownComment(text);
    if (n === null || n <= 0 || !text || !own || !live.state?.issues[n] || pendingRef.current.has(n)) return;
    setBusy(n, true);
    setDrafts((current) => ({ ...current, [row.key]: '' }));
    startSending(n, own);
    setAnnounce(`${row.name}: sending`);
    try {
      await api.decide(n, text);
      const confirmed: Partial<LiveIssue> = { state: 'closed', reason: 'completed', closedAt: new Date().toISOString(), comments: (confirmedIssue(n)?.comments ?? 0) + 1 };
      patchIssue(n, () => confirmed);
      confirmComment(n, own);
      remember(n, confirmed, own);
      loadDetail(n, true);
    } catch (error) {
      setDrafts((current) => ({ ...current, [row.key]: current[row.key] ? current[row.key] : text }));
      failed(row.name, error);
    } finally {
      setBusy(n, false);
      stopSending(n, own);
    }
  };

  const votingRef = useRef(new Set<number>());
  const toggleVote = async (row: Row) => {
    const n = row.issue;
    if (n === null || n <= 0 || !me || votingRef.current.has(n)) return;
    votingRef.current.add(n);
    setVotePending(new Set(votingRef.current));
    setAnnounce('Sending vote');
    try {
      const was = n in votes ? votes[n] : await api.myVote(n);
      const on = !was;
      await api.vote(n, on);
      const up = Math.max(0, (confirmedIssue(n)?.up ?? 0) + (on ? 1 : -1));
      setVotes((current) => ({ ...current, [n]: on }));
      patchIssue(n, () => ({ up }));
      remember(n, { up });
    } catch (error) {
      failed('Vote', error);
    } finally {
      votingRef.current.delete(n);
      setVotePending(new Set(votingRef.current));
    }
  };

  const sendSuggestion = async (event: FormEvent) => {
    event.preventDefault();
    const title = suggestDraft.title.trim();
    const text = suggestDraft.text.trim();
    if (!title || !me || !live.state) return;
    const id = Date.now() + Math.random();
    const author = me.login;
    setSuggestionsSending((current) => [...current, { id, title, author }]);
    setSuggesting(false);
    setSuggestDraft({ title: '', text: '' });
    setTab('plan');
    setAnnounce('Sending suggestion');
    try {
      const { number } = await api.suggest(title, text);
      const issue: LiveIssue = { kind: 'suggestion', entry: null, title, state: 'open', reason: null, verdict: null, up: 0, comments: 0, author, bot: false, createdAt: new Date().toISOString(), closedAt: null };
      setLive((current) => (current.state ? { ...current, state: { ...current.state, issues: { ...current.state.issues, [number]: issue } } } : current));
      setDetails((current) => ({ ...current, [number]: { number, title, body: text, url: '', up: 0, comments: [] } }));
      keep(number, issue, Date.now());
    } catch (error) {
      setSuggestDraft({ title, text });
      setSuggesting(true);
      failed('Suggestion', error);
    } finally {
      setSuggestionsSending((current) => current.filter((item) => item.id !== id));
    }
  };

  const signOut = async () => {
    try {
      await api.logout();
      setMe(null);
      setVotes({});
    } catch (error) {
      failed('Sign out', error);
    }
  };

  const hash = openKey ? `#${openKey}` : '';
  const signIn = loginUrl(hash);

  const account = !liveOk || me === undefined ? null : me ? (
    <span className="ws-account">
      <span className="ws-login">{me.login}</span>
      <button type="button" className="ws-link" onClick={() => void signOut()}>Sign out</button>
    </span>
  ) : (
    <a className="button small" href={signIn}>Sign in with GitHub</a>
  );

  const vote = (row: Row): ReactNode => {
    if (!liveOk || row.up === null || (row.kind !== 'entry' && row.kind !== 'suggestion')) return null;
    const on = row.issue !== null && votes[row.issue] === true;
    const busy = row.issue !== null && votePending.has(row.issue);
    if (!me) return <a className="ws-vote" href={signIn} aria-label={`${row.up} votes, sign in to vote`}>👍 {row.up}</a>;
    return (
      <button type="button" className={`ws-vote${on ? ' on' : ''}${busy ? ' ws-pending' : ''}`} aria-pressed={on} aria-busy={busy} aria-label={`${row.up} votes`} onClick={() => void toggleVote(row)} disabled={busy || (row.issue ?? 0) <= 0}>
        👍 {row.up}
      </button>
    );
  };

  const side = (row: Row): ReactNode => {
    const showComments = liveOk && row.comments !== null && (row.kind === 'decision' || row.kind === 'decided' || row.comments > 0);
    return (
      <span className="ws-side">
        {row.kind === 'suggestion' && row.author ? <span>{row.author}</span> : null}
        {row.kind === 'decided' && row.date ? <span>{shortDate(row.date)}</span> : null}
        {vote(row)}
        {showComments ? <span aria-label={`${row.comments} comments`}>💬 {row.comments}</span> : null}
      </span>
    );
  };

  const noteBox = (row: Row, busy: boolean): ReactNode => (refining === row.key ? (
    <form className="ws-box" onSubmit={(event) => {
      event.preventDefault();
      const note = (drafts[`note-${row.key}`] ?? '').trim();
      if (note) void sendVerdict(row, 'refine', note);
    }}>
      <textarea aria-label="Note" placeholder="Note…" maxLength={5000} rows={2} autoFocus value={drafts[`note-${row.key}`] ?? ''} onChange={(event) => setDrafts((current) => ({ ...current, [`note-${row.key}`]: event.target.value }))} />
      <button type="submit" className="ws-btn" disabled={busy || !(drafts[`note-${row.key}`] ?? '').trim()}>Refine</button>
    </form>
  ) : null);

  const verdictButtons = (row: Row): ReactNode => {
    const busy = row.issue !== null && pending.has(row.issue);
    if (row.carried && row.fileVerdict) {
      return (
        <>
          <div className="ws-state">
            <span>{stateText(row).replace(/\.$/, '')} · carried over</span>
            <button type="button" className="ws-btn go" disabled={busy} onClick={() => mainAction(row)}>Confirm <kbd>↵</kbd></button>
          </div>
          {noteBox(row, busy)}
        </>
      );
    }
    return (
      <>
        {row.status === 'refine' ? <div className="ws-state"><span className="ws-muted">Being refined.</span></div> : null}
        <div className="ws-state">
          <button type="button" className="ws-btn go" disabled={busy} onClick={() => mainAction(row)}>Take <kbd>↵</kbd></button>
          <button type="button" className="ws-btn" disabled={busy} onClick={() => void sendVerdict(row, 'later')}>Later</button>
          <button type="button" className="ws-btn" disabled={busy} onClick={() => void sendVerdict(row, 'drop')}>Drop</button>
          <button type="button" className="ws-btn" disabled={busy} aria-expanded={refining === row.key} onClick={() => setRefining(refining === row.key ? null : row.key)}>Refine…</button>
        </div>
        {noteBox(row, busy)}
        {row.suggest ? <p className="ws-hint">Suggested: {row.suggest}.{row.why ? ` ${row.why}` : ''}</p> : null}
      </>
    );
  };

  const stateLine = (row: Row): ReactNode => {
    const hasIssue = row.issue !== null && Boolean(live.state?.issues[row.issue]);
    if (row.kind === 'decided') {
      return <div className="ws-state"><span className="ws-date">{row.date ? shortDate(row.date) : ''} · {row.who}</span></div>;
    }
    if (row.kind === 'decision') {
      const decided = row.status === 'planned';
      if (decided) return <div className="ws-state"><span className="ws-muted">Decided{row.date ? `, ${shortDate(row.date)}` : ''}.</span></div>;
      if (canDecide && hasIssue && pending.has(row.issue as number)) return null;
      if (canDecide && hasIssue) {
        const busy = pending.has(row.issue as number);
        return (
          <form className="ws-box" onSubmit={(event) => void sendDecide(row, event)}>
            <textarea aria-label="Decision" placeholder="Decision…" maxLength={5000} rows={2} value={drafts[row.key] ?? ''} onChange={(event) => setDrafts((current) => ({ ...current, [row.key]: event.target.value }))} />
            <button type="submit" className="ws-btn go" disabled={busy || !(drafts[row.key] ?? '').trim()}>Decide</button>
          </form>
        );
      }
      return <div className="ws-state"><span className="ws-muted">Not decided yet.</span></div>;
    }
    if (row.issue !== null && saving[row.issue]) return <div className="ws-state"><span className="ws-muted">Saving…</span></div>;
    if (canDecide && hasIssue && (row.issue as number) > 0 && (awaitsVerdict(row) || (row.carried && row.fileVerdict && row.status !== 'built'))) return verdictButtons(row);
    const text = stateText(row);
    return (
      <div className="ws-state">
        <span className={isDecided(row) && row.status !== 'dropped' ? 'ws-yes' : 'ws-muted'}>{row.carried ? `${text.replace(/\.$/, '')} · carried over` : text}</span>
      </div>
    );
  };

  const talk = (row: Row): ReactNode => {
    if (row.issue === null || row.issue <= 0) return null;
    const n = row.issue;
    const detail = details[n];
    const url = (typeof detail === 'object' && safeHref(detail.url)) || safeHref(`https://github.com/${live.state?.repo ?? 'amontlabs/silo'}/issues/${n}`);
    const github = url ? <a className="ws-mono" href={url} target="_blank" rel="noopener noreferrer">GitHub #{n} ↗</a> : null;
    if (!liveOk) return <div className="ws-talk"><div className="ws-box">{github}</div></div>;
    return (
      <div className="ws-talk">
        {detail === 'loading' ? <p className="ws-quiet" aria-live="polite">Loading…</p> : null}
        {detail === 'error' ? <p className="ws-quiet">Comments could not load.</p> : null}
        {typeof detail === 'object' && detail.more && url ? <a className="ws-mono ws-earlier" href={url} target="_blank" rel="noopener noreferrer">Earlier comments on GitHub ↗</a> : null}
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
        {(sending[n] ?? []).map((comment) => (
          <div className="ws-c ws-pending" key={comment.id} aria-busy="true">
            <i aria-hidden="true">{initials(comment.author)}</i>
            <div>
              <b>{comment.author} <small>Sending…</small></b>
              <RichText text={comment.text} />
            </div>
          </div>
        ))}
        {me ? (
          <form className="ws-box" onSubmit={(event) => void sendComment(row, event)}>
            <textarea aria-label="Comment" placeholder="Reply…" maxLength={5000} rows={1} value={drafts[row.key] ?? ''} onChange={(event) => setDrafts((current) => ({ ...current, [row.key]: event.target.value }))} />
            <button type="submit" className="ws-btn" disabled={!(drafts[row.key] ?? '').trim()}>Comment</button>
            {github}
          </form>
        ) : me === null ? (
          <div className="ws-box"><a className="ws-signin" href={signIn}>Sign in to comment</a>{github}</div>
        ) : <div className="ws-box">{github}</div>}
      </div>
    );
  };

  const more = (row: Row): ReactNode => {
    const detail = row.issue === null ? undefined : details[row.issue];
    const what = row.kind === 'entry' || row.kind === 'decided' ? row.what : typeof detail === 'object' ? detail.body : null;
    return (
      <div className="ws-more" id={`${row.key}-more`}>
        {what ? <div className="ws-what"><RichText text={what} /></div> : null}
        {row.recordings.map((file) => (
          <video className="ws-recording" key={file} controls preload="metadata" src={`/workshop/recordings/${encodeURIComponent(file)}`} />
        ))}
        {rowNotice?.key === row.key ? <p className="ws-notice ws-row-notice" role="alert">{rowNotice.text}</p> : null}
        {stateLine(row)}
        {talk(row)}
      </div>
    );
  };

  const item = (row: Row): ReactNode => {
    const open = openKey === row.key;
    const quiet = (row.kind === 'entry' || row.kind === 'suggestion') && !isDecided(row);
    const isSaving = row.issue !== null && saving[row.issue] === true;
    const word = row.kind === 'entry' || row.kind === 'suggestion' ? (isSaving ? 'Saving…' : WORDS[row.status]) : undefined;
    return (
      <div className={open ? 'ws-item open' : 'ws-item'} id={row.key} key={row.key}>
        <div className="ws-line">
          <button type="button" className={quiet ? 'ws-row undecided' : 'ws-row'} aria-expanded={open} aria-controls={open ? `${row.key}-more` : undefined} onClick={() => toggleRow(row)}>
            <span className={dotClass(row)} aria-hidden="true" />
            <span className="ws-name">{row.name}</span>
            {word ? <b className={isSaving ? 'ws-word ws-pending' : 'ws-word'}>{word}</b> : null}
          </button>
          {side(row)}
        </div>
        {open ? more(row) : null}
      </div>
    );
  };

  const planView = (
    <>
      {plan.stages.map((group, index) => {
        const auto = index <= plan.lastOpenStage;
        // A stage open by rule stays open; only later stages have a toggle.
        const shown = auto || stageOpen[group.stage.id] === true;
        const phrase = stagePhrase(group.stage.line);
        const title = <>{group.stage.title}{phrase ? <span>{phrase}</span> : null}</>;
        return (
          <section key={group.stage.id} aria-label={group.stage.title}>
            <div className="ws-stage">
              <span className="ws-num">{group.number}</span>
              <h2>
                {auto ? title : (
                  <button type="button" className="ws-stage-toggle" aria-expanded={shown} onClick={() => setStageOpen((current) => ({ ...current, [group.stage.id]: !shown }))}>{title}</button>
                )}
              </h2>
              <span className="ws-mono">{group.decided} of {group.rows.length}</span>
            </div>
            {shown ? group.rows.map(item) : null}
            {group.stage.id === 'living' ? <div className="ws-cut" role="separator">USABLE EVERY DAY FROM HERE</div> : null}
          </section>
        );
      })}
      {liveOk && (plan.suggestions.length || suggestionsSending.length) ? (
        <section aria-label="Suggested by people">
          <div className="ws-stage ws-suggested"><span className="ws-num">+</span><h2>Suggested by people</h2><span className="ws-mono">sorted by votes</span></div>
          {plan.suggestions.map(item)}
          {suggestionsSending.map((pendingSuggestion) => (
            <div className="ws-item ws-pending" key={pendingSuggestion.id} aria-busy="true">
              <div className="ws-line">
                <div className="ws-row undecided">
                  <span className="ws-dot" aria-hidden="true" />
                  <span className="ws-name">{pendingSuggestion.title}</span>
                </div>
                <span className="ws-side"><span>{pendingSuggestion.author}</span><span>Sending…</span></span>
              </div>
            </div>
          ))}
        </section>
      ) : null}
      {plan.dropped.length ? (
        <details className="ws-fold" open={droppedOpen} onToggle={(event) => setDroppedOpen((event.target as HTMLDetailsElement).open)}>
          <summary>{plan.dropped.length} dropped</summary>
          <div className="ws-dropped">{plan.dropped.map(item)}</div>
        </details>
      ) : <div className="ws-end" />}
    </>
  );

  const decisionsView = (
    <>
      {plan.openDecisions.length ? (
        <section aria-label="Open">
          <div className="ws-stage"><span className="ws-num" /><h2>Open</h2><span className="ws-mono">say what you would do</span></div>
          {plan.openDecisions.map(item)}
        </section>
      ) : null}
      <section aria-label="Decided">
        <div className="ws-stage"><span className="ws-num" /><h2>Decided</h2><span className="ws-mono">{plan.decided.length}</span></div>
        {plan.decided.map(item)}
      </section>
      <div className="ws-end" />
    </>
  );

  const suggestButton = !liveOk || me === undefined ? null : me ? (
    <button type="button" className="button ws-suggest" aria-expanded={suggesting} onClick={() => setSuggesting(!suggesting)}>Suggest a feature</button>
  ) : (
    <a className="button ws-suggest" href={signIn}>Suggest a feature</a>
  );

  return (
    <>
      {accountSlot ? createPortal(account, accountSlot) : <div className="ws-account-inline">{account}</div>}
      <div className="ws-top">
        <div>
          <h1>Workshop</h1>
          <p>Silo is being rebuilt from scratch. This is the plan, in build order.</p>
        </div>
        {suggestButton}
      </div>
      {suggesting && me ? (
        <form className="ws-suggest-form" onSubmit={(event) => void sendSuggestion(event)}>
          <input aria-label="Title" placeholder="Title" maxLength={120} required autoFocus value={suggestDraft.title} onChange={(event) => setSuggestDraft({ ...suggestDraft, title: event.target.value })} />
          <textarea aria-label="Why" placeholder="Why" maxLength={5000} rows={2} value={suggestDraft.text} onChange={(event) => setSuggestDraft({ ...suggestDraft, text: event.target.value })} />
          <div className="ws-state">
            <button type="submit" className="ws-btn go" disabled={!suggestDraft.title.trim()}>Suggest</button>
            <button type="button" className="ws-btn" onClick={() => setSuggesting(false)}>Cancel</button>
          </div>
        </form>
      ) : null}
      <div className="ws-tabs joint">
        <div role="tablist" aria-label="Workshop">
          <button type="button" role="tab" id="ws-tab-plan" aria-controls="ws-plan" aria-selected={tab === 'plan'} className={tab === 'plan' ? 'on' : ''} onClick={() => setTab('plan')}>Plan</button>
          <button type="button" role="tab" id="ws-tab-decisions" aria-controls="ws-decisions" aria-selected={tab === 'decisions'} className={tab === 'decisions' ? 'on' : ''} onClick={() => setTab('decisions')}>Decisions</button>
        </div>
        <span className="ws-mono ws-count">{plan.planned} planned · {plan.undecided} undecided</span>
      </div>
      {live.status === 'unavailable' ? <p className="ws-quiet ws-unavailable">Live comments are unavailable.</p> : null}
      {signinFailed ? <p className="ws-quiet ws-unavailable">Sign-in did not complete.</p> : null}
      {notice ? <p className="ws-notice" role="alert">{notice}</p> : null}
      <p className="sr-only" aria-live="polite">{announce}</p>
      <div role="tabpanel" id="ws-plan" aria-labelledby="ws-tab-plan" hidden={tab !== 'plan'}>{planView}</div>
      <div role="tabpanel" id="ws-decisions" aria-labelledby="ws-tab-decisions" hidden={tab !== 'decisions'}>{decisionsView}</div>
    </>
  );
}
