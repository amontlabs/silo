# Receipts on pull requests

## The choice

An agent's part, 24 November 2026. A reviewer opens a pull request of 40 files an agent wrote, and
wants to know what it was asked, what ran, and what passed.

The options:

- Build it (recommended). The member prints the receipts behind the pull request at the level they
  pick (`receipt-on-a-pull-request`), signed by their Silo (`shared-receipt-signed`). It costs a
  key per Mac.
- Leave it out. It costs the one place where a receipt is read by someone other than its owner.

Recommended: build it. It also makes Silo seen by every reviewer of every such pull request.
