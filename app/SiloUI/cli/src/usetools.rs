//! The computer-use tools of `silo mcp`: what each asks of the driver, and how its screenshots
//! are scaled before an agent sees them.
//!
//! The driver tool names and argument names below are Cua Driver's; `computer_use_tools` lists
//! what the pinned driver really offers and `computer_use_call` runs any of its tools by name,
//! so a rename at a driver bump does not strand an agent.

use std::io::Cursor;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use image::{imageops::FilterType, ImageFormat};
use serde_json::{json, Value};

use crate::error::CliError;

/// The longest side of a screenshot an agent receives.
pub const DEFAULT_MAX_SIDE: u32 = 1280;

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Screenshot,
    Click {
        x: f64,
        y: f64,
        button: String,
        double: bool,
    },
    Type {
        text: String,
    },
    Key {
        key: String,
    },
    Scroll {
        x: f64,
        y: f64,
        dx: f64,
        dy: f64,
    },
    Move {
        x: f64,
        y: f64,
    },
    Drag {
        from: (f64, f64),
        to: (f64, f64),
    },
}

/// The driver tool and arguments for `action`; coordinates arrive in the pixels of the last
/// screenshot the agent saw and leave in screen pixels (`scale` screen pixels per image pixel).
pub fn driver_call(action: &Action, scale: f64) -> (&'static str, Value) {
    let at = |value: f64| (value * scale).round();
    match action {
        Action::Screenshot => ("screenshot", json!({})),
        Action::Click {
            x,
            y,
            button,
            double,
        } => (
            if *double { "double_click" } else { "click" },
            json!({"x": at(*x), "y": at(*y), "button": button}),
        ),
        Action::Type { text } => ("type_text", json!({"text": text})),
        Action::Key { key } => ("press_key", json!({"key": key})),
        Action::Scroll { x, y, dx, dy } => (
            "scroll",
            json!({"x": at(*x), "y": at(*y), "delta_x": dx, "delta_y": dy}),
        ),
        Action::Move { x, y } => ("move_cursor", json!({"x": at(*x), "y": at(*y)})),
        Action::Drag { from, to } => (
            "drag",
            json!({"start_x": at(from.0), "start_y": at(from.1), "end_x": at(to.0), "end_y": at(to.1)}),
        ),
    }
}

/// One piece of a tool's answer.
#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    Text(String),
    Image { data: String, mime: String },
}

/// The answer of a driver tool: its parts, with images scaled so the longest side is at most
/// `max_side`, and the scale (screen pixels per image pixel) of the last image, if any.
pub struct Answer {
    pub parts: Vec<Part>,
    pub is_error: bool,
    pub scale: Option<f64>,
}

pub fn answer_of(result: &Value, max_side: u32) -> Result<Answer, CliError> {
    let mut parts = Vec::new();
    let mut scale = None;
    for item in result["content"].as_array().into_iter().flatten() {
        match item["type"].as_str() {
            Some("image") => {
                let data = item["data"].as_str().unwrap_or_default();
                let (data, mime, factor, note) = scaled(data, max_side)?;
                scale = Some(factor);
                parts.push(Part::Image { data, mime });
                parts.push(Part::Text(note));
            }
            Some("text") => parts.push(Part::Text(item["text"].as_str().unwrap_or("").to_owned())),
            _ => {}
        }
    }
    if parts.is_empty() {
        parts.push(Part::Text(result.to_string()));
    }
    Ok(Answer {
        parts,
        is_error: result["isError"].as_bool().unwrap_or(false),
        scale,
    })
}

/// Scales a base64 PNG or JPEG down to `max_side`. Returns the new base64 PNG, its type, the
/// screen pixels per image pixel, and a line telling the agent what the image is.
pub fn scaled(base64: &str, max_side: u32) -> Result<(String, String, f64, String), CliError> {
    let bytes = STANDARD
        .decode(base64.trim())
        .map_err(|_| CliError::new("The screenshot was not valid base64."))?;
    let picture = image::load_from_memory(&bytes)
        .map_err(|_| CliError::new("The screenshot could not be read as an image."))?;
    let (width, height) = (picture.width(), picture.height());
    let longest = width.max(height);
    let max_side = max_side.max(64);
    if longest <= max_side {
        let note = format!("Screenshot {width}x{height}; coordinates are in this image's pixels.");
        return Ok((STANDARD.encode(&bytes), mime_of(&bytes), 1.0, note));
    }
    let factor = f64::from(longest) / f64::from(max_side);
    let new_width = ((f64::from(width) / factor).round() as u32).max(1);
    let new_height = ((f64::from(height) / factor).round() as u32).max(1);
    let small = picture.resize_exact(new_width, new_height, FilterType::Triangle);
    let mut out = Cursor::new(Vec::new());
    small
        .write_to(&mut out, ImageFormat::Png)
        .map_err(|_| CliError::new("The scaled screenshot could not be encoded."))?;
    let note = format!(
        "Screenshot scaled to {new_width}x{new_height} (the screen is {width}x{height}); give coordinates in this image's pixels."
    );
    Ok((
        STANDARD.encode(out.into_inner()),
        "image/png".into(),
        factor,
        note,
    ))
}

fn mime_of(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xff, 0xd8]) {
        "image/jpeg".into()
    } else {
        "image/png".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> String {
        let mut out = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(width, height)
            .write_to(&mut out, ImageFormat::Png)
            .unwrap();
        STANDARD.encode(out.into_inner())
    }

    #[test]
    fn big_screenshots_are_scaled_and_report_the_factor() {
        let (data, mime, factor, note) = scaled(&png(2560, 1600), 1280).unwrap();
        assert_eq!(factor, 2.0);
        assert_eq!(mime, "image/png");
        assert!(note.contains("1280x800") && note.contains("2560x1600"));
        let decoded = image::load_from_memory(&STANDARD.decode(data).unwrap()).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (1280, 800));
    }

    #[test]
    fn small_screenshots_pass_through() {
        let (_, _, factor, _) = scaled(&png(800, 600), 1280).unwrap();
        assert_eq!(factor, 1.0);
    }

    #[test]
    fn coordinates_are_mapped_back_to_the_screen() {
        let (tool, args) = driver_call(
            &Action::Click {
                x: 100.0,
                y: 50.0,
                button: "left".into(),
                double: false,
            },
            2.0,
        );
        assert_eq!(
            (tool, args["x"].as_f64(), args["y"].as_f64()),
            ("click", Some(200.0), Some(100.0))
        );
        let (tool, _) = driver_call(
            &Action::Click {
                x: 1.0,
                y: 1.0,
                button: "left".into(),
                double: true,
            },
            1.0,
        );
        assert_eq!(tool, "double_click");
    }

    #[test]
    fn an_answer_keeps_text_and_flags_errors() {
        let result = json!({"isError": true, "content": [{"type": "text", "text": "no window"}]});
        let answer = answer_of(&result, 1280).unwrap();
        assert!(answer.is_error);
        assert_eq!(answer.parts, vec![Part::Text("no window".into())]);
        assert_eq!(answer.scale, None);
        let image = json!({"content": [{"type": "image", "data": png(2000, 1000), "mimeType": "image/png"}]});
        let answer = answer_of(&image, 1000).unwrap();
        assert_eq!(answer.scale, Some(2.0));
        assert!(matches!(answer.parts[0], Part::Image { .. }));
    }
}
