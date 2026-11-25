# How a hostile agent gets past Silo's precautions

Research of 24 November 2026, by agents, for polarzero. It takes the side of the attacker: for each
precaution Silo puts outside a computer, it lists the concrete ways a hostile agent defeats it,
ranks them, tests what can be tested on Linux, and proposes a fix for each, named by plan entry.
These are recommendations only. Nothing under `plan/` is changed. `plan show <id>` prints an entry.

Three other drafts already cover parts of this ground, and this page does not repeat them:

- PR #351, `record-and-spend/details/dns-and-provider-tools.md`: name lookups, domain fronting,
  QUIC, tools the provider runs (web fetch, MCP connector), hosts that store what they are sent,
  and the overshoot of a budget by calls running at once.
- PR #352, `first-build-decisions/04` and `06`: gvproxy sends `192.168.127.254` to the Mac's
  `127.0.0.1`, and Ghostty writes the Mac's clipboard on OSC 52.
- PR #355, `gap-fixes/`: where a placeholder is swapped, a worm's own token, files that run on
  the Mac, secrets Silo never held, and how a hold matches a retry.

They appear in the ranked list below with a pointer, so that the list is whole.

## Who attacks

The plan assumes the agent is hostile (`security-framing`). In practice that means two attackers,
and a precaution can stop one and not the other.

- **Code with `sudo` in the computer.** A malicious dependency, or an agent whose model was steered
  by a web page and already ran one command. It can do anything inside: send raw bytes, read every
  file, stop the probe, draw on the screen. Only what runs outside the computer stops it.
- **A steered model behind an honest agent program.** It can only ask for tool calls. A precaution
  on tool calls (`denied-tool-call`) stops this one and never the first, which runs its commands
  without asking the model.

Everything ranked below assumes the first attacker unless it says otherwise.

## What was tested here

Four tests, each with its files and output in this folder. All ran on Linux in this container.

| Test | Result | Folder |
|---|---|---|
| Malformed requests sent through the boundary spike of PR #347 to a fake host whose parser is lenient | 3 of 7 cases smuggle a whole second request (with a `web_fetch` tool) past the spike's record, and the program reads its answer. 3 more pass a body the spike cannot read. A 100-line patch refuses all 7, and Claude Code 2.1.331 still runs through it with nothing refused | [`boundary-tests/`](boundary-tests/) |
| A page served from a computer, opened through a forwarded port in Chromium | The page sends the computer's secret to the internet 9 ways, and reads a service on the Mac and sends that too. A strict Content-Security-Policy added by the forwarder leaves 1 way: navigating the page itself | [`ports/`](ports/) |
| gvproxy at the commit #352 tested, with a guest in a network namespace | From inside, with no token, one `curl` to `192.168.127.1` makes gvproxy listen on `0.0.0.0:2299` of the host and forward it into the guest. The host's network address then serves the guest's page | [`gvproxy/`](gvproxy/) |
| Duplicate keys in a JSON body | Zig's `std.json` refuses the body; Python and Node take the last key. The spike then forwards a body it never read | [`boundary-tests/`](boundary-tests/) case c |

## The ranked list

Feasible: **high** works today with ordinary tools; **medium** needs a condition the user or Silo's
design may create; **low** needs a new bug. Impact: **critical** is code running on the Mac;
**high** is data or a credential leaving, or the Mac reached; **medium** is the record or a budget
made wrong; **low** is noise.

