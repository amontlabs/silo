//! Virtualization.framework side of macOS computers.
//!
//! Every framework object is created and used on the main thread: virtual
//! machines are built with `initWithConfiguration`, which binds them to the main
//! queue, and their completion handlers and delegate callbacks run there too.
//! Worker threads reach the main thread through `on_main`, which waits for the
//! closure's result. The closure itself never waits for a framework callback;
//! callbacks send their result over a channel that the worker reads.
use super::guest_screen;
use super::input::{self, KeyEvent, Monitors, PointerKind, TextLine};
use super::store::{self, HostLimits, Layout, Record};
use block2::RcBlock;
use objc2::Message as _;
use objc2::{
    define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject},
    sel, AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly,
};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSEvent, NSEventMask, NSLayoutGuide, NSResponder, NSToolbar,
    NSToolbarDelegate, NSToolbarDisplayMode, NSToolbarItem, NSView, NSWindow,
};
use objc2_foundation::{
    NSArray, NSData, NSError, NSObject, NSObjectProtocol, NSOperationQueue, NSString, NSURL,
};
use objc2_virtualization::*;
use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::{c_char, c_void},
    path::Path,
    ptr::NonNull,
    sync::{mpsc, Arc, Mutex, OnceLock},
    time::Duration,
};
use tauri::AppHandle;

const MAIN_THREAD_WAIT: Duration = Duration::from_secs(10);
const FETCH_WAIT: Duration = Duration::from_secs(60);
const CANCEL_WAIT: Duration = Duration::from_secs(30);
const VIRTUALIZATION_ENTITLEMENT: &str = "com.apple.security.virtualization";
const LIMIT_EXCEEDED: &str =
    "macOS allows at most two macOS computers running at once on this Mac.";

/// What a main-thread slot holds for one computer.
struct Slot {
    vm: Retained<VZVirtualMachine>,
    /// The machine keeps its delegate weakly.
    _delegate: Retained<MachineDelegate>,
    installer: Option<Retained<VZMacOSInstaller>>,
    view: Option<Retained<VZVirtualMachineView>>,
}

thread_local! {
    /// Main thread only: framework objects are not `Send`.
    static SLOTS: RefCell<HashMap<String, Slot>> = RefCell::new(HashMap::new());
    /// Main thread only: the toolbar delegate of each open screen window, which the toolbar
    /// and its buttons hold weakly.
    static TOOLBARS: RefCell<HashMap<String, Retained<ToolbarDelegate>>> = RefCell::new(HashMap::new());
}

struct DelegateState {
    app: AppHandle,
    id: String,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; the delegate state is immutable.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = DelegateState]
    struct MachineDelegate;

    unsafe impl NSObjectProtocol for MachineDelegate {}

    unsafe impl VZVirtualMachineDelegate for MachineDelegate {
        #[unsafe(method(guestDidStopVirtualMachine:))]
        fn guest_did_stop(&self, _machine: &VZVirtualMachine) {
            let state = self.ivars();
            super::machine_stopped(&state.app.clone(), &state.id.clone(), None);
        }

        #[unsafe(method(virtualMachine:didStopWithError:))]
        fn did_stop_with_error(&self, _machine: &VZVirtualMachine, error: &NSError) {
            let state = self.ivars();
            let detail = format!("The computer stopped unexpectedly. {}", describe(error));
            super::machine_stopped(&state.app.clone(), &state.id.clone(), Some(detail));
        }
    }
);

impl MachineDelegate {
    fn new(mtm: MainThreadMarker, app: AppHandle, id: String) -> Retained<Self> {
        let allocated = mtm.alloc::<Self>().set_ivars(DelegateState { app, id });
        // SAFETY: NSObject's init has the declared signature and initializes our subclass.
        unsafe { msg_send![super(allocated), init] }
    }
}

fn describe(error: &NSError) -> String {
    // SAFETY: VZErrorDomain is an immutable framework constant.
    let limit = unsafe { VZErrorDomain }
        .is_some_and(|domain| error.domain().isEqualToString(domain))
        && error.code() == VZErrorCode::VirtualMachineLimitExceeded.0;
    if limit {
        LIMIT_EXCEEDED.to_string()
    } else {
        error.localizedDescription().to_string()
    }
}

fn describe_ptr(error: *mut NSError) -> String {
    // SAFETY: Framework completion handlers pass either null or a valid NSError.
    unsafe { error.as_ref() }.map_or_else(|| "Unknown error.".into(), describe)
}

/// Runs `work` on the main thread and waits for its result. Never call this
/// from the main thread: it would wait on itself.
pub(super) fn on_main<T: Send + 'static>(
    app: &AppHandle,
    work: impl FnOnce(MainThreadMarker) -> T + Send + 'static,
) -> Result<T, String> {
    if MainThreadMarker::new().is_some() {
        return Err("macOS computers cannot wait on the main thread.".into());
    }
    // `Pending` work that its caller has given up on must never run; work already
    // running is waited for, so its effects are always reported.
    let phase = Arc::new(Mutex::new(Phase::Pending));
    let (send, receive) = mpsc::channel();
    let worker_phase = phase.clone();
    app.run_on_main_thread(move || {
        {
            let mut phase = worker_phase.lock().unwrap_or_else(|p| p.into_inner());
            if *phase == Phase::Abandoned {
                return;
            }
            *phase = Phase::Running;
        }
        if let Some(mtm) = MainThreadMarker::new() {
            let _ = send.send(work(mtm));
        }
    })
    .map_err(|_| "Silo could not reach its main thread.".to_string())?;
    match receive.recv_timeout(MAIN_THREAD_WAIT) {
        Ok(result) => Ok(result),
        Err(_) => {
            let mut phase = phase.lock().unwrap_or_else(|p| p.into_inner());
            if *phase == Phase::Pending {
                *phase = Phase::Abandoned;
                return Err("Silo's main thread did not respond.".to_string());
            }
            drop(phase);
            receive
                .recv()
                .map_err(|_| "Silo's main thread did not respond.".to_string())
        }
    }
}

