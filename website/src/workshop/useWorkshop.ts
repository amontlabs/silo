// The Workshop's live state and every write: what the server answered, what is still on its way,
// and what the page remembers of its own writes. Navigation lives in App and reaches in through `nav`.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { FormEvent, MutableRefObject } from 'react';
import { api, ApiError } from './api';
import type { Comment, IssueDetail, Me } from './api';
import { applyKept, keep, keptComments } from './writes';
import { allRows, buildPlan } from './plan';
import type { BuildData, LiveIssue, LiveState, Row, Verdict } from './plan';

/** A row that just opened refuses its main action for this long. */
export const OPEN_GUARD_MS = 700;
const NOTICE_MS = 8000;
/** A fetch of an issue's comments this recent serves an opening. */
const FRESH_MS = 4000;

export type Live = { status: 'loading' | 'ok' | 'unavailable'; state: LiveState | null };
export type Detail = IssueDetail | 'loading' | 'error';

/** What the writes need from navigation. */
export interface Nav {
  openKey: string | null;
  openedAt: number;
  /** After a verdict on the open row: open the next one waiting, or close. */
  advanceFrom: (row: Row) => void;
  /** Back to a row whose write was refused. */
  reopen: (row: Row) => void;
}

function takeSigninFailure(): boolean {
  const params = new URLSearchParams(location.search);
  if (params.get('signin') !== 'failed') return false;
  params.delete('signin');
  const search = params.toString();
  history.replaceState(null, '', `${location.pathname}${search ? `?${search}` : ''}${location.hash}`);
  return true;
}

function verdictPatch(verdict: Verdict | null): Partial<LiveIssue> {
  return verdict === 'drop'
    ? { verdict: 'drop', state: 'closed', reason: 'not_planned' }
    : { verdict, state: 'open', reason: null, closedAt: null };
}

export function useWorkshop(data: BuildData, openKey: string | null, openKeys: ReadonlySet<string>, openings: number, nav: MutableRefObject<Nav>) {
  const [live, setLive] = useState<Live>({ status: 'loading', state: null });
  const [me, setMe] = useState<Me | null | undefined>(undefined);
  const [details, setDetails] = useState<Record<number, Detail>>({});
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
  const [suggestDraft, setSuggestDraft] = useState({ title: '', text: '' });
  const [announce, setAnnounce] = useState('');
  const [votes, setVotes] = useState<Record<number, boolean>>({});
  const [signinFailed] = useState(takeSigninFailure);
  const pendingRef = useRef(new Set<number>());

  const liveOk = live.status === 'ok';
  const liveRefOk = useRef(liveOk);
  liveRefOk.current = liveOk;
  const canDecide = liveOk && me?.canDecide === true;
  const plan = useMemo(() => buildPlan(data, live.state), [data, live.state]);
  const rows = useMemo(() => allRows(plan), [plan]);
  const rowByKey = useMemo(() => new Map(rows.map((row) => [row.key, row])), [rows]);

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

  // An issue's comments: fetched on every opening, in the background when some are already shown; a pointer
  // over the row or focus on it starts the fetch early, and a fetch younger than FRESH_MS counts as that opening's.
  const fetched = useRef(new Map<number, number>());
  const fetching = useRef(new Set<number>());
  const loadDetail = useCallback((n: number, force = false) => {
    if (n <= 0 || fetching.current.has(n)) return;
    const at = fetched.current.get(n);
    if (!force && at !== undefined && Date.now() - at < FRESH_MS) return;
    fetching.current.add(n);
    setDetails((current) => (typeof current[n] === 'object' ? current : { ...current, [n]: 'loading' }));
    api.issue(n).then(
      (detail) => {
        fetched.current.set(n, Date.now());
        setDetails((current) => ({ ...current, [n]: { ...detail, comments: keptComments(n, detail.comments ?? [], Date.now()) } }));
      },
      () => setDetails((current) => (typeof current[n] === 'object' ? current : { ...current, [n]: 'error' })),
    ).finally(() => fetching.current.delete(n));
  }, []);
  /** Early fetch for a row the person is about to open. */
  const prefetch = useCallback((row: Row) => {
    if (liveRefOk.current && row.issue !== null && row.issue > 0 && !openKeys.has(row.key)) loadDetail(row.issue);
  }, [loadDetail, openKeys]);

  // Each opening loads the row's comments once the live state is there.
  useEffect(() => {
    const row = openKey ? rowByKey.get(openKey) : undefined;
    if (liveOk && row?.issue != null) loadDetail(row.issue);
    // The plan changing under the same open row does not load again.
  }, [liveOk, openKey, openings, loadDetail]);
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
  }, [openKey, openings, liveOk, signedInAs]);

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

  /** The one guard every verdict passes, by key or by click. */
  const verdictReady = (row: Row): boolean => {
    const n = row.issue;
    return canDecide && n !== null && n > 0 && Boolean(live.state?.issues[n]) && !pendingRef.current.has(n)
      && nav.current.openKey === row.key && Date.now() - nav.current.openedAt >= OPEN_GUARD_MS;
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
    if (nav.current.openKey === row.key) {
      setAnnounce('');
      nav.current.advanceFrom(row);
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
      nav.current.reopen(row);
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
    setSuggestDraft({ title: '', text: '' });
    setAnnounce('Sending suggestion');
    try {
      const { number } = await api.suggest(title, text);
      const issue: LiveIssue = { kind: 'suggestion', entry: null, title, state: 'open', reason: null, verdict: null, up: 0, comments: 0, author, bot: false, createdAt: new Date().toISOString(), closedAt: null };
      setLive((current) => (current.state ? { ...current, state: { ...current.state, issues: { ...current.state.issues, [number]: issue } } } : current));
      setDetails((current) => ({ ...current, [number]: { number, title, body: text, url: '', up: 0, comments: [] } }));
      keep(number, issue, Date.now());
    } catch (error) {
      setSuggestDraft({ title, text });
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
      if (error instanceof ApiError && error.status === 401) signedOut();
      else setNotice('Could not sign out.');
    }
  };

  return {
    live, liveOk, me, canDecide, plan, rowByKey, details, prefetch, pending, pendingRef, saving, sending, votePending,
    votes, suggestionsSending, notice, rowNotice, setRowNotice, drafts, setDrafts, refining, setRefining, suggestDraft,
    setSuggestDraft, announce, setAnnounce, signinFailed, verdictReady, sendVerdict, sendComment, sendDecide, toggleVote,
    sendSuggestion, signOut,
  };
}

export type Workshop = ReturnType<typeof useWorkshop>;
