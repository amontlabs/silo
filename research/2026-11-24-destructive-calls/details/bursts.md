# Bursts: how many asks a program's many deletes should make

Experiment run on 24 November 2026 in a Linux container. It answers the open decision
`asks-for-a-burst` (`plan/boundary/decisions/asks-for-a-burst.md`) with measurements. Every number
below comes from a trace in `proto/burst/traces/` or an output in `proto/burst/outputs/`, unless it
carries a URL.

## In short

- **Holding a request changes the burst.** A program that waits for its deletes cannot send the
  ones that depend on them, and it cannot send more than its own parallel limit. So one ask can
  never gather a whole `terraform destroy` or a whole `aws s3 rm`, whatever the grouping rule, as
  long as a yes covers only requests already sent.
- With the best grouping we measured (one open ask per computer), a `terraform destroy` of 23
  resources made **7 asks**, and `aws s3 rm --recursive` of 500 objects made **50 asks of 10**.
  Grouping per host, as the open decision recommends, made **17 asks** for the same destroy.
- Terraform waits on a held request without a timeout (400 s measured), and never retries a 403. On
  a refusal it goes on with every delete that does not depend on the refused one, then exits 1 with
  the stack half destroyed (17 of 23 gone).
- Joining a client's retry to the line it repeats is needed for correctness, not just comfort:
  without it, the `aws` CLI's retry after its 60 s timeout was listed twice and **sent twice** on
  one yes.
- Recommended: one ask per computer, with the rule in section 5. And a challenge: let a yes cover
  requests not sent yet when the ask names each one from a list the provider returned (section 6).
  That takes `aws s3 rm` of 500 objects from 50 asks to 1.

## 1. How the bursts were made

- `proto/burst/stack/main.tf`: a VPC, two subnets, an internet gateway, a route table, a security
  group with two rules, an RDS subnet group and Postgres instance, an S3 bucket with versioning and
  five objects, a DynamoDB table, an IAM role, policy and attachment, a log group and a Lambda
  function. 23 resources. Terraform 1.17.2, provider `hashicorp/aws` 6.74.0, default
  `-parallelism=10`.
- The provider's `endpoints` point at `proto/burst/proxy.py` on `127.0.0.1:4566`, which logs every
  request and forwards it to moto 5.2.6 (`moto_server`). No real AWS, no `/etc/hosts` change, fake
  keys (`test`). The proxy reads the service from the SigV4 credential scope and the operation from
  `X-Amz-Target`, the form field `Action`, or the S3 method and path. It logs the SDK's
  `amz-sdk-invocation-id` and `amz-sdk-request`, the connection, and a hash of
  method+path+body. It never logs the `Authorization` header.
- `aws s3 rm --recursive` ran with aws-cli 1.46.3 (pip; retry mode "legacy", 5 attempts, as its
  User-Agent and headers say). `details/cli-capture.md` used aws-cli 2.42.2, which makes 3
  attempts.
- moto 5.2.6 crashed on deleting an S3 delete marker (`'FakeDeleteMarker' object has no attribute
  'dispose'`), which made Terraform's `force_destroy` retry 10 times on a 500. We patched moto in the
  test venv with an empty `dispose`; the first destroy trace with the crash was discarded.
- `run.sh`, `scenarios.sh`, `s3.sh` and `policies.sh` are the runs, in order. They expect
  Terraform and a venv with moto and awscli under `/opt/burst`, which was removed afterwards.
- `asks.py` runs a grouping policy live inside the proxy, with a simulated person who answers one
  ask at a time, 8 s after looking at it. `test_asks.py` checks its rules (`python3 -m pytest -q
  test_asks.py`). `analyze.py` summarises traces (`analyze.py traces/x.jsonl`), replays policies
  offline (`--simulate`) and checks every live run (`--check`).

## 2. What the bursts look like with nothing held

### `terraform destroy`, 23 resources (`traces/destroy.jsonl`)

