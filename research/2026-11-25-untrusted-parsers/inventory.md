# Parsers of bytes a computer or a host controls, in the drafts of 24 and 25 November

"Writer" is who chooses the bytes: the computer (code in it, root included), a model host, or a
host the computer reaches. "Second reader" is the program that also reads them, when one does:
where both read, they must agree. The finding numbers are the README's.

| Draft | File | Reads | Writer | Second reader | Tested | Finding |
|---|---|---|---|---|---|---|
| #374/#385 | `boundary/src/main.zig` `readHead`, `pumpBody`, `Decoder` | A program's HTTP/1.1 request: head, framing, gzip and zstd bodies | computer | the model host | end to end, against Go net/http and the real hosts | 1, 2, 3 |
| #374/#385 | `boundary/src/stream.zig` | Chunked framing, server-sent events, tool calls of both providers, tool results in requests | host, computer | the program | libFuzzer | none |
| #374/#385 | `boundary/src/usage.zig` | Usage in Anthropic, Responses, Chat Completions and Gemini replies | host | the provider's bill | libFuzzer | none |
| #374/#385 | `boundary/src/spend.zig` `readRequest`, `credentialOf`, `planWindows` | A request's JSON (model, limits, conversation), its key, plan headers | computer, host | the model host | libFuzzer | none |
| #374/#385 | `boundary/src/ws.zig` | WebSocket frames, masks, fragments, permessage-deflate | computer, host | the program, the host | libFuzzer | 4 |
| #386 | `proto/src/verify.zig`, `format.zig` | The record's segments, frames, lines, head | any program running as you on the Mac | none | libFuzzer, checksums written by the harness | none (14) |
| #379 | `ssh/silo_ssh.py` agent | SSH agent messages: identities, session-bind, sign requests, SSHSIG | computer | OpenSSH's agent | atheris | 6 |
| #379 | `ssh/silo_ssh.py` relay sniffer | The server's SSH packets up to NEWKEYS: host key, exchange signature | the SSH host | the SSH client | read only | none |
| #379 | `cloud/boundary.py` front | HTTPS requests to cloud hosts: Host, Authorization, token forms | computer | the cloud host | end to end | 5 |
| #379 | `cloud/boundary.py` metadata server | Metadata requests, Google and Azure | computer | the clouds' metadata servers | read only | none |
| #379 | `cloud/h2_swap.py` | HTTP/2 frames and HPACK, for the swap in gRPC | computer | the cloud host | not tested | |
| #381 | `proto/ext4read.py` | ext4: superblock, groups, inodes, extents, directories, the jbd2 journal | root in the computer | Linux | atheris, and against Linux | 7, 8, 9 |
| #381 | `proto/qcow2raw.py` | qcow2 headers and tables | the disk file the engine writes | QEMU | not tested: it reads Silo's own file | |
| #384 | `port-previews/prototype/silo_preview.py` forwarder | The app's HTTP answer, the browser's request | computer (the app), the browser | the browser | end to end, in Chromium | 10 |
| #384 | `port-previews/prototype/silo_preview.py` rule proxy | The browser's proxy requests, CONNECT | the page in the browser | the host | read only | |
| #384 | `local-models/prototype/gate.py` | Requests to a model server on the Mac, their JSON | computer | Ollama, LM Studio | end to end, against Go net/http | 11 |
| #384 | `local-models/prototype/gate.py` `Usage` | The local server's reply, four formats | the model on the Mac | the program | read only | |
| #378 | `closed-network/proxy.py` | CONNECT and plain HTTP proxy requests | computer | the host behind the allowed address | end to end, against Go net/http | 12 |
| #380 | `gvproxy-silo.patch` `outbound.go` | The destination address of each connection, against the deny list | computer | the Mac's network | Go fuzzing | 13 |
| #380 | `gvproxy-silo.patch` `dns.go` | DNS questions forwarded to a chosen server | computer | the DNS server | read only: the parsing is miekg/dns | |

"Read only" means the code was read for the questions this folder asks and nothing was found
worth a test. "Not tested" means it was not reached. The two not tested are the next ones to
fuzz: `h2_swap.py` reads HPACK from the computer, and HPACK is where HTTP/2 readers usually
differ.
