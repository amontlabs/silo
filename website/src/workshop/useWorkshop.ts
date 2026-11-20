// The Workshop's live state and every write: what the server answered, what is still on its way,
// and what the page remembers of its own writes. Navigation lives in App and reaches in through `nav`.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { FormEvent, MutableRefObject } from 'react';
import { api, ApiError, hasSessionHint } from './api';
import type { Comment, IssueDetail, Me } from './api';
import { recallPerson, recallState, rememberPerson, rememberState } from './memory';
import { applyKept, keep, keptComments, newerAnswer } from './writes';
import type { Write } from './writes';
import { allRows, buildPlan } from './plan';
import type { BuildData, LiveIssue, LiveState, Notes, Row, Verdict } from './plan';

/** A row that just opened refuses its main action for this long. */
export const OPEN_GUARD_MS = 700;
const NOTICE_MS = 8000;
/** A fetch of an issue's comments this recent serves an opening. */
const FRESH_MS = 4000;

/** How long the page waits before asking `state` a second time after a failure. */
export const loadConfig = { retryMs: 1500 };
/** A state fetched this long before now is old: the page asks for a fresh one. */
const OLD_MS = 2 * 60_000;

/**
 * Two separate things: the state the page shows (remembered, or from the server; null when there is none yet), and
 * how this visit's load stands. Whatever shows numbers reads `state`, whatever the load's status.
 */
export type Live = { load: 'pending' | 'answered' | 'failed'; state: LiveState | null };
/** `me` as answered on this visit: not yet, failed, or the answer. */
type MeAnswer = Me | null | undefined | 'error';
export type Detail = IssueDetail | 'loading' | 'error';

/** What the writes need from navigation. */
export interface Nav {
  /** The row opened last: the address follows it. */
  openKey: string | null;
  /** Every open row and when it opened: each has its own guard. */
  openedAt: Map<string, number>;
  /** After a verdict on the open row: open the next one waiting, or close. */
  advanceFrom: (row: Row) => void;
  /** Back to a row whose write was refused. */
  reopen: (row: Row) => void;
}

