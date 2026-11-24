// The parts of silovm that need only Foundation: arguments, JSON lines, the bundle's files, the
// DHCP lease file, Unix sockets, and the commands `version`, `ip` and `ctl`. This file imports no
// Apple-only framework, so it also builds on Linux (linux/test-vm-helpers.sh typechecks and runs
// it there). Everything that touches the Virtualization framework is in Machine.swift.

import Foundation
import Dispatch
#if canImport(Darwin)
import Darwin
#elseif canImport(Glibc)
import Glibc
#endif

let siloVMVersion = "0.1.0"

// MARK: - Errors and output

/// A failure silovm reports and exits on. Never swallowed.
struct Fail: Error, CustomStringConvertible {
  let message: String
  init(_ message: String) { self.message = message }
  var description: String { message }
}

func posixFail(_ what: String, _ code: Int32 = errno) -> Fail {
  Fail("\(what): \(String(cString: strerror(code))) (errno \(code))")
}

/// A JSON value whose object keys keep their order, so every event starts with "event".
enum J {
  case s(String)
  case i(Int)
  case d(Double)
  case b(Bool)
  case null
  case arr([J])
  case obj([(String, J)])
}

func jsonQuote(_ s: String) -> String {
  var out = "\""
  for u in s.unicodeScalars {
    switch u {
    case "\"": out += "\\\""
    case "\\": out += "\\\\"
    case "\n": out += "\\n"
    case "\r": out += "\\r"
    case "\t": out += "\\t"
    default:
      if u.value < 0x20 {
        out += String(format: "\\u%04x", u.value)
      } else {
        out.unicodeScalars.append(u)
      }
    }
  }
  return out + "\""
}

func encodeJSON(_ j: J) -> String {
  switch j {
  case .s(let v): return jsonQuote(v)
  case .i(let v): return String(v)
  case .d(let v): return v.isFinite ? String(format: "%.3f", v) : "null"
  case .b(let v): return v ? "true" : "false"
  case .null: return "null"
  case .arr(let a): return "[" + a.map(encodeJSON).joined(separator: ",") + "]"
  case .obj(let o): return "{" + o.map { jsonQuote($0.0) + ":" + encodeJSON($0.1) }.joined(separator: ",") + "}"
  }
}

/// One line on stdout. FileHandle writes are unbuffered, so a reader sees each line at once.
func printLine(_ s: String) {
  FileHandle.standardOutput.write(Data((s + "\n").utf8))
}

func printJSON(_ fields: [(String, J)]) {
  printLine(encodeJSON(.obj(fields)))
}

func logErr(_ s: String) {
  FileHandle.standardError.write(Data(("silovm: " + s + "\n").utf8))
}

func now() -> Double { Date().timeIntervalSince1970 }

// MARK: - Errors from the framework

/// VZError.Code names by raw value, from the macOS 27 SDK (VZError.h). By raw value rather than by
/// a switch over the enum, so the names of macOS 27 codes print even when this was built with an
/// older SDK.
let vzErrorNames: [Int: String] = [
  1: "internalError",
  2: "invalidVirtualMachineConfiguration",
  3: "invalidVirtualMachineState",
  4: "invalidVirtualMachineStateTransition",
  5: "invalidDiskImage",
  6: "virtualMachineLimitExceeded",
  7: "networkError",
  8: "outOfDiskSpace",
  9: "operationCancelled",
  10: "notSupported",
  11: "save",
  12: "restore",
  10001: "restoreImageCatalogLoadFailed",
  10002: "invalidRestoreImageCatalog",
  10003: "noSupportedRestoreImagesInCatalog",
  10004: "restoreImageLoadFailed",
  10005: "invalidRestoreImage",
  10006: "installationRequiresUpdate",
  10007: "installationFailed",
  20001: "networkBlockDeviceNegotiationFailed",
  20002: "networkBlockDeviceDisconnected",
  30001: "usbControllerNotFound",
  30002: "deviceAlreadyAttached",
  30003: "deviceInitializationFailure",
  30004: "deviceNotFound",
  40001: "guestProvisioningInvalidFullName",
  40002: "guestProvisioningInvalidUsername",
  40003: "guestProvisioningInvalidPassword",
  50001: "efiSecureBootEnrollmentFailed",
  50002: "efiVariableInaccessible",
]