#[derive(PartialEq, Eq)]
enum Phase {
    Pending,
    Running,
    Abandoned,
}

// MARK: Host support

#[link(name = "Security", kind = "framework")]
extern "C" {
    fn SecTaskCreateFromSelf(allocator: *const c_void) -> *const c_void;
    fn SecTaskCopyValueForEntitlement(
        task: *const c_void,
        entitlement: *const c_void,
        error: *mut *const c_void,
    ) -> *const c_void;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringCreateWithCString(
        allocator: *const c_void,
        string: *const c_char,
        encoding: u32,
    ) -> *const c_void;
    fn CFBooleanGetTypeID() -> usize;
    fn CFGetTypeID(value: *const c_void) -> usize;
    fn CFBooleanGetValue(value: *const c_void) -> bool;
    fn CFRelease(value: *const c_void);
}

const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

/// Whether this executable carries the virtualization entitlement. An unsigned
/// development binary does not, and every framework call then fails.
fn has_virtualization_entitlement() -> bool {
    let key = c"com.apple.security.virtualization";
    debug_assert_eq!(key.to_str(), Ok(VIRTUALIZATION_ENTITLEMENT));
    // SAFETY: Plain Core Foundation and Security calls. Every object created here is
    // released, and the entitlement value is only read after a type check.
    unsafe {
        let task = SecTaskCreateFromSelf(std::ptr::null());
        if task.is_null() {
            return false;
        }
        let name =
            CFStringCreateWithCString(std::ptr::null(), key.as_ptr(), CF_STRING_ENCODING_UTF8);
        let value = SecTaskCopyValueForEntitlement(task, name, std::ptr::null_mut());
        let granted = !value.is_null()
            && CFGetTypeID(value) == CFBooleanGetTypeID()
            && CFBooleanGetValue(value);
        if !value.is_null() {
            CFRelease(value);
        }
        if !name.is_null() {
            CFRelease(name);
        }
        CFRelease(task);
        granted
    }
}

pub(super) fn unsupported_reason() -> Option<String> {
    static REASON: OnceLock<Option<String>> = OnceLock::new();
    REASON
        .get_or_init(|| {
            if !has_virtualization_entitlement() {
                return Some(
                    "This build of Silo can't run macOS computers because it is not signed with the virtualization entitlement. Use a signed build such as desktop:build:debug."
                        .into(),
                );
            }
            // SAFETY: A class method without arguments.
            if !unsafe { VZVirtualMachine::isSupported() } {
                return Some("This Mac can't run macOS computers.".into());
            }
            None
        })
        .clone()
}

pub(super) fn host_limits() -> HostLimits {
    let cpus = std::thread::available_parallelism().map_or(2, |count| count.get() as u64);
    let mut memory: u64 = 0;
    let mut size = std::mem::size_of::<u64>();
    // SAFETY: `memory` is a u64 and `size` describes it, as hw.memsize requires.
    let status = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&raw mut memory).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    HostLimits {
        cpus,
        memory_gib: if status == 0 { memory >> 30 } else { 8 },
    }
}

pub(super) fn random_mac() -> String {
    // SAFETY: A class method without arguments.
    unsafe {
        VZMACAddress::randomLocallyAdministeredAddress()
            .string()
            .to_string()
    }
}

// MARK: Restore images

pub(super) struct LatestImage {
    pub url: String,
    pub version: String,
    pub build: String,
}

fn version_string(version: objc2_foundation::NSOperatingSystemVersion) -> String {
    format!(
        "{}.{}.{}",
        version.majorVersion, version.minorVersion, version.patchVersion
    )
}

pub(super) fn fetch_latest() -> Result<LatestImage, String> {
    let (send, receive) = mpsc::channel();
    let handler = RcBlock::new(
        move |image: *mut VZMacOSRestoreImage, error: *mut NSError| {
            // SAFETY: The framework passes either a valid image or a valid error.
            let result = match unsafe { image.as_ref() } {
                Some(image) => Ok(unsafe {
                    LatestImage {
                        url: image
                            .URL()
                            .absoluteString()
                            .map(|s| s.to_string())
                            .unwrap_or_default(),
                        version: version_string(image.operatingSystemVersion()),
                        build: image.buildVersion().to_string(),
                    }
                }),
                None => Err(format!(
                    "Silo could not look up the latest macOS: {}",
                    describe_ptr(error)
                )),
            };
            let _ = send.send(result);
        },
    );
    // SAFETY: The handler stays alive until the framework has called it.
    unsafe { VZMacOSRestoreImage::fetchLatestSupportedWithCompletionHandler(&handler) };
    receive
        .recv_timeout(FETCH_WAIT)
        .map_err(|_| "Silo could not reach Apple to look up the latest macOS.".to_string())?
}

/// What an installation needs from the restore image, copied out as plain data.
struct Requirements {
    hardware_model: Vec<u8>,
    min_cpus: u64,
    min_memory: u64,
}

fn load_requirements(image: &Path) -> Result<Requirements, String> {
    let url = NSURL::fileURLWithPath(&NSString::from_str(&image.to_string_lossy()));
    let (send, receive) = mpsc::channel();
    let handler = RcBlock::new(
        move |image: *mut VZMacOSRestoreImage, error: *mut NSError| {
            // SAFETY: The framework passes either a valid image or a valid error.
            let result = match unsafe { image.as_ref() } {
                Some(image) => unsafe { requirements(image) },
                None => Err(format!(
                    "Silo could not read the macOS download: {}",
                    describe_ptr(error)
                )),
            };
            let _ = send.send(result);
        },
    );
    // SAFETY: The handler stays alive until the framework has called it.
    unsafe { VZMacOSRestoreImage::loadFileURL_completionHandler(&url, &handler) };
    receive
        .recv_timeout(FETCH_WAIT)
        .map_err(|_| "Silo could not read the macOS download.".to_string())?
}

