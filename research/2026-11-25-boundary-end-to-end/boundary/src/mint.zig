//! A certificate for each host Silo reads, made when a computer first asks for that name and kept
//! for the boundary's life. #347 and #374 used one certificate made in advance for the providers'
//! names; reading any host a secret goes to needs one per name.
const std = @import("std");
pub const c = @cImport({
    @cInclude("openssl/ssl.h");
    @cInclude("openssl/err.h");
    @cInclude("openssl/x509v3.h");
    @cInclude("openssl/pem.h");
    @cInclude("zlib.h");
    @cInclude("zstd.h");
    @cInclude("nghttp2/nghttp2.h");
});

pub const Mint = struct {
    gpa: std.mem.Allocator,
    ca: *c.X509,
    ca_key: *c.EVP_PKEY,
    key: *c.EVP_PKEY,
    backdate_s: i64,
    mu: std.Thread.Mutex = .{},
    certs: std.StringHashMapUnmanaged(*c.X509) = .empty,
    /// Certificates made, and the time spent making them.
    made: u64 = 0,
    made_ns: u64 = 0,

    pub fn init(gpa: std.mem.Allocator, ca_path: []const u8, ca_key_path: []const u8, key_path: []const u8, backdate_s: i64) !Mint {
        return .{
            .gpa = gpa,
            .ca = try readCert(gpa, ca_path),
            .ca_key = try readKey(gpa, ca_key_path),
            .key = try readKey(gpa, key_path),
            .backdate_s = backdate_s,
        };
    }

    /// The certificate for `name`, made on first use. The caller does not free it.
    pub fn get(self: *Mint, name: []const u8) !*c.X509 {
        self.mu.lock();
        defer self.mu.unlock();
        if (self.certs.get(name)) |x| return x;
        const t0 = std.time.nanoTimestamp();
        const x = try self.make(name);
        self.made += 1;
        self.made_ns += @intCast(std.time.nanoTimestamp() - t0);
        try self.certs.put(self.gpa, try self.gpa.dupe(u8, name), x);
        return x;
    }

    fn make(self: *Mint, name: []const u8) !*c.X509 {
        const x = c.X509_new() orelse return error.Mint;
        errdefer c.X509_free(x);
        _ = c.X509_set_version(x, 2);
        var serial: [16]u8 = undefined;
        std.crypto.random.bytes(&serial);
        serial[0] &= 0x7f;
        const bn = c.BN_bin2bn(&serial, serial.len, null);
        defer c.BN_free(bn);
        _ = c.BN_to_ASN1_INTEGER(bn, c.X509_get_serialNumber(x));
        _ = c.X509_gmtime_adj(c.X509_getm_notBefore(x), -self.backdate_s);
        _ = c.X509_gmtime_adj(c.X509_getm_notAfter(x), 7 * 86400);
        _ = c.X509_set_pubkey(x, self.key);
        const subj = c.X509_get_subject_name(x);
        const cn = try self.gpa.dupeZ(u8, name[0..@min(name.len, 64)]);
        defer self.gpa.free(cn);
        _ = c.X509_NAME_add_entry_by_txt(subj, "CN", c.MBSTRING_ASC, cn.ptr, -1, -1, 0);
        _ = c.X509_set_issuer_name(x, c.X509_get_subject_name(self.ca));
        var ctx: c.X509V3_CTX = undefined;
        c.X509V3_set_ctx(&ctx, self.ca, x, null, null, 0);
        const is_ip = std.net.Address.parseIp(name, 0) catch null;
        const san = try std.fmt.allocPrintSentinel(self.gpa, "{s}:{s}", .{ if (is_ip != null) "IP" else "DNS", name }, 0);
        defer self.gpa.free(san);
        const exts = [_]struct { nid: c_int, val: [*c]const u8 }{
            .{ .nid = c.NID_subject_alt_name, .val = san.ptr },
            .{ .nid = c.NID_basic_constraints, .val = "critical,CA:FALSE" },
            .{ .nid = c.NID_ext_key_usage, .val = "serverAuth" },
            .{ .nid = c.NID_key_usage, .val = "critical,digitalSignature" },
            // Python 3.13 checks certificates strictly (VERIFY_X509_STRICT) and refuses one
            // without these two: "Missing Authority Key Identifier". curl and Node do not.
            .{ .nid = c.NID_subject_key_identifier, .val = "hash" },
            .{ .nid = c.NID_authority_key_identifier, .val = "keyid:always" },
        };
        for (exts) |e| {
            const ext = c.X509V3_EXT_conf_nid(null, &ctx, e.nid, e.val) orelse return error.Mint;
            defer c.X509_EXTENSION_free(ext);
            _ = c.X509_add_ext(x, ext, -1);
        }
        if (c.X509_sign(x, self.ca_key, c.EVP_sha256()) == 0) return error.Mint;
        return x;
    }
};

fn readCert(gpa: std.mem.Allocator, path: []const u8) !*c.X509 {
    const z = try gpa.dupeZ(u8, path);
    defer gpa.free(z);
    const bio = c.BIO_new_file(z.ptr, "r") orelse return error.Read;
    defer _ = c.BIO_free(bio);
    return c.PEM_read_bio_X509(bio, null, null, null) orelse error.Read;
}

fn readKey(gpa: std.mem.Allocator, path: []const u8) !*c.EVP_PKEY {
    const z = try gpa.dupeZ(u8, path);
    defer gpa.free(z);
    const bio = c.BIO_new_file(z.ptr, "r") orelse return error.Read;
    defer _ = c.BIO_free(bio);
    return c.PEM_read_bio_PrivateKey(bio, null, null, null) orelse error.Read;
}

/// The name in a ClientHello's server_name extension, as OpenSSL hands it over.
pub fn sniFromExt(d: []const u8) ?[]const u8 {
    if (d.len < 5 or d[2] != 0) return null;
    const nl: usize = std.mem.readInt(u16, d[3..5], .big);
    if (d.len < 5 + nl) return null;
    return d[5 .. 5 + nl];
}
