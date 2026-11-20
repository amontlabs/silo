// The browser side of the Workshop's HTTP contract (`/api/workshop/`).
import type { LiveState } from './plan';

export const API = '/api/workshop';

export interface Me {
  login: string;
  avatar: string;
}

export interface Comment {
  id: string | number;
  author: string;
  avatar: string;
  bot: boolean;
  createdAt: string;
  text: string;
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
  constructor(readonly status: number, readonly code: string, message: string) {
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
    try {
      const body = (await response.json()) as { error?: unknown; message?: unknown };
      if (typeof body.error === 'string') code = body.error;
      if (typeof body.message === 'string' && body.message.trim()) message = body.message;
    } catch {
      // The status alone says enough.
    }
    throw new ApiError(response.status, code, message);
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
  myVote: async (n: number) => (await request<{ on: boolean }>(`vote?n=${n}`, { cache: 'no-store' })).on === true,
  /** `comments`: the issue's whole count after the comment (history lines included), when the server says. */
  comment: (n: number, text: string) => post<{ comments?: unknown }>('comment', { n, text }),
  /** `up`: the issue's 👍 count after the vote, when the server says. */
  vote: (n: number, on: boolean) => post<{ up?: unknown }>('vote', { n, on }),
  suggest: (title: string, text: string) => post<{ number: number }>('suggest', { title, text }),
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
