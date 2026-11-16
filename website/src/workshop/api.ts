// The browser side of the Workshop's HTTP contract (`/api/workshop/`).
import type { LiveState, Verdict } from './plan';

export const API = '/api/workshop';

export interface Me {
  login: string;
  avatar: string;
  canDecide: boolean;
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

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  let response: Response;
  try {
    response = await fetch(`${API}/${path}`, { credentials: 'same-origin', ...init });
  } catch {
    throw new ApiError(0, 'network', 'No connection.');
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
  return (await response.json()) as T;
}

function post<T>(path: string, body: unknown): Promise<T> {
  return request<T>(path, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', 'X-Workshop': '1' },
    body: JSON.stringify(body),
  });
}

export const api = {
  state: () => request<LiveState>('state'),
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
  comment: (n: number, text: string) => post<unknown>('comment', { n, text }),
  vote: (n: number, on: boolean) => post<unknown>('vote', { n, on }),
  suggest: (title: string, text: string) => post<{ number: number }>('suggest', { title, text }),
  verdict: (n: number, verdict: Verdict | null, note?: string) =>
    post<unknown>('verdict', note === undefined ? { n, verdict } : { n, verdict, note }),
  decide: (n: number, text: string) => post<unknown>('decide', { n, text }),
  logout: () => post<unknown>('logout', {}),
};

export function loginUrl(hash: string): string {
  return `${API}/login?return=${encodeURIComponent(`/workshop${hash}`)}`;
}

/** The server sets this readable marker beside the session cookie; without it there is no session to ask about. */
export function hasSessionHint(): boolean {
  return /(?:^|;\s*)workshop-hint=1(?:;|$)/.test(document.cookie);
}
