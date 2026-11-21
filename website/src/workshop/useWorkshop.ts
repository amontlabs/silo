// The Workshop's live state and every write: what the server answered, what is still on its way,
// and what the page remembers of its own writes. Navigation lives in App.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { FormEvent } from 'react';
import { api, ApiError, hasSessionHint } from './api';
import type { Comment, IssueDetail, Me, MyVotes, Target } from './api';
import { recallPerson, recallState, rememberPerson, rememberState } from './memory';
import { applyKept, keep, keepCommentUp, keptComments, newerAnswer } from './writes';
import type { Write } from './writes';
import { allRows, buildPlan } from './plan';
import type { BuildData, Facts, Histories, LiveIssue, LiveState, Notes, Row } from './plan';
import { SiteChanged } from './chunks';

/** A chunk of the page as it loads: not asked for yet, under way, in, failed, or gone with a newer deployment. */
export type ChunkState<T> = T | 'loading' | 'error' | 'changed' | undefined;

/**
 * A chunk loaded once, and what asks for it. A failed load is tried again only when `again` is set: an opening asks
 * again, while a pointer passing over does not, so the failure stays as it is under it. A browser may keep a failed
 * import failed for the life of the page, so that retry may never succeed: what a failure offers is a reload. A chunk
 * gone with a newer deployment is never asked for again.
 */
function useChunk<T extends object>(load: () => Promise<T>): [ChunkState<T>, (again?: boolean) => void] {
  const [value, setValue] = useState<ChunkState<T>>(undefined);
  const current = useRef(value);
  current.current = value;
  const want = useCallback((again = false) => {
    const now = current.current;
    if (now === 'loading' || now === 'changed' || typeof now === 'object' || (now === 'error' && !again)) return;
    current.current = 'loading';
    setValue('loading');
    load().then((loaded) => setValue(loaded), (error: unknown) => setValue(error instanceof SiteChanged ? 'changed' : 'error'));
  }, [load]);
  return [value, want];
}

/** How the page reloads itself; tests replace it. */
export const pageConfig = { reload: () => location.reload() };

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

