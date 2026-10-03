//! Device clipboard access for the desktop viewer, kept Rust-only.
//!
//! `arboard` is Send + Sync and has no main-thread requirement on macOS, so every method here may
//! run on a blocking worker. Calls block on the platform clipboard and must not run on an async
//! executor thread.
//!
//! The system clipboard lives in a process-lifetime static. On X11 and Wayland, arboard serves
//! the selection from a background thread owned by the `Clipboard`; dropping it would end
//! ownership, so the static is never dropped and shutdown never waits on it.
//!
//! Platform calls run on a dedicated worker thread and each caller waits a bounded time. A call
//! that does not return in time is reported as unavailable and its worker is abandoned; the next
//! call starts a fresh worker, up to a small cap on workers that are still stuck.

#![allow(dead_code)]

use std::fmt;
use std::io::Cursor;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};

/// Limits applied to image reads and writes in either direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageLimits {
    pub max_encoded_bytes: usize,
    pub max_pixels: u64,
}

impl ImageLimits {
    pub const DEFAULT: ImageLimits = ImageLimits {
        max_encoded_bytes: 16 * 1024 * 1024,
        max_pixels: 32 * 1024 * 1024,
    };
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PngBytes {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipboardError {
    /// A write was given no data.
    Empty,
    /// The content exceeds a byte or pixel limit.
    TooLarge { limit: u64 },
    /// The MIME type or image format is not accepted.
    Unsupported(String),
    /// The platform clipboard cannot be reached.
    Unavailable(String),
    /// The image bytes do not decode as the declared format.
    Decode(String),
}

impl fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("There is nothing to put on the clipboard."),
            Self::TooLarge { limit } => {
                write!(f, "The clipboard content exceeds the limit of {limit}.")
            }
            Self::Unsupported(what) => write!(f, "Unsupported clipboard content: {what}."),
            Self::Unavailable(why) => write!(f, "The device clipboard is unavailable: {why}."),
            Self::Decode(why) => write!(f, "The image could not be decoded: {why}."),
        }
    }
}

impl std::error::Error for ClipboardError {}

pub type ClipboardResult<T> = Result<T, ClipboardError>;

/// Clipboard operations the viewer bridge depends on. Implemented by [`SystemClipboard`] and by
/// the in-memory fake used in tests.
pub trait DeviceClipboard: Send + Sync {
    /// Returns `None` when the clipboard holds no text. Text larger than `max_bytes` is rejected
    /// with [`ClipboardError::TooLarge`], never truncated.
    fn read_text(&self, max_bytes: usize) -> ClipboardResult<Option<String>>;

    fn write_text(&self, text: &str) -> ClipboardResult<()>;

    /// Returns the clipboard image as PNG, or `None` when it holds no image.
    fn read_image(&self, limits: ImageLimits) -> ClipboardResult<Option<PngBytes>>;

    /// Decodes PNG, JPEG, WebP or BMP bytes and places the pixels on the clipboard.
    fn write_image_from_encoded(
        &self,
        bytes: &[u8],
        mime: &str,
        limits: ImageLimits,
    ) -> ClipboardResult<()>;
}

/// Straight-alpha RGBA8 pixels.
pub struct RawImage {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

/// Platform clipboard primitives that [`ClipboardService`] builds its policy on.
pub trait RawClipboard: Send + Sync {
    fn get_text(&self) -> ClipboardResult<Option<String>>;
    fn set_text(&self, text: &str) -> ClipboardResult<()>;
    fn get_image(&self) -> ClipboardResult<Option<RawImage>>;
    fn set_image(&self, image: RawImage) -> ClipboardResult<()>;
}

/// Size, pixel and format policy over a [`RawClipboard`].
pub struct ClipboardService<R> {
    raw: R,
}

impl<R: RawClipboard> ClipboardService<R> {
    pub fn new(raw: R) -> Self {
        Self { raw }
    }
}

pub type SystemClipboard = ClipboardService<BoundedClipboard<ArboardClipboard>>;

/// The device clipboard shared by the whole process.
pub fn system() -> &'static SystemClipboard {
    static SYSTEM: OnceLock<SystemClipboard> = OnceLock::new();
    SYSTEM.get_or_init(|| ClipboardService::new(BoundedClipboard::new(ArboardClipboard::default)))
}

impl<R: RawClipboard> DeviceClipboard for ClipboardService<R> {
    fn read_text(&self, max_bytes: usize) -> ClipboardResult<Option<String>> {
        match self.raw.get_text()? {
            Some(text) if text.is_empty() => Ok(None),
            Some(text) if text.len() > max_bytes => Err(ClipboardError::TooLarge {
                limit: max_bytes as u64,
            }),
            other => Ok(other),
        }
    }

