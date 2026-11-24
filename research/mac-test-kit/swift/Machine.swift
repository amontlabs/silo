// The parts of silovm that use Apple's Virtualization framework: host-check, ipsw-url, install,
// clone and run. Every symbol here was checked against Apple's documentation and the macOS 27
// SDK's headers (as scanned by pyobjc), and against Tart (cirruslabs/tart) and VirtualBuddy, which
// use the same calls. The guest provisioning API exists only in the macOS 27 SDK and on macOS 27,
// so it sits behind `#if compiler(>=6.4)` (as in Tart) and `if #available(macOS 27, *)`.

import AppKit
import Darwin
import Foundation
import Virtualization

// macOS guests run only on Apple silicon; SILOVM_STUBCHECK is linux/test-vm-helpers.sh's typecheck.
#if arch(arm64) || SILOVM_STUBCHECK

// MARK: - The host

func sysctlString(_ name: String) -> String {
  var size = 0
  guard sysctlbyname(name, nil, &size, nil, 0) == 0, size > 0 else { return "" }
  var buf = [CChar](repeating: 0, count: size)
  guard sysctlbyname(name, &buf, &size, nil, 0) == 0 else { return "" }
  return String(decoding: buf.prefix { $0 != 0 }.map { UInt8(bitPattern: $0) }, as: UTF8.self)
}

/// True when this build has the provisioning API (macOS 27 SDK) and this Mac runs macOS 27.
func provisioningAPIAvailable() -> Bool {
  #if compiler(>=6.4)
  if #available(macOS 27, *) { return true }
  #endif
  return false
}

func provisioningInSDK() -> Bool {
  #if compiler(>=6.4)
  return true
  #else
  return false
  #endif
}

@available(macOS 14, *)
func cmdHostCheck() -> Int32 {
  let v = ProcessInfo.processInfo.operatingSystemVersion
  var fields: [(String, J)] = [
    ("macos", .s("\(v.majorVersion).\(v.minorVersion).\(v.patchVersion)")),
    ("build", .s(sysctlString("kern.osversion"))),
    ("chip", .s(sysctlString("machdep.cpu.brand_string"))),
    ("virtualization_supported", .b(VZVirtualMachine.isSupported)),
    ("provisioning_api", .b(provisioningAPIAvailable())),
    ("provisioning_in_sdk", .b(provisioningInSDK())),
    ("max_cpus", .i(VZVirtualMachineConfiguration.maximumAllowedCPUCount)),
    ("max_memory_gb", .i(Int(VZVirtualMachineConfiguration.maximumAllowedMemorySize >> 30))),
    ("silovm", .s(siloVMVersion)),
  ]
  #if arch(arm64)
  fields.append(("arch", .s("arm64")))
  #else
  fields.append(("arch", .s("x86_64")))
  #endif
  printJSON(fields)
  return 0
}

// MARK: - ipsw-url

@available(macOS 14, *)
func cmdIPSWURL() {
  VZMacOSRestoreImage.fetchLatestSupported { result in
    DispatchQueue.main.async {
      switch result {
      case .success(let image):
        let v = image.operatingSystemVersion
        printJSON([("url", .s(image.url.absoluteString)), ("build", .s(image.buildVersion)),
                   ("os_version", .s("\(v.majorVersion).\(v.minorVersion).\(v.patchVersion)"))])
        exit(0)
      case .failure(let error):
        let info = ErrorInfo(error)
        printJSON([("ok", .b(false))] + info.fields)
        logErr("fetchLatestSupported failed: \(info.text)")
        exit(1)
      }
    }
  }
}

// MARK: - The machine's configuration

enum NetChoice {
  case nat
  case none
  case vfkit(String)

  static func parse(_ s: String) throws -> NetChoice {
    if s == "nat" { return .nat }
    if s == "none" { return .none }
    if s.hasPrefix("vfkit:") {
      let p = String(s.dropFirst("vfkit:".count))
      guard !p.isEmpty else { throw Fail("--net vfkit:SOCKETPATH needs a path") }
      return .vfkit(absolutePath(p))
    }
    throw Fail("--net must be nat, none or vfkit:SOCKETPATH, got \(s)")
  }

  var name: String {
    switch self {
    case .nat: return "nat"
    case .none: return "none"
    case .vfkit(let p): return "vfkit:\(p)"
    }
  }
}

@available(macOS 14, *)
struct DiskModes {
  var cacheName = "automatic"
  var syncName = "full"

