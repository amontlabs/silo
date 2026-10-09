//! The AppKit, Core Graphics and Vision side of driving a guest's display
//! window: synthesized input events, an image of the machine's screen and the
//! text in it. Only the screen is captured, never the window's title bar.
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
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType, NSView, NSWindow};
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
    fn CGImageGetWidth(image: *mut CGImage) -> usize;
    fn CGImageGetHeight(image: *mut CGImage) -> usize;
    fn CGImageCreateWithImageInRect(image: *mut CGImage, rect: NSRect) -> *mut CGImage;
}

/// Where the machine's screen sits in its window, in points with the origin at
/// the window's bottom left.
#[derive(Clone, Copy, Debug)]
pub(super) struct ViewGeometry {
    pub window: NSSize,
    pub view: NSRect,
}

impl ViewGeometry {
    pub(super) fn of(window: &NSWindow, view: &NSView) -> Self {
        Self {
            window: window.frame().size,
            view: view.convertRect_toView(view.bounds(), None),
        }
    }
}

#[link(name = "Vision", kind = "framework")]
extern "C" {}

#[link(name = "ImageIO", kind = "framework")]
extern "C" {
    fn CGImageDestinationCreateWithURL(
        url: *const AnyObject,
        kind: *const AnyObject,
        count: usize,
        options: *const AnyObject,
    ) -> *mut AnyObject;
    fn CGImageDestinationAddImage(
        destination: *mut AnyObject,
        image: *mut CGImage,
        properties: *const AnyObject,
    );
    fn CGImageDestinationFinalize(destination: *mut AnyObject) -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(object: *const AnyObject);
}

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
        let symbol =
            unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"CGWindowListCreateImage".as_ptr()) };
        // SAFETY: The exported function has this signature.
        (!symbol.is_null())
            .then(|| unsafe { std::mem::transmute::<*mut libc::c_void, CaptureFunction>(symbol) })
    })
}

/// Captures the part of the window with this number that shows the machine's
/// screen, leaving out the title bar. Callable from any thread.
pub(super) fn capture(window_number: isize, geometry: ViewGeometry) -> Result<Capture, String> {
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
        return Err("The computer's window could not be captured.".into());
    }
    let whole = Capture(image);
    // SAFETY: Plain Core Graphics calls on a valid image.
    let (width, height) = unsafe { (CGImageGetWidth(whole.0), CGImageGetHeight(whole.0)) };
    let (scale_x, scale_y) = (
        width as f64 / geometry.window.width,
        height as f64 / geometry.window.height,
    );
    let view = geometry.view;
    let crop = NSRect::new(
        NSPoint::new(
            view.origin.x * scale_x,
            (geometry.window.height - view.origin.y - view.size.height) * scale_y,
        ),
        NSSize::new(view.size.width * scale_x, view.size.height * scale_y),
    );
    // SAFETY: The rectangle is in the image's pixel space; the result is owned.
    let cropped = unsafe { CGImageCreateWithImageInRect(whole.0, crop) };
    if cropped.is_null() {
        return Err("The computer's screen could not be captured.".into());
    }
    Ok(Capture(cropped))
}

impl Capture {
    /// Writes the image to `path` as a PNG.
    pub(super) fn write_png(&self, path: &std::path::Path) -> Result<(), String> {
        let url =
            objc2_foundation::NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        let kind = NSString::from_str("public.png");
        // SAFETY: The URL and type are toll-free bridged to their Core Foundation
        // counterparts; the destination is created, finalized and released here.
        unsafe {
            let destination = CGImageDestinationCreateWithURL(
                Retained::as_ptr(&url).cast(),
                Retained::as_ptr(&kind).cast(),
                1,
                std::ptr::null(),
            );
            if destination.is_null() {
                return Err("The screen image could not be saved.".into());
            }
            CGImageDestinationAddImage(destination, self.0, std::ptr::null());
            let written = CGImageDestinationFinalize(destination);
            CFRelease(destination);
            if written {
                Ok(())
            } else {
                Err("The screen image could not be saved.".into())
            }
        }
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
                format!("Text recognition failed: {}", error.localizedDescription())
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

/// Sends a pointer event at a position given as fractions of the machine's
/// screen, measured from its bottom left.
pub(super) fn deliver_pointer(
    window: &NSWindow,
    view: &NSView,
    kind: PointerKind,
    x: f64,
    y: f64,
) -> Result<(), String> {
    let bounds = view.bounds();
    let y = if view.isFlipped() { 1.0 - y } else { y };
    let location = view.convertPoint_toView(
        NSPoint::new(
            bounds.origin.x + x * bounds.size.width,
            bounds.origin.y + y * bounds.size.height,
        ),
        None,
    );
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