    fn write_text(&self, text: &str) -> ClipboardResult<()> {
        if text.is_empty() {
            return Err(ClipboardError::Empty);
        }
        self.raw.set_text(text)
    }

    fn read_image(&self, limits: ImageLimits) -> ClipboardResult<Option<PngBytes>> {
        let Some(image) = self.raw.get_image()? else {
            return Ok(None);
        };
        let (Ok(width), Ok(height)) = (u32::try_from(image.width), u32::try_from(image.height))
        else {
            return Err(ClipboardError::TooLarge {
                limit: limits.max_pixels,
            });
        };
        if width == 0 || height == 0 {
            return Ok(None);
        }
        check_pixels(width, height, limits)?;
        let Some(expected) = rgba_len(width, height) else {
            return Err(ClipboardError::TooLarge {
                limit: limits.max_pixels,
            });
        };
        if expected != image.rgba.len() {
            return Err(ClipboardError::Decode(
                "pixel buffer does not match its dimensions".into(),
            ));
        }
        let bytes = encode_png(width, height, image.rgba, limits.max_encoded_bytes)?;
        Ok(Some(PngBytes {
            bytes,
            width,
            height,
        }))
    }

    fn write_image_from_encoded(
        &self,
        bytes: &[u8],
        mime: &str,
        limits: ImageLimits,
    ) -> ClipboardResult<()> {
        let image = decode_image(bytes, mime, limits)?;
        self.raw.set_image(image)
    }
}

/// Maps a MIME type, ignoring case and parameters, to an accepted image format.
pub fn format_for_mime(mime: &str) -> ClipboardResult<ImageFormat> {
    let essence = mime
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match essence.as_str() {
        "image/png" => Ok(ImageFormat::Png),
        "image/jpeg" | "image/jpg" => Ok(ImageFormat::Jpeg),
        "image/webp" => Ok(ImageFormat::WebP),
        "image/bmp" | "image/x-bmp" | "image/x-ms-bmp" => Ok(ImageFormat::Bmp),
        _ => Err(ClipboardError::Unsupported(format!(
            "MIME type {essence:?}"
        ))),
    }
}

fn pixel_limits(limits: ImageLimits) -> Limits {
    let side = u32::try_from(limits.max_pixels).unwrap_or(u32::MAX);
    let mut out = Limits::default();
    out.max_image_width = Some(side);
    out.max_image_height = Some(side);
    out.max_alloc = Some(limits.max_pixels.saturating_mul(8));
    out
}

/// Byte length of an RGBA8 buffer, or `None` when it does not fit in `usize`.
fn rgba_len(width: u32, height: u32) -> Option<usize> {
    (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)
}

fn check_pixels(width: u32, height: u32, limits: ImageLimits) -> ClipboardResult<()> {
    match u64::from(width).checked_mul(u64::from(height)) {
        Some(pixels) if pixels <= limits.max_pixels => Ok(()),
        _ => Err(ClipboardError::TooLarge {
            limit: limits.max_pixels,
        }),
    }
}

fn decode_image(bytes: &[u8], mime: &str, limits: ImageLimits) -> ClipboardResult<RawImage> {
    let declared = format_for_mime(mime)?;
    if bytes.is_empty() {
        return Err(ClipboardError::Empty);
    }
    if bytes.len() > limits.max_encoded_bytes {
        return Err(ClipboardError::TooLarge {
            limit: limits.max_encoded_bytes as u64,
        });
    }
    let sniffed =
        image::guess_format(bytes).map_err(|error| ClipboardError::Decode(error.to_string()))?;
    if sniffed != declared {
        return Err(ClipboardError::Decode(format!(
            "content is {sniffed:?}, not the declared {declared:?}"
        )));
    }

    let mut reader = ImageReader::with_format(Cursor::new(bytes), declared);
    reader.limits(pixel_limits(limits));
    let decoder = reader
        .into_decoder()
        .map_err(|error| map_image_error(error, limits))?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 {
        return Err(ClipboardError::Decode("image has no pixels".into()));
    }
    check_pixels(width, height, limits)?;
    if decoder.total_bytes() > limits.max_pixels.saturating_mul(8) {
        return Err(ClipboardError::TooLarge {
            limit: limits.max_pixels,
        });
    }
    let image =
        DynamicImage::from_decoder(decoder).map_err(|error| map_image_error(error, limits))?;
    let (width, height) = (image.width(), image.height());
    check_pixels(width, height, limits)?;
    let len = rgba_len(width, height).ok_or(ClipboardError::TooLarge {
        limit: limits.max_pixels,
    })?;
    let rgba = image.into_rgba8().into_raw();
    if rgba.len() != len {
        return Err(ClipboardError::Decode(
            "pixel buffer does not match its dimensions".into(),
        ));
    }
    Ok(RawImage {
        width: width as usize,
        height: height as usize,
        rgba,
    })
}