function takeSigninFailure(): boolean {
  // At build time there is no address.
  if (typeof location === 'undefined') return false;
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

export function useWorkshop(data: BuildData, openKey: string | null, openKeys: ReadonlySet<string>, openings: number, nav: MutableRefObject<Nav>, loadNotes: () => Promise<Notes> = () => Promise.resolve({})) {
  // The newest answer as the server sent it (or as remembered), how the load stands, and whether a `fresh=1` ask is
  // still out. What the page shows is that answer with the page's own writes applied (writes.ts).
  const [loaded, setLoaded] = useState<{ load: Live['load']; answer: LiveState | null; fresh: boolean }>(() => ({ load: 'pending', answer: recallState(Date.now()), fresh: false }));
  const [writesTick, setWritesTick] = useState(0);
  const shownState = useMemo(() => (loaded.answer ? applyKept(loaded.answer, Date.now()) : null), [loaded.answer, writesTick]);
  const live: Live = useMemo(() => ({ load: loaded.load, state: shownState }), [loaded.load, shownState]);
  /** The load has ended, the `fresh=1` ask included when one was made. */
  const settled = loaded.load !== 'pending' && !loaded.fresh;
  const [answer, setMe] = useState<MeAnswer>(undefined);
  // The person remembered from the last visit, for display until `me` answers; without the session marker, nobody.
  const [remembered] = useState(recallPerson);
  const meKnown = answer !== undefined && answer !== 'error';
  /** Who the page shows as signed in: `me`'s answer, else the remembered person; undefined while not known. */
  const me: Me | null | undefined = meKnown ? answer : !hasSessionHint() ? null : remembered ?? (answer === 'error' ? null : undefined);
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
  const [suggestDraft, setSuggestDraft] = useState({ title: '', text: '' });
  const [announce, setAnnounce] = useState('');
  const [votes, setVotes] = useState<Record<number, boolean>>({});
  const [signinFailed] = useState(takeSigninFailure);
  const pendingRef = useRef(new Set<number>());

  /** A state is shown, from memory or from the server. */
  const liveOk = live.state !== null;
  /** Nothing is shown yet and the load is under way: what will come has a placeholder. */
  // After a failed load, the page says it is offline until a load answers; trying again keeps that look.
  const [offline, setOffline] = useState(false);
  const waiting = !liveOk && live.load === 'pending' && !offline;
  const liveRefOk = useRef(liveOk);
  liveRefOk.current = liveOk;
  /**
   * The owner's marks and words show for someone the page shows as deciding, remembered or answered, and only in a
   * build that offers verdicts (BuildData.decide).
   */
  const canDecide = data.decide && liveOk && me?.canDecide === true;
  /** A verdict is sent only once `me` answered on this visit that this person decides. */
  const mayDecide = data.decide && liveOk && meKnown && answer?.canDecide === true;
  const plan = useMemo(() => buildPlan(data, live.state), [data, live.state]);
  const rows = useMemo(() => allRows(plan), [plan]);
  const rowByKey = useMemo(() => new Map(rows.map((row) => [row.key, row])), [rows]);

  // The load: `state` once, again after a failure (not when the server has no live state at all), and once more
  // with `fresh=1` when the answer is old. An answer is shown unless the state shown is newer.
  const mounted = useRef(true);
  const loading = useRef(false);
  const show = useCallback((state: LiveState) => {
    setLoaded((current) => {
      const next = newerAnswer(current.answer, state);
      return next === current.answer ? current : { ...current, answer: next };
    });
  }, []);
  const askMe = useCallback(() => {
    api.me().then(
      (who) => { if (mounted.current) setMe(who); },
      () => { if (mounted.current) setMe((current) => (current === undefined ? 'error' : current)); },
    );
  }, []);
  const load = useCallback(async () => {
    if (loading.current) return;
    loading.current = true;
    setLoaded((current) => (current.load === 'pending' ? current : { ...current, load: 'pending' }));
    let first: LiveState | null = null;
    try {
      first = await api.state();
    } catch (error) {
      if (!(error instanceof ApiError && error.code === 'not_configured')) {
        await new Promise((resolve) => setTimeout(resolve, loadConfig.retryMs));
        if (mounted.current) first = await api.state().catch(() => null);
      }
    }
    loading.current = false;
    if (!mounted.current) return;
    if (!first) {
      setLoaded((current) => ({ ...current, load: 'failed' }));
      setOffline(true);
      return;
    }
    setOffline(false);
    show(first);
    const old = first.old === true || Date.now() - Date.parse(first.fetchedAt) > OLD_MS;
    setLoaded((current) => ({ ...current, load: 'answered', fresh: old }));
    if (!old) return;
    const done = () => { if (mounted.current) setLoaded((current) => ({ ...current, fresh: false })); };
    api.state(true).then((fresh) => { if (mounted.current) show(fresh); done(); }, done);
  }, [show]);
  useEffect(() => {
    mounted.current = true;
    void load();
    askMe();
    return () => { mounted.current = false; };
  }, []);
  /** "Try again" after a failed load; `me` is asked again too when it failed. */
  const retry = useCallback(() => {
    void load();
    if (answer === 'error') askMe();
  }, [load, askMe, answer]);

  // What the page shows is what the next visit shows first. The person is forgotten once known to be signed out.
  // The answer is remembered as it came; the page's writes are kept apart and applied again on the next visit.
  useEffect(() => {
    if (loaded.answer) rememberState(loaded.answer);
  }, [loaded.answer]);
  useEffect(() => {
    if (answer === null || (answer !== undefined && answer !== 'error')) rememberPerson(answer);
  }, [answer]);

  useEffect(() => {
    if (!notice) return;
    const timer = setTimeout(() => setNotice(null), NOTICE_MS);
    return () => clearTimeout(timer);
  }, [notice]);

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
  // The features' notes: loaded once, the first time a person's pointer, press or focus reaches a "Notes"; a
  // failed load is tried again at the next.
  const [notes, setNotes] = useState<Notes | 'loading' | 'error' | undefined>(undefined);
  const notesRef = useRef(notes);
  notesRef.current = notes;
  const wantNotes = useCallback(() => {
    if (notesRef.current === 'loading' || typeof notesRef.current === 'object') return;
    notesRef.current = 'loading';
    setNotes('loading');
    loadNotes().then((loaded) => setNotes(loaded), () => setNotes('error'));
  }, [loadNotes]);

  const prefetch = useCallback((row: Row) => {
    if (liveRefOk.current && row.issue !== null && row.issue > 0 && !openKeys.has(row.key)) loadDetail(row.issue);
  }, [loadDetail, openKeys]);
  /** The fetch of a row being opened, asked for in the same event so the row never shows a stale error first. */
  const fetchNow = useCallback((row: Row) => {
    if (liveRefOk.current && row.issue !== null && row.issue > 0) loadDetail(row.issue);
  }, [loadDetail]);

  // Each opening loads the row's comments once the live state is there.
  useEffect(() => {
    const row = openKey ? rowByKey.get(openKey) : undefined;
    if (liveOk && row?.issue != null) loadDetail(row.issue);
    // The plan changing under the same open row does not load again.
  }, [liveOk, openKey, openings, loadDetail]);
  // One check of the person's own vote per opening of a row, never retried: a failure leaves 👍 off. It waits for
  // `me` to answer on this visit (not the remembered person), and a session found gone there says nothing.
  const signedInAs = meKnown && answer ? answer.login : null;
  useEffect(() => {
    const row = openKey ? rowByKey.get(openKey) : undefined;
    const n = row?.issue;
    if (!liveOk || !signedInAs || n == null || n <= 0 || (row?.kind !== 'entry' && row?.kind !== 'suggestion')) return;
    api.myVote(n).then(
      (on) => setVotes((current) => (n in current ? current : { ...current, [n]: on })),
      (error) => {
        if (error instanceof ApiError && error.status === 401) {
          setMe(null);
          setVotes({});
          rememberPerson(null);
        }
      },
    );
    // Re-rendering, or the plan changing under the same open row, does not ask again.
  }, [openKey, openings, liveOk, signedInAs]);

  const setBusy = (n: number, busy: boolean) => {
    if (busy) pendingRef.current.add(n);
    else pendingRef.current.delete(n);
    setPending(new Set(pendingRef.current));
  };

  // The count the server answered for a write; an older server answers none, and the count shown moves by one.
  const shownRef = useRef(shownState);
  shownRef.current = shownState;
  const count = (answered: unknown, fallback: number) => (typeof answered === 'number' && Number.isInteger(answered) && answered >= 0 ? answered : fallback);
  /** A write the server confirmed: kept, and shown at once over the answer. */
  const remember = (write: Omit<Write, 'at'>, comment?: Comment) => {
    keep(write, Date.now(), comment);
    setWritesTick((tick) => tick + 1);
  };

  function signedOut() {
    setMe(null);
    rememberPerson(null);
    setVotes({});
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
    return mayDecide && n !== null && n > 0 && Boolean(live.state?.issues[n]) && !pendingRef.current.has(n)
      && nav.current.openedAt.has(row.key) && Date.now() - (nav.current.openedAt.get(row.key) as number) >= OPEN_GUARD_MS;
  };

  const sendVerdict = async (row: Row, verdict: Verdict | null) => {
    const n = row.issue;
    if (n === null || !live.state?.issues[n] || !verdictReady(row)) return;
    setBusy(n, true);
    setSaving((current) => ({ ...current, [n]: true }));
    setNotice(null);
    setRowNotice(null);
    // The queue moves on at once; the row shows "Saving…" until the server answers.
    if (nav.current.openedAt.has(row.key)) {
      setAnnounce('');
      nav.current.advanceFrom(row);
      setAnnounce((next) => (next ? `${row.name}: saving. ${next}` : `${row.name}: saving`));
    } else setAnnounce(`${row.name}: saving`);
    try {
      await api.verdict(n, verdict);
      const confirmed = verdictPatch(verdict);
      remember({ n, set: confirmed });
    } catch (error) {
      if (error instanceof ApiError && error.status === 401) {
        signedOut();
        return;
      }
      // Back to the row that was refused.
      nav.current.reopen(row);
      const message = error instanceof ApiError ? error.message : 'Something went wrong.';
      setRowNotice({ key: row.key, text: `${row.name}: not saved. ${message}` });
    } finally {
      setBusy(n, false);
      setSaving((current) => {
        const next = { ...current };
        delete next[n];
        return next;
      });
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
      const answered = await api.comment(n, text);
      confirmComment(n, own);
      remember({ n, set: { comments: count(answered?.comments, (shownRef.current?.issues[n]?.comments ?? 0) + 1) } }, own);
    } catch (error) {
      setDrafts((current) => ({ ...current, [row.key]: current[row.key] ? current[row.key] : text }));
      failed('Comment', error);
    } finally {
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
      const answered = await api.vote(n, on);
      setVotes((current) => ({ ...current, [n]: on }));
      remember({ n, set: { up: count(answered?.up, Math.max(0, (shownRef.current?.issues[n]?.up ?? 0) + (on ? 1 : -1))) } });
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
      setDetails((current) => ({ ...current, [number]: { number, title, body: text, url: '', up: 0, comments: [] } }));
      remember({ n: number, set: issue });
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
      rememberPerson(null);
      setVotes({});
    } catch (error) {
      if (error instanceof ApiError && error.status === 401) signedOut();
      else setNotice('Could not sign out.');
    }
  };

  return {
    live, liveOk, waiting, offline, settled, me, meKnown, canDecide, mayDecide, retry, plan, rowByKey, details, prefetch, notes, wantNotes, fetchNow, pending, pendingRef, saving, sending, votePending,
    votes, suggestionsSending, notice, rowNotice, setRowNotice, drafts, setDrafts, suggestDraft,
    setSuggestDraft, announce, setAnnounce, signinFailed, verdictReady, sendVerdict, sendComment, toggleVote,
    sendSuggestion, signOut,
  };
}

export type Workshop = ReturnType<typeof useWorkshop>;
