// The live state of the plan on GitHub, as `status` and `show` read it.
//
// One interface: `read()` resolves to either
//   { available: false, reason, failed }   `failed` true when GitHub was tried and could not be read (exit 3)
//   { available: true, entries, suggestions }
// with
//   entries[id] = { number, url, verdict, building, branch, up, comments: [{ author, createdAt, body }],
//                   history: [{ createdAt, url, change }] }   (`change` as model/markers.js readChange gives it)
//   suggestions = [{ number, url, title, up, open }]
//
// The only implementation for now answers "not available"; the GitHub side replaces it.

/** @typedef {{ available: false, reason: string, failed: boolean }} LiveUnavailable */
/**
 * @typedef {{ number: number, url: string, verdict: import('../model/status.js').Verdict | null, building: boolean, branch?: string | null, up: number,
 *   comments: { author: string, createdAt: string, body: string }[], history: { createdAt: string, url?: string, change: import('../model/markers.js').Change }[] }} LiveEntry
 */
/** @typedef {{ number: number, url: string, title: string, up: number, open: boolean }} LiveSuggestion */
/** @typedef {LiveUnavailable | { available: true, entries: Record<string, LiveEntry>, suggestions: LiveSuggestion[] }} LiveState */
/** @typedef {{ read(): Promise<LiveState> }} Live */

/** @returns {Live} */
export function offlineLive() {
  return {
    async read() {
      return { available: false, reason: 'live state from GitHub is not available yet', failed: false };
    },
  };
}
