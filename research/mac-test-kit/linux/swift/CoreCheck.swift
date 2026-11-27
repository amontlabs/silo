// Linux check of swift/Core.swift, the part of silovm that imports only Foundation. Built by
// linux/test-vm-helpers.sh with swiftc when a Swift toolchain is on PATH (or in SWIFTC):
//   swiftc -parse-as-library swift/Core.swift linux/swift/CoreCheck.swift -o corecheck
//   corecheck selftest            unit checks; prints "ok N" or the first failure, exit 1
//   corecheck ip|ctl ARGS...      the real `silovm ip` / `silovm ctl` code
//   corecheck ctl-server BUNDLE   a ControlServer answering like `silovm run` (state, stop)
//   corecheck vfkit LOCAL REMOTE  the --net vfkit socket: bind, connect, send VFKT

import Foundation
import Dispatch
#if canImport(Glibc)
import Glibc
#endif

var checks = 0
func check(_ cond: Bool, _ what: String) {
  checks += 1
  if !cond {
    FileHandle.standardError.write(Data("FAIL: \(what)\n".utf8))
    exit(1)
  }
}

func selftest() throws {
  // Arguments
  let a = try Args(["--bundle", "/x.silovm", "--net=vfkit:/s", "--gui", "--extra-disk", "a", "--extra-disk", "b:ro", "pos"],
                   valued: ["bundle", "net", "extra-disk"], flags: ["gui"])
  check(a.value("bundle") == "/x.silovm", "valued option")
  check(a.value("net") == "vfkit:/s", "--name=value")
  check(a.has("gui"), "flag")
  check(a.all("extra-disk") == ["a", "b:ro"], "repeated option")
  check(a.positional == ["pos"], "positional")
  var threw = false
  do { _ = try Args(["--nope"], valued: [], flags: []) } catch { threw = true }
  check(threw, "unknown option is refused")
  threw = false
  do { _ = try Args(["--bundle"], valued: ["bundle"], flags: []) } catch { threw = true }
  check(threw, "missing value is refused")
  threw = false
  do { _ = try Args(["--cpus", "0"], valued: ["cpus"], flags: []).int("cpus", default: 4) } catch { threw = true }
  check(threw, "--cpus 0 is refused")
  threw = false
  do { _ = try Args(["--disk-sync", "sometimes"], valued: ["disk-sync"], flags: []).choice("disk-sync", ["full", "fsync", "none"], default: "full") } catch { threw = true }
  check(threw, "bad choice is refused")

  // JSON
  check(encodeJSON(.obj([("event", .s("x\"y\n")), ("n", .i(3)), ("ok", .b(false)), ("t", .d(1.5)), ("z", .null)]))
        == "{\"event\":\"x\\\"y\\n\",\"n\":3,\"ok\":false,\"t\":1.500,\"z\":null}", "JSON encoding")
  let parsed = try JSONSerialization.jsonObject(with: Data(encodeJSON(.obj([("s", .s("é\u{1}\\"))])).utf8)) as? [String: Any]
  check((parsed?["s"] as? String) == "é\u{1}\\", "JSON round trip")

  // vsock bridges
  let vin = try VsockBridgeSpec.parse("22:/tmp/ssh.sock", inward: true)
  check(vin == VsockBridgeSpec(port: 22, path: "/tmp/ssh.sock", inward: true), "--vsock-in PORT:PATH")
  for bad in ["22", "0:/x", "x:/x", "22:", String(repeating: "a", count: 120)] {
    threw = false
    do { _ = try VsockBridgeSpec.parse(bad, inward: false) } catch { threw = true }
    check(threw, "--vsock-out \(bad.prefix(12)) is refused")
  }
  // splice: two socket pairs joined, bytes both ways and the ends passed on.
  var p1: [Int32] = [0, 0], p2: [Int32] = [0, 0]
  check(socketpair(AF_UNIX, sockStream, 0, &p1) == 0 && socketpair(AF_UNIX, sockStream, 0, &p2) == 0, "socketpair")
  let finished = DispatchSemaphore(value: 0)
  var copied: (UInt64, UInt64) = (0, 0)
  splice(p1[1], p2[0]) { ab, ba in copied = (ab, ba); finished.signal() }
  let big = [UInt8](repeating: 7, count: 300_000)
  let w0 = p1[0]
  let writer = Thread {
    var off = 0
    while off < big.count {
      let n = big[off...].withUnsafeBytes { write(w0, $0.baseAddress, $0.count) }
      if n <= 0 { break }
      off += n
    }
    _ = shutdown(w0, Int32(SHUT_WR))
  }
  writer.start()
  var got = 0
  var buf = [UInt8](repeating: 0, count: 65536)
  while true {
    let n = buf.withUnsafeMutableBytes { read(p2[1], $0.baseAddress, $0.count) }
    if n <= 0 { break }
    got += n
  }
  check(got == big.count, "splice carried \(got) of \(big.count) bytes and passed the end on")
  try writeAll(p2[1], "back\n")
  _ = shutdown(p2[1], Int32(SHUT_WR))
  check(try readLine(p1[0]) == "back", "splice carries the other way after one way ended")
  check(finished.wait(timeout: .now() + 5) == .success, "splice ends when both ways end")
  check(copied == (UInt64(big.count), 5), "splice counts \(copied)")
  close(p1[0]); close(p2[1])

  // MAC addresses and leases
  check(normaliseMAC("1,a:b:c:d:e:f") == "0a:0b:0c:0d:0e:0f", "lease MAC without zeros")
  check(normaliseMAC("1A:2b:03:04:5:60") == "1a:2b:03:04:05:60", "mixed MAC")
  check(normaliseMAC("1,a:b:c") == nil, "short MAC")
  check(normaliseMAC("zz:b:c:d:e:f") == nil, "not hex")
  let leases = """
  {
  \tname=old
  \tip_address=192.168.64.5
  \thw_address=1,b6:e:4:c0:1:2
  \tidentifier=1,b6:e:4:c0:1:2
  \tlease=0x10
  }
  {
  \tname=Mac
  \tip_address=192.168.64.7
  \thw_address=1,b6:e:4:c0:1:2
  \tidentifier=1,b6:e:4:c0:1:2
  \tlease=0x7fffffff
  }
  {
  \tname=other
  \tip_address=192.168.64.9
  \thw_address=1,2:0:0:0:0:1
  \tlease=0x7ffffff0
  }
  """
  check(parseLeases(leases).count == 3, "three leases")
  check(leaseIP(for: "b6:0e:04:c0:01:02", in: leases, at: 1000) == "192.168.64.7", "newest unexpired lease")
  check(leaseIP(for: "b6:0e:04:c0:01:02", in: leases, at: Double(0x7fffffff) + 1) == nil, "expired leases ignored")
  check(leaseIP(for: "02:00:00:00:00:01", in: leases, at: 1000) == "192.168.64.9", "other MAC")
  check(leaseIP(for: "02:00:00:00:00:02", in: leases, at: 1000) == nil, "unknown MAC")

  // Error names
  let e = NSError(domain: "VZErrorDomain", code: 40002, userInfo: [NSLocalizedDescriptionKey: "user name 'x' is not valid"])
  let info = ErrorInfo(e)
  check(info.name == "guestProvisioningInvalidUsername" && info.isProvisioning, "provisioning error name")
  check(info.text.hasPrefix("VZErrorDomain 40002 guestProvisioningInvalidUsername: "), "error text")
  check(ErrorInfo(NSError(domain: "NSPOSIXErrorDomain", code: 2)).name == nil, "no name outside VZErrorDomain")
  check(ErrorInfo(Fail("x")).domain == "silovm", "Fail domain")

  // VMBundle config
  let dir = NSTemporaryDirectory() + "corecheck-\(getpid()).silovm"
  try FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
  defer {
    do { try FileManager.default.removeItem(atPath: dir) } catch { FileHandle.standardError.write(Data("cleanup: \(error)\n".utf8)) }
  }
  let b = VMBundle(dir)
  try BundleConfig(mac: "b6:0e:04:c0:01:02", cpus: 4, memoryGB: 8, diskFormat: "raw").save(b)
  let raw = try JSONSerialization.jsonObject(with: try readFile(b.config)) as? [String: Any]
  check(raw?["memory_gb"] as? Int == 8 && raw?["disk_format"] as? String == "raw", "config.json keys")
  check(try BundleConfig.load(b).mac == "b6:0e:04:c0:01:02", "config round trip")

  // Socket path limit
  threw = false
  do { _ = try unixAddress("/" + String(repeating: "x", count: 200)) } catch { threw = true }
  check(threw, "long socket path refused")
}

