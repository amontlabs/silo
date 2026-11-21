# The secrets of an extension

The owner, on 21 November 2026: an extension should be able to declare secrets that the user has
to give, and it then just uses Silo's mechanism for secrets.

How a computer uses a secret is in the part `boundary`: `secret-placeholder`, `secret-entry`,
`secret-values`. The hosts the extension's computer may reach are rules of that computer, shown
when the extension is imported.
