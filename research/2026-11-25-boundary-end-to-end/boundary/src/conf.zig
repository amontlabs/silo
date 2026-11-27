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
    /// How Claude signs in here (#393, `claude-key-or-plan`): "key", "plan", or "read" (absent:
    /// read whatever it carries). "plan" passes api.anthropic.com through unread.
    anthropic: []const u8 = "read",
    /// Hold a destructive call to a platform's API (#353, #383) until a person allows it.
    /// "hold" (absent): held, and refused when no answer comes; "off": not looked at.
    destructive: []const u8 = "hold",
};

pub const Host = struct {
    name: []const u8,
    ports: []const u16 = &.{443},
    /// "read": Silo ends TLS and reads every request (model hosts, hosts a secret goes to).
    /// "pass": Silo checks the name and carries the bytes unread.
    mode: []const u8 = "pass",
    /// On a host Silo reads: offer HTTP/2 to a program that asks for it, and speak it to the host.
    /// For hosts a program reaches by gRPC, which cannot fall back to HTTP/1.1. Never on a model
    /// host, where #374 counts calls over HTTP/1.1.
    h2: bool = false,
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
    /// The precaution that holds destructive calls (#383's rules in #353's detector): its socket,
    /// or null for none. Asked about every request to a host Silo reads that is not a model's.
    precaution: ?[]const u8 = null,
    /// How long the boundary waits for its opinion (`classifier-wait-limit`), and what it does
    /// when none comes in time: "deny" or "rules" (pass, as the rules decide).
    classify_limit_ms: u32 = 2000,
    on_no_opinion: []const u8 = "deny",
    /// How long a held request waits for a person (`hold-limit-length`: 90 s).
    hold_limit_ms: u32 = 90000,
};

/// Every placeholder names one secret of one computer: no two are equal, none holds another (the
/// swap replaces substrings), and none holds a value or is held by one. Checked when the boundary
/// starts; a config that fails it is refused.
pub fn checkPlaceholders(computers: []const Computer) error{ PlaceholderEmpty, PlaceholderShared, PlaceholderInsideAnother, PlaceholderAndValueOverlap }!void {
    for (computers, 0..) |a, ka| for (a.secrets, 0..) |sa, ia| {
        if (sa.placeholder.len == 0) return error.PlaceholderEmpty;
        if (std.mem.indexOf(u8, sa.value, sa.placeholder) != null or std.mem.indexOf(u8, sa.placeholder, sa.value) != null) return error.PlaceholderAndValueOverlap;
        for (computers, 0..) |b, kb| for (b.secrets, 0..) |sb, ib| {
            if (ka == kb and ia == ib) continue;
            if (std.mem.eql(u8, sa.placeholder, sb.placeholder)) return error.PlaceholderShared;
            if (std.mem.indexOf(u8, sb.placeholder, sa.placeholder) != null) return error.PlaceholderInsideAnother;
            if (std.mem.indexOf(u8, sb.value, sa.placeholder) != null or std.mem.indexOf(u8, sa.placeholder, sb.value) != null) return error.PlaceholderAndValueOverlap;
        };
    };
}

test "one placeholder per secret and per computer" {
    const t = std.testing;
    var h = [_][]const u8{"api.anthropic.com"};
    // #377: the same placeholder for every secret. Two keys for one host: which one was meant?
    var shared = [_]Secret{
        .{ .name = "anthropic-work", .placeholder = "placeholder-not-a-key", .value = "sk-ant-api03-WORK", .hosts = &h },
        .{ .name = "anthropic-home", .placeholder = "placeholder-not-a-key", .value = "sk-ant-api03-HOME", .hosts = &h },
    };
    try t.expectError(error.PlaceholderShared, checkPlaceholders(&.{.{ .name = "main", .id = "c1", .secrets = &shared }}));
    // The same placeholder on two computers.
    var one = [_]Secret{.{ .name = "a", .placeholder = "sk-ant-api03-PH", .value = "sk-ant-api03-V", .hosts = &h }};
    try t.expectError(error.PlaceholderShared, checkPlaceholders(&.{ .{ .name = "main", .id = "c1", .secrets = &one }, .{ .name = "fork", .id = "c2", .secrets = &one } }));
    var nested = [_]Secret{
        .{ .name = "a", .placeholder = "sk-ant-api03-PH", .value = "sk-ant-api03-V1", .hosts = &h },
        .{ .name = "b", .placeholder = "sk-ant-api03-PH2", .value = "sk-ant-api03-V2", .hosts = &h },
    };
    try t.expectError(error.PlaceholderInsideAnother, checkPlaceholders(&.{.{ .name = "main", .id = "c1", .secrets = &nested }}));
    var good = [_]Secret{
        .{ .name = "anthropic-work", .placeholder = "sk-ant-api03-PHwork", .value = "sk-ant-api03-WORK", .hosts = &h },
        .{ .name = "anthropic-home", .placeholder = "sk-ant-api03-PHhome", .value = "sk-ant-api03-HOME", .hosts = &h },
    };
    try checkPlaceholders(&.{.{ .name = "main", .id = "c1", .secrets = &good }});
}
