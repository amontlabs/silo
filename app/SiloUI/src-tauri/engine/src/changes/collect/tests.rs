use std::{
    io::Write,
    process::{Command, Stdio},
    sync::Mutex,
};

use super::*;

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

/// A manifest whose compressed form is larger than one `guest.exec` result (768 KiB).
fn large_manifest() -> String {
    let mut manifest = String::from("META\tformat\t2\nMETA\tos\tLinux\n");
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    for index in 0..40_000 {
        let mut digest = String::new();
        for _ in 0..4 {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            digest.push_str(&format!("{state:016x}"));
        }
        manifest.push_str(&format!(
            "F\t/srv/data/file-{index}\t{index}\t1700000000\t{digest}\n"
        ));
    }
    manifest
}

/// A guest that answers the capture protocol in memory.
struct FakeGuest {
    gz: Vec<u8>,
    digest: String,
    running_polls: Mutex<u32>,
    status: &'static str,
    reads: Mutex<Vec<(u64, u64)>>,
    commands: Mutex<Vec<String>>,
    owners: &'static str,
}

impl FakeGuest {
    fn new(manifest: &str) -> Self {
        let gz = gzip(manifest.as_bytes());
        Self {
            digest: hex(&Sha256::digest(&gz)),
            gz,
            running_polls: Mutex::new(2),
            status: "0",
            reads: Mutex::new(Vec::new()),
            commands: Mutex::new(Vec::new()),
            owners: "",
        }
    }
}

fn out(text: &str) -> ExecOut {
    ExecOut {
        exit_code: Some(0),
        stdout: text.as_bytes().to_vec(),
        stderr: Vec::new(),
    }
}

impl GuestRunner for FakeGuest {
    fn is_macos(&self) -> bool {
        false
    }

    fn exec(
        &self,
        argv: &[String],
        stdin: Option<Vec<u8>>,
        user: GuestUser,
        timeout: Duration,
    ) -> Result<ExecOut, GuestError> {
        assert_eq!(user, GuestUser::Root, "a Linux capture runs as root");
        assert!(
            timeout <= Duration::from_millis(MAX_BOUNDED_TIMEOUT_MS),
            "{timeout:?} is over the bounded cap"
        );
        self.commands.lock().unwrap().push(argv.join(" "));
        match argv[0].as_str() {
            "sh" if argv[2] == START => {
                assert_eq!(stdin.as_deref(), Some(COLLECT_SCRIPT.as_bytes()));
                Ok(out("started\n"))
            }
            "sh" if argv[2] == POLL => {
                let mut left = self.running_polls.lock().unwrap();
                if *left > 0 {
                    *left -= 1;
                    return Ok(out("running\n"));
                }
                Ok(out(&format!("done {} {}\n", self.status, self.digest)))
            }
            "rm" | "tail" => Ok(out("collector said no\n")),
            "bash" => {
                assert!(String::from_utf8(stdin.unwrap())
                    .unwrap()
                    .contains("/etc/a"));
                Ok(out(self.owners))
            }
            other => panic!("unexpected command {other}"),
        }
    }

    fn read(&self, path: &str, offset: u64, length: u64) -> Result<FileChunk, GuestError> {
        assert!(path.ends_with("/m.tsv.gz"));
        assert!(length <= MAX_RESPONSE_RAW_BYTES as u64);
        self.reads.lock().unwrap().push((offset, length));
        let start = (offset as usize).min(self.gz.len());
        let end = (start + length as usize).min(self.gz.len());
        Ok(FileChunk {
            data: self.gz[start..end].to_vec(),
            size: self.gz.len() as u64,
        })
    }
}

fn quick() -> Options {
    Options {
        budget: Duration::from_secs(5),
        poll: Duration::from_secs(1),
        ..Options::default()
    }
}

#[test]
fn a_manifest_past_the_exec_cap_is_read_back_in_chunks() {
    let manifest = large_manifest();
    let guest = FakeGuest::new(&manifest);
    assert!(
        guest.gz.len() > MAX_OUTPUT_BYTES,
        "the fixture must be larger than one exec result ({} bytes)",
        guest.gz.len()
    );
    let captured = capture(&guest, &quick()).unwrap();
    assert_eq!(captured, manifest);
    let reads = guest.reads.lock().unwrap();
    assert!(reads.len() >= 2, "{reads:?}");
    assert_eq!(reads[0].0, 0);
    assert!(reads.windows(2).all(|pair| pair[1].0 > pair[0].0));
    let commands = guest.commands.lock().unwrap();
    assert_eq!(commands.iter().filter(|c| c.contains("sleep 1")).count(), 3);
    assert!(
        commands
            .last()
            .unwrap()
            .starts_with("rm -rf /tmp/silo-collect-"),
        "the temp folder is removed: {commands:?}"
    );
}

