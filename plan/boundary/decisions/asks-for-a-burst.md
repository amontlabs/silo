# When a program sends many held requests at once, how many asks do you get

## The choice

An agent's part, 24 November 2026. An agent runs `terraform destroy` on a stack of 23 resources:
a network, an RDS database, an S3 bucket of backups, a DynamoDB table, a Lambda function and its
role. Terraform sends at most 10 requests at a time, and a delete that another one depends on
waits for it. Every delete is held.

The options, measured on that stack against a fake AWS:

- One open ask per computer (recommended). Held requests join the computer's open ask until you
  answer. The destroy makes 7 asks, one per wave of deletes that do not wait on each other. It
  costs asks that grow with the program's waves: `aws s3 rm --recursive` on 500 objects makes 50
  asks of 10, and a script that deletes one item at a time makes one ask per item.
- One open ask per host. It makes 17 asks for the same destroy, because one wave of Terraform
  reaches 7 AWS hosts at once.
- One ask per request. It makes 23 asks, and 500 for the bucket. People accept nearly every
  prompt that comes one after another: in Anthropic's figures, 97% of prompts were accepted, and
  the catch rate on dangerous commands fell from about 17% to about 5% after 50 prompts.

Recommended: one open ask per computer. Grouping never grants anything, so the simplest key that
gathers a burst is the best one, and a yes still covers only the requests you saw listed.

## A second question in it

May a yes cover requests not sent yet, when the ask names each one? Before `aws s3 rm` deletes, it
lists the bucket, and Silo reads the provider's answer. An ask could say "these 500 objects" and
let pass a delete of each one named, and nothing else. It takes the bucket from 50 asks to 1. It
costs a yes for requests that have not happened, which `what-an-ask-shows` does not allow today: an
ask shows a request that waits. Not recommended yet; it is written here so it can be decided.

## An agent's notes

- `what-an-ask-holds` (answered) says a pending ask holds only the one request it is about. This
  entry keeps its point, that the computer keeps running, and lets one ask list several requests.
  If both are taken, the property `ask-holds-only-its-request` would read "a pending ask holds only
  the requests it lists".
- Joining retries is needed for a yes to be right: without it, the AWS CLI's copy sent after its
  own 60 s timeout was listed twice, and one yes sent each of 30 deletes to AWS twice.
- Terraform has no timeout of its own on a held request: it waited 400 s and never retried a 403.
  On a refusal it finished every delete that did not depend on the refused one, printed Silo's
  sentence and exited with 17 of 23 resources gone.
- The order of the asks can hide what matters. `terraform destroy` turned S3 versioning off and
  deleted files in the first ask; the database, with `DeleteAutomatedBackups=true`, came in the
  second. The first ask can show what is likely to follow, marked as the agent's claim: the
  agent's tool call `terraform destroy` and the plan Terraform printed ("23 to destroy") are in the
  model traffic Silo reads (`opinion-on-an-ask`).
- Measured on 24 November 2026: `aws` and `supabase` send a held delete again after their own 60 s
  timeout, 3 and 6 times. `aws` keeps the same `amz-sdk-invocation-id` header; `supabase` sends
  nothing that ties the copies together. `research/2026-11-24-destructive-calls/details/cli-capture.md`.
- Traces, the policies simulated and the text of the first two asks of the destroy:
  `research/2026-11-24-destructive-calls/details/bursts.md`.

## A burst seen on the phone (25 November 2026)

An agent's part, 25 November 2026, from `research/2026-11-25-ask-reaches-a-phone/`. Six deletes sent within 11 seconds made one ask in
six versions. Waiting 2 seconds for quiet, the phone was sent three of them, and the yes covered
exactly the version it showed; a line held after that version went to the next ask, as this entry
says. `pip download` put its own version check (`/simple/pip/`) in the same ask as the package it
was asked for: an ask can list requests nobody thinks of, so the line should say which program
sent it.