let vzErrorDomain = "VZErrorDomain"

struct ErrorInfo {
  var domain: String
  var code: Int
  var name: String?
  var message: String
  var underlying: String?

  init(_ error: Error) {
    if let f = error as? Fail {
      domain = "silovm"
      code = 0
      name = nil
      message = f.message
      underlying = nil
      return
    }
    let ns = error as NSError
    domain = ns.domain
    code = ns.code
    name = ns.domain == vzErrorDomain ? vzErrorNames[ns.code] : nil
    message = ns.localizedDescription
    if let reason = ns.localizedFailureReason { message += " (\(reason))" }
    if let u = ns.userInfo[NSUnderlyingErrorKey] as? NSError {
      underlying = "\(u.domain) \(u.code): \(u.localizedDescription)"
    } else {
      underlying = nil
    }
  }

  var isProvisioning: Bool { domain == vzErrorDomain && (40001...40003).contains(code) }

  var text: String {
    var s = "\(domain) \(code)"
    if let n = name { s += " \(n)" }
    s += ": \(message)"
    if let u = underlying { s += " [underlying: \(u)]" }
    return s
  }

  var fields: [(String, J)] {
    var f: [(String, J)] = [("domain", .s(domain)), ("code", .i(code)), ("name", name.map { J.s($0) } ?? J.null),
                            ("message", .s(message))]
    if let u = underlying { f.append(("underlying", .s(u))) }
    return f
  }
}

// MARK: - Arguments

/// Options of one command. `--name value`, `--name=value` and flags; anything unknown is an error.
struct Args {
  private(set) var positional: [String] = []
  private var values: [String: [String]] = [:]
  private var flags: Set<String> = []

  init(_ argv: [String], valued: Set<String>, flags known: Set<String>) throws {
    var i = 0
    while i < argv.count {
      let a = argv[i]
      if a.hasPrefix("--") && a.count > 2 {
        var name = String(a.dropFirst(2))
        var inline: String? = nil
        if let eq = name.firstIndex(of: "=") {
          inline = String(name[name.index(after: eq)...])
          name = String(name[..<eq])
        }
        if valued.contains(name) {
          let v: String
          if let inline = inline {
            v = inline
          } else {
            guard i + 1 < argv.count else { throw Fail("--\(name) needs a value") }
            i += 1
            v = argv[i]
          }
          values[name, default: []].append(v)
        } else if known.contains(name) {
          guard inline == nil else { throw Fail("--\(name) takes no value") }
          flags.insert(name)
        } else {
          throw Fail("unknown option --\(name)")
        }
      } else {
        positional.append(a)
      }
      i += 1
    }
  }

  func value(_ name: String) -> String? { values[name]?.last }
  func all(_ name: String) -> [String] { values[name] ?? [] }
  func has(_ name: String) -> Bool { flags.contains(name) }

  func require(_ name: String) throws -> String {
    guard let v = value(name), !v.isEmpty else { throw Fail("--\(name) is required") }
    return v
  }

  func int(_ name: String, default d: Int, min lo: Int = 1) throws -> Int {
    guard let v = value(name) else { return d }
    guard let n = Int(v), n >= lo else { throw Fail("--\(name) must be a whole number of at least \(lo), got \(v)") }
    return n
  }

  func choice(_ name: String, _ allowed: [String], default d: String) throws -> String {
    let v = value(name) ?? d
    guard allowed.contains(v) else { throw Fail("--\(name) must be one of \(allowed.joined(separator: ", ")), got \(v)") }
    return v
  }
}

// MARK: - A bundle

func absolutePath(_ p: String) -> String {
  let expanded = (p as NSString).expandingTildeInPath
  if expanded.hasPrefix("/") { return (expanded as NSString).standardizingPath }
  return ((FileManager.default.currentDirectoryPath as NSString).appendingPathComponent(expanded) as NSString).standardizingPath
}

