//! `silo-ctr enter` joins a holder's namespaces. Skipped where user namespaces are unavailable.

#![cfg(target_os = "linux")]

use std::process::{Command, Stdio};

fn ctr() -> Command {
    Command::new(env!("CARGO_BIN_EXE_silo-ctr"))
}

#[test]
fn a_command_entered_into_a_holder_shares_its_network_namespace_and_not_the_hosts() {
    let probe = ctr()
        .args(["userns", "--net", "--", "true"])
        .output()
        .unwrap();
    if !probe.status.success() {
        eprintln!("skipped: user namespaces with sub-ids are unavailable here");
        return;
    }
    let state = tempfile::tempdir().unwrap();
    let mut holder = ctr()
        .args(["userns", "--net", "--", "sh", "-c"])
        .arg(format!(
            "echo $$ > {}/outer.pid; readlink /proc/self/ns/net > {}/net; sleep 20",
            state.path().display(),
            state.path().display()
        ))
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    for _ in 0..100 {
        if state.path().join("net").is_file()
            && !std::fs::read(state.path().join("net")).unwrap().is_empty()
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let output = ctr()
        .args(["enter", "--state"])
        .arg(state.path())
        .args([
            "--",
            "sh",
            "-c",
            "readlink /proc/self/ns/net; cat /proc/net/dev | tail -n +3 | cut -d: -f1",
        ])
        .output()
        .unwrap();
    let _ = holder.kill();
    let _ = holder.wait();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = text.lines();
    let entered = lines.next().unwrap();
    let held = std::fs::read_to_string(state.path().join("net")).unwrap();
    assert_eq!(entered, held.trim());
    assert_ne!(
        entered,
        std::fs::read_link("/proc/self/ns/net")
            .unwrap()
            .to_string_lossy()
    );
    assert_eq!(
        lines.map(str::trim).collect::<Vec<_>>(),
        ["lo:"].map(|s| s.trim_end_matches(':'))
    );
}