  var caching: VZDiskImageCachingMode {
    switch cacheName {
    case "cached": return .cached
    case "uncached": return .uncached
    default: return .automatic
    }
  }

  var sync: VZDiskImageSynchronizationMode {
    switch syncName {
    case "fsync": return .fsync
    case "none": return VZDiskImageSynchronizationMode.none
    default: return .full
    }
  }
}

@available(macOS 14, *)
func loadHardwareModel(_ b: VMBundle) throws -> VZMacHardwareModel {
  guard let hw = VZMacHardwareModel(dataRepresentation: try readFile(b.hardwareModel)) else {
    throw Fail("\(b.hardwareModel) is not a hardware model this macOS can read")
  }
  guard hw.isSupported else {
    throw Fail("the bundle's hardware model is not supported on this Mac (VZMacHardwareModel.isSupported is false)")
  }
  return hw
}

/// The virtual machine of a bundle. The vfkit socket's file handle lives as long as the
/// configuration's attachment.
@available(macOS 14, *)
func makeConfiguration(_ b: VMBundle, _ cfg: BundleConfig, net: NetChoice, disk: DiskModes,
                       extraDisks: [(String, Bool)], mtu: Int? = nil) throws -> VZVirtualMachineConfiguration {
  let c = VZVirtualMachineConfiguration()
  c.bootLoader = VZMacOSBootLoader()

  let platform = VZMacPlatformConfiguration()
  platform.hardwareModel = try loadHardwareModel(b)
  guard let mid = VZMacMachineIdentifier(dataRepresentation: try readFile(b.machineID)) else {
    throw Fail("\(b.machineID) is not a machine identifier")
  }
  platform.machineIdentifier = mid
  guard FileManager.default.fileExists(atPath: b.aux) else { throw Fail("no auxiliary storage at \(b.aux)") }
  platform.auxiliaryStorage = VZMacAuxiliaryStorage(url: URL(fileURLWithPath: b.aux))
  c.platform = platform

  c.cpuCount = min(max(cfg.cpus, VZVirtualMachineConfiguration.minimumAllowedCPUCount),
                   VZVirtualMachineConfiguration.maximumAllowedCPUCount)
  let mem = UInt64(cfg.memoryGB) << 30
  c.memorySize = min(max(mem, VZVirtualMachineConfiguration.minimumAllowedMemorySize),
                     VZVirtualMachineConfiguration.maximumAllowedMemorySize)

  // A macOS guest needs a display even with no window; Apple's sample uses 1920x1200 at 80 ppi.
  let graphics = VZMacGraphicsDeviceConfiguration()
  graphics.displays = [VZMacGraphicsDisplayConfiguration(widthInPixels: 1920, heightInPixels: 1200, pixelsPerInch: 80)]
  c.graphicsDevices = [graphics]
  c.keyboards = [VZUSBKeyboardConfiguration(), VZMacKeyboardConfiguration()]
  c.pointingDevices = [VZUSBScreenCoordinatePointingDeviceConfiguration(), VZMacTrackpadConfiguration()]
  c.entropyDevices = [VZVirtioEntropyDeviceConfiguration()]

  let mainDisk = try VZDiskImageStorageDeviceAttachment(url: URL(fileURLWithPath: b.disk), readOnly: false,
                                                        cachingMode: disk.caching, synchronizationMode: disk.sync)
  var storage: [VZStorageDeviceConfiguration] = [VZVirtioBlockDeviceConfiguration(attachment: mainDisk)]
  for (path, ro) in extraDisks {
    guard FileManager.default.fileExists(atPath: path) else { throw Fail("no disk image at \(path)") }
    let a = try VZDiskImageStorageDeviceAttachment(url: URL(fileURLWithPath: path), readOnly: ro,
                                                   cachingMode: disk.caching, synchronizationMode: disk.sync)
    storage.append(VZVirtioBlockDeviceConfiguration(attachment: a))
  }
  c.storageDevices = storage

  switch net {
  case .none:
    c.networkDevices = []
  case .nat, .vfkit:
    guard let mac = VZMACAddress(string: cfg.mac) else { throw Fail("config.json has no valid mac: \(cfg.mac)") }
    let dev = VZVirtioNetworkDeviceConfiguration()
    dev.macAddress = mac
    if case .vfkit(let remote) = net {
      let fd = try vfkitDatagramSocket(local: b.netSocket, remote: remote)
      let att = VZFileHandleNetworkDeviceAttachment(fileHandle: FileHandle(fileDescriptor: fd, closeOnDealloc: true))
      // Apple: 1500 by default, 1500 to 65535 allowed (macOS 13 or later).
      if let m = mtu { att.maximumTransmissionUnit = m }
      dev.attachment = att
    } else {
      dev.attachment = VZNATNetworkDeviceAttachment()
    }
    c.networkDevices = [dev]
  }

  try c.validate()
  return c
}