unsafe fn requirements(image: &VZMacOSRestoreImage) -> Result<Requirements, String> {
    let unsupported = || {
        format!(
            "This Mac can't install macOS {}.",
            version_string(unsafe { image.operatingSystemVersion() })
        )
    };
    let configuration =
        unsafe { image.mostFeaturefulSupportedConfiguration() }.ok_or_else(unsupported)?;
    let model = unsafe { configuration.hardwareModel() };
    if !unsafe { model.isSupported() } {
        return Err(unsupported());
    }
    Ok(Requirements {
        hardware_model: unsafe { model.dataRepresentation() }.to_vec(),
        min_cpus: unsafe { configuration.minimumSupportedCPUCount() } as u64,
        min_memory: unsafe { configuration.minimumSupportedMemorySize() },
    })
}

// MARK: Configuration

const GIB: u64 = 1 << 30;

fn nsurl(path: &Path) -> Retained<NSURL> {
    NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
}

/// Builds and validates the machine configuration of a computer whose hardware
/// model and identifier are already on disk (or passed in for a new one).
fn configuration(
    record: &Record,
    layout: &Layout,
    hardware_model: &[u8],
    auxiliary_storage: &VZMacAuxiliaryStorage,
    machine_identifier: &VZMacMachineIdentifier,
) -> Result<Retained<VZVirtualMachineConfiguration>, String> {
    // SAFETY: Every call below uses framework classes with valid arguments; arrays
    // contain only the declared element classes.
    unsafe {
        let model = VZMacHardwareModel::initWithDataRepresentation(
            VZMacHardwareModel::alloc(),
            &NSData::with_bytes(hardware_model),
        )
        .ok_or("The computer's hardware model is damaged. Delete it and create it again.")?;
        if !model.isSupported() {
            return Err("This Mac can no longer run this computer's macOS.".into());
        }
        let platform = VZMacPlatformConfiguration::new();
        platform.setHardwareModel(&model);
        platform.setAuxiliaryStorage(Some(auxiliary_storage));
        platform.setMachineIdentifier(machine_identifier);

        let configuration = VZVirtualMachineConfiguration::new();
        let boot_loader = VZMacOSBootLoader::new();
        configuration.setBootLoader(Some(&boot_loader));
        configuration.setPlatform(&platform);
        configuration.setCPUCount(record.cpus as usize);
        configuration.setMemorySize(record.memory_gib * GIB);

        let display =
            VZMacGraphicsDisplayConfiguration::initWithWidthInPixels_heightInPixels_pixelsPerInch(
                VZMacGraphicsDisplayConfiguration::alloc(),
                2560,
                1600,
                220,
            );
        let graphics = VZMacGraphicsDeviceConfiguration::new();
        graphics.setDisplays(&NSArray::from_retained_slice(&[display]));
        configuration.setGraphicsDevices(&NSArray::from_retained_slice(&[graphics.into_super()]));

        let attachment = VZDiskImageStorageDeviceAttachment::initWithURL_readOnly_error(
            VZDiskImageStorageDeviceAttachment::alloc(),
            &nsurl(&layout.disk()),
            false,
        )
        .map_err(|error| format!("Silo could not open the disk: {}", describe(&error)))?;
        let disk = VZVirtioBlockDeviceConfiguration::initWithAttachment(
            VZVirtioBlockDeviceConfiguration::alloc(),
            &attachment,
        );
        configuration.setStorageDevices(&NSArray::from_retained_slice(&[disk.into_super()]));

        let network = VZVirtioNetworkDeviceConfiguration::new();
        network.setAttachment(Some(&VZNATNetworkDeviceAttachment::new()));
        let address = VZMACAddress::initWithString(
            VZMACAddress::alloc(),
            &NSString::from_str(&record.mac_address),
        )
        .ok_or("The computer's network address is damaged. Delete it and create it again.")?;
        network.setMACAddress(&address);
        configuration.setNetworkDevices(&NSArray::from_retained_slice(&[network.into_super()]));

        configuration.setPointingDevices(&NSArray::from_retained_slice(&[
            VZMacTrackpadConfiguration::new().into_super(),
            VZUSBScreenCoordinatePointingDeviceConfiguration::new().into_super(),
        ]));
        configuration.setKeyboards(&NSArray::from_retained_slice(&[
            VZMacKeyboardConfiguration::new().into_super(),
        ]));

        let output = VZVirtioSoundDeviceOutputStreamConfiguration::new();
        output.setSink(Some(&VZHostAudioOutputStreamSink::new()));
        let sound = VZVirtioSoundDeviceConfiguration::new();
        sound.setStreams(&NSArray::from_retained_slice(&[output.into_super()]));
        configuration.setAudioDevices(&NSArray::from_retained_slice(&[sound.into_super()]));
        configuration.setEntropyDevices(&NSArray::from_retained_slice(&[
            VZVirtioEntropyDeviceConfiguration::new().into_super(),
        ]));

        configuration.validateWithError().map_err(|error| {
            format!(
                "macOS computer configuration is invalid: {}",
                describe(&error)
            )
        })?;
        Ok(configuration)
    }
}

fn read(path: &Path, what: &str) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|error| store::io_error(&format!("read the {what}"), &error))
}

fn machine_identifier_from(bytes: &[u8]) -> Result<Retained<VZMacMachineIdentifier>, String> {
    // SAFETY: Initializes a framework object from data.
    unsafe {
        VZMacMachineIdentifier::initWithDataRepresentation(
            VZMacMachineIdentifier::alloc(),
            &NSData::with_bytes(bytes),
        )
    }
    .ok_or_else(|| "The computer's identity is damaged. Delete it and create it again.".into())
}

