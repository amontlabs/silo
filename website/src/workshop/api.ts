// The browser side of the Workshop's HTTP contract (`/api/workshop/`).
import type { LiveState, Verdict } from './plan';

export const API = '/api/workshop';

export interface Me {
  login: string;
  avatar: string;
  /** The owner, who takes and drops entries (`POST verdict`); `me` says so, and nothing remembers it. */
  owner?: true;
}

export interface Comment {
  id: string | number;
  author: string;
  avatar: string;
  bot: boolean;
  createdAt: string;
  text: string;
  /** The comment's 👍 count; absent counts as 0. */
  up?: number;
}

/** What a write names: an issue by its number, or an entry of the plan by its id (the server finds or opens its thread). */
export type Target = { n: number } | { entry: string };

/** What the person's own votes on an issue are: its 👍, and the comments they gave one. */
export interface MyVotes {
  on: boolean;
  comments: (string | number)[];
}

export interface IssueDetail {
  number: number;
  title: string;
  body: string;
  url: string;
  up: number;
  comments: Comment[];
  /** True when GitHub holds earlier comments than these. */
  more?: boolean;
}

export class ApiError extends Error {
  /** `n`: the thread, when the server names it in its refusal of a write on an entry. */
  constructor(readonly status: number, readonly code: string, message: string, readonly n: number | null = null) {
    super(message);
  }
}

const FALLBACK: Record<number, string> = {
  400: 'The request was refused.',
  401: 'Sign in again.',
  403: 'Not allowed.',
  404: 'Not found.',
  429: 'GitHub is busy. Try again in a minute.',
  502: 'GitHub did not answer.',
  503: 'Not available.',
};

/** A write the server has not answered after this long counts as not saved. */
export const apiConfig = { writeTimeoutMs: 20_000 };

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  return (await respond<T>(path, init)).body;
}

async function respond<T>(path: string, init?: RequestInit): Promise<{ body: T; headers: Headers }> {
  let response: Response;
  const write = init?.method === 'POST';
  const controller = write ? new AbortController() : null;
  const timer = controller ? setTimeout(() => controller.abort(), apiConfig.writeTimeoutMs) : undefined;
  try {
    response = await fetch(`${API}/${path}`, { credentials: 'same-origin', ...init, ...(controller ? { signal: controller.signal } : {}) });
  } catch {
    if (controller?.signal.aborted) throw new ApiError(0, 'timeout', 'No answer.');
    throw new ApiError(0, 'network', 'No connection.');
  } finally {
    clearTimeout(timer);
  }
  if (!response.ok) {
    let code = 'error';
    let message = FALLBACK[response.status] ?? 'Something went wrong.';
    let n: number | null = null;
    try {
      const body = (await response.json()) as { error?: unknown; message?: unknown; n?: unknown };
      if (typeof body.error === 'string') code = body.error;
      if (typeof body.n === 'number' && Number.isSafeInteger(body.n) && body.n > 0) n = body.n;
      if (typeof body.message === 'string' && body.message.trim()) message = body.message;
    } catch {
      // The status alone says enough.
    }
    throw new ApiError(response.status, code, message, n);
  }
  return { body: (await response.json()) as T, headers: response.headers };
}

/** A cached answer this many seconds old or more counts as old: the CDN serves it while it revalidates. */
const STALE_AGE_S = 10;

/** The live state; `fresh` waits for a snapshot at most 30 seconds old. `old` is set when the answer is older. */
async function state(fresh = false): Promise<LiveState> {
  const { body, headers } = await respond<LiveState>(fresh ? 'state?fresh=1' : 'state');
  const age = Number(headers.get('Age'));
  return Number.isFinite(age) && age > STALE_AGE_S ? { ...body, old: true } : body;
}

function post<T>(path: string, body: unknown): Promise<T> {
  return request<T>(path, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', 'X-Workshop': '1' },
    body: JSON.stringify(body),
  });
}

export const api = {
  state,
  me: async (): Promise<Me | null> => {
    if (!hasSessionHint()) return null;
    try {
      return await request<Me>('me', { cache: 'no-store' });
    } catch (error) {
      if (error instanceof ApiError && error.status === 401) return null;
      throw error;
    }
  },
  issue: (n: number) => request<IssueDetail>(`issue?n=${n}`),
  myVote: async (n: number): Promise<MyVotes> => {
    const body = await request<{ on?: unknown; comments?: unknown }>(`vote?n=${n}`, { cache: 'no-store' });
    return { on: body.on === true, comments: Array.isArray(body.comments) ? body.comments.filter((id): id is string | number => typeof id === 'number' || typeof id === 'string') : [] };
  },
  /** `comments`: the issue's whole count after the comment, when the server says; `id`: the comment; `n`: the thread, answered when the write named an entry. */
  comment: (target: Target, text: string) => post<{ comments?: unknown; id?: unknown; n?: unknown }>('comment', { ...target, text }),
  /** `up`: the issue's 👍 count after the vote, when the server says; `n`: the thread, answered when the write named an entry. */
  vote: (target: Target, on: boolean) => post<{ up?: unknown; n?: unknown }>('vote', { ...target, on }),
  /** `up`: the comment's 👍 count after the vote, when the server says. */
  commentVote: (n: number, comment: string | number, on: boolean) => post<{ up?: unknown }>('comment-vote', { n, comment, on }),
  suggest: (title: string, text: string) => post<{ number: number }>('suggest', { title, text }),
  /**
   * The owner's verdict on an entry, written on main as `plan take` or `plan drop` would. `built` is the commit the
   * page was built from: the server refuses when the entry changed on main since. `commit` is the commit made, null
   * when the entry already had that verdict.
   */
  verdict: (entry: string, verdict: Verdict, built: string) => post<{ commit?: unknown; already?: unknown }>('verdict', { entry, verdict, built }),
  logout: () => post<unknown>('logout', {}),
};

export function loginUrl(hash: string): string {
  return `${API}/login?return=${encodeURIComponent(`/workshop${hash}`)}`;
}

/** The server sets this readable marker beside the session cookie; without it there is no session to ask about. */
export function hasSessionHint(): boolean {
  // At build time there is no document: nobody is signed in.
  if (typeof document === 'undefined') return false;
  return /(?:^|;\s*)workshop-hint=1(?:;|$)/.test(document.cookie);
}