24 destructive requests (one per resource, plus `DetachInternetGateway`) to 7 hosts, in three
waves. 93 s in all.

| From the first delete | Requests |
|---|---|
| 0.00 to 0.33 s (14) | `PutBucketVersioning` with `<Status>Suspended</Status>`, `DeleteDBInstance prod-db` (body `DeleteAutomatedBackups=true&SkipFinalSnapshot=true`), `DeleteRouteTable`, `RevokeSecurityGroupIngress`, `RevokeSecurityGroupEgress`, 5 × S3 `DELETE /prod-backups/db/dump-N.sql`, Lambda `DELETE /2015-03-31/functions/orders`, S3 `DELETE /prod-backups` (0.11 s), `DetachInternetGateway` (0.23 s), `DeleteInternetGateway` (0.33 s) |
| 5.06 to 5.18 s (5) | `DeleteLogGroup`, `DetachRolePolicy`, `DeletePolicy`, `DeleteRole`, `DeleteTable` |
| 80.19 to 80.40 s (5) | `DeleteDBSubnetGroup`, `DeleteSecurityGroup`, 2 × `DeleteSubnet`, `DeleteVpc` |

- At most 8 deletes in flight, on 10 connections. Each request had its own invocation id.
- The 5 s gap follows the Lambda delete (Terraform waited 5 s after it in every run; why is
  unverified). The 75 s gap is Terraform polling `DescribeDBInstances` until the database is gone.
  On real AWS an RDS delete takes minutes, so the third wave comes minutes later (unverified here).
- **Terraform switches a protection off first**: destroying `aws_s3_bucket_versioning` sends
  `PutBucketVersioning ... Suspended`. A rule set that holds "switching a protection off" holds it.
- **Terraform's default for an RDS instance deletes its automated backups**
  (`DeleteAutomatedBackups=true` in the body, `delete_automated_backups = true` in the plan). That
  is the DataTalks.Club case.
- With `-parallelism=1` (`traces/destroy-p1.jsonl`): the same 24 requests, one at a time, 101 s.

### Forced replacement (`traces/replace.jsonl`)

Renaming the DynamoDB table forces a replacement: `DeleteTable orders` at 10.34 s, then
`CreateTable orders-v2` at 10.41 s. Delete first, then create, as Terraform does by default. The
RDS rename did not force one: the provider renames in place with `ModifyDBInstance`. If the
`DeleteTable` is held and refused, the create is not sent and the old table stays (inferred from the
dependency order, not run).

### `aws s3 rm --recursive`, 500 objects (`traces/s3rm-500.jsonl`)

One `ListObjectsV2`, then 500 `DELETE /prod-backups/db/part-NNN.dump`, all within 1.34 s, exactly 10
in flight on 10 connections. 1.9 s in all. It never used the batch `POST /?delete`.

## 3. What a held request does to each client

| Run | What happened |
|---|---|
| Terraform, `DeleteDBInstance` held 60 s then 403 (`destroy-hold60`) | One attempt, no retry. While it waited, Terraform printed `aws_db_instance.main: Still destroying... [id=..., 00m50s elapsed]` every 10 s and finished every delete that does not depend on the database: 17 of 23. Then `Error: deleting RDS DB Instance (prod-db): operation error RDS: DeleteDBInstance, https response error StatusCode: 403, RequestID: silo, api error AccessDenied: Held by Silo and not answered: this request needs your yes.` Exit 1 after 72 s. State kept 6: the instance, subnet group, security group, two subnets, VPC. A second `terraform destroy` removed those 6. |
| Same, held 90 s (`destroy-hold90`) | The same, exit 1 after 102 s, the same 6 left. |
| Same, held 400 s (`destroy-hold400`) | The same. Terraform waited the full 400 s on one attempt: it has no read timeout of its own on this call, so for Terraform the hold length is Silo's to choose. |
| Terraform, every delete held 30 s then let pass (`destroy-holdall30-pass`) | Never more than 10 in flight: with 10 held, Terraform sent nothing else. `DeleteLogGroup`, ready at 5 s, was sent at 30 s when a slot freed. The 24 deletes came in 9 rounds; 214 s instead of 93 s. |
| `aws s3 rm`, one `DELETE` held 30 s then 403 (`s3rm-hold30-403`) | The other 49 deletes finished at once. The CLI printed `delete failed: s3://prod-backups/db/part-01.dump An error occurred (AccessDenied) when calling the DeleteObject operation: Held by Silo ...` and exited 1 after 30.4 s. No retry of the 403. |
| `aws s3 rm`, one `DELETE` held 70 s then let pass (`s3rm-hold70-pass`) | At 60 s the CLI gave up and sent the same delete again, same invocation id, `attempt=2; max=5`, on a new connection. The copy passed and the CLI exited 0 after 60.8 s while the first copy was still held. Released at 70 s, the first copy would have reached the provider too. |

