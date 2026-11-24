# When a program sends many held requests at once, how many asks do you get

## The choice

An agent's part, 24 November 2026. An agent runs `aws s3 rm s3://backups --recursive` on a bucket
of 500 objects. The AWS CLI sends one `DELETE` per object, ten at a time. Each one is held.

The options:

- One ask for the burst (recommended). The requests of one computer to one host that are held
  while an ask about that host is open join it. The ask lists all of them, and your yes lets pass
  exactly those. A request a program sends again after its own timeout joins the ask it repeats.
  It costs a list that can be long, and a rule for when two requests are the same one.
- One ask per request. It costs 500 asks for one command. People accept nearly every prompt when
  they come one after another: in Anthropic's figures, 97% of prompts were accepted and the catch
  rate on dangerous commands fell from about 17% to about 5% after 50 prompts.

Recommended: one ask for the burst. It is the shape that works for Terraform, whose plan lists
everything before you say yes.

## An agent's notes

- `what-an-ask-holds` (answered) says a pending ask holds only the one request it is about. This
  entry keeps its point, that the computer keeps running, and lets one ask list several requests.
  If both are taken, the property `ask-holds-only-its-request` would read "a pending ask holds only
  the requests it lists".
- Measured on 24 November 2026: `aws` and `supabase` send a held delete again after their own 60 s
  timeout, 3 and 6 times. `aws` keeps the same `amz-sdk-invocation-id` header; `supabase` sends
  nothing that ties the copies together. `research/2026-11-24-destructive-calls/details/cli-capture.md`.
- Traces of `terraform destroy` and the grouping policies simulated on them:
  `research/2026-11-24-destructive-calls/details/bursts.md`.