fn map_image_error(error: image::ImageError, limits: ImageLimits) -> ClipboardError {
    match error {
        image::ImageError::Limits(_) => ClipboardError::TooLarge {
            limit: limits.max_pixels,
        },
        other => ClipboardError::Decode(other.to_string()),
    }
}

fn encode_png(
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    max_bytes: usize,
) -> ClipboardResult<Vec<u8>> {
    let buffer = image::RgbaImage::from_raw(width, height, rgba).ok_or_else(|| {
        ClipboardError::Decode("pixel buffer does not match its dimensions".into())
    })?;
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(buffer)
        .write_to(&mut out, ImageFormat::Png)
        .map_err(|error| ClipboardError::Decode(error.to_string()))?;
    let out = out.into_inner();
    if out.len() > max_bytes {
        return Err(ClipboardError::TooLarge {
            limit: max_bytes as u64,
        });
    }
    Ok(out)
}

/// [`RawClipboard`] backed by `arboard`. The handle is created on first use and retained for the
/// life of the process so that a Linux selection stays served after a write returns.
#[derive(Default)]
pub struct ArboardClipboard {
    handle: Mutex<Option<arboard::Clipboard>>,
}

impl ArboardClipboard {
    fn with<T>(
        &self,
        action: impl FnOnce(&mut arboard::Clipboard) -> Result<T, arboard::Error>,
    ) -> ClipboardResult<T> {
        let mut guard = self
            .handle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.is_none() {
            *guard = Some(arboard::Clipboard::new().map_err(map_arboard_error)?);
        }
        let clipboard = guard.as_mut().expect("clipboard handle was just created");
        action(clipboard).map_err(map_arboard_error)
    }
}

impl Drop for ArboardClipboard {
    fn drop(&mut self) {
        let handle = self
            .handle
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        std::mem::forget(handle);
    }
}

/// How long a caller waits for a platform clipboard call.
const CALL_TIMEOUT: Duration = Duration::from_secs(3);
/// Workers allowed to exist at once, counting abandoned ones that are still stuck.
const MAX_WORKERS: usize = 3;

type Job<R> = Box<dyn FnOnce(&R) + Send>;

struct Worker<R> {
    id: u64,
    jobs: mpsc::Sender<Job<R>>,
}

/// Runs each [`RawClipboard`] call on a worker thread that owns the platform handle, so a call
/// that never returns cannot block other callers. After a timeout the worker is abandoned and
/// the next call creates a new one from `make`.
pub struct BoundedClipboard<R: RawClipboard + 'static> {
    make: Arc<dyn Fn() -> R + Send + Sync>,
    timeout: Duration,
    current: Mutex<Option<Worker<R>>>,
    live: Arc<AtomicUsize>,
    next_id: AtomicUsize,
}

