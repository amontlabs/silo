// Errors the store, the CLI and the server share. Each carries the exit code
// the CLI uses and the HTTP status the server uses.
export class DashboardError extends Error {
  constructor(message, { exit = 1, status = 500 } = {}) {
    super(message);
    this.name = new.target.name;
    this.exit = exit;
    this.status = status;
  }
}

export class ValidationError extends DashboardError {
  constructor(message) {
    super(message, { exit: 2, status: 400 });
  }
}

export class NotFoundError extends DashboardError {
  constructor(message) {
    super(message, { exit: 1, status: 404 });
  }
}

export class ConflictError extends DashboardError {
  constructor(message) {
    super(message, { exit: 1, status: 409 });
  }
}

export class StoreVersionError extends DashboardError {
  constructor(found, known) {
    super(
      `dashboard.db has schema version ${found}, but this dashboard only knows version ${known}. ` +
        'Update the repository checkout that runs the dashboard; the database was not changed.',
      { exit: 1, status: 500 },
    );
  }
}
