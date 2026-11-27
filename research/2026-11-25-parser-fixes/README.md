# The disk, preview and closed-network findings of #401, fixed

Research of 25 November 2026, by an agent, for polarzero. #401 compared the drafts' parsers with
the programs they sit in front of. Three of its findings are not about the spend boundary: the
ext4 readers of #381 and #398, the preview forwarder of #384, and the closed network's proxy of
#378. This branch merges those drafts and fixes each finding. Every test was run first on the
drafts' own code and failed there. The output of both runs is in [`results.txt`](results.txt).

Nothing here is built, and no entry is taken by it. The fixes are commits on this branch only;
nothing is pushed to the drafts' branches.

## What was fixed

### The ext4 readers (#381 `proto/ext4read.py`, #398 `reader/ckptread.py`)

Both readers had the same directory code, so both had the same bugs. Each case below is in
[`ext4/test_readers_against_kernel.py`](ext4/test_readers_against_kernel.py). The test mounts each
image read-only through a loop device and compares the paths and kinds with each reader's walk.

| Case | Before | After |
|---|---|---|
| A dirent with type byte 0xDE, no `metadata_csum` (#401, finding 7) | Linux lists the file; both readers hide it | Both list it |
| A directory whose size is one block smaller than its blocks (finding 8) | Both readers list 9 files that Linux does not | Both read up to the size, as Linux does |
| A directory block whose checksum fails (finding 8) | Linux refuses the block; both readers list its 30 files | Both refuse, with the gap `dir-block-checksum` |
| A name with a NUL byte (finding 8) | `file\0with…` to the readers, `file` to Linux | Both refuse, with the gap `name` |
| **New:** an htree leaf the index does not name | The kernel lists an indexed directory through its index, so it misses that leaf's files (17 and 20 in two runs); both readers listed them | Both read an indexed directory through its index, and list what Linux lists |
| Six crashes the fuzzer found (finding 9), each made by hand: zero inodes or blocks per group, an inode in a group past the end, an inode size too small or past a block, a dirent header past the block | `ZeroDivisionError`, `IndexError`, `struct.error` | A refusal |
| **New:** a journal with no blocks (found by fuzzing the fixed `ext4read`) | `IndexError` in `ext4read`; `ckptread` already refused it | A refusal |

What the readers do now, in both files:

- A directory is read up to its size.
- With `metadata_csum`, each directory block's checksum is checked: the dirent tail of a leaf, and
  the dx tail of an index block. A block that fails is refused. Linux refuses it too.
- A directory with the htree flag, on a file system with `dir_index`, is read through its index:
  the leaves its entries name, each once, with the hashes in order. An index the kernel calls bad
  (an unknown hash, a wrong limit, too many levels) is read block by block, as the kernel then
  reads it.
- An entry is listed when its inode is not 0. The checksum tail has inode 0, so it is never
  listed. The old check on type 0xDE is gone.
- A dirent is refused when Linux's `ext4_check_dir_entry` would reject it: a length under 12 or not
  a multiple of 4, an entry past the block, or an inode past the count.
- The superblock is refused when the kernel would not mount it: no inodes or blocks per group, an
  inode count other than inodes per group times groups, an inode size that is not a power of two
  between 128 and the block size, or a 64-bit descriptor size outside 64 to 1024.

The drafts' own tests still pass on the fixed readers. #381's `check.sh` gives the same lines as
its `results.txt` (3,350 and 1,700 paths, 0 differing lines with replay). #398's
`test_ckptread.py` passes all 12 tests. Fuzzing with [`ext4/fz_readers.py`](ext4/fz_readers.py),
#401's harness for both readers, found nothing else; the runs are in `results.txt`.

### The preview forwarder (#384 `prototype/silo_preview.py`)

The forwarder adds Silo's Content-Security-Policy and Permissions-Policy to the head of an app's
answer. The page escapes them whenever Chromium reads a different head than the forwarder.
[`preview/test.sh`](preview/test.sh) serves five answers and opens each preview in Chromium. The
page's script fetches a host that `connect-src 'self'` forbids.

| Answer from the app | Before | After |
|---|---|---|
| A well-formed head | Policy on; the fetch is blocked | The same |
| A head ended by bare LFs, a CRLF pair in the body (#401, finding 10) | No policy; the fetch reaches the host | 502 |
| **New:** `100 Continue`, then the real head | Silo's policy lands on the 100, which Chromium drops; the fetch reaches the host | The 100 is dropped, and the policy is on the 200 |
| **New:** `103 Early Hints`, then the real head | The same as 100 | The same as 100 |
| A folded header line | The forwarder's handler raised, and the browser got an empty answer | 502 |

The forwarder now reads the app's head strictly: CRLF line ends only, no folded line, a token and
a colon on each line, no NUL, at most 64 KiB. Anything else gets a 502. An interim answer (1xx
other than 101) is read and dropped, and the next head is read. #384's own scenarios (`run.js`:
cross, toss, login, persist, ui, background) give the same results as its `results.json`, up to
the order of two lists.

### The closed network (#378 `closed-network/proxy.py`)

For plain `http://`, the proxy checked the first request's URL and then joined the two ends. Any
later bytes on the connection reached the allowed host unread.
[`closed-network/test_proxy.py`](closed-network/test_proxy.py) records the raw bytes that reach
the allowed host and fails when they name another host.

| Request | Before | After |
|---|---|---|
| A second request on the connection, for `collector.example` (#401, finding 12) | Reaches the host; the log shows one request | Never sent |
| **New:** a Host header other than the URL's host | Reaches the host | 400 |
| **New:** a request hidden after a chunked body | Reaches the host | 501 (chunked is refused) |
| **New:** a request after a body of declared length | Reaches the host | Never sent |
| **New:** a bare LF before a second Host header | Reaches the host | 400 |

The proxy now takes one plain-HTTP request per connection and checks it whole. The head is read
strictly. The Host header must name the URL's host and port. A body passes by its
`Content-Length`, and nothing after it is sent. Hop-by-hop headers are removed, and the request
goes up with `Connection: close`. curl through the relay, inside a namespace with only its
loopback, still gets two GETs and a POST answered; the log now shows two GETs where it showed one.
CONNECT is unchanged.

## What this changes in the plan

- `file-changes-from-outside`: a reader of a computer's disk has to list a directory the way the
  kernel does, through its htree index and up to its size, and refuse what the kernel refuses.
  Silo's Zig reader needs every rule listed above, and the tests here are its cases.
- `ports-reached-from-your-mac`: a forwarder that adds headers must read the app's answer as
  strictly as the browser reads it, and must handle interim answers.
- `network-stack`: a proxy that allows by name must check every request on a connection, not the
  first.
- `fuzz-untrusted-parsers` (#401's notes): two of the five new cases came from comparing with the
  real program (Chromium's 1xx handling, the kernel's htree readdir). One came from fuzzing the
  fixed code.

No plan file is changed here.

## What was not verified

- **Name hashes in an htree.** The kernel lists an htree leaf from the hash where its index entry
  starts, so it skips an entry whose name hashes below that. These readers do not compute hashes:
  an entry placed in the wrong leaf is listed by them and may be skipped by Linux. The Zig reader
  should compute the hash (half MD4, TEA or legacy, from the superblock) and refuse an entry outside
  its leaf's range.
- **Where the kernel reads past an error and the readers refuse.** Linux skips one bad directory
  block and lists the others; the readers refuse the walk. That is stricter, never a different
  listing.
- **Unwritten extents in a directory** read the same as Linux in the one case tried. The kernel
  code for that path was not checked.
- **#384's DNS scenarios and real dev servers** (Vite, Next.js) were not run again.
- **#378's full `run.sh`**, with Claude Code and package managers inside the namespace, was not run
  again. Only curl was. A client that expects keep-alive through an HTTP proxy now opens one
  connection per request.
- **CONNECT** still checks only the name in the CONNECT line. A TLS name or a Host header inside
  the tunnel can name another host served at the same address. This proxy cannot see inside TLS;
  the spend boundary, which ends TLS, checks it there.

## Run it

```
sudo python3 -m unittest -v research/2026-11-25-parser-fixes/ext4/test_readers_against_kernel.py
pip install atheris && READER=ext4read SEED=<image> python3 research/2026-11-25-parser-fixes/ext4/fz_readers.py -max_total_time=1200
research/2026-11-25-parser-fixes/preview/test.sh          # Node with Playwright's Chromium
python3 -m unittest -v research/2026-11-25-parser-fixes/closed-network/test_proxy.py
```

Linux, as root for the ext4 test (loop mounts). Python 3.13 and e2fsprogs. To see the tests fail,
put the drafts' code back first:

```
git checkout ca459ee -- research/2026-11-25-linux-main research/2026-11-25-write-log   # #381, #398
git checkout 4b6cf69 -- research/2026-11-25-port-previews                              # #384
git checkout 08e74ee -- research/2026-11-25-techniques-from-two-tools/closed-network   # #378
```

## The research at a glance

#401's disk, preview and closed-network findings are fixed on a branch that merges #381, #398,
#384 and #378, each shown failing first. Five more cases came up and are fixed with them: an
htree leaf the index does not name (Silo listed about 20 files Linux does not), a 100 or 103 interim
answer that drops the preview's policy, a Host header or a chunked body that carries a request
past the closed network, and a journal with no blocks that crashed one reader.