// MARK: - install

func createDisk(_ path: String, gb: Int, format: String) throws {
  if format == "raw" {
    let fd = open(path, O_RDWR | O_CREAT | O_EXCL, 0o600)
    guard fd >= 0 else { throw posixFail("create \(path)") }
    defer { close(fd) }
    guard ftruncate(fd, off_t(gb) << 30) == 0 else { throw posixFail("ftruncate \(path)") }
    return
  }
  // ASIF: the command of Apple's VZDiskImageStorageDeviceAttachment page (macOS 26 or later).
  let p = Process()
  p.executableURL = URL(fileURLWithPath: "/usr/sbin/diskutil")
  p.arguments = ["image", "create", "blank", "--fs", "none", "--format", "ASIF", "--size", "\(gb)GiB", path]
  let out = Pipe()
  p.standardOutput = out
  p.standardError = out
  do {
    try p.run()
  } catch {
    throw Fail("cannot run diskutil: \(error.localizedDescription)")
  }
  let text = String(decoding: out.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
  p.waitUntilExit()
  guard p.terminationStatus == 0 else {
    throw Fail("diskutil image create blank --format ASIF failed (exit \(p.terminationStatus)); ASIF needs macOS 26 or later: \(text)")
  }
  if !FileManager.default.fileExists(atPath: path) && FileManager.default.fileExists(atPath: path + ".asif") {
    do {
      try FileManager.default.moveItem(atPath: path + ".asif", toPath: path)
    } catch {
      throw Fail("diskutil wrote \(path).asif and it could not be renamed: \(error.localizedDescription)")
    }
  }
  guard FileManager.default.fileExists(atPath: path) else { throw Fail("diskutil reported success but \(path) is missing: \(text)") }
}

/// The last percentage the installer printed. A class, because the KVO change handler may be
/// @Sendable, and a @Sendable closure cannot mutate a captured var.
final class PercentBox {
  var last = -1
}

/// Kept alive while the installer runs.
var installer: AnyObject?
var installObservation: NSKeyValueObservation?
var installMachine: AnyObject?

@available(macOS 14, *)
func cmdInstall(_ argv: [String]) throws {
  let a = try Args(argv, valued: ["ipsw", "out", "disk-gb", "cpus", "memory-gb", "disk-format"], flags: [])
  let ipsw = absolutePath(try a.require("ipsw"))
  let outPath = absolutePath(try a.require("out"))
  let diskGB = try a.int("disk-gb", default: 80, min: 20)
  let cpus = try a.int("cpus", default: 4)
  let memGB = try a.int("memory-gb", default: 8)
  let format = try a.choice("disk-format", ["raw", "asif"], default: "raw")
  guard FileManager.default.fileExists(atPath: ipsw) else { throw Fail("no restore image at \(ipsw)") }
  guard !FileManager.default.fileExists(atPath: outPath) else { throw Fail("\(outPath) exists; delete it first") }

  // Installed into OUT.partial and renamed at the end, so a failed install never looks finished.
  let partial = VMBundle(outPath + ".partial")
  if FileManager.default.fileExists(atPath: partial.path) {
    try FileManager.default.removeItem(atPath: partial.path)
  }
  try FileManager.default.createDirectory(atPath: partial.path, withIntermediateDirectories: true)

  let t0 = now()
  logErr("loading \(ipsw)")
  VZMacOSRestoreImage.load(from: URL(fileURLWithPath: ipsw)) { result in
    DispatchQueue.main.async {
      do {
        let image: VZMacOSRestoreImage
        switch result {
        case .success(let i): image = i
        case .failure(let e): throw e
        }
        let v = image.operatingSystemVersion
        let osVersion = "\(v.majorVersion).\(v.minorVersion).\(v.patchVersion)"
        guard let req = image.mostFeaturefulSupportedConfiguration else {
          throw Fail("this Mac cannot run macOS \(osVersion) (\(image.buildVersion)): no supported configuration; update macOS on the Mac")
        }
        guard req.hardwareModel.isSupported else { throw Fail("the restore image's hardware model is not supported on this Mac") }

        try req.hardwareModel.dataRepresentation.write(to: URL(fileURLWithPath: partial.hardwareModel))
        try VZMacMachineIdentifier().dataRepresentation.write(to: URL(fileURLWithPath: partial.machineID))
        _ = try VZMacAuxiliaryStorage(creatingStorageAt: URL(fileURLWithPath: partial.aux),
                                      hardwareModel: req.hardwareModel, options: [])
        try createDisk(partial.disk, gb: diskGB, format: format)
        let minMemGB = Int((req.minimumSupportedMemorySize + (1 << 30) - 1) >> 30)
        let cfg = BundleConfig(mac: VZMACAddress.randomLocallyAdministered().string,
                               cpus: max(cpus, req.minimumSupportedCPUCount),
                               memoryGB: max(memGB, minMemGB), diskFormat: format)
        try cfg.save(partial)

        let conf = try makeConfiguration(partial, cfg, net: .nat, disk: DiskModes(), extraDisks: [])
        let vm = VZVirtualMachine(configuration: conf)
        installMachine = vm
        let inst = VZMacOSInstaller(virtualMachine: vm, restoringFromImageAt: URL(fileURLWithPath: ipsw))
        installer = inst
        let lastPercent = PercentBox()
        installObservation = inst.progress.observe(\.fractionCompleted, options: [.new]) { p, _ in
          let pct = Int(p.fractionCompleted * 100)
          if pct != lastPercent.last {
            lastPercent.last = pct
            logErr("install \(pct)%")
          }
        }
        let tInstall = now()
        logErr("installing macOS \(osVersion) (\(image.buildVersion)) into \(outPath)")
        inst.install { result in
          DispatchQueue.main.async {
            installObservation = nil
            switch result {
            case .success:
              do {
                try FileManager.default.moveItem(atPath: partial.path, toPath: outPath)
              } catch {
                printJSON([("ok", .b(false)), ("error", .s("installed, but could not rename \(partial.path): \(error.localizedDescription)"))])
                exit(1)
              }
              printJSON([("ok", .b(true)), ("bundle", .s(outPath)), ("os_version", .s(osVersion)),
                         ("build", .s(image.buildVersion)), ("disk_format", .s(format)), ("disk_gb", .i(diskGB)),
                         ("install_seconds", .d(now() - tInstall)), ("total_seconds", .d(now() - t0))])
              exit(0)
            case .failure(let error):
              let info = ErrorInfo(error)
              logErr("install failed: \(info.text)")
              printJSON([("ok", .b(false)), ("stage", .s("install"))] + info.fields)
              exit(1)
            }
          }
        }
      } catch {
        let info = ErrorInfo(error)
        logErr("install failed: \(info.text)")
        printJSON([("ok", .b(false)), ("stage", .s("prepare"))] + info.fields)
        exit(1)
      }
    }
  }
}

// MARK: - clone

@available(macOS 14, *)
func cmdClone(_ argv: [String]) throws -> Int32 {
  let a = try Args(argv, valued: ["from", "to"], flags: ["new-identity"])
  let from = VMBundle(try a.require("from"))
  let to = VMBundle(try a.require("to"))
  try from.mustExist()
  guard !FileManager.default.fileExists(atPath: to.path) else { throw Fail("\(to.path) exists; delete it first") }
  let t0 = now()
  try FileManager.default.createDirectory(atPath: to.path, withIntermediateDirectories: true)
  do {
    for name in try FileManager.default.contentsOfDirectory(atPath: from.path).sorted() {
      if VMBundle.runFiles.contains(name) { continue }
      let src = from.file(name)
      var isDir: ObjCBool = false
      _ = FileManager.default.fileExists(atPath: src, isDirectory: &isDir)
      if isDir.boolValue { throw Fail("\(src) is a folder; a bundle holds only files") }
      guard clonefile(src, to.file(name), 0) == 0 else {
        throw posixFail("clonefile \(src) (both bundles must be on the same APFS volume)")
      }
    }
    if a.has("new-identity") {
      let hw = try loadHardwareModel(from)
      try VZMacMachineIdentifier().dataRepresentation.write(to: URL(fileURLWithPath: to.machineID), options: .atomic)
      _ = try VZMacAuxiliaryStorage(creatingStorageAt: URL(fileURLWithPath: to.aux), hardwareModel: hw,
                                    options: [.allowOverwrite])
      var cfg = try BundleConfig.load(to)
      cfg.mac = VZMACAddress.randomLocallyAdministered().string
      try cfg.save(to)
    }
  } catch {
    do {
      try FileManager.default.removeItem(atPath: to.path)
    } catch let cleanup {
      logErr("could not remove the half-made \(to.path): \(cleanup.localizedDescription)")
    }
    throw error
  }
  let cfg = try BundleConfig.load(to)
  printJSON([("ok", .b(true)), ("to", .s(to.path)), ("new_identity", .b(a.has("new-identity"))),
             ("mac", .s(cfg.mac)), ("seconds", .d(now() - t0))])
  return 0
}

// MARK: - run

struct ProvisionRequest {
  var user: String
  var password: String
  var fullName: String
  var autologin: Bool
  var ssh: Bool
}

@available(macOS 14, *)
func stateName(_ s: VZVirtualMachine.State) -> String {
  switch s {
  case .stopped: return "stopped"
  case .running: return "running"
  case .paused: return "paused"
  case .error: return "error"
  case .starting: return "starting"
  case .pausing: return "pausing"
  case .resuming: return "resuming"
  case .stopping: return "stopping"
  case .saving: return "saving"
  case .restoring: return "restoring"
  @unknown default: return "unknown-\(s.rawValue)"
  }
}

func emit(_ event: String, _ fields: [(String, J)]) {
  printJSON([("event", .s(event))] + fields)
}

/// One `silovm run`. Everything runs on the main queue, which the framework requires for a machine
/// made with VZVirtualMachine(configuration:). Runner does not conform to NSWindowDelegate: that
/// protocol is @MainActor, and in Swift 5 mode a class conforming to it is inferred @MainActor,
/// which would make the calls from cmdRun and the control socket's handler cross-actor calls. The
/// window's closing is observed through NotificationCenter instead.
@available(macOS 14, *)
final class Runner: NSObject, VZVirtualMachineDelegate {
  let bundle: VMBundle
  let vm: VZVirtualMachine
  let provision: ProvisionRequest?
  let continueOnProvisioningError: Bool
  let maxSeconds: Int?
  let gui: Bool
  var control: ControlServer?
  var stateObservation: NSKeyValueObservation?
  var signalSources: [DispatchSourceSignal] = []
  var window: NSWindow?
  var view: VZVirtualMachineView?
  var windowCloseObserver: NSObjectProtocol?
  var stopRequested = false
  var finished = false
  var startedAt: Double?
  var provisioned = false

  init(bundle: VMBundle, vm: VZVirtualMachine, provision: ProvisionRequest?, continueOnProvisioningError: Bool,
       maxSeconds: Int?, gui: Bool) {
    self.bundle = bundle
    self.vm = vm
    self.provision = provision
    self.continueOnProvisioningError = continueOnProvisioningError
    self.maxSeconds = maxSeconds
    self.gui = gui
    super.init()
    vm.delegate = self
  }

  func begin() throws {
    stateObservation = vm.observe(\.state, options: [.new]) { [weak self] machine, _ in
      let s = machine.state
      DispatchQueue.main.async {
        guard let self = self, !self.finished else { return }
        emit("state", [("value", .s(stateName(s))), ("t", .d(now()))])
      }
    }
    control = try ControlServer(path: bundle.ctlSocket) { [weak self] req, reply in
      guard let self = self else { return }
      self.handle(req, reply)
    }
    for sig in [SIGTERM, SIGINT] {
      signal(sig, SIG_IGN)
      let src = DispatchSource.makeSignalSource(signal: sig, queue: .main)
      src.setEventHandler { [weak self] in self?.onSignal(sig) }
      src.resume()
      signalSources.append(src)
    }
    if gui { openWindow() }
    start()
  }

  // MARK: Starting, with provisioning

  func provisioningFailed(_ stage: String, _ info: ErrorInfo) -> Bool {
    emit("provisioning", [("stage", .s(stage)), ("ok", .b(false))] + info.fields)
    logErr("provisioning refused at \(stage): \(info.text)")
    if continueOnProvisioningError {
      logErr("starting without provisioning (--continue-on-provisioning-error)")
      return true
    }
    finish("error", error: "provisioning refused at \(stage): \(info.text)", code: 3)
    return false
  }

  func start() {
    let options = VZMacOSVirtualMachineStartOptions()
    var withProvisioning = false
    if let p = provision {
      #if compiler(>=6.4)
      if #available(macOS 27, *) {
        let o = VZMacGuestProvisioningOptions()
        o.fullName = p.fullName
        o.username = p.user
        o.password = p.password
        o.logsInAutomatically = p.autologin
        o.enablesRemoteLogin = p.ssh
        var ok = true
        do {
          try o.validate()
          emit("provisioning", [("stage", .s("validate")), ("ok", .b(true))])
        } catch {
          ok = false
          if !provisioningFailed("validate", ErrorInfo(error)) { return }
        }
        if ok {
          do {
            try options.setGuestProvisioning(o)
            emit("provisioning", [("stage", .s("set")), ("ok", .b(true))])
            withProvisioning = true
          } catch {
            if !provisioningFailed("set", ErrorInfo(error)) { return }
          }
        }
      } else {
        let info = ErrorInfo(Fail("guest provisioning needs macOS 27 or later on this Mac"))
        if !provisioningFailed("validate", info) { return }
      }
      #else
      let info = ErrorInfo(Fail("this silovm was built with an SDK older than macOS 27, which has no guest provisioning; install the Command Line Tools for macOS 27 and run ./kit build"))
      if !provisioningFailed("validate", info) { return }
      #endif
    }
    startMachine(options, withProvisioning: withProvisioning)
  }

  func startMachine(_ options: VZMacOSVirtualMachineStartOptions, withProvisioning: Bool) {
    vm.start(options: options) { [weak self] error in
      guard let self = self else { return }
      if let error = error {
        let info = ErrorInfo(error)
        if withProvisioning && info.isProvisioning {
          if self.provisioningFailed("start", info) {
            self.startMachine(VZMacOSVirtualMachineStartOptions(), withProvisioning: false)
          }
          return
        }
        logErr("start failed: \(info.text)")
        self.finish("error", error: info.text, code: 1, info: info)
        return
      }
      if withProvisioning {
        self.provisioned = true
        emit("provisioning", [("stage", .s("start")), ("ok", .b(true))])
      }
      let t = now()
      self.startedAt = t
      emit("started", [("t", .d(t)), ("provisioned", .b(withProvisioning))])
      if let n = self.maxSeconds {
        DispatchQueue.main.asyncAfter(deadline: .now() + .seconds(n)) { [weak self] in
          guard let self = self, !self.finished else { return }
          logErr("--max-seconds \(n) reached: forcing a stop")
          self.forceStop("max-seconds", reply: nil)
        }
      }
    }
  }

  // MARK: Stopping

  func finish(_ how: String, error: String? = nil, code: Int32 = 0, info: ErrorInfo? = nil) {
    if finished { return }
    finished = true
    var f: [(String, J)] = [("how", .s(how))]
    if let e = error { f.append(("error", .s(e))) }
    if let i = info { f += [("error_domain", .s(i.domain)), ("error_code", .i(i.code)), ("error_name", i.name.map { J.s($0) } ?? J.null)] }
    f.append(("t", .d(now())))
    emit("stopped", f)
    control?.shutdown()
    if unlink(bundle.netSocket) != 0 && errno != ENOENT {
      logErr("could not remove \(bundle.netSocket): \(String(cString: strerror(errno)))")
    }
    exit(code)
  }

  func requestStop() throws {
    try vm.requestStop()
    stopRequested = true
  }

  func forceStop(_ how: String, reply: (([(String, J)]) -> Void)?) {
    guard vm.canStop else {
      reply?([("ok", .b(false)), ("error", .s("cannot stop in state \(stateName(vm.state))"))])
      if vm.state == .stopped || vm.state == .error { finish(how) }
      return
    }
    vm.stop { [weak self] error in
      if let error = error {
        let info = ErrorInfo(error)
        reply?([("ok", .b(false))] + info.fields)
        logErr("stop failed: \(info.text)")
        return
      }
      reply?([("ok", .b(true))])
      self?.finish(how)
    }
  }

  func onSignal(_ sig: Int32) {
    if stopRequested || !vm.canRequestStop {
      logErr("signal \(sig): forcing a stop")
      forceStop("forced", reply: nil)
      return
    }
    do {
      try requestStop()
      logErr("signal \(sig): asked the guest to stop (send it again to force)")
    } catch {
      logErr("signal \(sig): requestStop failed (\(ErrorInfo(error).text)): forcing a stop")
      forceStop("forced", reply: nil)
    }
  }

  func guestDidStop(_ virtualMachine: VZVirtualMachine) {
    finish(stopRequested ? "requested" : "guest")
  }

  func virtualMachine(_ virtualMachine: VZVirtualMachine, didStopWithError error: Error) {
    let info = ErrorInfo(error)
    logErr("stopped with an error: \(info.text)")
    finish("error", error: info.text, code: 1, info: info)
  }

  func virtualMachine(_ virtualMachine: VZVirtualMachine, networkDevice: VZNetworkDevice,
                      attachmentWasDisconnectedWithError error: Error) {
    let info = ErrorInfo(error)
    logErr("network attachment disconnected: \(info.text)")
    emit("network", [("ok", .b(false))] + info.fields)
  }

  // MARK: The control socket

  func handle(_ req: [String: Any], _ reply: @escaping ([(String, J)]) -> Void) {
    let cmd = req["cmd"] as? String ?? ""
    switch cmd {
    case "state":
      reply([("ok", .b(true)), ("state", .s(stateName(vm.state))), ("provisioned", .b(provisioned)),
             ("started_t", startedAt.map { J.d($0) } ?? J.null), ("can_request_stop", .b(vm.canRequestStop))])
    case "request-stop":
      do {
        try requestStop()
        reply([("ok", .b(true))])
      } catch {
        reply([("ok", .b(false))] + ErrorInfo(error).fields)
      }
    case "stop":
      forceStop("forced", reply: reply)
    case "pause":
      vm.pause { result in
        switch result {
        case .success: reply([("ok", .b(true))])
        case .failure(let e): reply([("ok", .b(false))] + ErrorInfo(e).fields)
        }
      }
    case "resume":
      vm.resume { result in
        switch result {
        case .success: reply([("ok", .b(true))])
        case .failure(let e): reply([("ok", .b(false))] + ErrorInfo(e).fields)
        }
      }
    case "screenshot":
      guard let path = req["path"] as? String else {
        reply([("ok", .b(false)), ("error", .s("screenshot needs a path"))])
        return
      }
      do {
        reply([("ok", .b(true))] + (try screenshot(path)))
      } catch {
        reply([("ok", .b(false))] + ErrorInfo(error).fields)
      }
    default:
      reply([("ok", .b(false)), ("error", .s("unknown command \(cmd)"))])
    }
  }

  // MARK: The window

  func openWindow() {
    let app = NSApplication.shared
    app.setActivationPolicy(.regular)
    let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1440, height: 900),
                     styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
    w.title = "silovm: " + (bundle.path as NSString).lastPathComponent
    w.isReleasedWhenClosed = false
    let v = VZVirtualMachineView()
    v.virtualMachine = vm
    v.capturesSystemKeys = false
    v.automaticallyReconfiguresDisplay = false
    w.contentView = v
    w.center()
    w.makeKeyAndOrderFront(nil)
    // Activation works only once the application has finished launching, inside app.run(); the
    // main queue runs this block then (Tart activates in applicationDidFinishLaunching).
    DispatchQueue.main.async { NSApplication.shared.activate() }
    windowCloseObserver = NotificationCenter.default.addObserver(
      forName: NSWindow.willCloseNotification, object: w, queue: .main
    ) { [weak self] _ in
      guard let self = self, !self.finished else { return }
      self.onSignal(SIGTERM)
    }
    window = w
    view = v
  }

  /// A PNG of the window's view. VZVirtualMachineView may draw through a layer that this does not
  /// capture; "uniform": true says the picture is one colour, so probably not the screen.
  func screenshot(_ path: String) throws -> [(String, J)] {
    guard let v = view else { throw Fail("screenshot needs silovm run --gui") }
    guard let rep = v.bitmapImageRepForCachingDisplay(in: v.bounds) else { throw Fail("cannot make a bitmap of the view") }
    v.cacheDisplay(in: v.bounds, to: rep)
    guard let png = rep.representation(using: .png, properties: [:]) else { throw Fail("cannot encode PNG") }
    try png.write(to: URL(fileURLWithPath: path))
    var uniform = true
    if rep.pixelsWide > 0 && rep.pixelsHigh > 0, let first = rep.colorAt(x: 0, y: 0) {
      let stepX = max(1, rep.pixelsWide / 16), stepY = max(1, rep.pixelsHigh / 16)
      outer: for y in stride(from: 0, to: rep.pixelsHigh, by: stepY) {
        for x in stride(from: 0, to: rep.pixelsWide, by: stepX) {
          if let c = rep.colorAt(x: x, y: y), c != first { uniform = false; break outer }
        }
      }
    }
    return [("path", .s(path)), ("width", .i(rep.pixelsWide)), ("height", .i(rep.pixelsHigh)), ("uniform", .b(uniform))]
  }
}

