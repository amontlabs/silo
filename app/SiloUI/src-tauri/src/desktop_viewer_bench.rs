//! Development builds only: a file-driven benchmark of the desktop stream.
//!
//! With `SILO_DESKTOP_BENCH_DIR` set, a worker watches that directory for
//! `request.json` (`{"probes":30,"idleSeconds":10,"motionSeconds":10}`), runs
//! `desktop_viewer_bench.js` in the first connected viewer and writes
//! `result-<unix ms>.json`: the page's report plus the CPU time this device's
//! processes spent meanwhile. `scripts/desktop-stream-bench/run.py` drives it.
use crate::desktop_bridge::Op;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::AppHandle;

const SCRIPT: &str = include_str!("desktop_viewer_bench.js");
const ENV: &str = "SILO_DESKTOP_BENCH_DIR";

pub(crate) fn start(app: &AppHandle) {
    let Some(directory) = std::env::var_os(ENV).map(PathBuf::from) else {
        return;
    };
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(500));
        let request = directory.join("request.json");
        let Ok(text) = fs::read_to_string(&request) else {
            continue;
        };
        let _ = fs::remove_file(&request);
        // A `bench.js` beside the request replaces the built-in script while iterating on it.
        let script =
            fs::read_to_string(directory.join("bench.js")).unwrap_or_else(|_| SCRIPT.into());
        let result = match serde_json::from_str::<Value>(&text) {
            Ok(config) if config.is_object() => {
                run(&app, &config, &script).unwrap_or_else(|error| json!({ "error": error }))
            }
            _ => json!({ "error": "The benchmark request is not a JSON object." }),
        };
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        write(&directory, &format!("result-{stamp}.json"), &result);
    });
}

fn write(directory: &Path, name: &str, value: &Value) {
    let partial = directory.join(format!(".{name}.partial"));
    if fs::write(
        &partial,
        serde_json::to_vec_pretty(value).unwrap_or_default(),
    )
    .is_ok()
    {
        let _ = fs::rename(partial, directory.join(name));
    }
}

fn run(app: &AppHandle, config: &Value, script: &str) -> Result<Value, String> {
    let number = |key: &str, default: u64| config[key].as_u64().unwrap_or(default);
    let config = json!({
        "probes": number("probes", 30),
        "idleSeconds": number("idleSeconds", 10),
        "motionSeconds": number("motionSeconds", 10),
    });
    let label = crate::desktop_viewer::viewer_labels()
        .into_iter()
        .next()
        .ok_or("No desktop viewer is open.")?;
    let budget = Duration::from_secs(
        30 + config["probes"].as_u64().unwrap_or(0) * 4
            + config["idleSeconds"].as_u64().unwrap_or(0)
            + config["motionSeconds"].as_u64().unwrap_or(0),
    );
    keep_visible(app, &label);
    let before = cpu_times();
    let started = Instant::now();
    let reply = crate::desktop_viewer::with_bridge(app, &label, |bridge| {
        let expectation = bridge.inbox.expect(Op::Diagnostics, budget);
        bridge.page.eval(&format!(
            "({script})({},{});",
            json!(expectation.nonce),
            config
        ))?;
        expectation.wait(budget)
    })?;
    let wall = started.elapsed().as_secs_f64();
    let after = cpu_times();
    let page: Value =
        serde_json::from_slice(&reply.body).map_err(|_| "Unreadable benchmark report.")?;
    let processes: Vec<Value> = after
        .iter()
        .filter_map(|(pid, (name, seconds, rss))| {
            let spent = seconds - before.get(pid).map_or(0., |(_, s, _)| *s);
            (spent > 0.05).then(|| {
                json!({"pid": pid, "name": name, "cpuPercent": spent / wall * 100., "rssKiB": rss})
            })
        })
        .collect();
    Ok(json!({ "page": page, "hostProcesses": processes, "wallSeconds": wall }))
}

/// WebKit reports a covered window as hidden, and Selkies then pauses its
/// video; the benchmark must not depend on what else is on this screen.
fn keep_visible(app: &AppHandle, label: &str) {
    use tauri::Manager;
    let Some(view) = app.get_webview(&format!("guest-{label}")) else {
        return;
    };
    let _ = view.with_webview(|webview| {
        #[cfg(not(target_os = "macos"))]
        let _ = webview;
        #[cfg(target_os = "macos")]
        unsafe {
            use objc2::{msg_send, runtime::AnyObject, sel};
            let view = webview.inner() as *mut AnyObject;
            let selector = sel!(_setWindowOcclusionDetectionEnabled:);
            let responds: bool = msg_send![view, respondsToSelector: selector];
            if responds {
                let _: () = msg_send![view, _setWindowOcclusionDetectionEnabled: false];
            }
        }
    });
    std::thread::sleep(Duration::from_secs(1));
}

/// CPU seconds and resident KiB of this app, its descendants (the SSH forward
/// and MicroSandbox relay) and every WebKit helper process.
fn cpu_times() -> HashMap<u32, (String, f64, u64)> {
    let Ok(output) = Command::new("/bin/ps")
        .args(["-A", "-o", "pid=,ppid=,time=,rss=,comm="])
        .output()
    else {
        return HashMap::new();
    };
    let rows: Vec<(u32, u32, f64, u64, String)> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse().ok()?;
            let ppid = fields.next()?.parse().ok()?;
            let time = cpu_seconds(fields.next()?)?;
            let rss = fields.next()?.parse().ok()?;
            Some((pid, ppid, time, rss, fields.collect::<Vec<_>>().join(" ")))
        })
        .collect();
    let mut wanted = vec![std::process::id()];
    let mut index = 0;
    while index < wanted.len() {
        let parent = wanted[index];
        wanted.extend(rows.iter().filter(|row| row.1 == parent).map(|row| row.0));
        index += 1;
    }
    rows.into_iter()
        .filter(|row| wanted.contains(&row.0) || row.4.contains("com.apple.WebKit"))
        .map(|(pid, _, time, rss, name)| (pid, (name, time, rss)))
        .collect()
}

/// `ps` CPU time: `[[dd-]hh:]mm:ss.cc`.
fn cpu_seconds(text: &str) -> Option<f64> {
    let (days, rest) = match text.split_once('-') {
        Some((days, rest)) => (days.parse::<f64>().ok()?, rest),
        None => (0., text),
    };
    let mut total = 0.;
    for part in rest.split(':') {
        total = total * 60. + part.parse::<f64>().ok()?;
    }
    Some(days * 86400. + total)
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_ps_cpu_time() {
        assert_eq!(super::cpu_seconds("0:01.50"), Some(1.5));
        assert_eq!(super::cpu_seconds("1:02:03.00"), Some(3723.));
        assert_eq!(super::cpu_seconds("2-00:00:01.00"), Some(172801.));
        assert_eq!(super::cpu_seconds("x"), None);
    }
}