#[test]
fn polling_stays_inside_the_bounded_call_cap() {
    let guest = FakeGuest::new("META\tformat\t2\n");
    let options = Options {
        budget: Duration::from_secs(3600),
        poll: Duration::from_secs(900),
        ..quick()
    };
    capture(&guest, &options).unwrap();
    let commands = guest.commands.lock().unwrap();
    let poll = commands.iter().find(|c| c.contains("sleep 1")).unwrap();
    let seconds: u64 = poll.rsplit(' ').next().unwrap().parse().unwrap();
    assert!(seconds + 25 <= MAX_BOUNDED_TIMEOUT_MS / 1000, "{seconds}");
}

#[test]
fn a_collector_that_never_finishes_runs_out_of_budget() {
    let guest = FakeGuest::new("META\tformat\t2\n");
    *guest.running_polls.lock().unwrap() = u32::MAX;
    let options = Options {
        budget: Duration::from_millis(40),
        ..quick()
    };
    let error = capture(&guest, &options).unwrap_err();
    assert_eq!(error.code, ErrorCode::TimedOut);
}

#[test]
fn a_failed_collector_reports_its_error_output() {
    let mut guest = FakeGuest::new("META\tformat\t2\n");
    guest.status = "3";
    let error = capture(&guest, &quick()).unwrap_err();
    assert!(error.message.contains("status 3"), "{}", error.message);
    assert!(error.message.contains("collector said no"));
}

#[test]
fn a_corrupted_read_back_is_refused() {
    let mut guest = FakeGuest::new("META\tformat\t2\nF\t/a\t1\t1\t-\n");
    guest.digest = "00".repeat(32);
    let error = capture(&guest, &quick()).unwrap_err();
    assert!(error.message.contains("changed while"), "{}", error.message);
}

#[test]
fn only_a_format_2_manifest_is_accepted() {
    let guest = FakeGuest::new("META\tformat\t1\n");
    assert!(capture(&guest, &quick()).is_err());
}

#[test]
fn changed_paths_cover_new_and_modified_files_and_ignored_files() {
    let base = "META\tformat\t2\nF\t/etc/a\t1\t1\t-\nF\t/etc/same\t1\t1\t-\nGIG\t/srv/app\t.env\t9\t1\tabc\nF\t/etc/gone\t1\t1\t-\n";
    let current = "META\tformat\t2\nF\t/etc/a\t2\t1\t-\nF\t/etc/same\t1\t1\t-\nF\t/etc/new\t1\t1\t-\nGIG\t/srv/app\t.env\t9\t2\tdef\nL\t/usr/local/bin/x\t1\t1\t../lib/x\nF\t/a\\tb\t1\t1\t-\n";
    assert_eq!(
        changed_paths(base, current),
        ["/etc/a", "/etc/new", "/srv/app/.env", "/usr/local/bin/x"]
    );
}

#[test]
fn owner_records_are_appended_for_changed_paths_only() {
    let mut guest = FakeGuest::new("");
    guest.owners = "OWN\tdpkg\tcurl\t/etc/a\nnoise\n";
    let base = "META\tformat\t2\n";
    let current = "META\tformat\t2\nF\t/etc/a\t2\t1\t-\n".to_owned();
    let merged = with_owners(&guest, base, current);
    assert!(merged.ends_with("OWN\tdpkg\tcurl\t/etc/a\n"), "{merged}");
    assert!(!merged.contains("noise"));
    assert_eq!(owned_paths(&merged).len(), 1);
    // Nothing changed: the guest is not asked.
    let untouched = with_owners(&guest, &merged, merged.clone());
    assert_eq!(untouched, merged);
}

#[test]
fn a_failed_owner_query_keeps_the_manifest() {
    struct Failing;
    impl GuestRunner for Failing {
        fn is_macos(&self) -> bool {
            true
        }
        fn exec(
            &self,
            _: &[String],
            _: Option<Vec<u8>>,
            user: GuestUser,
            _: Duration,
        ) -> Result<ExecOut, GuestError> {
            assert_eq!(user, GuestUser::Account, "macOS runs as the account");
            Err(GuestError::internal("gone"))
        }
        fn read(&self, _: &str, _: u64, _: u64) -> Result<FileChunk, GuestError> {
            unreachable!()
        }
    }
    let current = "META\tformat\t2\nF\t/etc/a\t2\t1\t-\n".to_owned();
    assert_eq!(
        with_owners(&Failing, "META\tformat\t2\n", current.clone()),
        current
    );
}

// MARK: The scripts themselves, on this machine

