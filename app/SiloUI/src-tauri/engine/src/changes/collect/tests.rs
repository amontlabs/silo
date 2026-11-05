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
    launch: &'static str,
    claimed_size: Option<u64>,
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
            launch: "started\n",
            claimed_size: None,
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
                Ok(out(self.launch))
            }
            "sh" if argv[2] == CLEAN => Ok(out("")),
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
            size: self.claimed_size.unwrap_or(self.gz.len() as u64),
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
    assert_eq!(
        commands
            .iter()
            .filter(|c| c.contains("while [ ! -f"))
            .count(),
        3
    );
    assert!(
        commands.last().unwrap().contains("rm -rf \"$d\""),
        "the collector and its folder are cleaned up: {commands:?}"
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
    let poll = commands
        .iter()
        .find(|c| c.contains("while [ ! -f"))
        .unwrap();
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
fn a_timed_out_collector_is_stopped_and_cleaned_up() {
    let guest = FakeGuest::new("META\tformat\t2\n");
    *guest.running_polls.lock().unwrap() = u32::MAX;
    let options = Options {
        budget: Duration::from_millis(40),
        ..quick()
    };
    assert_eq!(
        capture(&guest, &options).unwrap_err().code,
        ErrorCode::TimedOut
    );
    let commands = guest.commands.lock().unwrap();
    let clean = commands.last().unwrap();
    assert!(clean.contains("kill -TERM -- \"-$p\""), "{clean}");
    assert!(clean.contains("kill -KILL"), "{clean}");
    assert!(clean.ends_with(&commands[0].rsplit(' ').next().unwrap().to_owned()));
}

#[test]
fn a_second_capture_is_refused_while_one_runs() {
    let mut guest = FakeGuest::new("META\tformat\t2\n");
    guest.launch = "busy\n";
    let error = capture(&guest, &quick()).unwrap_err();
    assert_eq!(error.code, ErrorCode::Busy);
    assert!(guest.reads.lock().unwrap().is_empty());
}

#[test]
fn a_file_over_the_download_cap_is_refused_before_it_is_read() {
    let mut guest = FakeGuest::new("META\tformat\t2\n");
    guest.claimed_size = Some(10 * 1024 * 1024 * 1024);
    let error = capture(&guest, &quick()).unwrap_err();
    assert_eq!(error.code, ErrorCode::OutputTooLarge);
    assert_eq!(guest.reads.lock().unwrap().len(), 1);
}

#[test]
fn reading_chunks_counts_against_the_deadline() {
    let guest = FakeGuest::new(&large_manifest());
    let error = read_bounded(&guest, "/x/m.tsv.gz", 1024, u64::MAX, Instant::now()).unwrap_err();
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
    let stdout = run_collector_with(
        &home,
        &home.join("no-bin"),
        &[("SILO_SKIP", "packages services procs postgres")],
    );
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
        "LISTEN", "PROC", "PKGD", "DB", "DBT", "LAGENT", "CRONT", "QPARK", "QCRON", "DBU",
    ];
    assert!(lines
        .iter()
        .all(|line| known.contains(&line.split('\t').next().unwrap())));
}

// MARK: Commands the collector runs on the user's behalf

/// How long one run of the collector may take before the test fails instead of hanging.
const COLLECTOR_DEADLINE: std::time::Duration = std::time::Duration::from_secs(60);