/// Creates the machine for `configuration` and keeps it in the main-thread slots.
fn register(
    mtm: MainThreadMarker,
    app: &AppHandle,
    record: &Record,
    configuration: &VZVirtualMachineConfiguration,
    installer_image: Option<&Path>,
) -> Slot {
    // SAFETY: Called on the main thread; the machine binds to the main queue.
    unsafe {
        let vm = VZVirtualMachine::initWithConfiguration(VZVirtualMachine::alloc(), configuration);
        let delegate = MachineDelegate::new(mtm, app.clone(), record.id.clone());
        vm.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        let installer = installer_image.map(|image| {
            VZMacOSInstaller::initWithVirtualMachine_restoreImageURL(
                VZMacOSInstaller::alloc(),
                &vm,
                &nsurl(image),
            )
        });
        Slot {
            vm,
            _delegate: delegate,
            installer,
            view: None,
        }
    }
}

// MARK: Installing

pub(super) enum InstallError {
    Cancelled,
    Failed(String),
}

impl From<String> for InstallError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

/// Installs macOS from `image` onto a new machine. Raises `record`'s resources
/// to what the image requires; the caller persists them.
pub(super) fn install(
    app: &AppHandle,
    record: &mut Record,
    layout: &Layout,
    image: &Path,
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(f64),
) -> Result<(), InstallError> {
    let needs = load_requirements(image)?;
    // SAFETY: Class methods without arguments.
    let (cpu_range, memory_range) = unsafe {
        (
            VZVirtualMachineConfiguration::minimumAllowedCPUCount() as u64
                ..=VZVirtualMachineConfiguration::maximumAllowedCPUCount() as u64,
            VZVirtualMachineConfiguration::minimumAllowedMemorySize()
                ..=VZVirtualMachineConfiguration::maximumAllowedMemorySize(),
        )
    };
    let cpus = record.cpus.max(needs.min_cpus);
    let memory = (record.memory_gib * GIB).max(needs.min_memory);
    if !cpu_range.contains(&cpus) || !memory_range.contains(&memory) {
        return Err(InstallError::Failed(
            "This Mac can't give this computer the CPUs and memory its macOS needs.".into(),
        ));
    }
    record.cpus = cpus;
    record.memory_gib = memory.div_ceil(GIB);
    std::fs::write(layout.hardware_model(), &needs.hardware_model)
        .map_err(|error| store::io_error("save the computer's hardware model", &error))?;
    store::create_disk(&layout.disk(), record.disk_gib)?;

    let (send, receive) = mpsc::channel::<Result<(), String>>();
    let prepared = {
        let (app, record, layout, image, model) = (
            app.clone(),
            record.clone(),
            layout.clone(),
            image.to_path_buf(),
            needs.hardware_model,
        );
        on_main(&app.clone(), move |mtm| -> Result<(), String> {
            // SAFETY: Main thread; arguments are valid framework objects.
            unsafe {
                let model_object = VZMacHardwareModel::initWithDataRepresentation(
                    VZMacHardwareModel::alloc(),
                    &NSData::with_bytes(&model),
                )
                .ok_or("The macOS download holds an unreadable hardware model.")?;
                let auxiliary =
                    VZMacAuxiliaryStorage::initCreatingStorageAtURL_hardwareModel_options_error(
                        VZMacAuxiliaryStorage::alloc(),
                        &nsurl(&layout.auxiliary_storage()),
                        &model_object,
                        VZMacAuxiliaryStorageInitializationOptions::AllowOverwrite,
                    )
                    .map_err(|error| describe(&error))?;
                let identifier = VZMacMachineIdentifier::new();
                std::fs::write(
                    layout.machine_identifier(),
                    identifier.dataRepresentation().to_vec(),
                )
                .map_err(|error| store::io_error("save the computer's identity", &error))?;
                let configuration =
                    configuration(&record, &layout, &model, &auxiliary, &identifier)?;
                let slot = register(mtm, &app, &record, &configuration, Some(&image));
                let installer = slot.installer.clone().ok_or("No installer.")?;
                let handler = RcBlock::new(move |error: *mut NSError| {
                    let _ = send.send(match error.as_ref() {
                        None => Ok(()),
                        Some(error) => Err(describe(error)),
                    });
                });
                SLOTS.with(|slots| slots.borrow_mut().insert(record.id.clone(), slot));
                installer.installWithCompletionHandler(&handler);
            }
            Ok(())
        })?
    };
    if let Err(message) = prepared {
        discard(app, &record.id);
        return Err(InstallError::Failed(message));
    }

    let id = record.id.clone();
    let outcome = loop {
        match receive.recv_timeout(Duration::from_secs(1)) {
            Ok(result) => break result.map_err(InstallError::Failed),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break Err(InstallError::Failed("The macOS installer stopped.".into()))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if cancelled() {
            cancel_install(app, &id);
            let _ = receive.recv_timeout(CANCEL_WAIT);
            break Err(InstallError::Cancelled);
        }
        let key = id.clone();
        if let Ok(Some(fraction)) = on_main(app, move |_| {
            SLOTS.with(|slots| {
                slots.borrow().get(&key).and_then(|slot| {
                    slot.installer
                        .as_ref()
                        // SAFETY: Main thread; progress is a framework property.
                        .map(|installer| unsafe { installer.progress().fractionCompleted() })
                })
            })
        }) {
            progress(fraction.clamp(0.0, 1.0));
        }
    };
    discard(app, &id);
    outcome
}

fn cancel_install(app: &AppHandle, id: &str) {
    let id = id.to_string();
    let _ = on_main(app, move |_| {
        SLOTS.with(|slots| {
            if let Some(slot) = slots.borrow().get(&id) {
                // SAFETY: Main thread; both calls are valid on a running installation.
                unsafe {
                    if let Some(installer) = &slot.installer {
                        installer.progress().cancel();
                    }
                    if slot.vm.canStop() {
                        slot.vm
                            .stopWithCompletionHandler(&RcBlock::new(|_: *mut NSError| {}));
                    }
                }
            }
        })
    });
}

/// Drops a computer's machine and its display view.
fn discard(app: &AppHandle, id: &str) {
    let id = id.to_string();
    let _ = on_main(app, move |_| release(&id));
}

fn release(id: &str) {
    if let Some(slot) = SLOTS.with(|slots| slots.borrow_mut().remove(id)) {
        if let Some(view) = &slot.view {
            // SAFETY: Main thread.
            unsafe { view.setVirtualMachine(None) };
            view.removeFromSuperview();
        }
    }
}

/// Drops a machine and its display view. Call on the main thread.
pub(super) fn release_slot(id: &str) {
    release(id);
}

/// Runs `work` on the main thread after the current callback has returned, so a
/// callback never releases the objects it is running on. Callable from any thread.
pub(super) fn defer(work: impl FnOnce() + Send + 'static) {
    let work = std::sync::Mutex::new(Some(work));
    let block = RcBlock::new(move || {
        if let Some(work) = work.lock().ok().and_then(|mut work| work.take()) {
            work();
        }
    });
    // SAFETY: The queue copies the block, and the block owns everything it uses.
    unsafe { NSOperationQueue::mainQueue().addOperationWithBlock(&block) };
}

// MARK: Running

pub(super) fn start(app: &AppHandle, record: &Record, layout: &Layout) -> Result<(), String> {
    start_machine(app, record, layout, false)
}

/// Starts the computer in macOS Recovery instead of its installed system.
pub(super) fn start_in_recovery(
    app: &AppHandle,
    record: &Record,
    layout: &Layout,
) -> Result<(), String> {
    start_machine(app, record, layout, true)
}

/// How often a start is retried while the framework still holds the previous
/// machine's lock on the computer's auxiliary storage, and how long it waits between tries.
const LOCK_RETRIES: u32 = 6;
const LOCK_BACKOFF: Duration = Duration::from_secs(2);

fn start_machine(
    app: &AppHandle,
    record: &Record,
    layout: &Layout,
    recovery: bool,
) -> Result<(), String> {
    retry_while_locked(LOCK_RETRIES, LOCK_BACKOFF, || {
        start_machine_once(app, record, layout, recovery)
    })
}

/// A machine that just ended (an installation, a stopped computer) releases its
/// auxiliary storage a moment after its object is dropped; the next start fails
/// with a lock error until then.
fn retry_while_locked(
    retries: u32,
    backoff: Duration,
    mut attempt: impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    let mut tries = 0;
    loop {
        match attempt() {
            Err(message) if tries < retries && is_lock_error(&message) => {
                tries += 1;
                std::thread::sleep(backoff);
            }
            other => return other,
        }
    }
}

fn is_lock_error(message: &str) -> bool {
    message.to_ascii_lowercase().contains("lock")
}

fn start_machine_once(
    app: &AppHandle,
    record: &Record,
    layout: &Layout,
    recovery: bool,
) -> Result<(), String> {
    let model = read(&layout.hardware_model(), "hardware model")?;
    let identifier = read(&layout.machine_identifier(), "identity")?;
    let (send, receive) = mpsc::channel::<Result<(), String>>();
    let (app_handle, record, layout) = (app.clone(), record.clone(), layout.clone());
    on_main(app, move |mtm| -> Result<(), String> {
        if SLOTS.with(|slots| slots.borrow().contains_key(&record.id)) {
            return Err("This computer is already running.".into());
        }
        let identifier = machine_identifier_from(&identifier)?;
        // SAFETY: Main thread; the storage opens an existing file.
        let auxiliary = unsafe {
            VZMacAuxiliaryStorage::initWithURL(
                VZMacAuxiliaryStorage::alloc(),
                &nsurl(&layout.auxiliary_storage()),
            )
        };
        let configuration = configuration(&record, &layout, &model, &auxiliary, &identifier)?;
        let slot = register(mtm, &app_handle, &record, &configuration, None);
        let vm = slot.vm.clone();
        SLOTS.with(|slots| slots.borrow_mut().insert(record.id.clone(), slot));
        let id = record.id.clone();
        let handler = RcBlock::new(move |error: *mut NSError| {
            // SAFETY: The framework passes null or a valid error.
            let result = match unsafe { error.as_ref() } {
                None => Ok(()),
                Some(error) => {
                    let id = id.clone();
                    defer(move || release(&id));
                    Err(describe(error))
                }
            };
            let _ = send.send(result);
        });
        // SAFETY: Main thread; the handler runs on the main queue.
        unsafe {
            if recovery {
                let options = VZMacOSVirtualMachineStartOptions::new();
                options.setStartUpFromMacOSRecovery(true);
                vm.startWithOptions_completionHandler(&options, &handler);
            } else {
                vm.startWithCompletionHandler(&handler);
            }
        }
        Ok(())
    })??;
    // The framework always answers a start. Giving up earlier would leave a machine
    // that may still come up without anything tracking it as started.
    receive
        .recv()
        .map_err(|_| "The computer did not start.".to_string())?
}

pub(super) fn request_stop(app: &AppHandle, id: &str) -> Result<(), String> {
    let id = id.to_string();
    on_main(app, move |_| {
        SLOTS.with(|slots| {
            let slots = slots.borrow();
            let slot = slots.get(&id).ok_or("This computer isn't running.")?;
            // SAFETY: Main thread.
            unsafe {
                if !slot.vm.canRequestStop() {
                    return Err(
                        "This computer can't be asked to stop right now. Use Force stop."
                            .to_string(),
                    );
                }
                slot.vm
                    .requestStopWithError()
                    .map_err(|error| describe(&error))
            }
        })
    })?
}

pub(super) fn force_stop(app: &AppHandle, id: &str) -> Result<(), String> {
    let (id, handle) = (id.to_string(), app.clone());
    on_main(app, move |_| {
        let machine = SLOTS.with(|slots| slots.borrow().get(&id).map(|slot| slot.vm.clone()));
        let Some(vm) = machine else {
            return Err("This computer isn't running.".to_string());
        };
        // SAFETY: Main thread.
        unsafe {
            if !vm.canStop() {
                return Err("This computer can't be force stopped right now.".to_string());
            }
            let (finished, machine) = (id.clone(), vm.clone());
            vm.stopWithCompletionHandler(&RcBlock::new(move |error: *mut NSError| {
                // A stop the host requested ends without a delegate callback.
                match error.as_ref() {
                    None => super::machine_stopped(&handle, &finished, None),
                    Some(error) => {
                        let message = describe(error);
                        let state = machine.state();
                        if state == VZVirtualMachineState::Stopped
                            || state == VZVirtualMachineState::Error
                        {
                            super::machine_stopped(&handle, &finished, Some(message));
                        } else {
                            super::force_stop_failed(&handle, &finished, message);
                        }
                    }
                }
            }));
        }
        Ok(())
    })?
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MachineState {
    Running,
    Stopped,
    Failed,
    Other,
}

/// The framework's state of every machine Silo holds.
pub(super) fn machine_states(app: &AppHandle) -> Result<Vec<(String, MachineState)>, String> {
    on_main(app, |_| {
        SLOTS.with(|slots| {
            slots
                .borrow()
                .iter()
                .map(|(id, slot)| {
                    // SAFETY: Main thread.
                    let state = unsafe { slot.vm.state() };
                    let state = if state == VZVirtualMachineState::Running {
                        MachineState::Running
                    } else if state == VZVirtualMachineState::Stopped {
                        MachineState::Stopped
                    } else if state == VZVirtualMachineState::Error {
                        MachineState::Failed
                    } else {
                        MachineState::Other
                    };
                    (id.clone(), state)
                })
                .collect()
        })
    })
}

// MARK: Display

const PASTE_ITEM: &str = "silo.paste-into-computer";
const COPY_ITEM: &str = "silo.copy-from-computer";

fn toolbar_item_ids() -> Retained<NSArray<NSString>> {
    NSArray::from_retained_slice(&[
        NSString::from_str(PASTE_ITEM),
        NSString::from_str(COPY_ITEM),
    ])
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; the delegate state is immutable.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = DelegateState]
    struct ToolbarDelegate;

    unsafe impl NSObjectProtocol for ToolbarDelegate {}

    impl ToolbarDelegate {
        #[unsafe(method(pasteIntoComputer:))]
        fn paste_into_computer(&self, _sender: Option<&AnyObject>) {
            let state = self.ivars();
            super::guest_clipboard::spawn_from_display(
                &state.app,
                &state.id,
                super::guest_clipboard::Direction::PasteInto,
            );
        }

        #[unsafe(method(copyFromComputer:))]
        fn copy_from_computer(&self, _sender: Option<&AnyObject>) {
            let state = self.ivars();
            super::guest_clipboard::spawn_from_display(
                &state.app,
                &state.id,
                super::guest_clipboard::Direction::CopyFrom,
            );
        }
    }

    unsafe impl NSToolbarDelegate for ToolbarDelegate {
        #[unsafe(method_id(toolbar:itemForItemIdentifier:willBeInsertedIntoToolbar:))]
        fn item_for_identifier(
            &self,
            _toolbar: &NSToolbar,
            identifier: &NSString,
            _inserted: bool,
        ) -> Retained<NSToolbarItem> {
            let (label, tip, action) = match identifier.to_string().as_str() {
                PASTE_ITEM => (
                    "Paste into Computer",
                    "Put this Mac's clipboard on the computer's clipboard",
                    sel!(pasteIntoComputer:),
                ),
                COPY_ITEM => (
                    "Copy from Computer",
                    "Put the computer's clipboard on this Mac's clipboard",
                    sel!(copyFromComputer:),
                ),
                _ => ("", "", sel!(copyFromComputer:)),
            };
            // SAFETY: Main thread; the target is this delegate, kept alive by TOOLBARS, and
            // the action is one of its methods.
            unsafe {
                let item = NSToolbarItem::initWithItemIdentifier(NSToolbarItem::alloc(self.mtm()), identifier);
                item.setLabel(&NSString::from_str(label));
                item.setToolTip(Some(&NSString::from_str(tip)));
                item.setTarget(Some(self));
                item.setAction(Some(action));
                item
            }
        }

        #[unsafe(method_id(toolbarDefaultItemIdentifiers:))]
        fn default_items(&self, _toolbar: &NSToolbar) -> Retained<NSArray<NSString>> {
            toolbar_item_ids()
        }

        #[unsafe(method_id(toolbarAllowedItemIdentifiers:))]
        fn allowed_items(&self, toolbar: &NSToolbar) -> Retained<NSArray<NSString>> {
            toolbar_item_ids()
        }
    }
);

/// Adds the clipboard buttons to `native`'s toolbar. Call on the main thread.
fn install_toolbar(mtm: MainThreadMarker, app: &AppHandle, id: &str, native: &NSWindow) {
    let delegate = mtm.alloc::<ToolbarDelegate>().set_ivars(DelegateState {
        app: app.clone(),
        id: id.to_string(),
    });
    // SAFETY: A plain NSObject subclass initialised through its superclass.
    let delegate: Retained<ToolbarDelegate> = unsafe { msg_send![super(delegate), init] };
    let toolbar = NSToolbar::initWithIdentifier(
        mtm.alloc::<NSToolbar>(),
        &NSString::from_str("silo.macos-display"),
    );
    toolbar.setDisplayMode(NSToolbarDisplayMode::LabelOnly);
    toolbar.setAllowsUserCustomization(false);
    toolbar.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    native.setToolbar(Some(&toolbar));
    TOOLBARS.with(|toolbars| toolbars.borrow_mut().insert(id.to_string(), delegate));
}

/// Shows `text` under the title of the computer's screen window; empty clears it.
pub(super) fn set_display_subtitle(app: &AppHandle, id: &str, text: &str) {
    let (app_handle, label, text) = (app.clone(), super::display_label(id), text.to_string());
    let _ = app.run_on_main_thread(move || {
        use tauri::Manager;
        let Some(window) = app_handle.get_webview_window(&label) else {
            return;
        };
        let Ok(pointer) = window.ns_window() else {
            return;
        };
        // SAFETY: Main thread; Tauri owns the NSWindow, which outlives this closure.
        let native = unsafe { &*pointer.cast::<NSWindow>() };
        native.setSubtitle(&NSString::from_str(&text));
    });
}

/// Adds the machine's screen to `window`, filling its content view.
pub(super) fn attach_display(
    app: &AppHandle,
    id: &str,
    window: &tauri::WebviewWindow,
) -> Result<(), String> {
    let (id, window, handle) = (id.to_string(), window.clone(), app.clone());
    on_main(app, move |mtm| {
        SLOTS.with(|slots| {
            let mut slots = slots.borrow_mut();
            let slot = slots.get_mut(&id).ok_or("This computer isn't running.")?;
            let pointer = window
                .ns_window()
                .map_err(|_| "Silo could not open the display.".to_string())?;
            // Tauri owns the NSWindow; this closure runs on its AppKit thread.
            let native = unsafe { &*pointer.cast::<NSWindow>() };
            install_toolbar(mtm, &handle, &id, native);
            let content = native
                .contentView()
                .ok_or("Silo could not open the display.")?;
            // SAFETY: Main thread; the view is configured before it is shown.
            unsafe {
                if let Some(previous) = slot.view.take() {
                    previous.setVirtualMachine(None);
                    previous.removeFromSuperview();
                }
                let view = VZVirtualMachineView::new(mtm);
                view.setVirtualMachine(Some(&slot.vm));
                view.setCapturesSystemKeys(true);
                view.setAutomaticallyReconfiguresDisplay(true);
                let responder: &NSResponder = &view;
                let content_view: &NSView = &content;
                content_view.addSubview(&view);
                // The window may draw its content under the title bar and toolbar. The
                // content layout guide is the area they leave free, and it follows resizing
                // and full screen, so the screen (and its automatic resolution) fits it.
                let guide = native
                    .contentLayoutGuide()
                    .and_then(|guide| guide.downcast::<NSLayoutGuide>().ok());
                if let Some(guide) = guide {
                    view.setTranslatesAutoresizingMaskIntoConstraints(false);
                    for constraint in [
                        view.topAnchor().constraintEqualToAnchor(&guide.topAnchor()),
                        view.bottomAnchor()
                            .constraintEqualToAnchor(&guide.bottomAnchor()),
                        view.leadingAnchor()
                            .constraintEqualToAnchor(&guide.leadingAnchor()),
                        view.trailingAnchor()
                            .constraintEqualToAnchor(&guide.trailingAnchor()),
                    ] {
                        constraint.setActive(true);
                    }
                } else {
                    view.setFrame(content.bounds());
                    view.setAutoresizingMask(
                        NSAutoresizingMaskOptions::ViewWidthSizable
                            | NSAutoresizingMaskOptions::ViewHeightSizable,
                    );
                }
                native.makeFirstResponder(Some(responder));
                slot.view = Some(view);
            }
            Ok(())
        })
    })?
}

/// Detaches the display view; the machine keeps running. Safe from any thread.
pub(super) fn detach_display(app: &AppHandle, id: &str) {
    let id = id.to_string();
    let _ = app.run_on_main_thread(move || {
        TOOLBARS.with(|toolbars| toolbars.borrow_mut().remove(&id));
        SLOTS.with(|slots| {
            if let Some(slot) = slots.borrow_mut().get_mut(&id) {
                if let Some(view) = slot.view.take() {
                    // SAFETY: Main thread.
                    unsafe { view.setVirtualMachine(None) };
                    view.removeFromSuperview();
                }
            }
        });
    });
}

// MARK: Driving the display

/// Runs `work` on the main thread with the window that shows the machine's screen.
fn with_display_window<T: Send + 'static>(
    app: &AppHandle,
    id: &str,
    work: impl FnOnce(&NSWindow, &VZVirtualMachineView) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let id = id.to_string();
    on_main(app, move |_| {
        SLOTS.with(|slots| {
            let slots = slots.borrow();
            let view = slots
                .get(&id)
                .and_then(|slot| slot.view.as_ref())
                .ok_or("This computer's display isn't open.")?;
            let window = view.window().ok_or("This computer's display isn't open.")?;
            work(&window, view)
        })
    })?
}