So a "no" on one line of a Terraform run never stops the run: it leaves a half-destroyed stack, and
what is gone by then is whatever the person let pass in earlier asks.

## 4. The grouping policies, run live

Same simulated person for every policy: answers yes 8 s after starting on an ask, one ask at a
time; an ask is shown 3 s after its first request (0 s for one per request); hold limit 90 s; retries
joined. Destroy times are with nothing held: 93 s.

| Policy | `terraform destroy` (24 deletes) | `aws s3 rm` (500) | Script deleting one at a time |
|---|---|---|---|
| One per request | 24 asks, all of 1; done at 206 s | 500 asks (follows: one line each) | 1 per delete |
| Per connection | 26 asks, all of 1 (Go's HTTP client sends one request at a time on a connection); 226 s. 26 lines, not 24: see the note below the table | 1 line per ask (15 measured, then stopped) | 1 per delete |
| Per (computer, host): the open decision's recommendation | 17 asks: `[5, 1, 3, 1, 1, 1, 1, 1, 1, 2, 1, 1, 1, 1, 1, 3, 1]`; 158 s | 50 (one host, so as per computer) | 1 per delete |
| Per program (User-Agent's product token) | 7 asks: `[10, 4, 4, 1, 2, 2, 1]`; 137 s | as per computer | 1 per delete |
| **Per computer** | **7 asks: `[10, 5, 3, 1, 2, 2, 1]`; 149 s** | **50 asks of exactly 10; 553 s** | **20 asks for 20 deletes; 230 s** |

The first request waits window + answer (11 s) in every grouped policy, and 8 s with one per request.

In the per-host and per-connection runs, Terraform's first `DeleteBucket` got `409` (bucket not
empty: the versioned bucket still held old versions), so `force_destroy` sent `POST
/prod-backups?delete` (`DeleteObjects`, every version) and then `DeleteBucket` again: 26 lines. In
the per-computer and per-program runs the first `DeleteBucket` passed, which we take to be moto
answering before its versions were counted; on real S3 a versioned bucket is never empty after
plain object deletes, so expect the 409 path every time (unverified). That `DeleteObjects` of every
version is the step that makes the files unrecoverable, and it comes in a later ask than the file
deletes.

The offline replay (`analyze.py --simulate`), which keeps the arrival times of the run without holds,
predicted 3 asks per computer. The live run made 7. The 4 extra are the dependency chain: a
`DeleteBucket` waits for its objects, `DeleteInternetGateway` for its detach, `DeleteRole` for its
policy detach, `DeleteTable` for the IAM policy that names it, subnets for the security group, the
VPC for the subnets. A policy cannot gather what the program has not sent, and the program does not
send it until the yes. Replays on a trace without holds overstate grouping.

`analyze.py --check` over all ten live runs: every request let pass on a yes was a line of exactly
one answered ask, and had been on screen at least 1 s before the tap. In no run did a request arrive
while an ask was shown: with its parallel slots full of held requests, a program sends nothing more.
The list grows only while the ask gathers, or when another program of the same computer sends too.

### Retries (`pol-slow-s3rm30`, `pol-slow-noretry-s3rm30`)

`aws s3 rm` of 30 objects, per computer, the person answering after 70 s (past aws's 60 s timeout).

- **Retries joined by invocation id:** 3 asks of 10 lines, each line showing 2 tries. The yes
  released only the newest copy; the older copy's client had already closed its connection. Each
  object was deleted once.
- **Retries not joined:** 3 asks of 20 lines, each delete listed twice. The yes released both
  copies: **each of the 30 deletes reached the provider twice.** For a delete the second one is a
  no-op; for a refund it is a second refund.
- Terraform's provider retries up to 25 times by default, but only "when AWS throttles requests or
  you experience transient failures" ([provider docs, v6.74.0](https://raw.githubusercontent.com/hashicorp/terraform-provider-aws/v6.74.0/website/docs/index.html.markdown),
  read 24 November 2026). It never retried a held request or a 403 here. The only identical
  destructive requests in its traces were the two `DeleteBucket` of the note above, 30 to 80 s
  apart, the first refused as not empty: two requests, rightly two lines.

## 5. Recommended policy

**One ask per computer.** The exact rule:

1. A held request joins the computer's open ask, if one is open and not yet answered. Otherwise it
   opens a new ask.
2. A new ask gathers for 2 s, then Silo notifies you. The deletes of one Terraform wave arrived
   within 0.33 s and the 10 deletes of an `aws s3 rm` round within 0.1 s; 2 s catches them and
   adds 2 s to the first wait.
3. A request held while the ask is shown is added to it, marked as new.
4. Each version of the list has a number. Your answer names the version you saw. It covers exactly
   the lines of that version, each once. Lines added after it, and requests held after your answer,
   make the next ask, shown at once.
5. A held request is a copy of a line still waiting when it has the same `amz-sdk-invocation-id`,
   or the same method, host, path and body within 120 s. A copy joins its line, which shows the
   number of tries. A yes releases only the newest waiting copy; older copies get the refusal
   "replaced by a newer try". A no refuses them all.
6. You can answer all lines at once or each line on its own.
7. Each request keeps its own hold limit (`hold-limit-length`). At the limit it is refused, and a yes
   that comes later is the gaps thread's late answer: a grant for that exact request, used once.

Why per computer, not per host: one Terraform run reaches 7 hosts in its first wave, so per host
splits one moment of the run into 4 asks at once and made 17 in all. Per program gave the same
count as per computer, but it splits on the client's own claim (User-Agent) and buys nothing a
hostile agent could not also fake; grouping never grants, so the simpler key wins. The ask shows its
lines grouped by host and by the program's claimed name, "gone" first.

What it cannot fix: a Terraform destroy is as many asks as its dependency depth (7 here), and a
pipelined client is as many asks as its count divided by its parallel limit (500 / 10 = 50). A
script that deletes one item at a time is one ask per item, whatever the policy.

### What the asks show for this `terraform destroy`

The person sees 7 asks. The first two, in Silo's words. The names come from the request bodies and
paths; "can be brought back" lines come from the rule set's data; the program's name is its own
claim.

> **main wants to change 10 things in AWS (account 123456789012, us-east-1)**
> The program says it is Terraform 1.17.2. Nothing below has happened yet.
>
> Switches a protection off
> - Versioning on S3 bucket `prod-backups`: suspended.
>
> Deletes
> - 5 files in S3 bucket `prod-backups`: `db/dump-0.sql` to `db/dump-4.sql`.
> - Lambda function `orders`.
>
> Stops something working
> - Security group `sg-fc20…b5fd8`: its inbound rule and its outbound rule.
> - Route table `rtb-bdd1…a138`.
>
> Yes lets these 10 requests through, once each. Anything main sends after your answer comes as a
> new question. [Yes to all 10] [No to all] [Choose]

> **main wants to change 5 more things in AWS (us-east-1)**
> The same program, 0.1 s after you let the last 10 through.
>
> Cannot be brought back
> - RDS database `prod-db`, with no final snapshot, and its automated backups deleted with it.
> - S3 bucket `prod-backups`.
> - Log group `/aws/lambda/orders`.
>
> Stops something working
> - Internet gateway detached from VPC `vpc-9ebd…`.
> - Policy `orders-fn-dynamo` detached from IAM role `orders-fn`.
>
> If you say no, Terraform stops before the network and leaves the stack half destroyed: what you
> let through before stays gone.
> Yes lets these 5 requests through, once each. [Yes to all 5] [No to all] [Choose]

Asks 3 to 7: the internet gateway, the IAM policy and role (3); the DynamoDB table `orders` (1); the
DB subnet group and security group (2), after the database is gone; two subnets (2); the VPC (1).

The database came in the second ask only because Terraform's 10 parallel slots were full of the
first ten. So the person may say yes to the files before seeing that the database comes next. The
first ask should say what is likely to follow when Silo has a source for it, marked as the agent's
claim: the agent's tool call `terraform destroy -auto-approve` and the plan it printed
("Plan: 0 to add, 0 to change, 23 to destroy") are in the model traffic Silo already reads
(`opinion-on-an-ask`).

## 6. Two things to put to polarzero

- **Challenge to `asks-for-a-burst` as recommended.** It says "one ask for the burst", grouped per
  computer and host, and calls it "the shape that works for Terraform, whose plan lists everything
  before you say yes". Measured, that rule makes 17 asks for a 23-resource destroy and 50 for a
  500-object `aws s3 rm`. Proposed: group per computer (7 and 50), take the rule of section 5, and
  say in the answer that a burst makes as many asks as the program's dependency depth or its count
  over its parallel limit.

- **May a yes cover requests not sent yet, when the ask names each one?** The open decision says
  "a request that arrives after you answered starts a new ask". The case: `aws s3 rm
  s3://prod-backups/db/ --recursive` on 500 objects. Before the first delete, the CLI listed the
  bucket, and Silo, which reads that host's traffic, saw the 500 keys in S3's answer. With the
  current rule that is 50 asks of 10, over 9 minutes of tapping; with 10 000 objects, 1 000 asks,
  and people stop reading long before. Proposed in its place: an ask may list requests not sent yet
  when each is named exactly from a list the provider returned and Silo read (S3 `ListObjectsV2`,
  Gmail `users.messages.list`): "delete these 500 objects under `s3://prod-backups/db/`". A yes then
  lets each listed request pass once, until a deadline; anything not on the list still makes its own
  ask. It lets nothing pass that the ask does not name. Its bad path: a list of 500 is not read
  line by line, and a slow agent may still be deleting after the person has moved on, so the
  deadline should be short and the count shown. Terraform's future deletes have no such fact, so
  Terraform stays at one ask per wave. (Gmail itself offers `POST
  .../users/{userId}/messages/batchDelete` with `ids[]`, one request for many messages:
  [Google docs](https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.messages/batchDelete),
  read 24 November 2026. The maximum number of ids is not stated there; unverified.)

## Not verified

- Real AWS. moto answers at once, so real RDS, Lambda and VPC deletes would spread the waves over
  minutes, not seconds; the number of asks would not change, the waits between them would.
- Why Terraform waits 5 s after deleting the Lambda function.
- `supabase`, which sends copies with nothing to tie them but method, path and body, was not run
  here; the method+path+body join was tested only in `test_asks.py`.
- The simulated person answers in a fixed 8 s; real answer times, and how many lines a person reads,
  were not measured.
- OpenTofu was not run; it shares Terraform's graph walk and `-parallelism` default
  ([Terraform docs](https://developer.hashicorp.com/terraform/cli/commands/apply), read 24 November
  2026: "Defaults to 10"), but its behaviour on a held request is unverified.