impl<R: RawClipboard + 'static> BoundedClipboard<R> {
    pub fn new(make: impl Fn() -> R + Send + Sync + 'static) -> Self {
        Self::with_timeout(make, CALL_TIMEOUT)
    }

    pub fn with_timeout(make: impl Fn() -> R + Send + Sync + 'static, timeout: Duration) -> Self {
        Self {
            make: Arc::new(make),
            timeout,
            current: Mutex::new(None),
            live: Arc::new(AtomicUsize::new(0)),
            next_id: AtomicUsize::new(0),
        }
    }

    fn sender(&self) -> ClipboardResult<(u64, mpsc::Sender<Job<R>>)> {
        let mut current = self
            .current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(worker) = current.as_ref() {
            return Ok((worker.id, worker.jobs.clone()));
        }
        if self.live.load(Ordering::SeqCst) >= MAX_WORKERS {
            return Err(ClipboardError::Unavailable(
                "earlier clipboard calls have not returned".into(),
            ));
        }
        let (jobs, queue) = mpsc::channel::<Job<R>>();
        let make = Arc::clone(&self.make);
        let live = Arc::clone(&self.live);
        live.fetch_add(1, Ordering::SeqCst);
        let spawned = std::thread::Builder::new()
            .name("silo-clipboard".into())
            .spawn({
                let live = Arc::clone(&live);
                move || {
                    let raw = make();
                    while let Ok(job) = queue.recv() {
                        job(&raw);
                    }
                    live.fetch_sub(1, Ordering::SeqCst);
                }
            });
        if let Err(error) = spawned {
            live.fetch_sub(1, Ordering::SeqCst);
            return Err(ClipboardError::Unavailable(error.to_string()));
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst) as u64;
        *current = Some(Worker {
            id,
            jobs: jobs.clone(),
        });
        Ok((id, jobs))
    }

    fn abandon(&self, id: u64) {
        let mut current = self
            .current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if current.as_ref().is_some_and(|worker| worker.id == id) {
            *current = None;
        }
    }

    fn run<T: Send + 'static>(
        &self,
        call: impl FnOnce(&R) -> ClipboardResult<T> + Send + 'static,
    ) -> ClipboardResult<T> {
        let (id, jobs) = self.sender()?;
        let (reply, answer) = mpsc::channel();
        let job: Job<R> = Box::new(move |raw| {
            let _ = reply.send(call(raw));
        });
        if jobs.send(job).is_err() {
            self.abandon(id);
            return Err(ClipboardError::Unavailable(
                "the clipboard worker stopped".into(),
            ));
        }
        match answer.recv_timeout(self.timeout) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                self.abandon(id);
                Err(ClipboardError::Unavailable("timed out".into()))
            }
            Err(RecvTimeoutError::Disconnected) => {
                self.abandon(id);
                Err(ClipboardError::Unavailable(
                    "the clipboard worker stopped".into(),
                ))
            }
        }
    }
}

impl<R: RawClipboard + 'static> RawClipboard for BoundedClipboard<R> {
    fn get_text(&self) -> ClipboardResult<Option<String>> {
        self.run(|raw| raw.get_text())
    }

    fn set_text(&self, text: &str) -> ClipboardResult<()> {
        let text = text.to_owned();
        self.run(move |raw| raw.set_text(&text))
    }

    fn get_image(&self) -> ClipboardResult<Option<RawImage>> {
        self.run(|raw| raw.get_image())
    }

    fn set_image(&self, image: RawImage) -> ClipboardResult<()> {
        self.run(move |raw| raw.set_image(image))
    }
}

fn map_arboard_error(error: arboard::Error) -> ClipboardError {
    match error {
        arboard::Error::ContentNotAvailable => ClipboardError::Empty,
        arboard::Error::ClipboardNotSupported => {
            ClipboardError::Unsupported("this clipboard selection".into())
        }
        arboard::Error::ClipboardOccupied => {
            ClipboardError::Unavailable("another program holds the clipboard".into())
        }
        arboard::Error::ConversionFailure => {
            ClipboardError::Decode("the clipboard content could not be converted".into())
        }
        other => ClipboardError::Unavailable(other.to_string()),
    }
}

fn absent_as_none<T>(result: ClipboardResult<T>) -> ClipboardResult<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(ClipboardError::Empty) => Ok(None),
        Err(error) => Err(error),
    }
}

impl RawClipboard for ArboardClipboard {
    fn get_text(&self) -> ClipboardResult<Option<String>> {
        absent_as_none(self.with(|clipboard| clipboard.get_text()))
    }

    fn set_text(&self, text: &str) -> ClipboardResult<()> {
        self.with(|clipboard| clipboard.set_text(text))
    }

    fn get_image(&self) -> ClipboardResult<Option<RawImage>> {
        absent_as_none(self.with(|clipboard| {
            clipboard.get_image().map(|image| RawImage {
                width: image.width,
                height: image.height,
                rgba: image.bytes.into_owned(),
            })
        }))
    }

    fn set_image(&self, image: RawImage) -> ClipboardResult<()> {
        self.with(|clipboard| {
            clipboard.set_image(arboard::ImageData {
                width: image.width,
                height: image.height,
                bytes: image.rgba.into(),
            })
        })
    }
}

#[cfg(test)]
pub mod fake {
    use super::*;

    /// In-memory [`DeviceClipboard`] for bridge tests. Applies the same text and image policy as
    /// the system clipboard.
    pub struct FakeClipboard {
        service: ClipboardService<FakeRaw>,
    }

    #[derive(Default)]
    pub struct FakeRaw {
        text: Mutex<Option<String>>,
        image: Mutex<Option<(usize, usize, Vec<u8>)>>,
        unavailable: Mutex<bool>,
    }

    impl FakeRaw {
        fn check(&self) -> ClipboardResult<()> {
            if *self.unavailable.lock().unwrap() {
                Err(ClipboardError::Unavailable(
                    "fake clipboard is offline".into(),
                ))
            } else {
                Ok(())
            }
        }
    }