/// The files of a `.silovm` folder (swift/README.md).
struct VMBundle {
  let path: String
  init(_ p: String) { path = absolutePath(p) }
  func file(_ name: String) -> String { (path as NSString).appendingPathComponent(name) }
  var disk: String { file("disk.img") }
  var aux: String { file("aux.img") }
  var hardwareModel: String { file("hardware-model.bin") }
  var machineID: String { file("machine-id.bin") }
  var config: String { file("config.json") }
  var ctlSocket: String { file("ctl.sock") }
  var netSocket: String { file("net.sock") }
  /// Files of a run, never copied by `clone`.
  static let runFiles: Set<String> = ["ctl.sock", "net.sock", "run.jsonl", "run.log", "run.pid"]

  func mustExist() throws {
    var isDir: ObjCBool = false
    guard FileManager.default.fileExists(atPath: path, isDirectory: &isDir), isDir.boolValue else {
      throw Fail("no bundle at \(path)")
    }
  }
}

struct BundleConfig: Codable {
  var mac: String
  var cpus: Int
  var memoryGB: Int
  var diskFormat: String

  enum CodingKeys: String, CodingKey {
    case mac
    case cpus
    case memoryGB = "memory_gb"
    case diskFormat = "disk_format"
  }

  static func load(_ b: VMBundle) throws -> BundleConfig {
    let data: Data
    do {
      data = try Data(contentsOf: URL(fileURLWithPath: b.config))
    } catch {
      throw Fail("cannot read \(b.config): \(error.localizedDescription)")
    }
    do {
      return try JSONDecoder().decode(BundleConfig.self, from: data)
    } catch {
      throw Fail("\(b.config) is not a valid config: \(error)")
    }
  }

  func save(_ b: VMBundle) throws {
    let enc = JSONEncoder()
    enc.outputFormatting = [.prettyPrinted, .sortedKeys]
    let data = try enc.encode(self)
    try data.write(to: URL(fileURLWithPath: b.config), options: .atomic)
  }
}

func readFile(_ path: String) throws -> Data {
  do {
    return try Data(contentsOf: URL(fileURLWithPath: path))
  } catch {
    throw Fail("cannot read \(path): \(error.localizedDescription)")
  }
}

// MARK: - DHCP leases of Apple's NAT (/var/db/dhcpd_leases)

/// "1,a:b:c:d:e:f" or "A:0B:c:..." -> "0a:0b:0c:0d:0e:0f". bootpd writes each byte in hex without
/// leading zeros and prefixes the hardware type; VZMACAddress.string writes two digits.
func normaliseMAC(_ raw: String) -> String? {
  var s = raw.trimmingCharacters(in: .whitespaces)
  if let comma = s.firstIndex(of: ",") { s = String(s[s.index(after: comma)...]) }
  let parts = s.split(separator: ":", omittingEmptySubsequences: false)
  guard parts.count == 6 else { return nil }
  var out: [String] = []
  for p in parts {
    guard !p.isEmpty, p.count <= 2, let byte = UInt8(p, radix: 16) else { return nil }
    out.append(String(format: "%02x", byte))
  }
  return out.joined(separator: ":")
}

struct Lease {
  var ip: String
  var mac: String
  var expires: Double?
}

/// Blocks of "key=value" lines between "{" and "}", as bootpd writes them.
func parseLeases(_ text: String) -> [Lease] {
  var leases: [Lease] = []
  var cur: [String: String]? = nil
  for rawLine in text.split(separator: "\n", omittingEmptySubsequences: true) {
    let line = rawLine.trimmingCharacters(in: .whitespaces)
    if line == "{" {
      cur = [:]
    } else if line == "}" {
      if let c = cur, let ip = c["ip_address"], let hw = c["hw_address"], let mac = normaliseMAC(hw) {
        var exp: Double? = nil
        if let l = c["lease"] {
          let hex = l.hasPrefix("0x") ? String(l.dropFirst(2)) : l
          if let v = UInt64(hex, radix: 16) { exp = Double(v) }
        }
        leases.append(Lease(ip: ip, mac: mac, expires: exp))
      }
      cur = nil
    } else if cur != nil, let eq = line.firstIndex(of: "=") {
      cur![String(line[..<eq])] = String(line[line.index(after: eq)...])
    }
  }
  return leases
}

