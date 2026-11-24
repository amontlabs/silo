// Stand-ins for the parts of Virtualization, AppKit and Darwin that swift/Machine.swift uses, with
// the Swift signatures of Apple's documentation (developer.apple.com/documentation/virtualization)
// and the macOS 27 SDK. Only for linux/test-vm-helpers.sh, which typechecks Machine.swift on Linux
// against them: it catches mistakes in silovm's own code, not in these declarations.

import Foundation

// Darwin
func clonefile(_ src: UnsafePointer<CChar>!, _ dst: UnsafePointer<CChar>!, _ flags: UInt32) -> Int32 { 0 }
func sysctlbyname(_ name: UnsafePointer<CChar>!, _ oldp: UnsafeMutableRawPointer!, _ oldlenp: UnsafeMutablePointer<Int>!,
                  _ newp: UnsafeMutableRawPointer!, _ newlen: Int) -> Int32 { 0 }

// Key-value observing, which Foundation on Linux lacks
struct NSKeyValueObservingOptions: OptionSet {
  let rawValue: Int
  static let new = NSKeyValueObservingOptions(rawValue: 1)
  static let initial = NSKeyValueObservingOptions(rawValue: 4)
}
struct NSKeyValueObservedChange<Value> {}
final class NSKeyValueObservation {}
protocol KVOStub {}
extension KVOStub {
  func observe<Value>(_ keyPath: KeyPath<Self, Value>, options: NSKeyValueObservingOptions = [],
                      changeHandler: @escaping (Self, NSKeyValueObservedChange<Value>) -> Void) -> NSKeyValueObservation {
    NSKeyValueObservation()
  }
}
extension Progress: KVOStub {}

// AppKit
class NSColor: NSObject {}
class NSBitmapImageRep: NSObject {
  enum FileType { case png }
  struct PropertyKey: Hashable { let rawValue: String }
  var pixelsWide: Int { 0 }
  var pixelsHigh: Int { 0 }
  func representation(using: FileType, properties: [PropertyKey: Any]) -> Data? { nil }
  func colorAt(x: Int, y: Int) -> NSColor? { nil }
}
class NSView: NSObject {
  var bounds: NSRect { NSRect() }
  func bitmapImageRepForCachingDisplay(in rect: NSRect) -> NSBitmapImageRep? { nil }
  func cacheDisplay(in rect: NSRect, to rep: NSBitmapImageRep) {}
}
class NSWindow: NSObject {
  struct StyleMask: OptionSet {
    let rawValue: UInt
    static let titled = StyleMask(rawValue: 1), closable = StyleMask(rawValue: 2)
    static let miniaturizable = StyleMask(rawValue: 4), resizable = StyleMask(rawValue: 8)
  }
  enum BackingStoreType { case buffered }
  init(contentRect: NSRect, styleMask: StyleMask, backing: BackingStoreType, defer flag: Bool) {}
  var title = ""
  var isReleasedWhenClosed = true
  static let willCloseNotification = Notification.Name("NSWindowWillCloseNotification")
  var contentView: NSView?
  func center() {}
  func makeKeyAndOrderFront(_ sender: Any?) {}
}
class NSApplication: NSObject {
  enum ActivationPolicy { case regular, accessory, prohibited }
  static let shared = NSApplication()
  @discardableResult func setActivationPolicy(_ p: ActivationPolicy) -> Bool { true }
  func activate() {}
  func run() {}
}

