//! Runs the built `silo-ctr` against the real kernel. Each test skips (and says why) when unprivileged user
//! namespaces, newuidmap with a sub-id range, overlayfs or the TUN device are unavailable.
#![cfg(target_os = "linux")]

use std::io::{BufRead, BufReader, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn ctr() -> Command {
    Command::new(env!("CARGO_BIN_EXE_silo-ctr"))
}

fn text(output: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// True when `silo-ctr userns` works here; otherwise prints why the test is skipped.
fn namespaces_available() -> bool {
    match ctr().args(["userns", "--", "true"]).output() {
        Ok(output) if output.status.success() => true,
        Ok(output) => {
            eprintln!(
                "skipped: silo-ctr userns does not work here: {}",
                text(&output).1.trim()
            );
            false
        }
        Err(error) => {
            eprintln!("skipped: cannot run silo-ctr: {error}");
            false
        }
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("silo-ctr-test-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create scratch directory");
    dir
}

fn userns_output(args: &[&str]) -> Output {
    ctr()
        .arg("userns")
        .args(args)
        .output()
        .expect("run silo-ctr")
}

#[test]
fn version_prints_the_crate_version() {
    let output = ctr().arg("version").output().unwrap();
    assert_eq!(text(&output).0, format!("{}\n", env!("CARGO_PKG_VERSION")));
    assert_eq!(ctr().output().unwrap().status.code(), Some(1));
    assert_eq!(ctr().arg("bogus").output().unwrap().status.code(), Some(1));
}

#[test]
fn userns_makes_the_caller_root_with_both_ranges_mapped() {
    if !namespaces_available() {
        return;
    }
    assert_eq!(text(&userns_output(&["--", "id", "-u"])).0, "0\n");
    // SAFETY: getuid cannot fail.
    let uid = unsafe { libc::getuid() };
    for map in ["uid_map", "gid_map"] {
        let output = userns_output(&["--", "cat", &format!("/proc/self/{map}")]);
        let (out, _) = text(&output);
        let lines: Vec<Vec<&str>> = out
            .lines()
            .map(|line| line.split_whitespace().collect())
            .collect();
        assert_eq!(lines.len(), 2, "{out:?}");
        assert_eq!((lines[0][0], lines[0][2]), ("0", "1"), "{out:?}");
        assert_eq!(
            (lines[1][0], lines[1][1].parse::<u64>().is_ok(), lines[1][2]),
            ("1", true, "65535")
        );
        if map == "uid_map" {
            assert_eq!(lines[0][1], uid.to_string());
        }
    }
}

#[test]
fn userns_keep_id_runs_as_root_and_puts_the_caller_at_that_id() {
    if !namespaces_available() {
        return;
    }
    assert_eq!(
        text(&userns_output(&["--keep-id", "1001", "--", "id", "-u"])).0,
        "0\n"
    );
    // A file the caller owns shows as owned by the kept id; one root makes is a sub-id on disk.
    let dir = scratch("keepid");
    let script = format!(
        "mkdir -p {d}/own && stat -c %u:%g {d}/own && : > {d}/own/root-file && chown 1001:1001 {d}/own/root-file",
        d = dir.display()
    );
    let created = userns_output(&["--keep-id", "1001", "--", "sh", "-c", &script]);
    assert_eq!(text(&created).0, "0:0\n", "{:?}", text(&created));
    // SAFETY: getuid and getgid cannot fail.
    let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
    use std::os::unix::fs::MetadataExt;
    // Container id 1001 is the caller on disk.
    let meta = std::fs::metadata(dir.join("own/root-file")).unwrap();
    assert_eq!((meta.uid(), meta.gid()), (uid, gid));
    // The caller's own file is the kept id inside.
    let seen = userns_output(&[
        "--keep-id",
        "1001",
        "--",
        "stat",
        "-c",
        "%u:%g",
        &dir.join("own/root-file").display().to_string(),
    ]);
    assert_eq!(text(&seen).0, "1001:1001\n");
    let _ = userns_output(&["--", "rm", "-rf", &dir.display().to_string()]);
}

#[test]
fn userns_can_create_and_chown_files_in_the_sub_id_range() {
    if !namespaces_available() {
        return;
    }
    let dir = scratch("chown");
    let script = format!(
        "f={}/file; : > $f && chown 1000:1000 $f && stat -c %u:%g $f && rm -rf {}",
        dir.display(),
        dir.display()
    );
    let output = userns_output(&["--", "sh", "-c", &script]);
    assert_eq!(text(&output), ("1000:1000\n".to_string(), String::new()));
    assert!(!dir.exists());
}

#[test]
fn userns_passes_the_exit_status_and_signal_through() {
    if !namespaces_available() {
        return;
    }
    assert_eq!(
        userns_output(&["--", "sh", "-c", "exit 5"]).status.code(),
        Some(5)
    );
    assert_eq!(
        userns_output(&["--", "sh", "-c", "kill -KILL $$"])
            .status
            .code(),
        Some(128 + 9)
    );
    let missing = userns_output(&["--", "/nonexistent/program"]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(text(&missing)
        .1
        .contains("silo-ctr: exec /nonexistent/program: No such file or directory"));
}

#[test]
fn userns_with_net_has_only_a_loopback_device() {
    if !namespaces_available() {
        return;
    }
    let output = userns_output(&["--net", "--", "cat", "/proc/net/dev"]);
    let (out, _) = text(&output);
    let devices: Vec<&str> = out
        .lines()
        .skip(2)
        .map(|line| line.split(':').next().unwrap().trim())
        .collect();
    assert_eq!(devices, ["lo"], "{out:?}");
}

/// A stand-in for crun that records how it was started and what the holder prepared.
fn fake_crun(dir: &Path, state: &Path, merged: &Path, script_tail: &str) -> PathBuf {
    let path = dir.join("fake-crun");
    let body = format!(
        "#!/bin/sh\n\
         out={dir}/out\n\
         echo \"args: $*\" > $out\n\
         echo \"holder: $(cat {state}/holder.pid)\" >> $out\n\
         echo \"outer: $(cat {state}/outer.pid)\" >> $out\n\
         echo \"mode: $(stat -c %a {state}/holder.pid {state}/outer.pid | tr '\\n' ' ')\" >> $out\n\
         echo \"hello: $(cat {merged}/hello)\" >> $out\n\
         echo \"id: $(id -u)\" >> $out\n\
         {script_tail}\n",
        dir = dir.display(),
        state = state.display(),
        merged = merged.display(),
    );
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

struct Layout {
    dir: PathBuf,
    state: PathBuf,
    merged: PathBuf,
}

fn layout(name: &str) -> Layout {
    let dir = scratch(name);
    let (state, merged) = (dir.join("state"), dir.join("merged"));
    for sub in ["state", "merged", "upper", "work", "top", "base", "bundle"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
    }
    std::fs::write(dir.join("base/hello"), "from base\n").unwrap();
    std::fs::write(dir.join("top/hello"), "from top\n").unwrap();
    Layout { dir, state, merged }
}

fn run_holder(layout: &Layout, crun: &Path, extra: &[&str]) -> Output {
    let dir = &layout.dir;
    ctr()
        .arg("run")
        .args(["--state", layout.state.to_str().unwrap()])
        .args([
            "--lower",
            &format!(
                "{}:{}",
                dir.join("top").display(),
                dir.join("base").display()
            ),
        ])
        .args(["--upper", dir.join("upper").to_str().unwrap()])
        .args(["--work", dir.join("work").to_str().unwrap()])
        .args(["--merged", layout.merged.to_str().unwrap()])
        .args(["--bundle", dir.join("bundle").to_str().unwrap()])
        .args(["--id", "test-id", "--crun", crun.to_str().unwrap()])
        .args(extra)
        .output()
        .expect("run silo-ctr run")
}

/// True unless the failure shows overlayfs cannot be mounted here, in which case the test is skipped.
fn overlay_usable(output: &Output) -> bool {
    let (_, err) = text(output);
    if err.contains("mount overlay") {
        eprintln!(
            "skipped: an unprivileged overlay mount is not possible here: {}",
            err.trim()
        );
        return false;
    }
    true
}

#[test]
fn run_mounts_the_overlay_then_starts_crun_and_cleans_up_its_pid_files() {
    if !namespaces_available() {
        return;
    }
    let layout = layout("run");
    let crun = fake_crun(&layout.dir, &layout.state, &layout.merged, "exit 3");
    let output = run_holder(&layout, &crun, &[]);
    if !overlay_usable(&output) {
        let _ = std::fs::remove_dir_all(&layout.dir);
        return;
    }
    assert_eq!(output.status.code(), Some(3), "{:?}", text(&output));
    let recorded = std::fs::read_to_string(layout.dir.join("out")).unwrap();
    let state = layout.state.display();
    assert!(
        recorded.contains(&format!(
            "args: --root {state}/crun --cgroup-manager=cgroupfs run --bundle {}/bundle --pid-file {state}/container.pid test-id",
            layout.dir.display()
        )),
        "{recorded}"
    );
    assert!(recorded.contains("hello: from top"), "{recorded}");
    assert!(recorded.contains("mode: 600 600"), "{recorded}");
    assert!(recorded.contains("id: 0"), "{recorded}");
    let pid = |label: &str| -> u32 {
        let line = recorded
            .lines()
            .find(|line| line.starts_with(label))
            .unwrap();
        line[label.len()..].trim().parse().unwrap()
    };
    assert_ne!(pid("holder:"), pid("outer:"));
    assert!(!layout.state.join("holder.pid").exists() && !layout.state.join("outer.pid").exists());
    // The overlay lived in the holder's mount namespace only.
    assert!(!layout.merged.join("hello").exists());
    let _ = std::fs::remove_dir_all(&layout.dir);
}

#[test]
fn run_refuses_a_lower_path_overlayfs_cannot_hold() {
    let layout = layout("badlower");
    let crun = fake_crun(&layout.dir, &layout.state, &layout.merged, "exit 0");
    let mut command = ctr();
    command.args([
        "run",
        "--state",
        layout.state.to_str().unwrap(),
        "--lower",
        "/a,b:/c",
    ]);
    command.args([
        "--upper", "/u", "--work", "/w", "--merged", "/m", "--bundle", "/b", "--id", "x",
    ]);
    command.args(["--crun", crun.to_str().unwrap()]);
    let output = command.output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        text(&output).1.contains("silo-ctr: lower path"),
        "{:?}",
        text(&output)
    );
    assert!(!layout.state.join("holder.pid").exists());
    let _ = std::fs::remove_dir_all(&layout.dir);
}

/// Receives one SCM_RIGHTS descriptor with the data that came with it.
fn receive_fd(stream: &std::os::unix::net::UnixStream) -> (Vec<u8>, OwnedFd) {
    let mut data = [0u8; 16];
    let mut iov = libc::iovec {
        iov_base: data.as_mut_ptr() as *mut libc::c_void,
        iov_len: data.len(),
    };
    let mut control = [0u64; 8];
    // SAFETY: the buffers outlive the call; a descriptor received in SCM_RIGHTS is a fresh descriptor of
    // this process, so owning it is sound.
    unsafe {
        let mut message: libc::msghdr = std::mem::zeroed();
        message.msg_iov = &mut iov;
        message.msg_iovlen = 1;
        message.msg_control = control.as_mut_ptr() as *mut libc::c_void;
        message.msg_controllen = std::mem::size_of_val(&control) as _;
        let got = libc::recvmsg(stream.as_raw_fd(), &mut message, 0);
        assert!(got > 0, "recvmsg failed");
        let header = libc::CMSG_FIRSTHDR(&message);
        assert!(!header.is_null(), "no descriptor attached");
        let mut fd: RawFd = -1;
        std::ptr::copy_nonoverlapping(
            libc::CMSG_DATA(header),
            &mut fd as *mut RawFd as *mut u8,
            std::mem::size_of::<RawFd>(),
        );
        (data[..got as usize].to_vec(), OwnedFd::from_raw_fd(fd))
    }
}

#[test]
fn run_hands_the_tap_to_the_engine_and_configures_it_as_told() {
    if !namespaces_available() {
        return;
    }
    let layout = layout("tap");
    let tail =
        "cat /proc/net/dev >> $out; cat /proc/net/route >> $out; cat /proc/net/fib_trie >> $out; \
                if command -v ip >/dev/null; then ip -o link show tap0 >> $out; fi; exit 0";
    let crun = fake_crun(&layout.dir, &layout.state, &layout.merged, tail);
    let socket = layout.dir.join("engine.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let engine = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let (data, tap) = receive_fd(&stream);
        assert_eq!(data, b"tap\n");
        let mut writer = &stream;
        writer
            .write_all(b"mac=02:00:5e:10:00:01\nmtu=1400\naddress=10.0.2.15\nprefix_len=24\ngateway=10.0.2.2\n\n")
            .unwrap();
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).unwrap();
        (line, tap)
    });
    let output = run_holder(&layout, &crun, &["--sock", socket.to_str().unwrap()]);
    let (err, skip) = (
        text(&output).1,
        !overlay_usable(&output) || text(&output).1.contains("/dev/net/tun"),
    );
    if skip {
        eprintln!("skipped: {}", err.trim());
        let _ = std::os::unix::net::UnixStream::connect(&socket);
        let _ = engine.join();
        let _ = std::fs::remove_dir_all(&layout.dir);
        return;
    }
    let (line, _tap) = engine.join().unwrap();
    assert_eq!(line, "up\n");
    assert_eq!(output.status.code(), Some(0), "{:?}", text(&output));
    let recorded = std::fs::read_to_string(layout.dir.join("out")).unwrap();
    let devices: Vec<&str> = recorded
        .lines()
        .skip_while(|line| !line.starts_with("Inter-|"))
        .skip(2)
        .take_while(|line| line.contains(':') && !line.starts_with("Iface"))
        .map(|line| line.split(':').next().unwrap().trim())
        .collect();
    assert_eq!(devices, ["lo", "tap0"], "{recorded}");
    assert!(recorded.contains("tap0\t00000000\t0202000A"), "{recorded}");
    assert!(recorded.contains("10.0.2.15"), "{recorded}");
    if recorded.contains("link/ether") {
        assert!(
            recorded.contains("link/ether 02:00:5e:10:00:01"),
            "{recorded}"
        );
        assert!(recorded.contains("mtu 1400"), "{recorded}");
    }
    let _ = std::fs::remove_dir_all(&layout.dir);
}

#[test]
fn run_exits_with_the_engines_error_text() {
    if !namespaces_available() {
        return;
    }
    let layout = layout("engine-error");
    let crun = fake_crun(&layout.dir, &layout.state, &layout.merged, "exit 0");
    let socket = layout.dir.join("engine.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let engine = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let _ = receive_fd(&stream);
        (&stream).write_all(b"error=no address left\n").unwrap();
    });
    let output = run_holder(&layout, &crun, &["--sock", socket.to_str().unwrap()]);
    if !overlay_usable(&output) || text(&output).1.contains("/dev/net/tun") {
        eprintln!("skipped: {}", text(&output).1.trim());
        let _ = std::os::unix::net::UnixStream::connect(&socket);
        let _ = engine.join();
        let _ = std::fs::remove_dir_all(&layout.dir);
        return;
    }
    engine.join().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        text(&output).1.contains("silo-ctr: no address left"),
        "{:?}",
        text(&output)
    );
    let _ = std::fs::remove_dir_all(&layout.dir);
}
