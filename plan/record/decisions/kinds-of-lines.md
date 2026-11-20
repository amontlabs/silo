# Kinds of facts and reports

| Kind | What it says | Fact or report |
|---|---|---|
| control | A computer was created, started, paused, resumed, stopped or dropped. A checkpoint, a fork, a restore. A rule or a precaution was set. An ask was raised and answered. A question was put to you and answered. An alert was raised. A classifier gave an opinion. A precaution acted. A secret was given or taken back. A classifier was let read a record. Each with who did it. | Fact |
| process | Started: command, parent, user. Exited: code, and totals of processor, memory and disk use. | Report |
| file | Created, changed, deleted or renamed: path and change in size. | Fact. Which process did it is a separate report that points to it. |
| connection | Host, port, bytes each way. Allowed, denied or held, and what decided it: a rule or a precaution with its version, your answer, Silo's limit on a hold, or no rule at all. For opened traffic: method, path, status and sizes. The secret used, by name. | Fact. Which process owned it is a separate report. |
| model call | Provider and login by name. Model asked and model that answered. Tokens in, out and cached, and cost. Conversation and turn. The tool calls the reply asked for, and the result sent back for each, which includes the output of commands. A refusal or a reroute. What the provider says is left of a limit. | Fact, when the traffic can be read. An agent program's own account of a call is a separate report. |
| screen | A stretch of recording while an agent drives the screen, and the input events. | Fact |
| meter | Disk, memory and processor use of a computer, sampled. | Fact |
| gap | A source lost facts or reports between two moments, with the count when known. | Fact; a count given by the inside is a report |
