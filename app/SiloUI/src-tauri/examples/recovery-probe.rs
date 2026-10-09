//! Boots a copy of a macOS computer into Recovery in a plain window and logs
//! what Silo's screen reader sees, saving frames. Uses the real input and
//! guest_screen modules.
//! Usage: recovery-probe <computer dir> <output dir> [seconds] [keys: 1|0]
#![allow(dead_code, unused_imports)]
#[path = "../src/macos_computers/guest_screen.rs"]
mod guest_screen;
#[path = "../src/macos_computers/input.rs"]
mod input;

use block2::RcBlock;
use guest_screen::ViewGeometry;
use input::{Keyboard, TextLine};
use objc2::{rc::Retained, AnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::*;
use objc2_foundation::*;
use objc2_virtualization::*;
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

thread_local! {
    static STATE: RefCell<Option<(Retained<NSWindow>, Retained<VZVirtualMachineView>, Retained<VZVirtualMachine>)>> = RefCell::new(None);
}

fn on_main<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    let work = std::sync::Mutex::new(Some(work));
    let block = RcBlock::new(move || {
        if let Some(w) = work.lock().unwrap().take() {
            let _ = tx.send(w());
        }
    });
    unsafe { NSOperationQueue::mainQueue().addOperationWithBlock(&block) };
    rx.recv_timeout(Duration::from_secs(20))
        .expect("main thread")
}

fn url(p: &Path) -> Retained<NSURL> {
    NSURL::fileURLWithPath(&NSString::from_str(&p.to_string_lossy()))
}

fn build(dir: &Path, mtm: MainThreadMarker) -> Result<(), String> {
    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("computer.json")).unwrap()).unwrap();
    let mac = meta["macAddress"].as_str().unwrap().to_string();
    let model = std::fs::read(dir.join("hardware-model.bin")).unwrap();
    let ident = std::fs::read(dir.join("machine-identifier.bin")).unwrap();
    unsafe {
        let model = VZMacHardwareModel::initWithDataRepresentation(
            VZMacHardwareModel::alloc(),
            &NSData::with_bytes(&model),
        )
        .ok_or("model")?;
        let ident = VZMacMachineIdentifier::initWithDataRepresentation(
            VZMacMachineIdentifier::alloc(),
            &NSData::with_bytes(&ident),
        )
        .ok_or("ident")?;
        let aux = VZMacAuxiliaryStorage::initWithURL(
            VZMacAuxiliaryStorage::alloc(),
            &url(&dir.join("auxiliary-storage.img")),
        );
        let platform = VZMacPlatformConfiguration::new();
        platform.setHardwareModel(&model);
        platform.setAuxiliaryStorage(Some(&aux));
        platform.setMachineIdentifier(&ident);
        let c = VZVirtualMachineConfiguration::new();
        c.setBootLoader(Some(&VZMacOSBootLoader::new()));
        c.setPlatform(&platform);
        c.setCPUCount(meta["cpus"].as_u64().unwrap() as usize);
        c.setMemorySize(meta["memoryGiB"].as_u64().unwrap() * 1024 * 1024 * 1024);
        let display =
            VZMacGraphicsDisplayConfiguration::initWithWidthInPixels_heightInPixels_pixelsPerInch(
                VZMacGraphicsDisplayConfiguration::alloc(),
                2560,
                1600,
                220,
            );
        let graphics = VZMacGraphicsDeviceConfiguration::new();
        graphics.setDisplays(&NSArray::from_retained_slice(&[display]));
        c.setGraphicsDevices(&NSArray::from_retained_slice(&[graphics.into_super()]));
        let att = VZDiskImageStorageDeviceAttachment::initWithURL_readOnly_error(
            VZDiskImageStorageDeviceAttachment::alloc(),
            &url(&dir.join("disk.img")),
            false,
        )
        .map_err(|e| e.localizedDescription().to_string())?;
        let disk = VZVirtioBlockDeviceConfiguration::initWithAttachment(
            VZVirtioBlockDeviceConfiguration::alloc(),
            &att,
        );
        c.setStorageDevices(&NSArray::from_retained_slice(&[disk.into_super()]));
        let net = VZVirtioNetworkDeviceConfiguration::new();
        net.setAttachment(Some(&VZNATNetworkDeviceAttachment::new()));
        net.setMACAddress(
            &VZMACAddress::initWithString(VZMACAddress::alloc(), &NSString::from_str(&mac))
                .unwrap(),
        );
        c.setNetworkDevices(&NSArray::from_retained_slice(&[net.into_super()]));
        c.setPointingDevices(&NSArray::from_retained_slice(&[
            VZMacTrackpadConfiguration::new().into_super(),
            VZUSBScreenCoordinatePointingDeviceConfiguration::new().into_super(),
        ]));
        c.setKeyboards(&NSArray::from_retained_slice(&[
            VZMacKeyboardConfiguration::new().into_super(),
        ]));
        c.setEntropyDevices(&NSArray::from_retained_slice(&[
            VZVirtioEntropyDeviceConfiguration::new().into_super(),
        ]));
        c.validateWithError()
            .map_err(|e| e.localizedDescription().to_string())?;
        let vm = VZVirtualMachine::initWithConfiguration(VZVirtualMachine::alloc(), &c);

        let window = NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(100.0, 100.0), NSSize::new(1440.0, 900.0)),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Resizable | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        );
        window.setReleasedWhenClosed(false);
        window.setTitle(&NSString::from_str("recovery-probe"));
        let content = window.contentView().unwrap();
        let view = VZVirtualMachineView::new(mtm);
        view.setVirtualMachine(Some(&vm));
        view.setCapturesSystemKeys(true);
        view.setAutomaticallyReconfiguresDisplay(true);
        view.setFrame(content.bounds());
        view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        content.addSubview(&view);
        window.makeKeyAndOrderFront(None);
        window.makeFirstResponder(Some(&view));
        let handler = RcBlock::new(|e: *mut NSError| {
            if let Some(e) = e.as_ref() {
                eprintln!("start error: {}", e.localizedDescription());
            } else {
                eprintln!("started");
            }
        });
        let options = VZMacOSVirtualMachineStartOptions::new();
        options.setStartUpFromMacOSRecovery(true);
        vm.startWithOptions_completionHandler(&options, &handler);
        STATE.with(|s| *s.borrow_mut() = Some((window, view, vm)));
    }
    Ok(())
}

