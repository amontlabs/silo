//! What this boundary is told, beside #374's budgets: which computers come in which way, which
//! hosts each may reach and whether Silo reads them, the secrets and their placeholders, and the
//! cloud metadata answers. It lives under the key "boundary" of the file #374's config is read
//! from, so one file gives both.
const std = @import("std");

pub const Secret = struct {
    /// The secret's name in Silo. The record names a secret by this, never by its value.
    name: []const u8,
    /// What the computer holds and sends.
    placeholder: []const u8,
    /// What leaves the boundary in its place.
    value: []const u8,
    /// The hosts it may go to. A request that carries the placeholder anywhere else is refused.
    hosts: [][]const u8,
    /// "key" (billed per call) or "plan" (a subscription's sign-in), as #374 reads credentials.
    kind: []const u8 = "key",
};

pub const Computer = struct {
    /// The same name as in #374's `computers`, in the same order.
    name: []const u8,
    /// The record's id of the computer, `c1` for main.
    id: []const u8,
    /// The socket the relay in this computer reaches over vsock: one per computer, so the socket
    /// says which computer a CONNECT comes from. A Unix socket stands in for vsock here.
    connect: ?[]const u8 = null,
    /// The socket this computer's gvproxy hands its TCP connections to: one per computer.
    handoff: ?[]const u8 = null,
    /// The hosts this computer may reach; null means every host in `hosts`. Closed is [].
    allow: ?[][]const u8 = null,
    /// Where this computer's gvproxy sends its name lookups (`-dns-upstream ip:port`), on the
    /// hand-off path. One per computer, so a lookup says which computer made it.
    dns: ?[]const u8 = null,
    secrets: []Secret = &.{},
    /// The cloud metadata server's answers, by the secret whose placeholder they hand out.
    google_token: ?[]const u8 = null,
    google_email: []const u8 = "silo@example.iam.gserviceaccount.com",
    google_project: []const u8 = "silo-test",
    azure_token: ?[]const u8 = null,
};

pub const Host = struct {
    name: []const u8,
    ports: []const u16 = &.{443},
    /// "read": Silo ends TLS and reads every request (model hosts, hosts a secret goes to).
    /// "pass": Silo checks the name and carries the bytes unread.
    mode: []const u8 = "pass",
};

pub const Config = struct {
    computers: []Computer = &.{},
    hosts: []Host = &.{},
    /// Where Silo dials a name, in tests: "name": "ip:port". Elsewhere the name is looked up on
    /// the device, whatever address the computer used.
    resolve: std.json.ArrayHashMap([]const u8) = .{},
    /// The recorder's socket. Null: lines are counted and dropped (a run without a record).
    recorder: ?[]const u8 = null,
    /// Room for decisions and changes to traffic not yet acknowledged by the recorder
    /// (`enforcing-and-recording`), and the buffer for other lines.
    decision_room: usize = 4096,
    observation_room: usize = 16384,
    /// How long a request that needs a decision waits for room, before it is refused.
    decision_wait_ms: u64 = 30000,
    /// Silo's authority, which signs a certificate for each host it reads.
    ca_cert: []const u8,
    ca_key: []const u8,
    /// The one key of every certificate Silo makes for a host.
    leaf_key: []const u8,
    /// How far back a certificate Silo makes starts, so that a computer whose clock fell behind
    /// the device's still takes it (#378: a guest was an hour and a half behind after a night).
    cert_backdate_s: i64 = 86400,
    /// The authorities Silo trusts for hosts, in tests the stand-ins'.
    upstream_ca: []const u8,
    /// A head that has not arrived whole after this long is refused (a CONNECT, a PROXY header,
    /// a ClientHello).
    head_timeout_ms: u32 = 10000,
};