/// The address of the newest lease for this MAC that has not expired.
func leaseIP(for mac: String, in text: String, at t: Double) -> String? {
  guard let want = normaliseMAC(mac) else { return nil }
  let mine = parseLeases(text).filter { $0.mac == want && ($0.expires ?? Double.infinity) > t }
  return mine.max(by: { ($0.expires ?? 0) < ($1.expires ?? 0) })?.ip
}

let defaultLeasesPath = "/var/db/dhcpd_leases"

/// silovm ip --bundle B [--wait S]: prints the bare address, so `ssh user@$(silovm ip ...)` works.
func cmdIP(_ argv: [String]) throws -> Int32 {
  let a = try Args(argv, valued: ["bundle", "wait"], flags: [])
  let b = VMBundle(try a.require("bundle"))
  try b.mustExist()
  let cfg = try BundleConfig.load(b)
  let wait = try a.int("wait", default: 0, min: 0)
  let leasesPath = ProcessInfo.processInfo.environment["SILOVM_DHCPD_LEASES"] ?? defaultLeasesPath
  let deadline = now() + Double(wait)
  while true {
    if FileManager.default.isReadableFile(atPath: leasesPath) {
      let text: String
      do {
        text = try String(contentsOfFile: leasesPath, encoding: .utf8)
      } catch {
        throw Fail("cannot read \(leasesPath): \(error.localizedDescription)")
      }
      if let ip = leaseIP(for: cfg.mac, in: text, at: now()) {
        printLine(ip)
        return 0
      }
    }
    if now() >= deadline { break }
    usleep(1_000_000)
  }
  logErr("no lease for \(cfg.mac) in \(leasesPath) after \(wait) s")
  return 1
}

// MARK: - Unix sockets

#if os(Linux)
let sockStream = Int32(SOCK_STREAM.rawValue)
let sockDgram = Int32(SOCK_DGRAM.rawValue)
#else
let sockStream = SOCK_STREAM
let sockDgram = SOCK_DGRAM
#endif

func unixAddress(_ path: String) throws -> sockaddr_un {
  var addr = sockaddr_un()
  addr.sun_family = sa_family_t(AF_UNIX)
  let bytes = Array(path.utf8)
  let capacity = MemoryLayout.size(ofValue: addr.sun_path)
  guard bytes.count < capacity else {
    throw Fail("socket path is \(bytes.count) bytes, the limit is \(capacity - 1): \(path)")
  }
  withUnsafeMutableBytes(of: &addr.sun_path) { raw in
    raw.copyBytes(from: bytes)
    raw[bytes.count] = 0
  }
  #if !os(Linux)
  addr.sun_len = UInt8(MemoryLayout<sockaddr_un>.size)
  #endif
  return addr
}

private func withSockaddr<T>(_ addr: inout sockaddr_un, _ body: (UnsafePointer<sockaddr>, socklen_t) -> T) -> T {
  let len = socklen_t(MemoryLayout<sockaddr_un>.size)
  return withUnsafePointer(to: &addr) { p in
    p.withMemoryRebound(to: sockaddr.self, capacity: 1) { body($0, len) }
  }
}

func setIntOption(_ fd: Int32, _ name: Int32, _ value: Int32) throws {
  var v = value
  guard setsockopt(fd, SOL_SOCKET, name, &v, socklen_t(MemoryLayout<Int32>.size)) == 0 else {
    throw posixFail("setsockopt \(name)")
  }
}

func setReceiveTimeout(_ fd: Int32, seconds: Int) throws {
  var tv = timeval()
  tv.tv_sec = seconds
  tv.tv_usec = 0
  guard setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, socklen_t(MemoryLayout<timeval>.size)) == 0 else {
    throw posixFail("setsockopt SO_RCVTIMEO")
  }
}

