# Mac tests 23 to 27, for the kit

Five tests for an iOS project in a macOS computer, written in the format of
`research/mac-test-kit/` (pull request #359). They are kept here until the kit is merged. Then
`tests/*.sh` go to `research/mac-test-kit/tests/` and `lib/ios.sh` to `research/mac-test-kit/lib/`,
and `./kit doc` rewrites the kit's `RESULTS.md`. [`RESULTS-23-27.md`](RESULTS-23-27.md) is what it
adds, to read before then.

Run 23 first: it copies the Xcode of your Mac into a computer, downloads the iOS simulator runtime
there and keeps the result as "the Xcode computer". 24 to 27 each start a clone of it and delete
the clone at the end. Nothing is installed or trusted on your Mac.

```
./kit run 23        # about an hour: copy Xcode in, first launch, download the runtime
./kit run 24 26 27  # about an hour together
./kit run 25        # about 40 minutes: simulators booted until the computer swaps
```

If 24 ends on `needs-screen-login`, run 25 and 27 on a computer whose user is logged in on its
screen: 24 shows how (automatic login, then a restart).

Checked on Linux, 24 November 2026, in a copy of the kit at #359's head: `./kit selftest` passes
on all five files and on `lib/ios.sh`, and `RESULTS.md` is up to date after `./kit doc` (its one
failure, the Linux analogue of test 06, needs reflinks this machine's file system did not have,
and fails the same way without these files; shellcheck was not installed, so it was skipped), and `./kit run` skips each
one on Linux with its reason. The helpers that parse `vm_stat` and `simctl list -j` were run
against fake output. Nothing ran on a Mac.