export function useWorkshop(data: BuildData, openKey: string | null, openKeys: ReadonlySet<string>, openings: number, loadNotes: () => Promise<Notes> = () => Promise.resolve({}), loadFacts: () => Promise<Facts> = () => Promise.resolve({ entries: {}, statements: {} }), loadHistory: () => Promise<Histories> = () => Promise.resolve({})) {
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
  const detailRef = useRef(details);
  detailRef.current = details;
  const [notice, setNotice] = useState<string | null>(null);
  // Writes the server has not answered yet, by the key of the row (an entry may have no thread yet). Nothing here counts as done.
  const [sending, setSending] = useState<Record<string, Comment[]>>({});
  const [votePending, setVotePending] = useState<ReadonlySet<string>>(new Set());
  // The page's own change to a row's 👍 while its vote is out: the pressed state it shows and the count's change
  // (one up or one down), by the key of the row. It is laid over the row's count, so a newer `state` or `issue`
  // answer moves the count under it without the change being lost or counted twice, and it is gone once the
  // server has answered or refused.
  const [voteOwn, setVoteOwn] = useState<Record<string, { on: boolean; delta: number }>>({});
  const [commentVotePending, setCommentVotePending] = useState<ReadonlySet<string>>(new Set());
  const [suggestionsSending, setSuggestionsSending] = useState<{ id: number; title: string; author: string }[]>([]);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [suggestDraft, setSuggestDraft] = useState({ title: '', text: '' });
  const [announce, setAnnounce] = useState('');
  const [votes, setVotes] = useState<Record<number, boolean>>({});
  // The comments of each issue the person has given a 👍, as `GET vote` says and as the person changes them.
  const [commentVotes, setCommentVotes] = useState<Record<number, readonly (string | number)[]>>({});
  // The rows whose thread the server refused a comment on because it is closed on GitHub.
  const [closedKeys, setClosedKeys] = useState<ReadonlySet<string>>(new Set());
  const [signinFailed] = useState(takeSigninFailure);

  /** A state is shown, from memory or from the server. */
  const liveOk = live.state !== null;
  /** Nothing is shown yet and the load is under way: what will come has a placeholder. */
  // After a failed load, the page says it is offline until a load answers; trying again keeps that look.
  const [offline, setOffline] = useState(false);
  const waiting = !liveOk && live.load === 'pending' && !offline;
  const liveRefOk = useRef(liveOk);
  liveRefOk.current = liveOk;
  // The changes of the entries: loaded once, the first time a pointer, a press or focus reaches a History.
  const [histories, wantHistory] = useChunk(loadHistory);
  const plan = useMemo(() => buildPlan(data, live.state, closedKeys), [data, live.state, closedKeys]);
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
        // The comments' 👍 counts are the thread's; the person's own votes on them come with the row's own vote.
      },
      () => setDetails((current) => (typeof current[n] === 'object' ? current : { ...current, [n]: 'error' })),
    ).finally(() => fetching.current.delete(n));
  }, []);
  // The features' notes: loaded once, the first time a person's pointer, press or focus reaches a "Notes".
  const [notes, wantNotes] = useChunk(loadNotes);

  // What the entries say of the statements: loaded once, the first time a row with something to show is reached or
  // opened.
  const [facts, wantFacts] = useChunk(loadFacts);

  /** Early fetch for a row the person is about to open. */
  const prefetch = useCallback((row: Row) => {
    if (row.more) wantFacts(false);
    if (liveRefOk.current && row.issue !== null && row.issue > 0 && !openKeys.has(row.key)) loadDetail(row.issue);
  }, [loadDetail, openKeys, wantFacts]);
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
  /** What the person's own votes on an issue are, as `GET vote` answered; what the page already knows stays. */
  const noteVotes = (n: number, mine: MyVotes) => {
    setVotes((current) => (n in current ? current : { ...current, [n]: mine.on }));
    setCommentVotes((current) => (n in current ? current : { ...current, [n]: mine.comments }));
  };
  // One check of the person's own votes per opening of a row, never retried: a failure leaves 👍 off. It waits for
  // `me` to answer on this visit (not the remembered person), and a session found gone there says nothing.
  const signedInAs = meKnown && answer ? answer.login : null;
  const askVotes = (n: number) => {
    api.myVote(n).then(
      (mine) => noteVotes(n, mine),
      (error) => {
        if (error instanceof ApiError && error.status === 401) {
          setMe(null);
          setVotes({});
          setCommentVotes({});
          rememberPerson(null);
        }
      },
    );
  };
  useEffect(() => {
    const row = openKey ? rowByKey.get(openKey) : undefined;
    const n = row?.issue;
    if (!liveOk || !signedInAs || n == null || n <= 0 || (row?.kind !== 'entry' && row?.kind !== 'suggestion')) return;
    askVotes(n);
    // Re-rendering, or the plan changing under the same open row, does not ask again.
  }, [openKey, openings, liveOk, signedInAs]);
  // An open row that takes a thread, or whose thread's number changes, asks for that thread and for the person's votes
  // as an opening does. Rows open independently, so each open row is followed.
  const threadAsked = useRef(new Map<string, number | null>());
  useEffect(() => {
    const asked = threadAsked.current;
    for (const key of [...asked.keys()]) if (!openKeys.has(key)) asked.delete(key);
    for (const key of openKeys) {
      const n = rowByKey.get(key)?.issue ?? null;
      const before = asked.get(key);
      asked.set(key, n);
      // The opening itself asks; only a thread that arrives under a row already open asks here.
      if (before === undefined || before === n || n === null || n <= 0 || !liveOk) continue;
      loadDetail(n, true);
      if (signedInAs) askVotes(n);
    }
  }, [rowByKey, openKeys, liveOk, signedInAs]);

  // The count the server answered for a write; an older server answers none, and the count shown moves by one.
  const shownRef = useRef(shownState);
  shownRef.current = shownState;
  const count = (answered: unknown, fallback: number) => (typeof answered === 'number' && Number.isInteger(answered) && answered >= 0 ? answered : fallback);
  /** A write the server confirmed: kept, and shown at once over the answer. */
  const sentRef = useRef(0);
  /** The moment a write is sent, with its place in the order of sending. */
  const sending$ = () => ({ at: Date.now(), seq: (sentRef.current += 1) });
  const remember = (write: Omit<Write, 'at' | 'seq'>, sent: { at: number; seq: number }, comment?: Comment) => {
    keep({ ...write, seq: sent.seq }, Date.now(), comment, sent.at);
    setWritesTick((tick) => tick + 1);
  };

  function signedOut() {
    setMe(null);
    rememberPerson(null);
    setVotes({});
    setCommentVotes({});
    setNotice('Sign in again.');
  }

  const ownComment = (text: string): Comment | null => (me
    ? { id: `own-${Date.now()}-${Math.random().toString(36).slice(2)}`, author: me.login, avatar: me.avatar, bot: false, createdAt: new Date().toISOString(), text, up: 0 }
    : null);
  const startSending = (key: string, comment: Comment | null) => {
    if (comment) setSending((current) => ({ ...current, [key]: [...(current[key] ?? []), comment] }));
  };
  const stopSending = (key: string, comment: Comment | null) => {
    if (comment) setSending((current) => ({ ...current, [key]: (current[key] ?? []).filter((c) => c.id !== comment.id) }));
  };
  /**
   * A comment the server confirmed joins the thread as an ordinary one, once. A thread the row had none of yet is
   * made here, so the comment shows without a load.
   */
  const confirmComment = (row: Row, n: number, comment: Comment | null) => {
    if (!comment) return;
    setDetails((current) => {
      const detail = current[n];
      if (typeof detail === 'object') {
        if (detail.comments.some((c) => c.id === comment.id || (c.author === comment.author && c.text.trim() === comment.text.trim()))) return current;
        return { ...current, [n]: { ...detail, comments: [...detail.comments, comment] } };
      }
      return row.issue === null ? { ...current, [n]: { number: n, title: row.name, body: '', url: '', up: 0, comments: [comment] } } : current;
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

  /** What a write on a row names: for a decision or a feature its id, so the server finds the thread and says when it is closed; a suggestion by its number. */
  const targetOf = (row: Row): Target | null => {
    if (row.kind === 'entry') return { entry: row.key.slice('entry-'.length) };
    return row.issue !== null && row.issue > 0 ? { n: row.issue } : null;
  };
  /** The thread a write answered, else the row's own. */
  const threadOf = (row: Row, answered: { n?: unknown } | undefined): number | null => {
    const n = answered?.n;
    if (typeof n === 'number' && Number.isSafeInteger(n) && n > 0) return n;
    return row.issue !== null && row.issue > 0 ? row.issue : null;
  };
  /** The thread as it stood before the write that made or found it; the write's own values are set over it. */
  const opened = (row: Row, n: number): Pick<Write, 'opens'> => (row.issue === n ? {} : {
    opens: {
      entry: row.key.slice('entry-'.length),
      issue: { kind: 'entry', entry: row.key.slice('entry-'.length), title: row.name, state: 'open', reason: null, up: 0, comments: 0, author: '', bot: true, createdAt: new Date().toISOString(), closedAt: null },
    },
  });
  /** A refusal because the thread is closed on GitHub: the row takes the thread the server names, and shows it closed. */
  const refusedClosed = (row: Row, error: unknown, sent: { at: number; seq: number }) => {
    if (!(error instanceof ApiError && error.status === 403 && error.code === 'closed')) return false;
    setClosedKeys((current) => new Set(current).add(row.key));
    const n = error.n ?? row.issue;
    if (n !== null && n > 0 && row.issue !== n) remember({ n, ...opened(row, n), set: {} }, sent);
    return true;
  };

  const sendComment = async (row: Row, event: FormEvent) => {
    event.preventDefault();
    const target = targetOf(row);
    const text = (drafts[row.key] ?? '').trim();
    const own = ownComment(text);
    if (!target || !text || !own) return;
    const sent = sending$();
    setDrafts((current) => ({ ...current, [row.key]: '' }));
    startSending(row.key, own);
    setAnnounce('Sending comment');
    try {
      const answered = await api.comment(target, text);
      const n = threadOf(row, answered);
      if (n === null) throw new Error('No thread answered.');
      const confirmed = { ...own, ...(typeof answered?.id === 'number' ? { id: answered.id } : {}), createdAt: new Date().toISOString() };
      confirmComment(row, n, confirmed);
      remember({ n, ...opened(row, n), set: { comments: count(answered?.comments, (shownRef.current?.issues[n]?.comments ?? 0) + 1) } }, sent, confirmed);
    } catch (error) {
      setDrafts((current) => ({ ...current, [row.key]: current[row.key] ? current[row.key] : text }));
      if (!refusedClosed(row, error, sent)) failed('Comment', error);
    } finally {
      stopSending(row.key, own);
    }
  };

  const votingRef = useRef(new Set<string>());
  const setOwnVote = (key: string, own: { on: boolean; delta: number } | null) => setVoteOwn((current) => {
    const { [key]: _gone, ...rest } = current;
    return own ? { ...rest, [key]: own } : rest;
  });
  /**
   * The person's 👍 on a row. When the page knows the person's vote on it, the pressed state and the count change
   * before anything is sent; when it does not, `GET vote` is asked first (the row shows it is working) and they
   * change when that answer is in. The server's count then replaces the page's own; a refusal takes it back.
   */
  const toggleVote = async (row: Row) => {
    const target = targetOf(row);
    if (!target || !me || votingRef.current.has(row.key)) return;
    votingRef.current.add(row.key);
    const sent = sending$();
    setVotePending(new Set(votingRef.current));
    setAnnounce('Sending vote');
    try {
      // An entry with no thread has no 👍 of the person's.
      const known = row.issue !== null ? votes[row.issue] : false;
      const was = known !== undefined ? known : row.issue !== null ? (await api.myVote(row.issue)).on : false;
      const on = !was;
      // The count never goes below 0: a 👍 taken back from a count of 0 changes nothing shown.
      const shown = row.up ?? 0;
      setOwnVote(row.key, { on, delta: on ? 1 : Math.max(0, shown - 1) - shown });
      const answered = await api.vote(target, on);
      const n = threadOf(row, answered);
      if (n === null) throw new Error('No thread answered.');
      setVotes((current) => ({ ...current, [n]: on }));
      remember({ n, ...opened(row, n), set: { up: count(answered?.up, Math.max(0, (shownRef.current?.issues[n]?.up ?? 0) + (on ? 1 : -1))) } }, sent);
    } catch (error) {
      if (!refusedClosed(row, error, sent)) failed('Vote', error);
    } finally {
      votingRef.current.delete(row.key);
      setOwnVote(row.key, null);
      setVotePending(new Set(votingRef.current));
    }
  };

  const commentVotingRef = useRef(new Set<string>());
  /** The person's 👍 on a comment: the count changes at once, and goes back when the server refuses. */
  const toggleCommentVote = async (row: Row, comment: Comment) => {
    const n = row.issue;
    const id = comment.id;
    if (n === null || n <= 0 || typeof id !== 'number' || !me) return;
    const pending = `${n}:${id}`;
    if (commentVotingRef.current.has(pending)) return;
    commentVotingRef.current.add(pending);
    setCommentVotePending(new Set(commentVotingRef.current));
    // The page's own change to the count: one up or one down, from the count shown now.
    const bump = (delta: number, on: boolean) => {
      setDetails((current) => {
        const detail = current[n];
        return typeof detail === 'object' ? { ...current, [n]: { ...detail, comments: detail.comments.map((c) => (c.id === id ? { ...c, up: Math.max(0, (c.up ?? 0) + delta) } : c)) } } : current;
      });
      setCommentVotes((current) => {
        const others = (current[n] ?? []).filter((other) => other !== id);
        return { ...current, [n]: on ? [...others, id] : others };
      });
    };
    let revert: (() => void) | null = null;
    try {
      let mine = commentVotes[n];
      if (mine === undefined) {
        const asked = await api.myVote(n);
        noteVotes(n, asked);
        mine = asked.comments;
      }
      const was = mine.includes(id);
      const on = !was;
      const delta = on ? 1 : -1;
      bump(delta, on);
      revert = () => bump(-delta, was);
      const answered = await api.commentVote(n, id, on);
      const shown = (detailRef.current[n] as IssueDetail | undefined)?.comments.find((c) => c.id === id)?.up ?? 0;
      const up = count(answered?.up, shown);
      revert = null;
      setDetails((current) => {
        const detail = current[n];
        return typeof detail === 'object' ? { ...current, [n]: { ...detail, comments: detail.comments.map((c) => (c.id === id ? { ...c, up } : c)) } } : current;
      });
      keepCommentUp(n, id, up, Date.now());
    } catch (error) {
      revert?.();
      failed('Vote', error);
    } finally {
      commentVotingRef.current.delete(pending);
      setCommentVotePending(new Set(commentVotingRef.current));
    }
  };

  /**
   * Whether a reload would lose something: text typed in a comment's box or the suggestion's form and not sent, or a
   * write still being sent. Reload then asks for a second press (Entry.tsx `ChunkFailed`).
   */
  const unsent = Object.values(drafts).some((draft) => draft !== '') || suggestDraft.title !== '' || suggestDraft.text !== ''
    || Object.values(sending).some((list) => list.length > 0) || suggestionsSending.length > 0 || votePending.size > 0 || commentVotePending.size > 0;

  const sendSuggestion = async (event: FormEvent) => {
    event.preventDefault();
    const title = suggestDraft.title.trim();
    const text = suggestDraft.text.trim();
    if (!title || !me || !live.state) return;
    const id = Date.now() + Math.random();
    const sent = sending$();
    const author = me.login;
    setSuggestionsSending((current) => [...current, { id, title, author }]);
    setSuggestDraft({ title: '', text: '' });
    setAnnounce('Sending suggestion');
    try {
      const { number } = await api.suggest(title, text);
      const issue: LiveIssue = { kind: 'suggestion', entry: null, title, state: 'open', reason: null, up: 0, comments: 0, author, bot: false, createdAt: new Date().toISOString(), closedAt: null };
      setDetails((current) => ({ ...current, [number]: { number, title, body: text, url: '', up: 0, comments: [] } }));
      remember({ n: number, set: issue }, sent);
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
      setCommentVotes({});
    } catch (error) {
      if (error instanceof ApiError && error.status === 401) signedOut();
      else setNotice('Could not sign out.');
    }
  };

  return {
    live, liveOk, waiting, offline, settled, me, retry, plan, rowByKey, details, prefetch, notes, wantNotes, facts, wantFacts, histories, wantHistory, fetchNow, sending, votePending, voteOwn,
    votes, commentVotes, commentVotePending, toggleCommentVote, suggestionsSending, notice, drafts, setDrafts, suggestDraft,
    setSuggestDraft, announce, setAnnounce, signinFailed, sendComment, toggleVote,
    sendSuggestion, signOut, unsent,
  };
}

export type Workshop = ReturnType<typeof useWorkshop>;
