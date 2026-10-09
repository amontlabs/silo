//! The AppKit, Core Graphics and Vision side of driving a guest's display
//! window: synthesized input events, an image of the window and the text in it.
//!
//! Silo's own window is captured with `CGWindowListCreateImage`, which needs no
//! Screen Recording permission for a window of the calling process. The function
//! is marked unavailable in the current SDK headers but is still exported, so it
//! is resolved at run time and its absence is reported as an error.
use super::input::{EventKind, KeyEvent, PointerKind, TextLine};
use objc2::{
    msg_send,
    rc::{autoreleasepool, Retained},
    runtime::{AnyClass, AnyObject},
    Encoding, RefEncode,
};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType, NSWindow};
use objc2_foundation::{
    NSArray, NSDictionary, NSError, NSPoint, NSProcessInfo, NSRect, NSSize, NSString,
};
use std::sync::OnceLock;

const LIST_OPTION_INCLUDING_WINDOW: u32 = 1 << 3;
const IMAGE_OPTION_BOUNDS_IGNORE_FRAMING: u32 = 1;
const IMAGE_OPTION_BEST_RESOLUTION: u32 = 1 << 3;
/// `VNRequestTextRecognitionLevelAccurate`.
const RECOGNITION_ACCURATE: isize = 0;

#[repr(C)]
pub(super) struct CGImage {
    _private: [u8; 0],
}

// SAFETY: The encoding is that of a pointer to the opaque `CGImage` struct.
unsafe impl RefEncode for CGImage {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("CGImage", &[]));
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGImageRelease(image: *mut CGImage);
}

#[link(name = "Vision", kind = "framework")]
extern "C" {}

/// An image of a window, released on drop.
pub(super) struct Capture(*mut CGImage);

// SAFETY: A `CGImage` is immutable and reference counted; it may move between threads.
unsafe impl Send for Capture {}

impl Drop for Capture {
    fn drop(&mut self) {
        // SAFETY: The pointer came from a create function and is released once.
        unsafe { CGImageRelease(self.0) };
    }
}

type CaptureFunction = unsafe extern "C" fn(NSRect, u32, u32, u32) -> *mut CGImage;

fn capture_function() -> Option<CaptureFunction> {
    static FUNCTION: OnceLock<Option<CaptureFunction>> = OnceLock::new();
    *FUNCTION.get_or_init(|| {
        // SAFETY: `dlsym` on the default namespace with a valid C string.
        let symbol = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"CGWindowListCreateImage".as_ptr()) };
        // SAFETY: The exported function has this signature.
        (!symbol.is_null())
            .then(|| unsafe { std::mem::transmute::<*mut libc::c_void, CaptureFunction>(symbol) })
    })
}

/// Captures the window with this number, including its title bar. Callable from
/// any thread.
pub(super) fn capture(window_number: isize) -> Result<Capture, String> {
    let function = capture_function().ok_or("This macOS cannot capture Silo's windows.")?;
    let everything = NSRect::new(
        NSPoint::new(f64::INFINITY, f64::INFINITY),
        NSSize::new(0.0, 0.0),
    );
    // SAFETY: Plain Core Graphics call with a window number.
    let image = unsafe {
        function(
            everything,
            LIST_OPTION_INCLUDING_WINDOW,
            window_number as u32,
            IMAGE_OPTION_BOUNDS_IGNORE_FRAMING | IMAGE_OPTION_BEST_RESOLUTION,
        )
    };
    if image.is_null() {
        Err("The computer's window could not be captured.".into())
    } else {
        Ok(Capture(image))
    }
}

/// Reads the text in a capture, top to bottom. Callable from any thread.
pub(super) fn recognize(capture: &Capture) -> Result<Vec<TextLine>, String> {
    autoreleasepool(|_| {
        let unavailable = || "Text recognition is not available on this Mac.".to_string();
        let request_class = AnyClass::get(c"VNRecognizeTextRequest").ok_or_else(unavailable)?;
        let handler_class = AnyClass::get(c"VNImageRequestHandler").ok_or_else(unavailable)?;
        // SAFETY: Vision messages with the documented argument and return types.
        unsafe {
            let request: Retained<AnyObject> = msg_send![request_class, new];
            let _: () = msg_send![&*request, setRecognitionLevel: RECOGNITION_ACCURATE];
            let _: () = msg_send![&*request, setUsesLanguageCorrection: false];
            let options = NSDictionary::<NSString, AnyObject>::new();
            let handler: Retained<AnyObject> = msg_send![
                msg_send![handler_class, alloc],
                initWithCGImage: capture.0,
                options: &*options
            ];
            let requests = NSArray::from_slice(&[&*request]);
            let performed: Result<(), Retained<NSError>> =
                msg_send![&*handler, performRequests: &*requests, error: _];
            performed.map_err(|error| {
                format!(
                    "Text recognition failed: {}",
                    error.localizedDescription()
                )
            })?;
            let results: Option<Retained<NSArray<AnyObject>>> = msg_send![&*request, results];
            let mut lines = Vec::new();
            for observation in results.iter().flat_map(|results| results.iter()) {
                let candidates: Retained<NSArray<AnyObject>> =
                    msg_send![&*observation, topCandidates: 1usize];
                let Some(top) = candidates.firstObject() else {
                    continue;
                };
                let text: Retained<NSString> = msg_send![&*top, string];
                let bounds: NSRect = msg_send![&*observation, boundingBox];
                lines.push(TextLine {
                    text: text.to_string(),
                    x: bounds.origin.x,
                    y: bounds.origin.y,
                    width: bounds.size.width,
                    height: bounds.size.height,
                });
            }
            lines.sort_by(|a, b| (b.y + b.height).total_cmp(&(a.y + a.height)));
            Ok(lines)
        }
    })
}

fn uptime() -> f64 {
    NSProcessInfo::processInfo().systemUptime()
}

/// Sends keyboard events to the window's first responder, the display view.
pub(super) fn deliver_keys(window: &NSWindow, events: &[KeyEvent]) -> Result<(), String> {
    for event in events {
        let kind = match event.kind {
            EventKind::KeyDown => NSEventType::KeyDown,
            EventKind::KeyUp => NSEventType::KeyUp,
            EventKind::FlagsChanged => NSEventType::FlagsChanged,
        };
        let characters = NSString::from_str(&event.characters);
        let built =
            NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
                kind,
                NSPoint::new(0.0, 0.0),
                NSEventModifierFlags(event.flags),
                uptime(),
                window.windowNumber(),
                None,
                &characters,
                &characters,
                false,
                event.code,
            )
            .ok_or("Silo could not build a keyboard event.")?;
        window.sendEvent(&built);
    }
    Ok(())
}

/// Sends a pointer event at a position given as fractions of the window's frame,
/// measured from its bottom left.
pub(super) fn deliver_pointer(
    window: &NSWindow,
    kind: PointerKind,
    x: f64,
    y: f64,
) -> Result<(), String> {
    let size = window.frame().size;
    let location = NSPoint::new(x * size.width, y * size.height);
    let (event_type, pressure) = match kind {
        PointerKind::Move => (NSEventType::MouseMoved, 0.0),
        PointerKind::Down => (NSEventType::LeftMouseDown, 1.0),
        PointerKind::Up => (NSEventType::LeftMouseUp, 0.0),
    };
    window.setAcceptsMouseMovedEvents(true);
    let built = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
        event_type,
        location,
        NSEventModifierFlags(0),
        uptime(),
        window.windowNumber(),
        None,
        0,
        1,
        pressure,
    )
    .ok_or("Silo could not build a pointer event.")?;
    window.sendEvent(&built);
    Ok(())
}
