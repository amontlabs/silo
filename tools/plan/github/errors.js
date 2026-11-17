// The GitHub side's errors. Messages never carry a token, a key or a response body.

/** A refusal or failure the command reports as one plain line: exit 1. */
export class Failure extends Error {}

/** GitHub, or the credentials to reach it, could not be used: exit 3. */
export class Unavailable extends Failure {}

/** GitHub refused for its rate limit; `resetAt` is a Date or null. */
export class RateLimited extends Unavailable {
  /** @param {Date | null} resetAt */
  constructor(resetAt) {
    super(`GitHub's rate limit is reached; run again ${resetAt ? `after ${resetAt.toISOString()}` : 'later'}`);
    this.resetAt = resetAt;
  }
}

/** The feature has no issue on GitHub yet. */
export class NoIssue extends Failure {}
