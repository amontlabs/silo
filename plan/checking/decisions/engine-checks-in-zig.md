# In which language the engine's checks are written

The owner, 21 November 2026, after a trial ran a small real core in Zig over every sequence of
calls and under a simulator, with no model in another language: "Yes we go full zig." And: "it's
our language, it's great that we can test with it, and we'll really use the language to prove,
conceptualize, visualize, etc, we want to build from extremely strong principles and this is part
of it."

The trial is described in the notes of `core-simulator` and of `model-tool`. What a second
description costs is in the notes of `model-of-access-first`: it drifts away from the code, and a
check on it says nothing about the code.

One example. The promise "a precaution never gives a secret" is not rewritten in a checking
language. A test in Zig runs the engine's real code and looks at who holds which secret after
every call.
