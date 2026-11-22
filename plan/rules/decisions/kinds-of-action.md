# What kind each action is

| Kind | Meaning | Actions | Who may trigger it |
|---|---|---|---|
| granting | A computer can do something it could not just before. | allow, resume, start, fork, give a secret, open a port or any other channel, grant an extension a tool (by installing it) | you, or a rule |
| destructive | It ends or erases something, and that cannot be undone. | stop, restore, drop | you, or a rule |
| protective | It grants nothing and destroys nothing. A wrong one costs only time. | alert, checkpoint, hold, deny, pause, take back a secret, close a network or any other channel | you, a rule, or a precaution |

polarzero agreed the three kinds, who may trigger each, and the actions allow, resume, give a
secret, open a port, fork; stop, restore, drop; alert, checkpoint, hold, deny, pause. polarzero then
agreed that start, and open a port or any other channel, are granting, and that
taking back a secret and closing a network or any other channel are protective.

"Grant an extension a tool" is granting because an extension can then do what it could not just
before: read a record, show something in the app, or protect (`extension-tools`, in `extensions`).

The kinds of the other actions the design names are not decided: `kinds-of-other-actions`. How
a fork becomes main, and the kind of that action, are open in `going-back`: `fork-becomes-main`.

`every-action-has-a-kind` is checked by a table checked at compile time.