fn run_script(
    script: &str,
    args: &[&str],
    stdin: &str,
    home: &std::path::Path,
) -> (String, String) {
    let mut child = Command::new("bash")
        .arg("-c")
        .arg(script)
        .args(args)
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn both_scripts_are_valid_bash() {
    for script in [COLLECT_SCRIPT, OWNER_SCRIPT] {
        let mut child = Command::new("bash")
            .arg("-n")
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(script.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn the_owner_script_attributes_brew_pip_and_links() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let cellar = root.join("brew/Cellar/jq/1.7/bin");
    std::fs::create_dir_all(&cellar).unwrap();
    std::fs::write(cellar.join("jq"), b"x").unwrap();
    std::fs::create_dir_all(root.join("brew/bin")).unwrap();
    std::os::unix::fs::symlink("../Cellar/jq/1.7/bin/jq", root.join("brew/bin/jq")).unwrap();
    let site = root.join("py/lib/python3.12/site-packages");
    std::fs::create_dir_all(site.join("requests-2.0.dist-info")).unwrap();
    std::fs::create_dir_all(site.join("requests")).unwrap();
    std::fs::write(site.join("requests/__init__.py"), b"x").unwrap();
    std::fs::write(
        site.join("requests-2.0.dist-info/RECORD"),
        "requests/__init__.py,sha256=abc,1\n../../../bin/req,,\nrequests-2.0.dist-info/RECORD,,\n",
    )
    .unwrap();
    let loose = root.join("py/other.txt");
    std::fs::write(&loose, b"x").unwrap();
    let paths = [
        cellar.join("jq"),
        root.join("brew/bin/jq"),
        site.join("requests/__init__.py"),
        loose,
    ];
    let stdin: String = paths
        .iter()
        .map(|path| format!("{}\n", path.display()))
        .collect();
    let (stdout, stderr) = run_script(OWNER_SCRIPT, &[], &stdin, root);
    let lines: BTreeMap<String, String> = stdout
        .lines()
        .map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            assert_eq!(f[0], "OWN", "{stdout} {stderr}");
            (f[3].to_owned(), format!("{}:{}", f[1], f[2]))
        })
        .collect();
    let owner = |path: &std::path::Path| lines.get(&path.display().to_string()).cloned();
    assert_eq!(owner(&paths[0]).as_deref(), Some("brew:jq"), "{stdout}");
    assert_eq!(owner(&paths[1]).as_deref(), Some("brew:jq"), "{stdout}");
    assert_eq!(
        owner(&paths[2]).as_deref(),
        Some("pip:requests"),
        "{stdout}"
    );
    assert_eq!(owner(&paths[3]), None);
}

#[test]
fn the_collector_describes_a_fixture_home() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let project = home.join("proj");
    std::fs::create_dir_all(project.join("node_modules/x")).unwrap();
    std::fs::create_dir_all(project.join("sub")).unwrap();
    let git = |args: &[&str]| {
        assert!(Command::new("git")
            .args(args)
            .current_dir(&project)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .status()
            .unwrap()
            .success())
    };
    git(&["init", "-q", "."]);
    std::fs::write(project.join(".gitignore"), ".env\n*.log\nnode_modules/\n").unwrap();
    std::fs::write(project.join(".env"), "SECRET=1\n").unwrap();
    std::fs::write(project.join("sub/a.log"), "hi\n").unwrap();
    std::fs::write(project.join("node_modules/x/i.js"), "n\n").unwrap();
    std::fs::write(project.join("we\tird.log"), "t\n").unwrap();
    std::fs::write(project.join("app.sqlite3"), b"SQLite format 3\0").unwrap();
    let (stdout, _) = run_script(COLLECT_SCRIPT, &[], "", &home);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines[0], "META\tformat\t2");
    let repo = project.display().to_string();
    let gig: Vec<&&str> = lines
        .iter()
        .filter(|line| line.starts_with("GIG\t") && line.contains(&format!("\t{repo}\t")))
        .collect();
    let names: Vec<&str> = gig
        .iter()
        .map(|line| line.split('\t').nth(2).unwrap())
        .collect();
    assert!(names.contains(&".env"), "{names:?}");
    assert!(names.contains(&"sub/a.log"), "{names:?}");
    assert!(
        !names.iter().any(|name| name.contains("node_modules")),
        "{names:?}"
    );
    let env = gig
        .iter()
        .find(|l| l.split('\t').nth(2) == Some(".env"))
        .unwrap();
    assert_eq!(env.split('\t').nth(3), Some("9"));
    assert_eq!(env.split('\t').nth(5).map(str::len), Some(64), "{env}");
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("DB\tsqlite\t") && l.contains("app.sqlite3")),
        "sqlite digests are listed"
    );
    // Every record is a known tag with tabs between its fields.
    let known = [
        "META", "F", "L", "P", "PKG", "PKGM", "GIT", "GITF", "GIG", "CRON", "CRONF", "SVC",
        "LISTEN", "PROC", "PKGD", "DB", "DBT", "LAGENT",
    ];
    assert!(lines
        .iter()
        .all(|line| known.contains(&line.split('\t').next().unwrap())));
}