/// Runs the collector against fixtures only: the scans cover `home`, no package manager, service
/// manager, process list, SQLite scan or PostgreSQL server is asked (unless `SILO_SKIP` says
/// otherwise), and the run fails after `COLLECTOR_DEADLINE`.
fn run_collector_with(
    home: &std::path::Path,
    path_prefix: &std::path::Path,
    env: &[(&str, &str)],
) -> String {
    let path = format!(
        "{}:{}",
        path_prefix.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let stdout_file = tempfile::NamedTempFile::new().unwrap();
    let mut command = Command::new("bash");
    command
        .arg("-c")
        .arg(COLLECT_SCRIPT)
        .env("HOME", home)
        .env("PATH", path)
        .env("SILO_ROOTS", home)
        .env("SILO_GIT_ROOTS", home)
        .env("SILO_CRON_DIRS", home.join("no-cron"))
        .env("SILO_QUIET_DISABLED", home.join("no-quiet"))
        .env("SILO_PROC_ROOT", home.join("no-proc"))
        .env("SILO_SKIP", "packages services procs sqlite postgres")
        .stdin(Stdio::null())
        .stdout(stdout_file.reopen().unwrap())
        .stderr(Stdio::null());
    for (name, value) in env {
        command.env(name, value);
    }
    let mut child = command.spawn().unwrap();
    let started = std::time::Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if started.elapsed() > COLLECTOR_DEADLINE {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the collector did not finish within {COLLECTOR_DEADLINE:?}");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    std::fs::read_to_string(stdout_file.path()).unwrap()
}

fn write_executable(path: &std::path::Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn a_repository_that_names_a_command_in_its_config_does_not_run_it() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let project = home.join("proj");
    std::fs::create_dir_all(&project).unwrap();
    let marker = dir.path().join("ran");
    let hook = dir.path().join("hook.sh");
    write_executable(
        &hook,
        &format!("#!/bin/sh\ntouch '{}'\nprintf '\\0'\n", marker.display()),
    );
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
    std::fs::write(project.join("a.txt"), "a\n").unwrap();
    git(&["config", "core.fsmonitor", hook.to_str().unwrap()]);
    git(&["config", "core.untrackedCache", "true"]);
    // The control: git itself runs the command when asked about this repository.
    git(&["status", "--porcelain"]);
    if !marker.exists() {
        eprintln!("this git does not run core.fsmonitor commands; nothing to guard against");
        return;
    }
    std::fs::remove_file(&marker).unwrap();
    let out = run_collector_with(&home, dir.path(), &[]);
    assert!(
        out.lines().any(|l| l.starts_with("GIT\t")),
        "the repository is still described: {out}"
    );
    assert!(!marker.exists(), "the repository's command ran");
}

#[test]
fn postgres_names_cannot_become_options_and_the_dump_key_is_not_hashed() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let log = dir.path().join("calls.log");
    write_executable(
        &bin.join("sudo"),
        "#!/bin/sh\nshift; shift; shift\nexec \"$@\"\n",
    );
    write_executable(
        &bin.join("psql"),
        &format!(
            "#!/bin/sh\necho \"psql $*\" >> '{}'\ncase \"$*\" in\n  *'select datname'*) printf 'app\\n--file=/tmp/pwned\\nhost=evil dbname=x\\n-bad\\n' ;;\nesac\nexit 0\n",
            log.display()
        ),
    );
    write_executable(
        &bin.join("pg_dump"),
        &format!(
            "#!/bin/sh\necho \"pg_dump $*\" >> '{}'\nprintf -- '-- PostgreSQL database dump\\n\\\\restrict %s\\nCREATE TABLE t (a int);\\n\\\\unrestrict %s\\n' \"$KEY\" \"$KEY\"\n",
            log.display()
        ),
    );
    let digest = |key: &str| {
        let out = run_collector_with(&home, &bin, &[("KEY", key), ("SILO_SKIP", "packages services procs sqlite")]);
        let lines: Vec<&str> = out
            .lines()
            .filter(|l| l.starts_with("DB\tpostgres\t"))
            .collect();
        assert_eq!(lines.len(), 1, "only the plain name is dumped: {out}");
        assert!(lines[0].starts_with("DB\tpostgres\tapp\tschema\t"), "{out}");
        lines[0].to_owned()
    };
    let first = digest("aaaaaaaa");
    let second = digest("zzzzzzzz");
    assert_eq!(first, second, "the random restrict key changed the digest");
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(
        calls
            .lines()
            .filter(|l| l.starts_with("pg_dump"))
            .all(|l| l == "pg_dump -w --schema-only --dbname=app"),
        "{calls}"
    );
    assert!(
        !calls.contains("pwned") && !calls.contains("evil"),
        "{calls}"
    );
}

// MARK: Crontabs, quiet parks, listeners and unreadable databases

fn records(out: &str, kind: &str) -> Vec<String> {
    out.lines()
        .filter(|l| l.split('\t').next() == Some(kind))
        .map(str::to_owned)
        .collect()
}

#[test]
fn crontabs_are_hashed_from_their_files_and_parked_ones_are_reported() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let spool = dir.path().join("spool");
    std::fs::create_dir_all(&spool).unwrap();
    std::fs::write(spool.join("silo"), "* * * * * tick\n").unwrap();
    std::fs::write(spool.join("empty"), "").unwrap();
    let secret = spool.join("locked");
    std::fs::write(&secret, "0 0 * * * x\n").unwrap();
    std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o000)).unwrap();
    let quiet = dir.path().join("quiet");
    std::fs::create_dir_all(quiet.join("var/spool/cron/crontabs")).unwrap();
    std::fs::write(
        quiet.join("var/spool/cron/crontabs/root"),
        "0 3 * * * backup\n",
    )
    .unwrap();
    let out = run_collector_with(
        &home,
        dir.path(),
        &[
            ("SILO_CRON_DIRS", spool.to_str().unwrap()),
            ("SILO_QUIET_DISABLED", quiet.to_str().unwrap()),
        ],
    );
    let sha = |text: &str| {
        let digest = Sha256::digest(text.as_bytes());
        hex(&digest)
    };
    let cron = records(&out, "CRON");
    assert!(
        cron.contains(&format!("CRON\tsilo\t{}", sha("* * * * * tick\n"))),
        "{out}"
    );
    assert!(cron.contains(&format!("CRON\tempty\t{}", sha(""))), "{out}");
    assert!(records(&out, "CRONT").contains(&"CRONT\tsilo\t* * * * * tick\\n".to_owned()));
    // The file nobody could read is marked, not hashed as empty. (When the tests run as root
    // nothing is unreadable.)
    if std::fs::read(&secret).is_err() {
        assert!(cron.contains(&"CRON\tlocked\t?".to_owned()), "{out}");
        assert!(records(&out, "CRONT")
            .iter()
            .all(|line| !line.starts_with("CRONT\tlocked")));
    }
    let backup = sha("0 3 * * * backup\n");
    assert_eq!(
        records(&out, "QPARK"),
        [format!("QPARK\t/var/spool/cron/crontabs/root\t{backup}")]
    );
    assert_eq!(
        records(&out, "QCRON"),
        [format!("QCRON\troot\t{backup}\t0 3 * * * backup\\n")]
    );
    std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn listeners_are_matched_to_the_process_holding_the_socket() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let proc = dir.path().join("proc");
    std::fs::create_dir_all(proc.join("net")).unwrap();
    // 0x1538 is 5432; 0A is the listening state, 01 an established connection.
    std::fs::write(
        proc.join("net/tcp"),
        "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 00000000:1538 00000000:0000 0A 00000000:00000000 00:00000000 00000000   110        0 4242 1 0 100 0 0 10 0\n   1: 0100007F:0050 0100007F:9999 01 00000000:00000000 00:00000000 00000000     0        0 5555 1 0 100 0 0 10 0\n",
    )
    .unwrap();
    std::fs::write(proc.join("net/tcp6"), "  sl  local_address\n").unwrap();
    for (pid, comm, inode) in [("100", "postgres", "4242"), ("200", "bash", "9999")] {
        std::fs::create_dir_all(proc.join(pid).join("fd")).unwrap();
        std::fs::write(proc.join(pid).join("comm"), format!("{comm}\n")).unwrap();
        std::os::unix::fs::symlink(format!("socket:[{inode}]"), proc.join(pid).join("fd/3"))
            .unwrap();
    }
    let out = run_collector_with(
        &home,
        dir.path(),
        &[("SILO_PROC_ROOT", proc.to_str().unwrap())],
    );
    assert_eq!(
        records(&out, "LISTEN"),
        ["LISTEN\ttcp\t5432\tpostgres"],
        "{out}"
    );
}

#[test]
fn postgres_that_does_not_answer_is_recorded_not_started_or_hidden() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let log = dir.path().join("calls.log");
    write_executable(
        &bin.join("sudo"),
        "#!/bin/sh\nshift; shift; shift\nexec \"$@\"\n",
    );
    write_executable(
        &bin.join("psql"),
        &format!(
            "#!/bin/sh\necho \"psql $*\" >> '{}'\nexit 2\n",
            log.display()
        ),
    );
    write_executable(&bin.join("pg_dump"), "#!/bin/sh\nexit 1\n");
    let out = run_collector_with(&home, &bin, &[("SILO_SKIP", "packages services procs sqlite")]);
    assert_eq!(
        records(&out, "DBU"),
        ["DBU\tpostgres\tthe server did not answer"],
        "{out}"
    );
    assert!(records(&out, "DB").is_empty() && records(&out, "DBT").is_empty());
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(
        calls.lines().all(|l| l.contains("select 1")),
        "only the probe ran: {calls}"
    );
}
