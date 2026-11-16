// The one in-memory snapshot that answers every anonymous read. A visitor's
// request never calls GitHub: it reads the snapshot, and when the snapshot is
// over 30 seconds old it starts one background refresh and waits for it at
// most 1.5 seconds. A change made on GitHub through the Workshop starts one at
// once. Only a cold instance waits for its first refresh.
//
// Refreshes are coalesced: at most one runs and one waits. A request for a
// refresh while one runs makes exactly one more start when it ends, so a write
// is always included. A refresh asked for by a write starts at most once every
// 5 seconds. None starts while GitHub has asked us to wait. A refresh that has
// not finished after 15 seconds is treated as lost, and the next request
// starts a new one; every refresh stops (its calls are aborted) after 12
// seconds or when declared lost, so two never spend at once. Work that outlives a response is handed to `keepAlive`
// (Vercel's `waitUntil`), but nothing depends on it for correctness.
//
// A refresh reads, with the installation token:
// - one issue list per Workshop label (entries, then decisions, then
//   suggestions; all states, newest first, 100 a page), each page sent with the
//   ETag from the last refresh, so an unchanged page is a 304; nothing else in
//   the repository is fetched or counted;
// - the repository-wide comment list: the newest comments in full on the first
//   refresh and every 10 minutes (so deleted comments disappear), and otherwise
//   only comments updated since the previous refresh, less a minute of overlap.
// Only ETags and the reduced Workshop data are kept, never GitHub's raw objects.
// A refresh builds a new snapshot and swaps it in only when it completed.
//
// Writes patch the snapshot at once. Each patch is kept with its time: after a
// refresh swaps in, the patches made since that refresh began are applied
// again, and older ones are dropped, since GitHub's answer already holds them.
import { GitHubError, nextLink } from './github.mjs';
import { clean, commentIssue, commentOf, kindOf, LABELS, readBody, reasonOf, verdictOf } from './issues.mjs';

export const SNAPSHOT = {
  maxAge: 30_000,
  writeGap: 5000,
  lostAfter: 15_000,
  refreshLimit: 12_000,
  ageWait: 1500,
  writeWait: 2000,
  staleAfter: 10 * 60_000,
  fullReloadEvery: 10 * 60_000,
  overlap: 60_000,
  retryAfter: 60_000,
  issues: 2000,
  issuePages: 20,
  fullCommentPages: 15,
  sinceCommentPages: 10,
  commentsPerIssue: 100,
  comments: 5000,
  patches: 500,
};

const byAge = (a, b) => (a.createdAt < b.createdAt ? -1 : a.createdAt > b.createdAt ? 1 : a.id - b.id);

// The Workshop part of one issue from the list, or null for anything else.
function reduceIssue(issue) {
  const kind = kindOf(issue);
  if (!kind) return null;
  const { entry, text } = readBody(issue.body);
  return {
    number: issue.number,
    summary: {
      kind,
      entry: kind === 'entry' ? entry : null,
      title: clean(issue.title, { oneLine: true }),
      state: issue.state === 'closed' ? 'closed' : 'open',
      reason: reasonOf(issue),
      verdict: verdictOf(issue),
      up: issue.reactions?.['+1'] ?? 0,
      comments: issue.comments ?? 0,
      author: clean(issue.user?.login ?? 'ghost', { oneLine: true }),
      bot: issue.user?.type === 'Bot',
      createdAt: issue.created_at,
      closedAt: issue.closed_at ?? null,
    },
    body: text,
    url: issue.html_url,
  };
}