func ctlServer(_ bundlePath: String) throws {
  let b = VMBundle(bundlePath)
  var server: ControlServer? = nil
  server = try ControlServer(path: b.ctlSocket) { req, reply in
    let cmd = req["cmd"] as? String ?? ""
    switch cmd {
    case "state": reply([("ok", .b(true)), ("state", .s("running"))])
    case "stop":
      reply([("ok", .b(true))])
      server?.shutdown()
      exit(0)
    case "screenshot": reply([("ok", .b(true)), ("path", .s(req["path"] as? String ?? ""))])
    default: reply([("ok", .b(false)), ("error", .s("unknown command \(cmd)"))])
    }
  }
  printLine("listening")
  dispatchMain()
}

@main
struct CoreCheck {
  static func main() {
    let argv = Array(CommandLine.arguments.dropFirst())
    do {
      switch argv.first ?? "" {
      case "selftest":
        try selftest()
        printLine("ok \(checks)")
        exit(0)
      case "ip": exit(try cmdIP(Array(argv.dropFirst())))
      case "ctl": exit(try cmdCtl(Array(argv.dropFirst())))
      case "ctl-server": try ctlServer(argv[1])
      case "vfkit":
        let fd = try vfkitDatagramSocket(local: argv[1], remote: argv[2])
        printLine("sent VFKT on fd \(fd)")
        exit(0)
      default:
        FileHandle.standardError.write(Data("usage: corecheck selftest|ip|ctl|ctl-server|vfkit\n".utf8))
        exit(2)
      }
    } catch {
      FileHandle.standardError.write(Data("error: \(error)\n".utf8))
      exit(1)
    }
  }
}