/// Brings the display window forward with the machine's view first in line for
/// keys, and returns the window's number.
pub(super) fn focus_display(app: &AppHandle, id: &str) -> Result<isize, String> {
    with_display_window(app, id, |window, view| {
        window.makeKeyAndOrderFront(None);
        let responder: &NSResponder = view;
        window.makeFirstResponder(Some(responder));
        Ok(window.windowNumber())
    })
}

pub(super) fn send_keys(app: &AppHandle, id: &str, events: Vec<KeyEvent>) -> Result<(), String> {
    with_display_window(app, id, move |window, _| {
        guest_screen::deliver_keys(window, &events)
    })
}

/// Moves, presses or releases the pointer at a position given as fractions of
/// the machine's screen, measured from its bottom left.
pub(super) fn send_pointer(
    app: &AppHandle,
    id: &str,
    kind: PointerKind,
    x: f64,
    y: f64,
) -> Result<(), String> {
    with_display_window(app, id, move |window, view| {
        guest_screen::deliver_pointer(window, view, kind, x, y)
    })
}

/// The text on the machine's screen (the display view, not the window's title
/// bar).
pub(super) fn read_screen(app: &AppHandle, id: &str) -> Result<Vec<TextLine>, String> {
    let (number, geometry) = with_display_window(app, id, |window, view| {
        Ok((
            window.windowNumber(),
            guest_screen::ViewGeometry::of(window, view),
        ))
    })?;
    let image = guest_screen::capture(number, geometry)?;
    guest_screen::recognize(&image)
}