/// A listening stream socket at path; an old socket file there is removed first.
func unixListen(_ path: String) throws -> Int32 {
  var addr = try unixAddress(path)
  if unlink(path) != 0 && errno != ENOENT { throw posixFail("unlink \(path)") }
  let fd = socket(AF_UNIX, sockStream, 0)
  guard fd >= 0 else { throw posixFail("socket") }
  guard withSockaddr(&addr, { bind(fd, $0, $1) }) == 0 else {
    let e = errno
    close(fd)
    throw posixFail("bind \(path)", e)
  }
  guard listen(fd, 8) == 0 else {
    let e = errno
    close(fd)
    throw posixFail("listen \(path)", e)
  }
  return fd
}

func unixConnect(_ path: String) throws -> Int32 {
  var addr = try unixAddress(path)
  let fd = socket(AF_UNIX, sockStream, 0)
  guard fd >= 0 else { throw posixFail("socket") }
  guard withSockaddr(&addr, { connect(fd, $0, $1) }) == 0 else {
    let e = errno
    close(fd)
    throw posixFail("connect \(path)", e)
  }
  return fd
}

/// The datagram socket of `--net vfkit:PATH`, set up as vfkit does it (pkg/vf/virtionet.go):
/// bound to a path of our own, connected to gvproxy's `-listen-vfkit unixgram://PATH`, buffers of
/// 1 MiB to send and 4 MiB to receive, then the 4 bytes "VFKT" so gvproxy learns our address
/// (gvisor-tap-vsock pkg/transport/unixgram_unix.go reads and drops them).
func vfkitDatagramSocket(local: String, remote: String) throws -> Int32 {
  var localAddr = try unixAddress(local)
  var remoteAddr = try unixAddress(remote)
  if unlink(local) != 0 && errno != ENOENT { throw posixFail("unlink \(local)") }
  let fd = socket(AF_UNIX, sockDgram, 0)
  guard fd >= 0 else { throw posixFail("socket") }
  do {
    guard withSockaddr(&localAddr, { bind(fd, $0, $1) }) == 0 else { throw posixFail("bind \(local)") }
    guard withSockaddr(&remoteAddr, { connect(fd, $0, $1) }) == 0 else {
      throw posixFail("connect \(remote) (is gvproxy running with -listen-vfkit unixgram://\(remote)?)")
    }
    try setIntOption(fd, SO_SNDBUF, 1 * 1024 * 1024)
    try setIntOption(fd, SO_RCVBUF, 4 * 1024 * 1024)
    let magic = Array("VFKT".utf8)
    let n = magic.withUnsafeBytes { write(fd, $0.baseAddress, $0.count) }
    guard n == magic.count else { throw posixFail("send VFKT to \(remote)") }
  } catch {
    close(fd)
    throw error
  }
  return fd
}

func writeAll(_ fd: Int32, _ s: String) throws {
  let bytes = Array(s.utf8)
  var off = 0
  while off < bytes.count {
    let n = bytes[off...].withUnsafeBytes { write(fd, $0.baseAddress, $0.count) }
    if n < 0 {
      if errno == EINTR { continue }
      throw posixFail("write")
    }
    off += n
  }
}

/// Reads up to the first newline (or the end), at most `limit` bytes.
func readLine(_ fd: Int32, limit: Int = 65536) throws -> String {
  var out: [UInt8] = []
  var byte: UInt8 = 0
  while out.count < limit {
    let n = read(fd, &byte, 1)
    if n == 0 { break }
    if n < 0 {
      if errno == EINTR { continue }
      if errno == EAGAIN || errno == EWOULDBLOCK { throw Fail("timed out waiting for a reply") }
      throw posixFail("read")
    }
    if byte == UInt8(ascii: "\n") { break }
    out.append(byte)
  }
  return String(decoding: out, as: UTF8.self)
}

// MARK: - The control socket

/// ctl.sock of a running `silovm run`: one JSON line in, one JSON line out, then the connection
/// closes. Requests: {"cmd":"state"|"request-stop"|"stop"|"pause"|"resume"|"screenshot","path":..}.
final class ControlServer {
  typealias Handler = (_ request: [String: Any], _ reply: @escaping ([(String, J)]) -> Void) -> Void
  let path: String
  private let fd: Int32
  private let source: DispatchSourceRead