    impl RawClipboard for FakeRaw {
        fn get_text(&self) -> ClipboardResult<Option<String>> {
            self.check()?;
            Ok(self.text.lock().unwrap().clone())
        }
        fn set_text(&self, text: &str) -> ClipboardResult<()> {
            self.check()?;
            *self.text.lock().unwrap() = Some(text.to_owned());
            *self.image.lock().unwrap() = None;
            Ok(())
        }
        fn get_image(&self) -> ClipboardResult<Option<RawImage>> {
            self.check()?;
            Ok(self
                .image
                .lock()
                .unwrap()
                .clone()
                .map(|(width, height, rgba)| RawImage {
                    width,
                    height,
                    rgba,
                }))
        }
        fn set_image(&self, image: RawImage) -> ClipboardResult<()> {
            self.check()?;
            *self.image.lock().unwrap() = Some((image.width, image.height, image.rgba));
            *self.text.lock().unwrap() = None;
            Ok(())
        }
    }

    impl Default for FakeClipboard {
        fn default() -> Self {
            Self {
                service: ClipboardService::new(FakeRaw::default()),
            }
        }
    }

    impl FakeClipboard {
        pub fn set_unavailable(&self, unavailable: bool) {
            *self.service.raw.unavailable.lock().unwrap() = unavailable;
        }

        pub fn set_raw_text(&self, text: &str) {
            self.service.raw.set_text(text).unwrap();
        }

        pub fn set_raw_image(&self, width: usize, height: usize, rgba: Vec<u8>) {
            *self.service.raw.image.lock().unwrap() = Some((width, height, rgba));
        }

        pub fn text(&self) -> Option<String> {
            self.service.raw.text.lock().unwrap().clone()
        }

        pub fn image_rgba(&self) -> Option<(usize, usize, Vec<u8>)> {
            self.service.raw.image.lock().unwrap().clone()
        }
    }

    impl DeviceClipboard for FakeClipboard {
        fn read_text(&self, max_bytes: usize) -> ClipboardResult<Option<String>> {
            self.service.read_text(max_bytes)
        }
        fn write_text(&self, text: &str) -> ClipboardResult<()> {
            self.service.write_text(text)
        }
        fn read_image(&self, limits: ImageLimits) -> ClipboardResult<Option<PngBytes>> {
            self.service.read_image(limits)
        }
        fn write_image_from_encoded(
            &self,
            bytes: &[u8],
            mime: &str,
            limits: ImageLimits,
        ) -> ClipboardResult<()> {
            self.service.write_image_from_encoded(bytes, mime, limits)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeClipboard;
    use super::*;

    fn sample_rgba(width: u32, height: u32) -> Vec<u8> {
        (0..width * height)
            .flat_map(|i| {
                [
                    (i % 251) as u8,
                    (i * 7 % 251) as u8,
                    (i * 13 % 251) as u8,
                    255,
                ]
            })
            .collect()
    }

    fn encode(format: ImageFormat, width: u32, height: u32) -> Vec<u8> {
        let rgba = image::RgbaImage::from_raw(width, height, sample_rgba(width, height)).unwrap();
        let image = match format {
            ImageFormat::Jpeg => DynamicImage::ImageRgb8(DynamicImage::ImageRgba8(rgba).to_rgb8()),
            _ => DynamicImage::ImageRgba8(rgba),
        };
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, format).unwrap();
        out.into_inner()
    }

    #[test]
    fn mime_types_are_normalized() {
        assert_eq!(format_for_mime("image/png").unwrap(), ImageFormat::Png);
        assert_eq!(
            format_for_mime(" IMAGE/JPEG; q=1 ").unwrap(),
            ImageFormat::Jpeg
        );
        assert_eq!(format_for_mime("image/jpg").unwrap(), ImageFormat::Jpeg);
        assert_eq!(format_for_mime("image/webp").unwrap(), ImageFormat::WebP);
        assert_eq!(format_for_mime("image/x-ms-bmp").unwrap(), ImageFormat::Bmp);
        for rejected in ["image/gif", "image/svg+xml", "text/plain", ""] {
            assert!(matches!(
                format_for_mime(rejected),
                Err(ClipboardError::Unsupported(_))
            ));
        }
    }