thread_local! {
    /// Main thread only: the monitor that drops the user's own input, per computer.
    static INPUT_MONITORS: RefCell<Monitors<Retained<AnyObject>>> =
        RefCell::new(Monitors::default());
}

/// Makes the display window ignore the user's keyboard and the pointer over the
/// machine's screen, and shows `subtitle` in its title bar. Events Silo
/// delivers with `window.sendEvent` do not pass through event monitors, so they
/// still arrive.
pub(super) fn lock_input(app: &AppHandle, id: &str, subtitle: &str) -> Result<(), String> {
    let (subtitle, key) = (subtitle.to_string(), id.to_string());
    with_display_window(app, id, move |window, view| {
        window.setSubtitle(&NSString::from_str(&subtitle));
        let number = window.windowNumber();
        let view = view.retain();
        let handler = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            // SAFETY: AppKit passes a valid event.
            let event_ref = unsafe { event.as_ref() };
            if event_ref.windowNumber() != number {
                return event.as_ptr();
            }
            let kind = event_ref.r#type().0;
            let keyboard = input::is_keyboard_event(kind);
            let over_screen = input::is_pointer_event(kind) && {
                let screen = view.convertRect_toView(view.bounds(), None);
                let at = event_ref.locationInWindow();
                at.x >= screen.origin.x
                    && at.x <= screen.origin.x + screen.size.width
                    && at.y >= screen.origin.y
                    && at.y <= screen.origin.y + screen.size.height
            };
            if keyboard || over_screen {
                std::ptr::null_mut()
            } else {
                event.as_ptr()
            }
        });
        // SAFETY: The block returns the event it was given or null.
        let monitor = unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(
                NSEventMask::from_bits_retain(u64::MAX),
                &handler,
            )
        };
        let monitor = monitor.ok_or("Silo could not lock the computer's window.")?;
        INPUT_MONITORS.with(|monitors| {
            if let Some(previous) = monitors.borrow_mut().insert(&key, monitor) {
                // SAFETY: The monitor came from an earlier call of this function.
                unsafe { NSEvent::removeMonitor(&previous) };
            }
            Ok(())
        })
    })
}