  /// The handler runs on the main queue; reply writes the answer and closes the connection.
  init(path: String, handler: @escaping Handler) throws {
    self.path = path
    fd = try unixListen(path)
    let listenFD = fd
    source = DispatchSource.makeReadSource(fileDescriptor: listenFD, queue: .main)
    source.setEventHandler {
      let client = accept(listenFD, nil, nil)
      if client < 0 {
        logErr("ctl: accept failed: \(String(cString: strerror(errno)))")
        return
      }
      DispatchQueue.global().async {
        var request: [String: Any] = [:]
        do {
          try setReceiveTimeout(client, seconds: 5)
          let line = try readLine(client)
          guard let obj = try JSONSerialization.jsonObject(with: Data(line.utf8)) as? [String: Any] else {
            throw Fail("not a JSON object: \(line)")
          }
          request = obj
        } catch {
          answer(client, [("ok", .b(false)), ("error", .s("\(error)"))])
          return
        }
        let req = request
        DispatchQueue.main.async {
          handler(req) { fields in answer(client, fields) }
        }
      }
    }
    source.resume()
  }

  func shutdown() {
    source.cancel()
    close(fd)
    unlink(path)
  }
}

/// Writes one JSON line to a ctl client and closes it.
func answer(_ client: Int32, _ fields: [(String, J)]) {
  do {
    try writeAll(client, encodeJSON(.obj(fields)) + "\n")
  } catch {
    logErr("ctl: could not answer: \(error)")
  }
  close(client)
}

/// silovm ctl --bundle B COMMAND [PATH]
func cmdCtl(_ argv: [String]) throws -> Int32 {
  let a = try Args(argv, valued: ["bundle", "timeout"], flags: [])
  let b = VMBundle(try a.require("bundle"))
  guard let cmd = a.positional.first else {
    throw Fail("ctl needs a command: state, request-stop, stop, pause, resume, screenshot PATH")
  }
  let known = ["state", "request-stop", "stop", "pause", "resume", "screenshot"]
  guard known.contains(cmd) else { throw Fail("unknown ctl command \(cmd); one of \(known.joined(separator: ", "))") }
  var fields: [(String, J)] = [("cmd", .s(cmd))]
  if cmd == "screenshot" {
    guard a.positional.count >= 2 else { throw Fail("ctl screenshot needs a PATH") }
    fields.append(("path", .s(absolutePath(a.positional[1]))))
  }
  guard FileManager.default.fileExists(atPath: b.ctlSocket) else {
    printJSON([("ok", .b(false)), ("error", .s("not running: no \(b.ctlSocket)"))])
    return 1
  }
  let fd = try unixConnect(b.ctlSocket)
  defer { close(fd) }
  try setReceiveTimeout(fd, seconds: try a.int("timeout", default: 60))
  try writeAll(fd, encodeJSON(.obj(fields)) + "\n")
  let reply = try readLine(fd)
  guard !reply.isEmpty else {
    printJSON([("ok", .b(false)), ("error", .s("silovm run closed the connection without answering"))])
    return 1
  }
  printLine(reply)
  if let data = reply.data(using: .utf8),
     let obj = try JSONSerialization.jsonObject(with: data) as? [String: Any],
     let ok = obj["ok"] as? Bool, ok {
    return 0
  }
  return 1
}

let usageText = """
silovm \(siloVMVersion): the Mac test kit's VM helper (see swift/README.md)
  silovm version
  silovm host-check
  silovm ipsw-url
  silovm install --ipsw PATH --out BUNDLE [--disk-gb 80] [--cpus 4] [--memory-gb 8] [--disk-format raw|asif]
  silovm clone --from BUNDLE --to BUNDLE [--new-identity]
  silovm run --bundle BUNDLE [--provision-user NAME --provision-password PW [--provision-full-name TEXT]
             [--no-autologin] [--no-ssh]] [--net nat|none|vfkit:SOCKETPATH] [--gui]
             [--disk-cache automatic|cached|uncached] [--disk-sync full|fsync|none]
             [--mtu N] [--extra-disk PATH[:ro]]... [--max-seconds N] [--continue-on-provisioning-error]
  silovm ctl --bundle BUNDLE state|request-stop|stop|pause|resume|screenshot PATH
  silovm ip --bundle BUNDLE [--wait SECONDS]
"""