// Virtualization
class VZBootLoader: NSObject {}
class VZMacOSBootLoader: VZBootLoader {}
class VZPlatformConfiguration: NSObject {}
class VZMacHardwareModel: NSObject {
  init?(dataRepresentation: Data) {}
  var dataRepresentation: Data { Data() }
  var isSupported: Bool { true }
}
class VZMacMachineIdentifier: NSObject {
  override init() {}
  init?(dataRepresentation: Data) {}
  var dataRepresentation: Data { Data() }
}
class VZMacAuxiliaryStorage: NSObject {
  struct InitializationOptions: OptionSet {
    let rawValue: UInt
    static let allowOverwrite = InitializationOptions(rawValue: 1)
  }
  init(url: URL) {}
  init(creatingStorageAt url: URL, hardwareModel: VZMacHardwareModel, options: InitializationOptions) throws {}
}
class VZMacPlatformConfiguration: VZPlatformConfiguration {
  var hardwareModel = VZMacHardwareModel(dataRepresentation: Data())!
  var machineIdentifier = VZMacMachineIdentifier()
  var auxiliaryStorage: VZMacAuxiliaryStorage?
}
class VZGraphicsDeviceConfiguration: NSObject {}
class VZMacGraphicsDisplayConfiguration: NSObject {
  init(widthInPixels: Int, heightInPixels: Int, pixelsPerInch: Int) {}
}
class VZMacGraphicsDeviceConfiguration: VZGraphicsDeviceConfiguration {
  var displays: [VZMacGraphicsDisplayConfiguration] = []
}
class VZKeyboardConfiguration: NSObject {}
class VZUSBKeyboardConfiguration: VZKeyboardConfiguration {}
class VZMacKeyboardConfiguration: VZKeyboardConfiguration {}
class VZPointingDeviceConfiguration: NSObject {}
class VZUSBScreenCoordinatePointingDeviceConfiguration: VZPointingDeviceConfiguration {}
class VZMacTrackpadConfiguration: VZPointingDeviceConfiguration {}
class VZEntropyDeviceConfiguration: NSObject {}
class VZVirtioEntropyDeviceConfiguration: VZEntropyDeviceConfiguration {}
enum VZDiskImageCachingMode: Int { case automatic = 0, uncached = 1, cached = 2 }
enum VZDiskImageSynchronizationMode: Int { case full = 1, fsync = 2, none = 3 }
class VZStorageDeviceAttachment: NSObject {}
class VZDiskImageStorageDeviceAttachment: VZStorageDeviceAttachment {
  init(url: URL, readOnly: Bool, cachingMode: VZDiskImageCachingMode, synchronizationMode: VZDiskImageSynchronizationMode) throws {}
}
enum VZDiskSynchronizationMode: Int { case full = 0, none = 1 }
protocol VZNetworkBlockDeviceStorageDeviceAttachmentDelegate: NSObjectProtocol {
  func attachmentWasConnected(_ attachment: VZNetworkBlockDeviceStorageDeviceAttachment)
  func attachment(_ attachment: VZNetworkBlockDeviceStorageDeviceAttachment, didEncounterError error: Error)
}
class VZNetworkBlockDeviceStorageDeviceAttachment: VZStorageDeviceAttachment {
  init(url: URL, timeout: TimeInterval, isForcedReadOnly: Bool, synchronizationMode: VZDiskSynchronizationMode) throws {}
  class func validate(_ url: URL) throws {}
  weak var delegate: VZNetworkBlockDeviceStorageDeviceAttachmentDelegate?
}
class VZStorageDeviceConfiguration: NSObject {}
class VZVirtioBlockDeviceConfiguration: VZStorageDeviceConfiguration {
  init(attachment: VZStorageDeviceAttachment) {}
}
class VZMACAddress: NSObject {
  init?(string: String) {}
  override init() {}
  class func randomLocallyAdministered() -> VZMACAddress { VZMACAddress() }
  var string: String { "" }
}
class VZNetworkDeviceAttachment: NSObject {}
class VZNATNetworkDeviceAttachment: VZNetworkDeviceAttachment {}
class VZFileHandleNetworkDeviceAttachment: VZNetworkDeviceAttachment {
  init(fileHandle: FileHandle) {}
  var maximumTransmissionUnit = 1500
}
class VZNetworkDeviceConfiguration: NSObject {
  var macAddress = VZMACAddress()
  var attachment: VZNetworkDeviceAttachment?
}
class VZVirtioNetworkDeviceConfiguration: VZNetworkDeviceConfiguration {}
class VZNetworkDevice: NSObject {}
class VZVirtualMachineConfiguration: NSObject {
  var bootLoader: VZBootLoader?
  var platform: VZPlatformConfiguration = VZPlatformConfiguration()
  var cpuCount = 1
  var memorySize: UInt64 = 0
  var graphicsDevices: [VZGraphicsDeviceConfiguration] = []
  var keyboards: [VZKeyboardConfiguration] = []
  var pointingDevices: [VZPointingDeviceConfiguration] = []
  var entropyDevices: [VZEntropyDeviceConfiguration] = []
  var storageDevices: [VZStorageDeviceConfiguration] = []
  var networkDevices: [VZNetworkDeviceConfiguration] = []
  class var minimumAllowedCPUCount: Int { 1 }
  class var maximumAllowedCPUCount: Int { 8 }
  class var minimumAllowedMemorySize: UInt64 { 1 << 30 }
  class var maximumAllowedMemorySize: UInt64 { 16 << 30 }
  func validate() throws {}
}
class VZVirtualMachineStartOptions: NSObject {}
class VZGuestProvisioningOptions: NSObject {
  func validate() throws {}
}
class VZMacGuestProvisioningOptions: VZGuestProvisioningOptions {
  var fullName = ""
  var username = ""
  var password = ""
  var logsInAutomatically = false
  var enablesRemoteLogin = false
}
class VZMacOSVirtualMachineStartOptions: VZVirtualMachineStartOptions {
  var startUpFromMacOSRecovery = false
  func setGuestProvisioning(_ options: VZMacGuestProvisioningOptions?) throws {}
}
// Apple's protocol is @objc with optional methods; Linux has no Objective-C runtime, so this one
// requires them, which Runner implements.
protocol VZVirtualMachineDelegate: AnyObject {
  func guestDidStop(_ virtualMachine: VZVirtualMachine)
  func virtualMachine(_ virtualMachine: VZVirtualMachine, didStopWithError error: Error)
  func virtualMachine(_ virtualMachine: VZVirtualMachine, networkDevice: VZNetworkDevice,
                      attachmentWasDisconnectedWithError error: Error)
}
class VZVirtualMachine: NSObject, KVOStub {
  enum State: Int { case stopped, running, paused, error, starting, pausing, resuming, stopping, saving, restoring }
  class var isSupported: Bool { true }
  init(configuration: VZVirtualMachineConfiguration) {}
  var state: State { .stopped }
  weak var delegate: VZVirtualMachineDelegate?
  var canStop: Bool { true }
  var canRequestStop: Bool { true }
  func start(options: VZVirtualMachineStartOptions, completionHandler: @escaping ((any Error)?) -> Void) {}
  func stop(completionHandler: @escaping ((any Error)?) -> Void) {}
  func pause(completionHandler: @escaping (Result<Void, any Error>) -> Void) {}
  func resume(completionHandler: @escaping (Result<Void, any Error>) -> Void) {}
  func requestStop() throws {}
}
class VZVirtualMachineView: NSView {
  var virtualMachine: VZVirtualMachine?
  var capturesSystemKeys = false
  var automaticallyReconfiguresDisplay = false
}
class VZMacOSConfigurationRequirements: NSObject {
  var hardwareModel: VZMacHardwareModel { VZMacHardwareModel(dataRepresentation: Data())! }
  var minimumSupportedCPUCount: Int { 1 }
  var minimumSupportedMemorySize: UInt64 { 1 << 30 }
}
class VZMacOSRestoreImage: NSObject {
  class func fetchLatestSupported(completionHandler: @escaping (Result<VZMacOSRestoreImage, any Error>) -> Void) {}
  class func load(from fileURL: URL, completionHandler: @escaping (Result<VZMacOSRestoreImage, any Error>) -> Void) {}
  var url: URL { URL(fileURLWithPath: "/") }
  var buildVersion: String { "" }
  var operatingSystemVersion: OperatingSystemVersion { OperatingSystemVersion(majorVersion: 27, minorVersion: 0, patchVersion: 0) }
  var mostFeaturefulSupportedConfiguration: VZMacOSConfigurationRequirements? { nil }
}
class VZMacOSInstaller: NSObject {
  init(virtualMachine: VZVirtualMachine, restoringFromImageAt restoreImageFileURL: URL) {}
  var progress: Progress { Progress(totalUnitCount: 100) }
  func install(completionHandler: @escaping (Result<Void, any Error>) -> Void) {}
}
