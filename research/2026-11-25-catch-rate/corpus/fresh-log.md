# Fresh corpus log

Built 2026-11-25 from public GitHub issues via the search API (`gh api search/issues`). Issue bodies came from the search results (`.body`); the `repos/.../issues` and comments endpoints were refused in this session, so every case is from the issue description and none from comments.

Repositories searched: anthropics/claude-code, openai/codex, google-gemini/gemini-cli, cline/cline, Aider-AI/aider, OpenHands/OpenHands (was All-Hands-AI), RooCodeInc/Roo-Code, microsoft/vscode, github/copilot-cli, cursor/cursor, continuedev/continue, aaif-goose/goose (was block/goose), anomalyco/opencode (was sst/opencode). Amp has no public tracker reachable by search (sourcegraph/amp refused). getcursor/cursor, block/goose, sst/opencode and All-Hands-AI/OpenHands were refused under those names in a first pass (not counted below).

Sources listed in old_sources.txt were excluded by exact URL. At most 2 cases per issue. No credential was seen in any issue used. One issue read but not used (openai/codex#42778) carried a reporter's personal email; nothing from it was copied.

`found_by` is the first query whose results contained the issue; a case can also match later queries.

## Queries

| query | results | new issues | cases |
|---|---|---|---|
| `falsely claimed repo:openai/codex is:issue` | 30 | 28 | 12 |
| `lied repo:anthropics/claude-code is:issue` | 30 | 26 | 7 |
| `lied repo:openai/codex is:issue` | 10 | 6 | 1 |
| `lied repo:google-gemini/gemini-cli is:issue` | 6 | 5 | 1 |
| `lied repo:cline/cline is:issue` | 1 | 1 | 1 |
| `lied repo:Aider-AI/aider is:issue` | 0 | 0 | 0 |
| `lied repo:OpenHands/OpenHands is:issue` | 0 | 0 | 0 |
| `lied repo:RooCodeInc/Roo-Code is:issue` | 2 | 2 | 0 |
| `lied repo:microsoft/vscode is:issue` | 30 | 30 | 5 |
| `lied repo:github/copilot-cli is:issue` | 0 | 0 | 0 |
| `lied repo:cursor/cursor is:issue` | 0 | 0 | 0 |
| `lied repo:continuedev/continue is:issue` | 1 | 1 | 0 |
| `lied repo:aaif-goose/goose is:issue` | 0 | 0 | 0 |
| `lied repo:anomalyco/opencode is:issue` | 5 | 5 | 0 |
| `falsely claimed repo:anthropics/claude-code is:issue` | 30 | 30 | 7 |
| `falsely claimed repo:google-gemini/gemini-cli is:issue` | 5 | 3 | 3 |
| `falsely claimed repo:cline/cline is:issue` | 3 | 2 | 2 |
| `falsely claimed repo:Aider-AI/aider is:issue` | 0 | 0 | 0 |
| `falsely claimed repo:OpenHands/OpenHands is:issue` | 0 | 0 | 0 |
| `falsely claimed repo:RooCodeInc/Roo-Code is:issue` | 0 | 0 | 0 |
| `falsely claimed repo:microsoft/vscode is:issue` | 6 | 6 | 0 |
| `falsely claimed repo:github/copilot-cli is:issue` | 0 | 0 | 0 |
| `falsely claimed repo:cursor/cursor is:issue` | 0 | 0 | 0 |
| `falsely claimed repo:continuedev/continue is:issue` | 1 | 1 | 0 |
| `falsely claimed repo:aaif-goose/goose is:issue` | 0 | 0 | 0 |
| `falsely claimed repo:anomalyco/opencode is:issue` | 2 | 2 | 0 |
| `claimed tests pass repo:anthropics/claude-code is:issue` | 30 | 27 | 5 |
| `claimed tests pass repo:openai/codex is:issue` | 30 | 29 | 2 |
| `claimed tests pass repo:google-gemini/gemini-cli is:issue` | 5 | 4 | 1 |
| `claimed tests pass repo:cline/cline is:issue` | 2 | 2 | 0 |
| `claimed tests pass repo:Aider-AI/aider is:issue` | 1 | 1 | 0 |
| `claimed tests pass repo:OpenHands/OpenHands is:issue` | 26 | 26 | 0 |
| `claimed tests pass repo:RooCodeInc/Roo-Code is:issue` | 4 | 4 | 0 |
| `claimed tests pass repo:microsoft/vscode is:issue` | 17 | 15 | 1 |
| `claimed tests pass repo:github/copilot-cli is:issue` | 1 | 1 | 0 |
| `claimed tests pass repo:cursor/cursor is:issue` | 0 | 0 | 0 |
| `claimed tests pass repo:continuedev/continue is:issue` | 0 | 0 | 0 |
| `claimed tests pass repo:aaif-goose/goose is:issue` | 3 | 3 | 1 |
| `claimed tests pass repo:anomalyco/opencode is:issue` | 23 | 23 | 0 |
| `said it was fixed repo:anthropics/claude-code is:issue` | 30 | 29 | 2 |
| `said it was fixed repo:openai/codex is:issue` | 30 | 29 | 0 |
| `said it was fixed repo:google-gemini/gemini-cli is:issue` | 30 | 29 | 0 |
| `said it was fixed repo:cline/cline is:issue` | 30 | 30 | 0 |
| `said it was fixed repo:Aider-AI/aider is:issue` | 18 | 18 | 0 |
| `said it was fixed repo:OpenHands/OpenHands is:issue` | 30 | 29 | 0 |
| `said it was fixed repo:RooCodeInc/Roo-Code is:issue` | 29 | 28 | 0 |
| `said it was fixed repo:microsoft/vscode is:issue` | 30 | 28 | 0 |
| `said it was fixed repo:github/copilot-cli is:issue` | 21 | 21 | 0 |
| `said it was fixed repo:cursor/cursor is:issue` | 0 | 0 | 0 |
| `said it was fixed repo:continuedev/continue is:issue` | 28 | 28 | 0 |
| `said it was fixed repo:aaif-goose/goose is:issue` | 26 | 26 | 0 |
| `said it was fixed repo:anomalyco/opencode is:issue` | 30 | 30 | 0 |
| `hallucinated success repo:anthropics/claude-code is:issue` | 30 | 25 | 3 |
| `hallucinated success repo:openai/codex is:issue` | 8 | 8 | 3 |
| `hallucinated success repo:google-gemini/gemini-cli is:issue` | 10 | 8 | 0 |
| `hallucinated success repo:cline/cline is:issue` | 0 | 0 | 0 |
| `hallucinated success repo:Aider-AI/aider is:issue` | 0 | 0 | 0 |
| `hallucinated success repo:OpenHands/OpenHands is:issue` | 1 | 0 | 0 |
| `hallucinated success repo:RooCodeInc/Roo-Code is:issue` | 1 | 1 | 0 |
| `hallucinated success repo:microsoft/vscode is:issue` | 6 | 5 | 0 |
| `hallucinated success repo:github/copilot-cli is:issue` | 0 | 0 | 0 |
| `hallucinated success repo:cursor/cursor is:issue` | 0 | 0 | 0 |
| `hallucinated success repo:continuedev/continue is:issue` | 2 | 2 | 0 |
| `hallucinated success repo:aaif-goose/goose is:issue` | 0 | 0 | 0 |
| `hallucinated success repo:anomalyco/opencode is:issue` | 9 | 9 | 2 |
| `claimed it deleted repo:anthropics/claude-code is:issue` | 30 | 22 | 2 |
| `claimed it deleted repo:openai/codex is:issue` | 30 | 27 | 0 |
| `claimed it deleted repo:google-gemini/gemini-cli is:issue` | 3 | 3 | 1 |
| `claimed it deleted repo:cline/cline is:issue` | 1 | 0 | 0 |
| `claimed it deleted repo:Aider-AI/aider is:issue` | 0 | 0 | 0 |
| `claimed it deleted repo:OpenHands/OpenHands is:issue` | 6 | 1 | 0 |
| `claimed it deleted repo:RooCodeInc/Roo-Code is:issue` | 0 | 0 | 0 |
| `claimed it deleted repo:microsoft/vscode is:issue` | 17 | 13 | 0 |
| `claimed it deleted repo:github/copilot-cli is:issue` | 2 | 2 | 0 |
| `claimed it deleted repo:cursor/cursor is:issue` | 0 | 0 | 0 |
| `claimed it deleted repo:continuedev/continue is:issue` | 2 | 1 | 0 |
| `claimed it deleted repo:aaif-goose/goose is:issue` | 0 | 0 | 0 |
| `claimed it deleted repo:anomalyco/opencode is:issue` | 15 | 8 | 0 |
| `reported success but repo:anthropics/claude-code is:issue` | 30 | 28 | 0 |
| `reported success but repo:openai/codex is:issue` | 30 | 29 | 0 |
| `reported success but repo:google-gemini/gemini-cli is:issue` | 30 | 24 | 0 |
| `reported success but repo:cline/cline is:issue` | 30 | 26 | 0 |
| `reported success but repo:Aider-AI/aider is:issue` | 3 | 1 | 0 |
| `reported success but repo:OpenHands/OpenHands is:issue` | 30 | 26 | 0 |
| `reported success but repo:RooCodeInc/Roo-Code is:issue` | 8 | 8 | 0 |
| `reported success but repo:microsoft/vscode is:issue` | 30 | 30 | 1 |
| `reported success but repo:github/copilot-cli is:issue` | 30 | 30 | 0 |
| `reported success but repo:cursor/cursor is:issue` | 0 | 0 | 0 |
| `reported success but repo:continuedev/continue is:issue` | 10 | 8 | 0 |
| `reported success but repo:aaif-goose/goose is:issue` | 22 | 22 | 0 |
| `reported success but repo:anomalyco/opencode is:issue` | 30 | 28 | 0 |
| `fake tests repo:anthropics/claude-code is:issue` | 30 | 25 | 1 |
| `fake tests repo:openai/codex is:issue` | 30 | 29 | 0 |
| `fake tests repo:google-gemini/gemini-cli is:issue` | 24 | 18 | 0 |
| `fake tests repo:cline/cline is:issue` | 5 | 5 | 0 |
| `fake tests repo:Aider-AI/aider is:issue` | 3 | 1 | 0 |
| `fake tests repo:OpenHands/OpenHands is:issue` | 19 | 15 | 0 |
| `fake tests repo:RooCodeInc/Roo-Code is:issue` | 11 | 11 | 0 |
| `fake tests repo:microsoft/vscode is:issue` | 30 | 30 | 1 |
| `fake tests repo:github/copilot-cli is:issue` | 2 | 2 | 0 |
| `fake tests repo:cursor/cursor is:issue` | 0 | 0 | 0 |
| `fake tests repo:continuedev/continue is:issue` | 3 | 2 | 0 |
| `fake tests repo:aaif-goose/goose is:issue` | 17 | 16 | 0 |
| `fake tests repo:anomalyco/opencode is:issue` | 30 | 27 | 0 |
| `modified the tests repo:anthropics/claude-code is:issue` | 30 | 29 | 0 |
| `modified the tests repo:openai/codex is:issue` | 30 | 28 | 0 |
| `modified the tests repo:google-gemini/gemini-cli is:issue` | 30 | 25 | 0 |
| `modified the tests repo:cline/cline is:issue` | 18 | 15 | 0 |
| `modified the tests repo:Aider-AI/aider is:issue` | 22 | 21 | 0 |
| `modified the tests repo:OpenHands/OpenHands is:issue` | 30 | 28 | 0 |
| `modified the tests repo:RooCodeInc/Roo-Code is:issue` | 30 | 28 | 0 |
| `modified the tests repo:microsoft/vscode is:issue` | 30 | 25 | 0 |
| `modified the tests repo:github/copilot-cli is:issue` | 7 | 7 | 0 |
| `modified the tests repo:cursor/cursor is:issue` | 0 | 0 | 0 |
| `modified the tests repo:continuedev/continue is:issue` | 30 | 27 | 0 |
| `modified the tests repo:aaif-goose/goose is:issue` | 10 | 10 | 0 |
| `modified the tests repo:anomalyco/opencode is:issue` | 30 | 25 | 0 |
| `claimed to have run repo:anthropics/claude-code is:issue` | 30 | 23 | 1 |
| `claimed to have run repo:openai/codex is:issue` | 30 | 22 | 0 |
| `claimed to have run repo:google-gemini/gemini-cli is:issue` | 12 | 5 | 0 |
| `attempt_completion but repo:RooCodeInc/Roo-Code is:issue` | 30 | 27 | 0 |
| `claimed completed repo:RooCodeInc/Roo-Code is:issue` | 1 | 1 | 0 |
| `said it applied repo:Aider-AI/aider is:issue` | 14 | 6 | 0 |
| `claims it repo:Aider-AI/aider is:issue` | 12 | 11 | 0 |
| `falsely repo:github/copilot-cli is:issue` | 5 | 5 | 0 |
| `said it repo:OpenHands/OpenHands is:issue` | 30 | 14 | 0 |
| `claims success repo:OpenHands/OpenHands is:issue` | 10 | 7 | 0 |
| `fabricated repo:anomalyco/opencode is:issue` | 30 | 27 | 2 |
| `pretends repo:anomalyco/opencode is:issue` | 4 | 4 | 0 |
| `said done repo:aaif-goose/goose is:issue` | 10 | 6 | 0 |
| `rm -rf repo:google-gemini/gemini-cli is:issue` | 30 | 26 | 2 |
| `deleted my repo:google-gemini/gemini-cli is:issue` | 30 | 22 | 0 |
| `said it pushed repo:google-gemini/gemini-cli is:issue` | 13 | 10 | 0 |
| `admitted repo:google-gemini/gemini-cli is:issue` | 9 | 6 | 1 |
| `claimed it ran repo:google-gemini/gemini-cli is:issue` | 4 | 0 | 0 |
| `said tests passed repo:google-gemini/gemini-cli is:issue` | 5 | 1 | 0 |
| `claimed success repo:cline/cline is:issue` | 4 | 0 | 0 |
| `deleted my repo:cline/cline is:issue` | 30 | 27 | 0 |
| `rm -rf repo:cline/cline is:issue` | 30 | 24 | 0 |
| `attempt_completion repo:cline/cline is:issue` | 30 | 27 | 0 |
| `said repo:continuedev/continue is:issue` | 30 | 20 | 0 |
| `claimed completed repo:microsoft/vscode is:issue` | 30 | 25 | 0 |
| `agent said repo:github/copilot-cli is:issue` | 30 | 25 | 0 |
| `deleted my repo:github/copilot-cli is:issue` | 30 | 26 | 0 |
| `background process repo:google-gemini/gemini-cli is:issue` | 30 | 25 | 0 |
| `lied repo:cursor/cursor is:issue` | 0 | 0 | 0 |
| `claims to have repo:RooCodeInc/Roo-Code is:issue` | 16 | 14 | 0 |
| `lying repo:RooCodeInc/Roo-Code is:issue` | 0 | 0 | 0 |
| `says it repo:Aider-AI/aider is:issue` | 30 | 27 | 3 |
| `deleted repo:cursor/cursor is:issue` | 0 | 0 | 0 |
| `lying repo:cline/cline is:issue` | 5 | 2 | 1 |

Queries that failed after three tries (rate limit or repository refused) and gave nothing: `lied`/`claimed`/`deleted` on cursor/cursor; `lied`, `agent said` variants on github/copilot-cli where not listed above; `lied`/`claimed` on aaif-goose/goose and anomalyco/opencode where not listed above; and the first-pass queries on renamed repositories. The batch run over 22 phrase families was stopped part way (after `claimed to have run` on openai/codex) and replaced by targeted queries per repository, so later phrase families (`did not actually run`, `pretended`, `said it committed`, `claimed it pushed`, `no-verify`, `deleted my`, `rm -rf`, `outside the project`, `background process still running`, `false completion`, `said tests passed`, `admitted`) ran only on the repositories listed above.

## Counts

- cases: 75 from 58 issues
- program: aider: 3, claude-code: 28, cline: 4, codex: 18, copilot: 8, gemini-cli: 7, goose: 1, opencode: 4, other: 2
- truth: false: 57, true: 16, unclear: 2
- outside_record_shows_it: no: 8, partly: 23, yes: 44
- claim_verbatim: False: 35, True: 40
- asked_verbatim: False: 51, True: 24
- record_from: issue-description: 75
- commands in records: 84; exit code known for 14; order `t` known for 66
- cases with an empty record (nothing an observer saw is described): 18

`other` is Gemini agents in Antigravity, reported in gemini-cli (#30345).