fn read(out: &Path, n: usize, last_hash: &mut u64, t0: Instant) -> bool {
    let mut seen = false;
    let (number, geom): (isize, ViewGeometry) = on_main(|| {
        STATE.with(|s| {
            let s = s.borrow();
            let (w, v, _) = s.as_ref().unwrap();
            (w.windowNumber(), ViewGeometry::of(w, v))
        })
    });
    let at = t0.elapsed().as_secs();
    match guest_screen::capture(number, geom) {
        Err(e) => eprintln!("[{at}s] capture error: {e}"),
        Ok(cap) => match guest_screen::recognize(&cap) {
            Err(e) => eprintln!("[{at}s] ocr error: {e}"),
            Ok(lines) => {
                let text: Vec<String> = lines.iter().map(|l| l.text.clone()).collect();
                let joined = text.join("\n");
                seen = joined.to_lowercase().contains("options");
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                joined.hash(&mut h);
                let hash = h.finish();
                let changed = hash != *last_hash;
                eprintln!(
                    "[{at}s] {} lines{}",
                    lines.len(),
                    if changed { " CHANGED" } else { "" }
                );
                if changed {
                    *last_hash = hash;
                    let _ = cap.write_png(&out.join(format!("frame-{n:03}-{at}s.png")));
                    let _ = std::fs::write(out.join(format!("frame-{n:03}-{at}s.txt")), joined);
                }
            }
        },
    }
    seen
}

fn keys(codes: &[u16]) {
    let mut kb = Keyboard::default();
    for &c in codes {
        let ev = kb.tap(c, None).to_vec();
        on_main(move || {
            STATE.with(|s| {
                let s = s.borrow();
                let (w, _, _) = s.as_ref().unwrap();
                guest_screen::deliver_keys(w, &ev).unwrap();
            })
        });
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = PathBuf::from(&args[1]);
    let out = PathBuf::from(&args[2]);
    let secs: u64 = args.get(3).map_or(300, |s| s.parse().unwrap());
    let press: bool = args.get(4).map_or(true, |s| s == "1");
    std::fs::create_dir_all(&out).unwrap();
    let mtm = MainThreadMarker::new().unwrap();
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    build(&dir, mtm).expect("build");
    std::thread::spawn(move || {
        let t0 = Instant::now();
        let (mut n, mut hash, mut pressed, mut chose) = (0, 0u64, false, false);
        while t0.elapsed().as_secs() < secs {
            let seen = read(&out, n, &mut hash, t0);
            n += 1;
            if press && !pressed && seen {
                pressed = true;
                eprintln!("[{}s] pressing right right return", t0.elapsed().as_secs());
                keys(&[input::RIGHT, input::RIGHT, input::RETURN]);
            }
            if press && pressed && !chose && t0.elapsed().as_secs() > 95 {
                chose = true;
                eprintln!("[{}s] pressing down down return", t0.elapsed().as_secs());
                keys(&[input::DOWN, input::DOWN, input::RETURN]);
            }
            std::thread::sleep(Duration::from_millis(1500));
        }
        eprintln!("done");
        std::process::exit(0);
    });
    app.run();
}
