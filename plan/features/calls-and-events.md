## Proposed

- The engine is state, calls and events. State is a set of records. A call asks for a change. An event says what happened. The app shows state and follows events.
- One name per concept everywhere: a part of the plan, a group of calls, one engine file, a command, a screen.

- Calls
  - Rule: One call carries one whole user intention, as fork and run does, and has a variant that waits and returns the result.
  - Rule: Every call carries an id chosen by the caller, and repeating the id returns the first result without acting again, so a lost reply can be retried.
  - Rule: Each call's definition lists the refusals it can return, and the typed client hands them to the caller as typed error values.
  - Property: A call can declare what it may touch, and the engine refuses it if it touches more.
  - Rule: The engine protocol is versioned.
  - Rule: A call marked unstable may still change its interface, and the mark never means that the call is broken.
  - Rule: Every list call is paged by cursor and limit from the first version.
  - Rule: Each kind of identifier (computer, fork, checkpoint, device, call) is its own type, checked at the boundary and kept as such in the Swift app.
  - Rule: The engine gets no query language (the entry engine-query) unless a written problem shows that typed calls cannot solve it.
- Events and state
  - Property: A client that starts from the state at event N and applies every later event in order holds the same state as the engine.
  - Rule: Events are numbered, a client reconnects by giving its last event number, and the engine keeps no state per client.
  - Rule: A client's surface is requests and one pushed stream of numbered events from one writer, and the first versions have no general sync engine.
  - An agent waits for an event (a fork finished, a command exited, an approval answered) with a wait call on the numbered events, so nothing polls a computer.
  - Another consumer, such as another device or a team view, can follow or export the same event stream from the first version.
  - Risk: Synced state copes badly with megabyte-sized values, so bulky data stays out of the state sent to clients.
  - Property: Engine state and events serialise to one stable form that every binding reads.
  - Rule: Records stay flat with readable fields, so a generic table browser can serve as a stopgap interface.
  - Status, block reason and unhealthy reason are computed once, in the engine or the Zig client, and every interface renders them the same way.
  - The docs list what a client can subscribe to (the state of a computer, a fork or a diff, approvals, checks), and the event design is checked against that list.
- What the definition generates
  - Calls and events are defined as plain Zig data, and record layouts, encoders and handlers are computed from that definition at comptime.
  - The definition also generates a typed client library and a skill that describes every call in machine-readable form, so an agent can use Silo without an MCP server.
  - An HTTP API and an MCP server can be generated from the same definition.
  - The client library and the command line ship as separate small packages that other tools can adopt.
- Clients and bindings
  - One Zig client serves the command line and every later interface, and it runs in the simulator.
  - Rule: C is the only binding promised, for Swift and the command line.
  - Rule: Any other binding (WebAssembly, Python, Kotlin) is added on demand, and has its own tests and docs before it is called ready.
  - Rule: The C interface has one written rule for memory ownership, either caller-provided buffers or a paired free call for every returned allocation, fixed before the app is built on it.
  - Rule: One stated rule says which version wins among the global command line, the client linked in the app and the engine.
  - Rule: A version mismatch is an error that names both versions.
  - Rule: The client validates every frame from the engine against an explicit shape and raises a named error on a malformed one.
  - Client transports are HTTP as the baseline, so that curl can drive the engine, an in-memory transport for tests, and a streaming channel for events.
  - Property: The client library exposes low-level calls that compose and imposes no architecture on its caller, and helpers for a UI framework are optional and sit on top.
  - Rule: Higher-level patterns of the client library ship as documented examples or agent skills, and no code is exported for them.
  - Risk: The build time of the bindings generated between the Zig client and Swift may grow faster than the number of record types, so it is measured as types are added.
- Tests
  - Test: The simulator repeats calls with the same id, and no repeated call acts twice.
  - Test: Add a call or an event without handling it in every binding, and the build fails.
  - Test: Each event type is round-tripped through every encoding, and every field comes back.
  - Test: The engine, the Zig client, the app and the command line run the same conformance tests, from one source of protocol definitions and fixtures, and all pass.
  - Test: A benchmark counts messages, round trips and state for the common flows.
- Other
  - A heavy call, such as a merge, a restore or a recipe build, can be asked as a dry run that returns its effects and changes nothing.
  - The docs show how an outside tool, such as a diff viewer or a notifier, follows the events and makes calls.
  - Rule: In calls, events and file formats, a list of named things is an array of records that each carry a name field, never a map keyed by name.
  - Rule: Call and command names follow written principles kept in these notes, and a design that splits opinion stays out unless it measures better.
  - Test: Every example in the generated docs runs against the command line in the whole check, so an example that no longer works fails it.

## Open

- Does the Swift app link the Zig client through a C interface, or speak the protocol in Swift generated from the definition?
  - Suggested: Link the Zig client, so reconnection and status are written once and run in the simulator; mac-app-in-swift rules out bindings only for driving macOS computers.
- Does Silo promise never to rename or break a published call or command, with an unstable mark as the only way out?
  - Suggested: Yes, with every call marked unstable until the starter is in daily use, because agents script against names and a break is silent for them.
- To find out: Do agents need to batch calls, or to pipeline them so that a call uses an earlier call's result without a round trip?

## From the old code

The old code had it built, about 6,500 lines, and its approach can be reused.

Lessons:
- 190 calls in 37 groups grew partly because app screens got their own calls (display.viewer.*, exports.dismiss); a rewrite should keep calls per user action.
- Events are not yet cut per computer, so a computer's agent cannot subscribe and has to poll.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/registry.rs`
- `app/SiloUI/api/src/methods/request.rs`
- `app/SiloUI/api/schema/silo-api.schema.json`
- `app/SiloUI/api/README.md`
- `app/SiloUI/src-tauri/engine/src/events/mod.rs`
- `docs/ROADMAP.md#L414`
- `docs/ROADMAP.md#L456`

Calls: `events.subscribe`.