    #[test]
    fn text_round_trips_and_enforces_cap() {
        let clipboard = FakeClipboard::default();
        assert_eq!(clipboard.read_text(10).unwrap(), None);
        clipboard.write_text("héllo").unwrap();
        assert_eq!(clipboard.read_text(10).unwrap().as_deref(), Some("héllo"));
        assert_eq!(
            clipboard.read_text(6),
            Ok(Some("héllo".to_owned())),
            "the cap is on bytes and equals the length"
        );
        assert_eq!(
            clipboard.read_text(5),
            Err(ClipboardError::TooLarge { limit: 5 })
        );
        assert_eq!(clipboard.write_text(""), Err(ClipboardError::Empty));
    }

    #[test]
    fn empty_text_reads_as_absent() {
        let clipboard = FakeClipboard::default();
        clipboard.set_raw_text("");
        assert_eq!(clipboard.read_text(10).unwrap(), None);
    }

    #[test]
    fn png_round_trip_preserves_pixels() {
        let clipboard = FakeClipboard::default();
        let png = encode(ImageFormat::Png, 7, 5);
        clipboard
            .write_image_from_encoded(&png, "image/png", ImageLimits::DEFAULT)
            .unwrap();
        let read = clipboard.read_image(ImageLimits::DEFAULT).unwrap().unwrap();
        assert_eq!((read.width, read.height), (7, 5));
        let decoded = image::load_from_memory_with_format(&read.bytes, ImageFormat::Png)
            .unwrap()
            .into_rgba8();
        assert_eq!(decoded.into_raw(), sample_rgba(7, 5));
    }

    #[test]
    fn jpeg_webp_and_bmp_decode() {
        for (format, mime) in [
            (ImageFormat::Jpeg, "image/jpeg"),
            (ImageFormat::WebP, "image/webp"),
            (ImageFormat::Bmp, "image/bmp"),
        ] {
            let clipboard = FakeClipboard::default();
            let bytes = encode(format, 16, 8);
            clipboard
                .write_image_from_encoded(&bytes, mime, ImageLimits::DEFAULT)
                .unwrap_or_else(|error| panic!("{mime}: {error}"));
            let (width, height, rgba) = clipboard.image_rgba().unwrap();
            assert_eq!((width, height, rgba.len()), (16, 8, 16 * 8 * 4), "{mime}");
        }
    }

    #[test]
    fn declared_mime_must_match_content() {
        let clipboard = FakeClipboard::default();
        let png = encode(ImageFormat::Png, 2, 2);
        assert!(matches!(
            clipboard.write_image_from_encoded(&png, "image/jpeg", ImageLimits::DEFAULT),
            Err(ClipboardError::Decode(_))
        ));
        assert!(matches!(
            clipboard.write_image_from_encoded(
                b"not an image at all",
                "image/png",
                ImageLimits::DEFAULT
            ),
            Err(ClipboardError::Decode(_))
        ));
        assert!(matches!(
            clipboard.write_image_from_encoded(&png, "image/gif", ImageLimits::DEFAULT),
            Err(ClipboardError::Unsupported(_))
        ));
        assert_eq!(
            clipboard.write_image_from_encoded(&[], "image/png", ImageLimits::DEFAULT),
            Err(ClipboardError::Empty)
        );
        assert!(clipboard.image_rgba().is_none());
    }

    #[test]
    fn truncated_png_is_a_decode_error() {
        let clipboard = FakeClipboard::default();
        let png = encode(ImageFormat::Png, 32, 32);
        let result = clipboard.write_image_from_encoded(
            &png[..png.len() / 2],
            "image/png",
            ImageLimits::DEFAULT,
        );
        assert!(
            matches!(result, Err(ClipboardError::Decode(_))),
            "{result:?}"
        );
    }

    #[test]
    fn write_caps_apply_before_decoding() {
        let clipboard = FakeClipboard::default();
        let png = encode(ImageFormat::Png, 16, 16);
        let bytes_cap = ImageLimits {
            max_encoded_bytes: png.len() - 1,
            max_pixels: u64::MAX,
        };
        assert_eq!(
            clipboard.write_image_from_encoded(&png, "image/png", bytes_cap),
            Err(ClipboardError::TooLarge {
                limit: bytes_cap.max_encoded_bytes as u64
            })
        );
        let pixel_cap = ImageLimits {
            max_encoded_bytes: usize::MAX,
            max_pixels: 16 * 16 - 1,
        };
        assert_eq!(
            clipboard.write_image_from_encoded(&png, "image/png", pixel_cap),
            Err(ClipboardError::TooLarge {
                limit: pixel_cap.max_pixels
            })
        );
        let exact = ImageLimits {
            max_encoded_bytes: png.len(),
            max_pixels: 16 * 16,
        };
        clipboard
            .write_image_from_encoded(&png, "image/png", exact)
            .unwrap();
    }

