//! The computer-use tools of `silo mcp`: which Cua Driver tool and arguments each asks for, and
//! how a driver answer is shaped for an agent.
//!
//! Names and arguments follow the driver contract of the pinned release
//! (`tests/fixtures/cua-driver-0.38.0-tools.json`, extracted from its `contract/manifest.json`);
//! a test fails when a call stops matching it. The driver scales a screenshot itself
//! (`max_image_dimension`) and maps coordinates taken from it back to the screen.

use serde_json::{json, Value};

/// The longest side of a screenshot an agent receives.
pub const DEFAULT_MAX_SIDE: u32 = 1280;

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Screenshot {
        max_side: u32,
    },
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
        modifiers: Vec<String>,
    },
    Hotkey {
        keys: Vec<String>,
    },
    Scroll {
        x: f64,
        y: f64,
        direction: String,
        amount: u32,
        by: Option<String>,
    },
    Move {
        x: f64,
        y: f64,
    },
    Drag {
        from: (f64, f64),
        to: (f64, f64),
        button: Option<String>,
    },
}

fn desktop() -> Value {
    json!({"kind": "desktop", "display_id": "primary"})
}

/// The driver tool and arguments for `action`.
pub fn driver_call(action: &Action) -> (&'static str, Value) {
    match action {
        Action::Screenshot { max_side } => (
            "get_desktop_state",
            json!({"max_image_dimension": max_side}),
        ),
        Action::Click {
            x,
            y,
            button,
            double,
        } => (
            "click",
            json!({"x": x, "y": y, "button": button, "count": if *double { 2 } else { 1 },
                   "target": desktop(), "delivery_mode": "foreground"}),
        ),
        Action::Type { text } => ("type_text", json!({"text": text, "target": desktop()})),
        Action::Key { key, modifiers } => (
            "press_key",
            json!({"key": key, "modifiers": modifiers, "target": desktop()}),
        ),
        Action::Hotkey { keys } => ("hotkey", json!({"keys": keys, "target": desktop()})),
        Action::Scroll {
            x,
            y,
            direction,
            amount,
            by,
        } => {
            let mut args = json!({"x": x, "y": y, "direction": direction, "amount": amount, "target": desktop()});
            if let Some(by) = by {
                args["by"] = json!(by);
            }
            ("scroll", args)
        }
        Action::Move { x, y } => ("move_cursor", json!({"x": x, "y": y, "target": desktop()})),
        Action::Drag { from, to, button } => {
            let mut args = json!({"from_x": from.0, "from_y": from.1, "to_x": to.0, "to_y": to.1, "target": desktop()});
            if let Some(button) = button {
                args["button"] = json!(button);
            }
            ("drag", args)
        }
    }
}

/// One piece of a tool's answer.
#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    Text(String),
    Image { data: String, mime: String },
}

pub struct Answer {
    pub parts: Vec<Part>,
    pub is_error: bool,
}

/// What a driver answer may hold: at most this many content blocks, text cut to this many
/// bytes, images of these types.
const MAX_BLOCKS: usize = 16;
const MAX_TEXT: usize = 64 * 1024;
const MAX_FALLBACK: usize = 16 * 1024;
const IMAGE_TYPES: [&str; 3] = ["image/png", "image/jpeg", "image/webp"];

fn cut(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}... (cut)", &text[..end])
}

/// An MCP `tools/call` result as parts. Blocks beyond the sixteenth, images of other types and
/// unknown blocks are dropped, long text is cut. A screenshot is followed by a line saying what
/// it is.
pub fn answer_of(result: &Value) -> Answer {
    let mut parts = Vec::new();
    for item in result["content"]
        .as_array()
        .into_iter()
        .flatten()
        .take(MAX_BLOCKS)
    {
        match item["type"].as_str() {
            Some("image") => {
                let mime = item["mimeType"].as_str().unwrap_or("image/png");
                if let (true, Some(data)) = (IMAGE_TYPES.contains(&mime), item["data"].as_str()) {
                    parts.push(Part::Image {
                        data: data.to_owned(),
                        mime: mime.to_owned(),
                    });
                }
            }
            Some("text") => parts.push(Part::Text(cut(
                item["text"].as_str().unwrap_or(""),
                MAX_TEXT,
            ))),
            _ => {}
        }
    }
    let state = &result["structuredContent"];
    if let (Some(w), Some(h)) = (
        state["screenshot_width"].as_u64(),
        state["screenshot_height"].as_u64(),
    ) {
        let screen = match (
            state["screen_width"].as_u64(),
            state["screen_height"].as_u64(),
        ) {
            (Some(sw), Some(sh)) if (sw, sh) != (w, h) => format!(" (the screen is {sw}x{sh})"),
            _ => String::new(),
        };
        parts.push(Part::Text(format!(
            "Screenshot {w}x{h}{screen}; give coordinates in this image's pixels."
        )));
    }
    if parts.is_empty() {
        parts.push(Part::Text(cut(&result.to_string(), MAX_FALLBACK)));
    }
    Answer {
        parts,
        is_error: result["isError"].as_bool().unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_double_click_is_a_count_of_two_on_the_desktop() {
        let (tool, args) = driver_call(&Action::Click {
            x: 1.0,
            y: 2.0,
            button: "left".into(),
            double: true,
        });
        assert_eq!(tool, "click");
        assert_eq!(args["count"], 2);
        assert_eq!(args["delivery_mode"], "foreground");
        assert_eq!(args["target"]["display_id"], "primary");
    }

    #[test]
    fn a_screenshot_asks_the_driver_to_scale() {
        let (tool, args) = driver_call(&Action::Screenshot { max_side: 1000 });
        assert_eq!(
            (tool, args["max_image_dimension"].as_u64()),
            ("get_desktop_state", Some(1000))
        );
    }

    #[test]
    fn an_answer_is_bounded_and_only_carries_known_blocks() {
        let blocks: Vec<Value> = (0..40)
            .map(|n| json!({"type": "text", "text": format!("{n}")}))
            .collect();
        let answer = answer_of(&json!({"content": blocks}));
        assert_eq!(answer.parts.len(), 16);
        let odd = json!({"content": [
            {"type": "image", "mimeType": "image/svg+xml", "data": "AAAA"},
            {"type": "resource", "uri": "file:///etc/passwd"},
            {"type": "text", "text": "x".repeat(200_000)}]});
        let Part::Text(text) = &answer_of(&odd).parts[0] else {
            panic!("text expected")
        };
        assert!(text.len() < 70_000, "long text is cut");
        let unknown = answer_of(&json!({"weird": "y".repeat(100_000)}));
        let Part::Text(text) = &unknown.parts[0] else {
            panic!("text expected")
        };
        assert!(text.len() < 20_000);
    }

    #[test]
    fn an_answer_keeps_text_images_and_errors() {
        let result = json!({"isError": true, "content": [{"type": "text", "text": "refused"}]});
        let answer = answer_of(&result);
        assert!(answer.is_error);
        assert_eq!(answer.parts, vec![Part::Text("refused".into())]);
        let shot = json!({"content": [{"type": "image", "mimeType": "image/png", "data": "AAAA"}],
            "structuredContent": {"screenshot_width": 1280, "screenshot_height": 800,
                                  "screen_width": 2560, "screen_height": 1600}});
        let answer = answer_of(&shot);
        assert!(matches!(answer.parts[0], Part::Image { .. }));
        let Part::Text(note) = &answer.parts[1] else {
            panic!("a note follows the image")
        };
        assert!(note.contains("1280x800") && note.contains("2560x1600"));
    }
}