/// Kept alive while the machine runs.
var runner: AnyObject?

@available(macOS 14, *)
func cmdRun(_ argv: [String]) throws {
  let a = try Args(argv,
                   valued: ["bundle", "provision-user", "provision-password", "provision-full-name", "net",
                            "disk-cache", "disk-sync", "extra-disk", "max-seconds", "mtu"],
                   flags: ["no-autologin", "no-ssh", "gui", "continue-on-provisioning-error"])
  let b = VMBundle(try a.require("bundle"))
  try b.mustExist()
  let cfg = try BundleConfig.load(b)
  let net = try NetChoice.parse(a.value("net") ?? "nat")
  var disk = DiskModes()
  disk.cacheName = try a.choice("disk-cache", ["automatic", "cached", "uncached"], default: "automatic")
  disk.syncName = try a.choice("disk-sync", ["full", "fsync", "none"], default: "full")
  var extra: [(String, Bool)] = []
  for e in a.all("extra-disk") {
    if e.hasSuffix(":ro") {
      extra.append((absolutePath(String(e.dropLast(3))), true))
    } else {
      extra.append((absolutePath(e), false))
    }
  }
  var mtu: Int? = nil
  if a.value("mtu") != nil {
    let m = try a.int("mtu", default: 1500, min: 1500)
    guard m <= 65535 else { throw Fail("--mtu must be between 1500 and 65535, got \(m)") }
    guard case .vfkit = net else { throw Fail("--mtu applies only to --net vfkit:SOCKETPATH") }
    mtu = m
  }
  var maxSeconds: Int? = nil
  if a.value("max-seconds") != nil { maxSeconds = try a.int("max-seconds", default: 0) }

  var provision: ProvisionRequest? = nil
  if let user = a.value("provision-user") {
    let pw = try a.require("provision-password")
    provision = ProvisionRequest(user: user, password: pw, fullName: a.value("provision-full-name") ?? user,
                                 autologin: !a.has("no-autologin"), ssh: !a.has("no-ssh"))
  } else if a.value("provision-password") != nil || a.value("provision-full-name") != nil {
    throw Fail("--provision-password and --provision-full-name need --provision-user")
  }

  // The window server must be reachable before the machine is made; headless, the application
  // never shows (Tart does the same with .prohibited).
  let app = NSApplication.shared
  if !a.has("gui") { app.setActivationPolicy(.prohibited) }

  let conf = try makeConfiguration(b, cfg, net: net, disk: disk, extraDisks: extra, mtu: mtu)
  let vm = VZVirtualMachine(configuration: conf)
  emit("config", [("mac", .s(cfg.mac)), ("cpus", .i(conf.cpuCount)), ("memory_gb", .i(Int(conf.memorySize >> 30))),
                  ("net", .s(net.name)), ("mtu", mtu.map { J.i($0) } ?? J.null), ("disk_cache", .s(disk.cacheName)), ("disk_sync", .s(disk.syncName)),
                  ("disk_format", .s(cfg.diskFormat)), ("extra_disks", .i(extra.count)), ("gui", .b(a.has("gui"))),
                  ("provision_user", provision.map { J.s($0.user) } ?? J.null),
                  ("provisioning_api", .b(provisioningAPIAvailable()))])
  let r = Runner(bundle: b, vm: vm, provision: provision,
                 continueOnProvisioningError: a.has("continue-on-provisioning-error"),
                 maxSeconds: maxSeconds, gui: a.has("gui"))
  runner = r
  try r.begin()
  app.run()
}

#else

func cmdHostCheck() -> Int32 {
  printJSON([("ok", .b(false)), ("message", .s("silovm runs macOS computers only on Apple silicon"))])
  return 1
}
func cmdIPSWURL() {
  logErr("silovm runs macOS computers only on Apple silicon")
  exit(1)
}
func cmdInstall(_ argv: [String]) throws { throw Fail("silovm runs macOS computers only on Apple silicon") }
func cmdClone(_ argv: [String]) throws -> Int32 { throw Fail("silovm runs macOS computers only on Apple silicon") }
func cmdRun(_ argv: [String]) throws { throw Fail("silovm runs macOS computers only on Apple silicon") }

#endif