    #[test]
    fn read_caps_apply_to_pixels_and_encoded_size() {
        let clipboard = FakeClipboard::default();
        let png = encode(ImageFormat::Png, 16, 16);
        clipboard
            .write_image_from_encoded(&png, "image/png", ImageLimits::DEFAULT)
            .unwrap();
        assert_eq!(
            clipboard.read_image(ImageLimits {
                max_encoded_bytes: usize::MAX,
                max_pixels: 255,
            }),
            Err(ClipboardError::TooLarge { limit: 255 })
        );
        assert_eq!(
            clipboard.read_image(ImageLimits {
                max_encoded_bytes: 8,
                max_pixels: u64::MAX,
            }),
            Err(ClipboardError::TooLarge { limit: 8 })
        );
    }

    #[test]
    fn unavailable_clipboard_reports_unavailable() {
        let clipboard = FakeClipboard::default();
        clipboard.set_unavailable(true);
        assert!(matches!(
            clipboard.read_text(10),
            Err(ClipboardError::Unavailable(_))
        ));
        assert!(matches!(
            clipboard.write_text("x"),
            Err(ClipboardError::Unavailable(_))
        ));
        assert!(matches!(
            clipboard.read_image(ImageLimits::DEFAULT),
            Err(ClipboardError::Unavailable(_))
        ));
    }

    #[test]
    fn arboard_errors_map_to_typed_errors() {
        assert_eq!(
            map_arboard_error(arboard::Error::ContentNotAvailable),
            ClipboardError::Empty
        );
        assert!(matches!(
            map_arboard_error(arboard::Error::ClipboardNotSupported),
            ClipboardError::Unsupported(_)
        ));
        assert!(matches!(
            map_arboard_error(arboard::Error::ClipboardOccupied),
            ClipboardError::Unavailable(_)
        ));
        assert!(matches!(
            map_arboard_error(arboard::Error::ConversionFailure),
            ClipboardError::Decode(_)
        ));
        assert!(matches!(
            map_arboard_error(arboard::Error::Unknown {
                description: "x".into()
            }),
            ClipboardError::Unavailable(_)
        ));
        assert_eq!(absent_as_none::<u8>(Err(ClipboardError::Empty)), Ok(None));
    }

    fn small_limits(max_pixels: u64) -> ImageLimits {
        ImageLimits {
            max_encoded_bytes: usize::MAX,
            max_pixels,
        }
    }

    #[test]
    fn every_format_is_capped_by_decoded_size() {
        for (format, mime) in [
            (ImageFormat::Png, "image/png"),
            (ImageFormat::Jpeg, "image/jpeg"),
            (ImageFormat::WebP, "image/webp"),
            (ImageFormat::Bmp, "image/bmp"),
        ] {
            for (width, height) in [(16, 16), (1, 300), (300, 1)] {
                let clipboard = FakeClipboard::default();
                let bytes = encode(format, width, height);
                let limits = small_limits(255);
                assert_eq!(
                    clipboard.write_image_from_encoded(&bytes, mime, limits),
                    Err(ClipboardError::TooLarge { limit: 255 }),
                    "{mime} {width}x{height}"
                );
                assert!(clipboard.image_rgba().is_none(), "{mime}");
            }
        }
    }

    #[test]
    fn decoder_limits_cover_width_height_and_allocation() {
        let limits = pixel_limits(small_limits(100));
        assert_eq!(limits.max_image_width, Some(100));
        assert_eq!(limits.max_image_height, Some(100));
        assert_eq!(limits.max_alloc, Some(800));
        let png = encode(ImageFormat::Png, 1, 150);
        let mut reader = ImageReader::with_format(Cursor::new(&png), ImageFormat::Png);
        reader.limits(limits);
        assert!(matches!(
            reader.into_decoder(),
            Err(image::ImageError::Limits(_))
        ));
    }

    #[test]
    fn pixel_checks_do_not_overflow() {
        assert!(check_pixels(u32::MAX, u32::MAX, small_limits(u64::MAX)).is_ok());
        assert!(check_pixels(u32::MAX, u32::MAX, small_limits(1 << 40)).is_err());
        assert_eq!(rgba_len(u32::MAX, u32::MAX), None);
        assert!(check_pixels(10, 10, small_limits(100)).is_ok());
        assert!(check_pixels(10, 11, small_limits(100)).is_err());
        assert_eq!(rgba_len(3, 2), Some(24));
    }