// `request(method, path, options)` calls GitHub with the installation token and
// throws a GitHubError when GitHub fails or the installation must wait.
// `detach(fn)` runs fn outside the current request (no request deadline).
// `keepAlive(promise)` asks the platform to keep the instance running until it settles.
// `afterRefresh()` runs, detached, after each successful refresh.
// `wall()` is the real clock for the floor, the lost-refresh limit and the waits.
export function createSnapshot({
  repo, request, now = Date.now, wall = Date.now, log = () => {},
  detach = (fn) => fn(), keepAlive = () => {}, afterRefresh = () => {},
}) {
  let current = null;
  let run = null;
  let generation = 0;
  let swapped = 0;
  let again = false;
  let timer = null;
  let timerAt = 0;
  let lastStart = -Infinity;
  let retryAt = 0;
  let lastError = null;
  let lastFull = 0;
  let fullDue = true;
  let issuePages = new Map();
  let commentPages = new Map();
  let patches = [];
  let waiters = [];

  function entryOf(reduced, previous) {
    return {
      number: reduced.number,
      summary: { ...reduced.summary },
      body: reduced.body,
      url: reduced.url,
      comments: previous ? new Map(previous.comments) : new Map(),
    };
  }

  // Pages of one listing, each conditional on its last ETag. Each page keeps only its ETag and the
  // items `reduce` returns (null drops an item). `fresh` collects the pages for the next refresh.
  async function pages(path, limit, cache, reduce, signal, fresh = new Map()) {
    const items = [];
    let complete = false;
    let used = 0;
    for (let page = 1; page <= limit; page += 1) {
      const url = `${path}&per_page=100&page=${page}`;
      const known = cache?.get(url);
      used += 1;
      const answer = await request('GET', url, { etag: known?.etag, signal });
      let entry;
      if (answer.status === 304 && known) {
        entry = known;
      } else {
        const raw = Array.isArray(answer.data) ? answer.data : [];
        entry = { etag: answer.headers.get('etag'), items: raw.map(reduce).filter(Boolean), next: Boolean(nextLink(answer.headers.get('link'))) };
      }
      fresh.set(url, entry);
      items.push(...entry.items);
      if (!entry.next) {
        complete = true;
        break;
      }
    }
    return { items, cache: fresh, complete, used };
  }

  function trim(issues) {
    for (const entry of issues.values()) {
      if (entry.comments.size <= SNAPSHOT.commentsPerIssue) continue;
      const kept = [...entry.comments.values()].sort(byAge).slice(-SNAPSHOT.commentsPerIssue);
      entry.comments = new Map(kept.map((comment) => [comment.id, comment]));
    }
    let total = 0;
    for (const entry of issues.values()) total += entry.comments.size;
    if (total <= SNAPSHOT.comments) return;
    const all = [];
    for (const entry of issues.values()) for (const comment of entry.comments.values()) all.push([entry, comment]);
    all.sort((a, b) => byAge(a[1], b[1]));
    for (const [entry, comment] of all.slice(0, total - SNAPSHOT.comments)) entry.comments.delete(comment.id);
  }

  async function build(signal) {
    const started = now();
    // One conditional list per kind label, sharing one page budget.
    const listing = { items: [], cache: new Map(), complete: true };
    let budget = SNAPSHOT.issuePages;
    for (const label of Object.values(LABELS)) {
      if (budget <= 0) {
        listing.complete = false;
        break;
      }
      const part = await pages(`/repos/${repo}/issues?labels=${encodeURIComponent(label)}&state=all&sort=created&direction=desc`, budget, issuePages, reduceIssue, signal, listing.cache);
      budget -= part.used;
      listing.items.push(...part.items);
      if (!part.complete) listing.complete = false;
    }
    if (!listing.complete) log({ snapshot: 'issue_pages_limit' });

    const full = fullDue || !current || started - lastFull >= SNAPSHOT.fullReloadEvery;
    const issues = new Map();
    for (const reduced of listing.items) {
      if (issues.size >= SNAPSHOT.issues) break;
      if (issues.has(reduced.number)) continue;
      issues.set(reduced.number, entryOf(reduced, full ? null : current?.issues.get(reduced.number)));
    }

    const reduceComment = (comment) => {
      const number = commentIssue(comment);
      return issues.has(number) ? { number, comment: commentOf(comment) } : null;
    };
    let comments;
    let incompleteSince = false;
    if (full) {
      comments = await pages(`/repos/${repo}/issues/comments?sort=created&direction=desc`, SNAPSHOT.fullCommentPages, commentPages, reduceComment, signal);
    } else {
      const since = new Date(current.startedAt - SNAPSHOT.overlap).toISOString().replace(/\.\d{3}Z$/, 'Z');
      comments = await pages(`/repos/${repo}/issues/comments?sort=updated&direction=asc&since=${since}`, SNAPSHOT.sinceCommentPages, null, reduceComment, signal);
      incompleteSince = !comments.complete;
    }
    for (const { number, comment } of comments.items) issues.get(number)?.comments.set(comment.id, comment);
    trim(issues);

    issuePages = listing.cache;
    if (full) {
      commentPages = comments.cache;
      lastFull = started;
    }
    fullDue = incompleteSince;
    return { startedAt: started, at: now(), issues, version: 0, stateAnswer: null };
  }

  function swapIn(snapshot) {
    patches = patches.filter((patch) => patch.at >= snapshot.startedAt);
    for (const patch of patches) patch.apply(snapshot);
    current = snapshot;
  }

  function settleWaiters(startedWall) {
    const done = waiters.filter((waiter) => waiter.since <= startedWall);
    waiters = waiters.filter((waiter) => waiter.since > startedWall);
    for (const waiter of done) waiter.resolve();
  }

  // A refresh that has run longer than SNAPSHOT.lostAfter is forgotten; its late result is ignored
  // when something newer has been swapped in meanwhile.
  function forgetLost() {
    if (run && wall() - run.startedWall > SNAPSHOT.lostAfter) {
      log({ snapshot: 'refresh_lost' });
      run.controller.abort();
      run = null;
    }
  }

  function blocked() {
    return now() < retryAt;
  }

  // Starts a refresh now; when it ends, a refresh asked for meanwhile is scheduled.
  function start() {
    const gen = ++generation;
    const startedWall = wall();
    lastStart = startedWall;
    let localRetry = 0;
    const controller = new AbortController();
    const limit = setTimeout(() => controller.abort(), SNAPSHOT.refreshLimit);
    limit.unref?.();
    const promise = detach(() => build(controller.signal))
      .then((snapshot) => {
        if (gen > swapped) {
          swapped = gen;
          swapIn(snapshot);
        }
        retryAt = 0;
        lastError = null;
        try {
          detach(afterRefresh);
        } catch {
          // The follow-up work is best effort and never fails the refresh.
        }
        return snapshot;
      }, (error) => {
        if (run?.gen === gen) {
          const wait = error instanceof GitHubError && error.retryAfter ? error.retryAfter * 1000 : 0;
          if (error instanceof GitHubError && error.local && current) {
            // Our own ceiling ran out: try again when it resets, without the wait meant for GitHub.
            localRetry = wait;
          } else if (wall() - startedWall > SNAPSHOT.lostAfter) {
            // A failure this late comes from an instance that was frozen, not from GitHub.
          } else {
            retryAt = now() + Math.max(SNAPSHOT.retryAfter, wait);
          }
          lastError = error;
        }
        log({ snapshot: 'refresh_failed', ...(error instanceof GitHubError ? { github: error.status } : { failure: error?.name || 'Error' }) });
        throw error;
      })
      .finally(() => {
        clearTimeout(limit);
        settleWaiters(startedWall);
        if (run?.gen !== gen) return;
        run = null;
        const followUp = again;
        again = false;
        if (localRetry) schedule({ delay: localRetry });
        else if (followUp && !blocked()) schedule({ write: true });
      });
    run = { gen, startedWall, promise, controller };
    keepAlive(promise.catch(() => {}));
    return promise;
  }

  // Asks for a refresh as soon as allowed: after the running one, and, when a write asks,
  // SNAPSHOT.writeGap after the last start.
  function schedule({ write = false, delay = 0 } = {}) {
    if (blocked()) return;
    forgetLost();
    if (run) {
      again = true;
      return;
    }
    if (timer) return;
    const wait = Math.max(delay, write ? lastStart + SNAPSHOT.writeGap - wall() : 0);
    if (wait <= 0) {
      start().catch(() => {});
      return;
    }
    let done;
    keepAlive(new Promise((resolve) => { done = resolve; }));
    timerAt = wall() + wait;
    timer = setTimeout(() => {
      timer = null;
      if (blocked()) return done();
      if (run) {
        again = true;
        return done();
      }
      start().catch(() => {}).finally(done);
    }, wait);
    timer.unref?.();
  }

  // Resolves when a refresh that started at or after wall-clock `since` has ended, or after `ms`.
  function waitFor(since, ms) {
    return new Promise((resolve) => {
      const waiter = { since, resolve: () => { clearTimeout(waiter.timer); resolve(); } };
      waiter.timer = setTimeout(() => {
        waiters = waiters.filter((other) => other !== waiter);
        resolve();
      }, ms);
      waiter.timer.unref?.();
      waiters.push(waiter);
    });
  }

  // The snapshot to answer from and whether it is stale; throws when there is none. When the snapshot
  // is over SNAPSHOT.maxAge old, a refresh starts and this read waits for it up to SNAPSHOT.ageWait.
  async function read() {
    forgetLost();
    if (!current) {
      if (blocked()) throw lastError instanceof GitHubError ? lastError : new GitHubError(502);
      await (run?.promise ?? start());
    } else if (now() - current.startedAt >= SNAPSHOT.maxAge && !blocked() && !run && !timer) {
      const since = wall();
      schedule();
      if (run) await waitFor(since, SNAPSHOT.ageWait);
    }
    return { snapshot: current, stale: now() - current.at > SNAPSHOT.staleAfter };
  }

  // After a change on GitHub: refresh at once (coalesced, at most one every SNAPSHOT.writeGap),
  // unless GitHub asked us to wait. Answers a promise that settles when a refresh that includes the
  // change has ended, or after SNAPSHOT.writeWait.
  // The write waits only when the refresh that will include it is running or starts within the wait.
  function refreshSoon() {
    const since = wall();
    if (blocked()) return Promise.resolve();
    schedule({ write: true });
    let startsAt = Infinity;
    if (run && run.startedWall >= since) startsAt = run.startedWall;
    else if (timer) startsAt = timerAt;
    else if (run) startsAt = Math.max(wall(), lastStart + SNAPSHOT.writeGap);
    if (startsAt - wall() > SNAPSHOT.writeWait) return Promise.resolve();
    return waitFor(since, SNAPSHOT.writeWait);
  }

  function stateAnswer(snapshot) {
    if (!snapshot.stateAnswer || snapshot.stateAnswer.version !== snapshot.version) {
      const issues = {};
      for (const [number, entry] of snapshot.issues) issues[number] = entry.summary;
      snapshot.stateAnswer = { version: snapshot.version, body: { repo, fetchedAt: new Date(snapshot.at).toISOString(), issues } };
    }
    return snapshot.stateAnswer.body;
  }

  function issueAnswer(snapshot, number) {
    const entry = snapshot.issues.get(number);
    if (!entry) return null;
    const comments = [...entry.comments.values()].sort(byAge);
    return {
      number,
      title: entry.summary.title,
      body: entry.body,
      url: entry.url,
      up: entry.summary.up,
      more: entry.summary.comments > comments.length,
      comments,
    };
  }

  // Records a patch and applies it to the current snapshot. `apply(snapshot)` must be safe to run
  // again on a newer snapshot: counts move only from the value they had when the write happened.
  function record(apply) {
    patches.push({ at: now(), apply });
    if (patches.length > SNAPSHOT.patches) patches.shift();
    if (current) apply(current);
  }

  function onEntry(number, change) {
    return (snapshot) => {
      const entry = snapshot.issues.get(number);
      if (!entry) return;
      change(entry);
      snapshot.version += 1;
    };
  }

  // A count moves from `from` to `to` only when the snapshot still shows `from`.
  function count(number, field, delta) {
    const entry = current?.issues.get(number);
    if (!entry) return;
    const from = entry.summary[field];
    const to = Math.max(0, from + delta);
    record(onEntry(number, (target) => { if (target.summary[field] === from) target.summary[field] = to; }));
  }

  const patch = {
    comment(number, comment) {
      const shown = commentOf(comment);
      count(number, 'comments', 1);
      record(onEntry(number, (entry) => {
        if (entry.comments.has(shown.id)) return;
        entry.comments.set(shown.id, shown);
        trim(new Map([[number, entry]]));
      }));
    },
    up(number, delta) {
      count(number, 'up', delta);
    },
    fields(number, fields) {
      record(onEntry(number, (entry) => Object.assign(entry.summary, fields)));
    },
    issue(issue, kind) {
      const reduced = reduceIssue({ ...issue, labels: [{ name: `workshop:${kind}` }] });
      if (!reduced) return;
      record((snapshot) => {
        if (snapshot.issues.has(issue.number) || snapshot.issues.size >= SNAPSHOT.issues) return;
        snapshot.issues.set(issue.number, entryOf(reduced, null));
        snapshot.version += 1;
      });
    },
  };

  // Settles once no refresh is running or waiting (for tests and graceful shutdown).
  async function idle() {
    for (;;) {
      if (run) await run.promise.then(() => {}, () => {});
      else if (timer) await new Promise((resolve) => setTimeout(resolve, Math.max(5, lastStart + SNAPSHOT.writeGap - wall() + 5)));
      else return;
    }
  }

  return { read, stateAnswer, issueAnswer, patch, idle, refreshSoon };
}