/// Gives the user's input back to the display window.
pub(super) fn unlock_input(app: &AppHandle, id: &str) {
    let id = id.to_string();
    let _ = on_main(app, move |_| {
        INPUT_MONITORS.with(|monitors| {
            if let Some(monitor) = monitors.borrow_mut().remove(&id) {
                // SAFETY: The monitor came from `lock_input`.
                unsafe { NSEvent::removeMonitor(&monitor) };
            }
        });
        SLOTS.with(|slots| {
            if let Some(window) = slots
                .borrow()
                .get(&id)
                .and_then(|slot| slot.view.as_ref())
                .and_then(|view| view.window())
            {
                window.setSubtitle(&NSString::from_str(""));
            }
        });
    });
}

/// Whether the machine has stopped and been released within `timeout`. A machine
/// that is stopping, stopped but not yet released, or running does not count.
pub(super) fn wait_until_stopped(app: &AppHandle, id: &str, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let released = machine_states(app)
            .is_ok_and(|states| !states.iter().any(|(machine, _)| machine == id));
        if released {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unsigned_test_binary_reports_the_missing_entitlement() {
        assert!(!has_virtualization_entitlement());
        let reason = unsupported_reason().unwrap();
        assert!(reason.contains("entitlement"), "{reason}");
    }

    #[test]
    fn a_start_is_retried_while_the_auxiliary_storage_is_locked() {
        let lock = "Invalid virtual machine configuration. Failed to lock auxiliary storage.";
        let mut calls = 0;
        let result = retry_while_locked(3, Duration::ZERO, || {
            calls += 1;
            if calls < 3 {
                Err(lock.to_string())
            } else {
                Ok(())
            }
        });
        assert_eq!(result, Ok(()));
        assert_eq!(calls, 3);

        let mut calls = 0;
        let result = retry_while_locked(2, Duration::ZERO, || {
            calls += 1;
            Err(lock.to_string())
        });
        assert_eq!(result, Err(lock.to_string()));
        assert_eq!(calls, 3);

        let mut calls = 0;
        let result = retry_while_locked(5, Duration::ZERO, || {
            calls += 1;
            Err("macOS allows at most two".to_string())
        });
        assert!(result.is_err());
        assert_eq!(calls, 1);
    }

    #[test]
    fn host_limits_describe_this_mac() {
        let host = host_limits();
        assert!(host.cpus >= 1);
        assert!(host.memory_gib >= 4);
    }

    #[test]
    fn generated_addresses_are_locally_administered_unicast() {
        let address = random_mac();
        let first = u8::from_str_radix(&address[..2], 16).unwrap();
        assert_eq!(first & 0b11, 0b10, "{address}");
        assert_eq!(address.len(), 17);
    }
}