| # | Attack | Precaution it defeats | Feasible | Impact | Where |
|---|---|---|---|---|---|
| 1 | Open a port on the Mac's every interface through gvproxy's API, from inside | `device-out-of-reach`, `reach-of-a-computer` | high, if gvproxy is used with its defaults | high | tested here, A1 |
| 2 | Serve a page on a forwarded port; the user's browser on the Mac carries data out and reaches Mac services | the network rules, `paths-out-are-shown`, `device-out-of-reach` | high | high | tested here, A2 |
| 3 | Reach the Mac's `127.0.0.1` through `192.168.127.254` | `device-out-of-reach` | high, with gvproxy's defaults | high | #352 |
| 4 | Have the provider fetch a URL or call an MCP server with the computer's own key | the network rules | high | high | #351 |
| 5 | Send a foreign credential to an allowed host (an attacker's GitHub token, Anthropic key) | the network rules | high | high | #351, #355 |
| 6 | Smuggle a request past the boundary, or send a body it cannot read | the record, every rule that reads a request | medium (smuggling, depends on the host's parser); high (unreadable body) | high | tested here, A3 |
| 7 | Steal the subscription login that sits in the computer, and use it elsewhere | budgets, `logins` | high | high | A4 |
| 8 | Change a file the Mac runs by itself (a Git hook, a VS Code task) and wait for the user | the boundary as a whole | high | critical | #355 G3 |
| 9 | Get a secret swapped toward an attacker's site on the same shared suffix (`*.vercel.app`) | `secret-placeholder` | medium (the user allowed a pattern) | high | A5 |
| 10 | Draw a fake Silo secret prompt on the computer's screen | `secret-entry` | medium | high | A6 |
| 11 | Write escape sequences that the Mac's terminal or Silo's own command line obeys | `channel-list` | medium | high | #352 (Ghostty); A7 for Silo's own output |
| 12 | Send one batch request that runs thousands of model calls later | budgets | high | medium | A8 |
| 13 | Start many calls at once just under the budget | budgets | high | medium | #351 |
| 14 | Wear the user down with asks, or make an ask look like something else | holds | medium | high | A9 |
| 15 | Craft a disk that crashes or takes over the host code that reads it | `file-changes-from-outside`, checkpoints | low to medium | critical | A10 |
| 16 | Use a device of the virtual machine monitor as a way onto the Mac (libkrun's TSI, virtio-fs) | the virtual machine | low, if configured right | critical | A11 |
| 17 | Escape Apple's Virtualization framework | the virtual machine | low; no public case found | critical | A11 |
| 18 | Tunnel data through name lookups | the network rules | high when open; depends when limited | high | #351 |
| 19 | Flood the record so the attack is buried, or the disk fills | the record | medium | medium | A12 |
| 20 | Leak data through reads that a host logs where the attacker can see them | `no-read-freely-preset` | medium | low | A13 |
| 21 | Stop or fool the probe | reports | high | low, since the plan already treats it as a report | `processes-from-inside-linux` |

The pattern behind the top of the list: the network rules are strong, and every high item goes
around them, through a part of Silo that was built for convenience (gvproxy's API, a forwarded
port, the provider's own tools, the login in the computer), or through a parser that disagrees with
another one.

## A1. gvproxy's API opens ports on the Mac from inside

gvproxy serves an HTTP API on the gateway address of the computer's network, `192.168.127.1:80`.
It is started unconditionally (`cmd/gvproxy/main.go`, `vn.Listen("tcp", GatewayIP:80)` then
`GatewayMux()`), and it is protected only if a token is set (`pkg/virtualnetwork/mux.go`: "If a
token is configured, wrap it with authentication middleware"). Its routes include
`/services/forwarder/expose`.

From inside the guest:

```
curl -X POST http://192.168.127.1/services/forwarder/expose \
     -d '{"local":"0.0.0.0:2299","remote":"192.168.127.2:8000"}'
```

answered 200, and `ss` on the host then showed `gvproxy` listening on `0.0.0.0:2299`. A request to
the host's network address on that port got the guest's page. On a Mac in a café, anyone on the
same Wi-Fi can now reach a server the agent runs: a way in that no outbound rule sees. The agent can
also expose port 22 of the computer, or take the address of a port the user wants for something
else.

**Fix.** Silo's own stack (`network-stack`) should have no API reachable from inside a computer.
If gvproxy is used as a base, as #352 measured: no gateway HTTP API at all (it has to be removed in
code, since it has no flag to switch off), `NAT` set empty with host loopback off, and the Mac-side
API only on a Unix socket of Silo's own user. A hostile test should try the gateway address on
every port.

## A2. A forwarded port carries data out through the Mac's browser

`ports-reached-from-your-mac` makes a port of a computer reachable at `localhost` on the Mac. The
user opens it in a browser to look at the app the agent built. The page is the agent's, and it runs
in the user's browser, which is on the Mac and outside every rule of the computer.

The test serves a page with a secret from "the computer" on `127.0.0.1:3000`, and opens it in
Chromium. With no change, the attacker's server got the secret by `fetch`, `sendBeacon`, an image,
a WebSocket, a prefetch link, an iframe and a navigation. The page also read a service on the Mac
that trusts `localhost` origins (a stand-in for a dev server or a local model's API) and sent what it
read. A computer whose network is closed is open the moment its page is opened.

Two more effects, not tested here because they are documented behaviour:

- Cookies ignore the port (RFC 6265). A page on `localhost:3000` reads and sets the cookies of every
  other app the user runs on `localhost`, unless they are `HttpOnly`.
- The page can ask for the camera, the microphone or notifications under `localhost`, which the
  user may have allowed for another app.

With a strict policy added to each reply by the forwarder
(`default-src 'self'; connect-src 'self'; img-src 'self'; frame-src 'none'; form-action 'self'`),
only the navigation of the page itself still left. That one cannot be blocked by a policy.

**Fix, in three parts.**

1. Serve each forwarded port on a name of its own, such as `3000.main.silo.localhost`, not on
   `localhost:3000`. Browsers send every `*.localhost` name to the loopback address, and a name of
   its own keeps the page's cookies, storage and permissions apart from the user's other apps.
2. Add the strict policy to every reply of a forwarded port by default, and let the user turn it
   off for one port. It closes 8 of the 9 ways measured, and the Mac services.
3. Count an opened port as a path out in `paths-out-are-shown`, for a computer whose network is
   limited or closed. Its sentence "a path ends at the first place outside the computer: your Mac"
   is wrong for a browser, which goes on to the internet.

## A3. The boundary and the host read a request differently

`forwarded-byte-for-byte` sends each request as the program wrote it. That is what the boundary
should do with a request it reads exactly as the host does. It goes wrong when the two disagree on
where a request ends, because then the host sees requests the boundary never saw.

The test sends seven requests through the spike of PR #347 to a fake host with a lenient parser.
The host follows RFC 9112 on one point (`Transfer-Encoding` wins over `Content-Length`) and is
lenient on two (the first of two `Content-Length` headers wins; a tab before a header's colon is
ignored).

| Case | The spike's record | What the host did |
|---|---|---|
| a. `Host: attacker.example` under the name `api.anthropic.com` | one request, Host not checked | served `attacker.example` |
| b. a body sent with `Content-Encoding: deflate` | one request; "decoded" bytes are the compressed bytes, unread | got the body; it held a `web_fetch` tool |
| c. a JSON body with `"tools"` twice | one request; Zig refused the JSON, so nothing was read | a host that takes the last key sees `web_fetch` |
| d. two `Content-Length` headers | one request | **two requests**: the second declared `web_fetch` to `attacker.example` |
| e. `Content-Length` and `Transfer-Encoding` together | two requests | two requests, the same ones |
| f. `Transfer-Encoding\t: chunked` | one request | **two requests**, as in d |
| g. d, then an ordinary request | two requests | **three requests**; the program got the smuggled request's answer as the answer to its second |

Cases d, f and g are a two-way channel to the model provider that the record never shows and that
no rule on a request's body can see. #351 proposes a rule that denies a request declaring a
provider's own tools; smuggling and unreadable bodies are how that rule is bypassed. Whether
`api.anthropic.com` or `chatgpt.com` are as lenient as the fake host is not known. It was not
probed, since that means sending malformed requests at someone else's servers. The fix does not
depend on it.

**Fix: read strictly, and refuse what is not read.** [`strict.patch`](boundary-tests/strict.patch)
adds to the spike, on hosts it reads:

- refuse a head with two `Content-Length` headers, both `Content-Length` and `Transfer-Encoding`,
  a `Transfer-Encoding` other than one plain `chunked`, whitespace in a header name, or a line
  without a colon;
- refuse a request whose `Host` is not the name in the handshake;
- refuse a body in an encoding it cannot decode;
- refuse a body that does not parse, duplicate keys included.

All seven cases are refused, the host parses none of them, and an ordinary pair of requests still
passes. Claude Code 2.1.331 ran a conversation with a tool call through the strict spike, and
nothing was refused ([`claude-code-strict.txt`](boundary-tests/claude-code-strict.txt)). A
refusal does not change any byte that is forwarded, so `forwarded-byte-for-byte` keeps its promise
for every request that passes.

Proposed property: **a request to a host the boundary reads leaves only if the boundary read all of
it, the way any conforming host would.** It is checkable by a hostile test with the seven cases
here and the published HTTP desync corpora. The same holds for HTTP/2 when the boundary speaks it.
HTTP/2 framing has no `Content-Length` ambiguity, but a `content-length` header that disagrees with
the frames has to be refused too.

## A4. The subscription login in the computer

`logins` says Silo holds no login of a subscription: the user signs in inside each computer. So
Claude Code's or Codex's refresh token sits on the computer's disk, readable with `sudo`. The agent
copies it out by any path on this list, and the attacker then spends the user's plan from anywhere.
No budget sees that spend, and the plan's limits run out for the user.

The plan keeps this decision for reasons of vendor terms (`anthropic-reads-intermediate`,
`unmodified-agent-programs`), and this page does not reopen them. What can be added without holding
the login:

- **A rule on a fact.** The boundary sees the login's token in the `Authorization` header of every
  request to the provider. It can remember a hash of it for that computer and search every other
  request it reads for that value, as #355 page 5 searches for known credential formats. A token
  that leaves to any other host is a fact, and the request is held.
- **The ask that makes main.** When main's network is open and a subscription login is in it, the
  app says, once, that the login can leave the computer by any host it can reach.

## A5. A secret swapped toward an attacker's site on a shared suffix

A secret is swapped only toward the hosts the user chose (`secret-entry`). If the user chooses a
pattern, such as `*.vercel.app` for their own deployments, `*.supabase.co` for their project or
`*.workers.dev`, the attacker's site on the same suffix gets the real value, because the hosting
service passes headers to its tenant's code. The same holds for a network rule that allows such a
pattern.

The list of such suffixes exists: the "private domains" section of the Public Suffix List. Checked
on 24 November 2026, it holds `workers.dev`, `vercel.app`, `herokuapp.com`, `supabase.co`,
`netlify.app`, `ngrok.io`, `ngrok-free.app`, `github.io`, `pages.dev`, `fly.dev`, `onrender.com`,
`s3.amazonaws.com`, `cloudfunctions.net`, `web.app`, `firebaseapp.com` and `azurewebsites.net`.
`run.app` is not on it.

**Fix.** A secret names exact hosts. A pattern that covers a public suffix, private section
included, is refused when the secret is set up, with the reason. A network rule with such a
pattern is allowed, and the app says that it reaches every tenant of that service.

## A6. A fake Silo prompt on the computer's screen

`secret-entry` puts secrets in "Silo's own prompt, outside any chat". The screen of a computer is
drawn by the computer (`linux-screen`, `macos-screen-from-a-linux-computer`). An agent with a
desktop can draw a window that looks exactly like Silo's prompt, "GITHUB_TOKEN for main", and the
user types the real value into the computer.

**Fix.**

- Silo's prompts and asks are never drawn inside a computer's screen. They are native windows of
  the Mac app, outside the frame that shows the screen.
- The frame always says that what is inside it comes from the computer.
- A secret prompt shows something the computer cannot know, such as an image the user picked when
  Silo was installed. This is the same idea as a bank's personal image at sign-in.

`secret-entry` should say this in its answer, since otherwise its promise rests on the user telling
two identical windows apart.

## A7. Escape sequences in what Silo itself prints

#352 found that Ghostty writes the Mac's clipboard when anything in the computer prints OSC 52.
The same risk sits in Silo's own command line. The record holds text the agent chose: commands,
paths, hostnames in a refusal, the agent's reason on an ask. If `silo` prints a record line as it
is, the agent can set the clipboard, change the terminal's title, write a hyperlink that shows one
address and opens another (OSC 8), or erase the line above.

**Fix.** Every string from a computer that Silo prints or draws goes through one function. It shows
control characters as visible escapes, shows a host with characters from more than one script in
its `xn--` form, and marks bidirectional-text controls. That covers asks too: `аpi.github.com`, with
a Cyrillic `а`, must not read as `api.github.com`.

## A8. One batch request, thousands of calls

A budget counts calls as they happen and lets calls under way finish (`what-a-budget-promises`).
Anthropic's Message Batches and OpenAI's Batch API do not fit that model. One request submits many
calls at half price, and they run later, on the provider's side, within 24 hours. Their results are
read with another request. So one request that a budget allowed can spend many times the budget
after the budget is reached, and no reply of the boundary ever carries the spend.

**Fix.** A rule, since the boundary reads the request: while a budget covers a computer, a request
to a batch endpoint is denied, with the reason. Or, better later, it is priced before it leaves:
the number of calls in its body times the cost of each call's `max_tokens`, reserved against the
budget, as LiteLLM and OpenRouter reserve for one call (#351). OpenAI's batch reads its calls from a
file uploaded first, so its price is known only if the boundary keeps that file's content. The same
holds for tools the provider runs and charges per use, such as web search.

## A9. Asks that wear the user down or mislead

The plan already caps what an ask holds and says what it shows (`what-an-ask-holds`,
`what-an-ask-shows`, `opinion-on-an-ask`). The attacks that remain:

- **Fatigue.** Thirty harmless asks in an hour, then the one that matters, at a time the user
  answers quickly. PR #355 page 8 measured 35 asks in one 20-page reading session.
- **The agent's reason.** "Pushing the fix you asked for" beside a push to another repository.
- **Text that hides its end.** A long URL cut where the attacker's host begins.

**Fix.**

- A computer may have a set number of asks open in an hour. Past it, its new holds are refused
  with the reason, and one alert says so.
- An ask always shows the registrable domain whole, and cuts a long path in the middle, never at
  the end.
- The agent's reason comes after the facts, in another style, and says it is the agent's words.

## A10. Host code that reads a disk the agent wrote

Silo reads computers' disks from outside: it compares checkpoints to list the files that changed
(`file-changes-from-outside`), and `11-reading-apfs-from-outside.md` in #352 reads APFS with a
patched `libfsapfs`. The agent controls every byte of that disk. A file system crafted to crash or
take over its reader is a known way out of a virtual machine, and here the reader is a host process
of Silo. Mounting a computer's image with the Mac's own kernel would put the macOS kernel's APFS
driver in the same position.

**Fix.**

- Read a computer's disk only in a helper process with no network and no file access beyond that
  disk and its output, under a macOS sandbox profile, or in a small helper virtual machine.
- Fuzz the reader with images made by an attacker.
- Never mount a computer's disk on the Mac.

This belongs in the notes of `file-changes-from-outside` and `checkpoint-per-system`.

## A11. The virtual machine monitor

libkrun's own security model says it plainly: "the VMM acts as a proxy for the guest within the
host. Host resources that are accessible to the VMM can potentially be accessed by the guest through
it." Two of its devices are named as the risk (`docs/security-model.md`):

- **TSI.** The guest's sockets are opened by the monitor on the host, in the host's network context,
  past any stack of Silo's. In today's API it is switched on explicitly, with `HIJACK_INET` and
  `HIJACK_UNIX` on a vsock device. Silo must never set those flags. A hostile test should check
  that a guest socket with no network device reaches nothing.
- **virtio-fs.** "Do not treat the virtio-fs export path as a sandbox for the VMM process." A
  symlink bug of this kind in another vendor's virtio-fs server on macOS is listed with a CVSS of
  9.4 (CVE-2026-77179, Docker Sandboxes, as vulmon lists it; not checked against Docker's own
  advisory). Silo should share no host folder with a computer by
  virtio-fs, and carry files through the files channel instead.

The libkrun page's own answer is to confine the monitor's process. On a Mac, the monitor's process
should run under its own sandbox profile: no network except the socket to Silo's stack, and files
limited to the computer's disk image. Then a bug in a device gives the attacker that process and
nothing more. The same applies to Apple's Virtualization framework, where Silo's own helper process
is the host side. A search on 24 November 2026 found no published guest-to-host escape in Apple's
framework. It found escapes in VirtualBox on Apple silicon (CVE-2025-61760 with CVE-2025-62592) and
in QEMU and UTM, which shows the class is real on Macs.

## A12. Flooding the record

An agent can make millions of tiny events: name lookups, connections refused, files written. That
buries the one event that matters, and it fills the disk where the record lives
(`record-disk-size`).

**Fix.** A limit on the record's growth per computer. Past it, the computer's network pauses, which
is the same failure as the stack stopping (`network-stack`), and an alert says why. Repeated events
of one kind are folded into a count, as `routine-stretch-collapses` does for the view, but in the
record itself.

## A13. A read is also a write

`no-read-freely-preset` already refuses a preset that lets every read pass. One more concrete case
for its notes: a repository on GitHub shows its owner the 10 most visited paths and the number of
clones (its Traffic page). An agent that only reads `github.com/attacker/repo/blob/main/<data>`
sends `<data>` to the attacker, a few bytes per path, with no write at all.

## Changes to decisions this page recommends

Each stays in force until polarzero changes it.

| Entry | Why it should change | Section |
|---|---|---|
| `forwarded-byte-for-byte` | Byte for byte is safe only for a request the boundary read exactly as the host does. Add: a request on a read host that is not strictly well-formed, or whose body cannot be read whole, is refused | A3 |
| `ending-the-encrypted-connection` | Add: the `Host` of each request must be the name in the handshake | A3 |
| `ports-reached-from-your-mac` | "at localhost" shares cookies and permissions with every other local app. Each port gets a name of its own under `.localhost`, and a strict content policy by default | A2 |
| `paths-out-are-shown` | "A path ends at the first place outside the computer: your Mac" is wrong when the place is a browser. An opened port is a path to the internet | A2 |
| `network-stack` | Say that no API of the stack is reachable from inside a computer. With gvproxy as a base, the gateway API has to be removed in code | A1 |
| `secret-entry` | Hosts are exact; a pattern over a public suffix is refused. Prompts are never drawn in a computer's screen, and show something the computer cannot know | A5, A6 |
| `what-a-budget-promises` | Batch endpoints spend after the budget is reached. Deny them under a budget, or price them before they leave | A8 |
| `logins` (notes only) | Name the cost: the login can leave the computer. Add the rule on the token's value | A4 |
| `what-an-ask-shows` | Domain always whole, the agent's words last and marked, one rendering function for every string from a computer | A7, A9 |
| `file-changes-from-outside` (notes) | The reader of a computer's disk is attack surface: a confined helper, fuzzed | A10 |
| `linux-vm-engine` (notes) | No TSI, no virtio-fs shares, the monitor's process confined | A11 |

New entries proposed, by kind:

- property `boundary-reads-whole`: a request to a host the boundary reads leaves only if the boundary
  read all of it as a conforming host would. Checked by a hostile test with the cases of A3.
- property `nothing-inside-reaches-the-stack`: no address inside a computer reaches an API or a
  service of Silo's network stack, except name lookups. Checked by a hostile test on the gateway
  address's every port.
- decision `ask-rate-per-computer`: how many asks a computer may have open in an hour, and what
  happens past it (A9).
- decision `record-growth-limit`: the most the record of one computer may grow in an hour, and the
  network pausing past it (A12).

## Hostile tests to add

For `hostile-tests-on-the-real-thing`, each run from inside a real computer, each expected to fail:

1. The seven cases of A3, against the real boundary, with a fake host that logs what it parsed.
2. `curl` to every port of the gateway address and to `192.168.127.254`.
3. A page on a forwarded port that tries the ten ways of A2. Expected: only navigation leaves.
4. A guest socket opened with no network device (TSI).
5. A request to `api.anthropic.com` that declares `web_fetch` or `mcp_servers`, sent whole, with
   duplicate keys, and deflated (#351, A3).
6. A batch request under a budget (A8).
7. A request on a read host that carries the computer's own login token in its body (A4).
8. Text with OSC 52, OSC 8 and bidirectional controls in a command, then `silo` showing the record
   line (A7).
9. A crafted disk image given to the checkpoint reader (A10), from a fuzzer's corpus.

## How to run the tests

`boundary-tests/`: build the spike of PR #347 (kept outside this repository, in the project's
shared folder under `research/2026-11-24-boundary-spike/src`) with Zig 0.15.2:
`zig build-exe src/main.zig -lc -lssl -lcrypto -lz -lzstd -O ReleaseSafe`. Apply
`strict.patch` for the strict build. Then `BOUNDARY=<binary> ./run.sh`, and
`STRICT=1 BOUNDARY=<strict binary> ./run.sh`. The script makes its own certificates.

`ports/`: `CSP=0 python3 servers.py hits.jsonl &` then `node browse.js` with Playwright installed;
the hits are in `hits.jsonl`. Run again with `CSP=1`.

`gvproxy/`: build `cmd/gvproxy` and `cmd/vm` of containers/gvisor-tap-vsock, start
`gvproxy -listen unix:///tmp/.../net.sock`, then as root `unshare -n -m bash guest.sh`. The script
bind-mounts its own `resolv.conf` so that the host's is not touched. It needs `ip` and `udhcpc`.

## What went wrong while testing

- To run Claude Code through the strict spike, `/etc/hosts` of this container was changed so that
  `api.anthropic.com` pointed at `127.0.0.1`. It was restored from a copy right after the run.
- No credential was used or found. The only requests to real providers were four unauthenticated
  `POST`s with made-up keys (three to Anthropic, one to OpenAI), to see whether the host reads the
  body before the key. It does not: all four answered 401, so the hosts' JSON parsing could not be
  compared this way.

## Sources

- libkrun, [`docs/security-model.md`](https://github.com/containers/libkrun/blob/main/docs/security-model.md)
  and [`docs/networking.md`](https://github.com/containers/libkrun/blob/main/docs/networking.md), read 24 November 2026.
- gvisor-tap-vsock at 197eea0: [`cmd/gvproxy/main.go`](https://github.com/containers/gvisor-tap-vsock/blob/main/cmd/gvproxy/main.go),
  [`pkg/virtualnetwork/mux.go`](https://github.com/containers/gvisor-tap-vsock/blob/main/pkg/virtualnetwork/mux.go),
  and its [README](https://github.com/containers/gvisor-tap-vsock), section Services.
- [Public Suffix List](https://publicsuffix.org/list/public_suffix_list.dat), private domains section.
- RFC 9112, sections 6.1 and 6.3 (message body length, `Transfer-Encoding` with `Content-Length`); RFC 6265, section 8.5 (cookies do not isolate by port).
- Anthropic, [Batch processing](https://platform.claude.com/docs/en/build-with-claude/batch-processing) and the
  [Message Batches API announcement](https://claude.com/blog/message-batches-api): half price, done within 24 hours.
- [CVE-2026-77179 on vulmon](https://vulmon.com/vulnerabilitydetails?qid=CVE-2026-77179);
  [VirtualBox on Apple silicon, CVE-2025-62592](https://bi.zone/eng/expertise/blog/analiz-uyazvimosti-cve-2025-62592-v-oracle-virtualbox-dlya-macos/);
  [QEMU and UTM escape](https://blog.calif.io/p/mad-bugs-qemu-and-utm-escape).
