#!/bin/sh
# id: K14
# title: #359's test 34 in its Linux form: a night of Claude Code and Codex through the boundary
# needs: K04, the boundary of step 6 (#373), a Claude subscription and a ChatGPT sign-in in main
# decides: model-traffic-read, recognising-a-conversation, provider-sees-silos-connection
# question: #373's conditions: every conversation recognised whole, the plan's headers read, every unusual reply a fact, nothing differs from the same night without the boundary
TEST=K14; . "$(dirname "$0")/../lib.sh"; need_kvm
[ -n "${KIT_BOUNDARY:-}" ] || skip "needs the boundary of step 6: set KIT_BOUNDARY to its command; #359's test 34 holds the night's script"
sh "$KIT_SRC/../../mac-test-kit/tests/34-night-through-the-boundary.sh" || fail "see #359's test 34 output"