    #[test]
    fn oversized_raw_image_dimensions_are_rejected() {
        let clipboard = FakeClipboard::default();
        clipboard.set_raw_image(usize::MAX, usize::MAX, Vec::new());
        assert!(matches!(
            clipboard.read_image(small_limits(u64::MAX)),
            Err(ClipboardError::TooLarge { .. })
        ));
        clipboard.set_raw_image(u32::MAX as usize, u32::MAX as usize, Vec::new());
        assert!(matches!(
            clipboard.read_image(small_limits(u64::MAX)),
            Err(ClipboardError::TooLarge { .. })
        ));
    }

    type Gate = Arc<(Mutex<bool>, std::sync::Condvar)>;

    fn release(gate: &Gate) {
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
    }

    /// A [`RawClipboard`] whose first text read on a shared counter blocks until the gate opens.
    struct Blocking {
        gate: Gate,
        reads: Arc<AtomicUsize>,
    }

    impl RawClipboard for Blocking {
        fn get_text(&self) -> ClipboardResult<Option<String>> {
            if self.reads.fetch_add(1, Ordering::SeqCst) == 0 {
                let mut open = self.gate.0.lock().unwrap();
                while !*open {
                    open = self.gate.1.wait(open).unwrap();
                }
            }
            Ok(Some("ok".into()))
        }
        fn set_text(&self, _: &str) -> ClipboardResult<()> {
            Ok(())
        }
        fn get_image(&self) -> ClipboardResult<Option<RawImage>> {
            Ok(None)
        }
        fn set_image(&self, _: RawImage) -> ClipboardResult<()> {
            Ok(())
        }
    }

    fn new_gate() -> Gate {
        Arc::new((Mutex::new(false), std::sync::Condvar::new()))
    }

    #[test]
    fn stuck_call_times_out_and_later_calls_use_a_fresh_worker() {
        let gate = new_gate();
        let reads = Arc::new(AtomicUsize::new(0));
        let shared = Arc::clone(&gate);
        let bounded = BoundedClipboard::with_timeout(
            move || Blocking {
                gate: Arc::clone(&shared),
                reads: Arc::clone(&reads),
            },
            Duration::from_millis(100),
        );
        assert_eq!(
            bounded.get_text(),
            Err(ClipboardError::Unavailable("timed out".into()))
        );
        assert_eq!(bounded.get_text(), Ok(Some("ok".into())));
        assert_eq!(bounded.set_text("x"), Ok(()));
        release(&gate);
    }

    #[test]
    fn stuck_workers_are_capped_and_recover_when_released() {
        let gate = new_gate();
        let shared = Arc::clone(&gate);
        let bounded = BoundedClipboard::with_timeout(
            move || Blocking {
                gate: Arc::clone(&shared),
                reads: Arc::new(AtomicUsize::new(0)),
            },
            Duration::from_millis(50),
        );
        for _ in 0..MAX_WORKERS {
            assert_eq!(
                bounded.get_text(),
                Err(ClipboardError::Unavailable("timed out".into()))
            );
        }
        let refused = bounded.get_text();
        assert!(
            matches!(&refused, Err(ClipboardError::Unavailable(why)) if why.contains("not returned")),
            "{refused:?}"
        );
        release(&gate);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match bounded.get_text() {
                Ok(text) => {
                    assert_eq!(text.as_deref(), Some("ok"));
                    break;
                }
                Err(_) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                Err(error) => panic!("did not recover: {error}"),
            }
        }
    }

    #[test]
    fn bounded_clipboard_passes_results_through() {
        let gate = new_gate();
        release(&gate);
        let bounded = BoundedClipboard::with_timeout(
            move || Blocking {
                gate: Arc::clone(&gate),
                reads: Arc::new(AtomicUsize::new(0)),
            },
            Duration::from_secs(5),
        );
        assert_eq!(bounded.get_text(), Ok(Some("ok".into())));
        assert_eq!(bounded.get_image().map(|image| image.is_none()), Ok(true));
    }

    /// Opt-in: touches the real device clipboard. Run with
    /// `cargo test --locked clipboard::tests::live -- --ignored --test-threads=1`.
    #[test]
    #[ignore = "reads and overwrites the real device clipboard"]
    fn live_text_and_image_round_trip() {
        let clipboard = system();
        clipboard.write_text("silo clipboard live test").unwrap();
        assert_eq!(
            clipboard.read_text(1024).unwrap().as_deref(),
            Some("silo clipboard live test")
        );
        let png = encode(ImageFormat::Png, 4, 3);
        clipboard
            .write_image_from_encoded(&png, "image/png", ImageLimits::DEFAULT)
            .unwrap();
        let read = clipboard.read_image(ImageLimits::DEFAULT).unwrap().unwrap();
        assert_eq!((read.width, read.height), (4, 3));
    }
}
