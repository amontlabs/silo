//! Whether this host can run container computers (crun, rootless). Silo only checks and names
//! what is missing; it never changes the host.

use std::io;
use std::path::{Path, PathBuf};

const NOT_LINUX: &str = "container computers run only on a Linux host";
const MIN_SUB_IDS: u64 = 65536;

/// How much a finding matters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Required,
    /// Works without, but a limit stays off.
    Advice,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Advice => "advice",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub id: &'static str,
    pub level: Level,
    pub ok: bool,
    /// What was checked, as a statement ("crun is installed").
    pub what: String,
    /// What to do when it is not ok, in plain words; empty when ok.
    pub fix: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub findings: Vec<Finding>,
}

impl Report {
    /// Every required finding is ok.
    pub fn ready(&self) -> bool {
        self.findings
            .iter()
            .all(|finding| finding.ok || finding.level != Level::Required)
    }

    /// The findings that are not ok, advice included.
    pub fn problems(&self) -> impl Iterator<Item = &Finding> {
        self.findings.iter().filter(|finding| !finding.ok)
    }

    /// One message naming every unmet required finding with its fix; `None` when ready. A host
    /// that is not Linux gets the one reason, not every check.
    pub fn refusal(&self) -> Option<String> {
        let unmet: Vec<&Finding> = self
            .problems()
            .filter(|finding| finding.level == Level::Required)
            .collect();
        if unmet.is_empty() {
            return None;
        }
        // A host that is not Linux will not become one: say that alone, with no "yet".
        if unmet.iter().any(|finding| finding.id == "linux") {
            return Some("Container computers run only on a Linux host.".to_owned());
        }
        let list: Vec<String> = unmet
            .iter()
            .map(|finding| format!("{} — {}", finding.what, finding.fix))
            .collect();
        Some(format!(
            "This device cannot run container computers yet: {}",
            list.join("; ")
        ))
    }
}

/// What the checks read from the host.
pub trait Probe {
    fn read_to_string(&self, path: &Path) -> io::Result<String>;
    fn exists(&self, path: &Path) -> bool;
    fn find_executable(&self, name: &str) -> Option<PathBuf>;
    /// The numeric id and user name of this account.
    fn account(&self) -> (u32, String);
    fn can_open_read_write(&self, path: &Path) -> bool;
    fn env(&self, name: &str) -> Option<String>;
    /// The file is owned by root and has the setuid bit.
    fn is_setuid_root(&self, path: &Path) -> bool;
}

/// Checks the real host.
pub fn check() -> Report {
    check_with(&HostProbe)
}

/// Checks the host a probe describes.
pub fn check_with(probe: &dyn Probe) -> Report {
    check_on(cfg!(target_os = "linux"), probe)
}

fn check_on(linux: bool, probe: &dyn Probe) -> Report {
    if linux {
        return Report {
            findings: linux_findings(probe),
        };
    }
    // Same ids on every host, each one unmet and saying why.
    let mut findings = linux_findings(&Nothing);
    for finding in &mut findings {
        finding.ok = false;
        finding.fix = NOT_LINUX.to_owned();
    }
    Report { findings }
}

fn linux_findings(probe: &dyn Probe) -> Vec<Finding> {
    let mut findings = vec![finding(
        "linux",
        Level::Required,
        true,
        "This host runs Linux",
        "run Silo's engine on a Linux host",
    )];
    for (id, tool) in [("crun", "crun"), ("skopeo", "skopeo"), ("umoci", "umoci")] {
        findings.push(finding(
            id,
            Level::Required,
            probe.find_executable(tool).is_some(),
            &format!("{tool} is installed"),
            &format!("install it (Ubuntu 24.04: sudo apt install {tool})"),
        ));
    }
    findings.push(uidmap(probe));
    let (uid, user) = probe.account();
    for (id, file, noun) in [
        ("subuid", "/etc/subuid", "user ids"),
        ("subgid", "/etc/subgid", "group ids"),
    ] {
        findings.push(finding(
            id,
            Level::Required,
            sub_ids_ok(probe, Path::new(file), uid, &user),
            &format!("This account has at least {MIN_SUB_IDS} subordinate {noun} in {file}"),
            "give it a range (sudo usermod --add-subuids 100000-165535 --add-subgids 100000-165535 $USER)",
        ));
    }
    let tun = Path::new("/dev/net/tun");
    findings.push(finding(
        "tun",
        Level::Required,
        probe.exists(tun) && probe.can_open_read_write(tun),
        "/dev/net/tun exists and this account can open it",
        "load the tun module (sudo modprobe tun) and make /dev/net/tun readable and writable by this account",
    ));
    findings.push(finding(
        "userns",
        Level::Required,
        max_user_namespaces(probe) > 0,
        "Unprivileged user namespaces are allowed",
        "raise the limit (sudo sysctl -w user.max_user_namespaces=15000)",
    ));
    let restrict = probe.read_to_string(Path::new(
        "/proc/sys/kernel/apparmor_restrict_unprivileged_userns",
    ));
    findings.push(finding(
        "apparmor-userns",
        Level::Required,
        restrict.map_or(true, |text| text.trim() == "0"),
        "AppArmor lets unprivileged accounts use user namespaces",
        "run sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0, and to keep it after a reboot put kernel.apparmor_restrict_unprivileged_userns=0 in a file in /etc/sysctl.d. This relaxes a host-wide guard; an AppArmor profile for Silo's helper is the narrower alternative",
    ));
    findings.push(finding(
        "overlayfs",
        Level::Required,
        probe
            .read_to_string(Path::new("/proc/filesystems"))
            .is_ok_and(|text| text.split_whitespace().any(|word| word == "overlay")),
        "The kernel supports overlay filesystems",
        "load the overlay module (sudo modprobe overlay)",
    ));
    findings.push(finding(
        "cgroup-v2",
        Level::Required,
        probe.exists(Path::new("/sys/fs/cgroup/cgroup.controllers")),
        "The host uses the unified cgroup v2 hierarchy",
        "boot with systemd.unified_cgroup_hierarchy=1 (current distributions already do)",
    ));
    let controllers = probe
        .read_to_string(Path::new(&format!(
            "/sys/fs/cgroup/user.slice/user-{uid}.slice/user@{uid}.service/cgroup.controllers"
        )))
        .ok();
    let has = |word: &str| {
        controllers
            .as_deref()
            .is_some_and(|text| text.split_whitespace().any(|item| item == word))
    };
    findings.push(finding(
        "cgroup-delegation",
        Level::Required,
        has("memory") && has("pids"),
        "This account's systemd user manager can limit memory and processes",
        "run sudo loginctl enable-linger $USER and log in again so a user manager runs; if memory and pids are still missing, create /etc/systemd/system/user@.service.d/delegate.conf containing [Service] and Delegate=memory pids cpu cpuset, then reboot",
    ));
    findings.push(finding(
        "cpu-limit",
        Level::Advice,
        has("cpu"),
        "CPU limits are not enforced",
        "add cpu to Delegate= in /etc/systemd/system/user@.service.d/delegate.conf (Delegate=memory pids cpu cpuset), then reboot",
    ));
    findings.push(finding(
        "systemd-run",
        Level::Required,
        probe.find_executable("systemd-run").is_some()
            && probe
                .env("XDG_RUNTIME_DIR")
                .is_some_and(|dir| !dir.is_empty()),
        "systemd-run is installed and this login has a runtime directory",
        "install systemd and log in through a normal session (ssh or console) so XDG_RUNTIME_DIR is set",
    ));
    findings.push(landlock(probe));
    findings
}

fn finding(id: &'static str, level: Level, ok: bool, what: &str, fix: &str) -> Finding {
    Finding {
        id,
        level,
        ok,
        what: what.to_owned(),
        fix: if ok { String::new() } else { fix.to_owned() },
    }
}

fn uidmap(probe: &dyn Probe) -> Finding {
    let found = ["newuidmap", "newgidmap"].map(|name| probe.find_executable(name));
    let (ok, fix) = match &found {
        [Some(uid), Some(gid)] => {
            if probe.is_setuid_root(uid) && probe.is_setuid_root(gid) {
                (true, "")
            } else {
                (
                    false,
                    "newuidmap and newgidmap must be owned by root with the setuid bit; reinstall them (sudo apt install --reinstall uidmap)",
                )
            }
        }
        _ => (
            false,
            "install them (Ubuntu 24.04: sudo apt install uidmap)",
        ),
    };
    finding(
        "uidmap",
        Level::Required,
        ok,
        "newuidmap and newgidmap are installed and setuid root",
        fix,
    )
}

fn max_user_namespaces(probe: &dyn Probe) -> u64 {
    probe
        .read_to_string(Path::new("/proc/sys/user/max_user_namespaces"))
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0)
}

/// A line `name:start:count` for this account, by user name or numeric uid, with a big enough count.
fn sub_ids_ok(probe: &dyn Probe, file: &Path, uid: u32, user: &str) -> bool {
    let Ok(text) = probe.read_to_string(file) else {
        return false;
    };
    let uid = uid.to_string();
    text.lines().any(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return false;
        }
        let mut parts = line.split(':');
        let (Some(name), Some(_start), Some(count)) = (parts.next(), parts.next(), parts.next())
        else {
            return false;
        };
        (name == user || name == uid) && count.trim().parse::<u64>().is_ok_and(|n| n >= MIN_SUB_IDS)
    })
}

fn landlock(probe: &dyn Probe) -> Finding {
    match probe.read_to_string(Path::new("/sys/kernel/security/lsm")) {
        Ok(text) => finding(
            "landlock",
            Level::Advice,
            text.split(|c: char| c == ',' || c.is_whitespace())
                .any(|word| word == "landlock"),
            "The kernel offers Landlock",
            "boot with lsm=...,landlock (add landlock to the kernel's lsm= list)",
        ),
        Err(_) => finding(
            "landlock",
            Level::Advice,
            true,
            "Landlock support is unknown (the kernel's security list is not readable)",
            "",
        ),
    }
}

/// A host with nothing on it.
struct Nothing;

impl Probe for Nothing {
    fn read_to_string(&self, _: &Path) -> io::Result<String> {
        Err(io::ErrorKind::NotFound.into())
    }
    fn exists(&self, _: &Path) -> bool {
        false
    }
    fn find_executable(&self, _: &str) -> Option<PathBuf> {
        None
    }
    fn account(&self) -> (u32, String) {
        (0, String::new())
    }
    fn can_open_read_write(&self, _: &Path) -> bool {
        false
    }
    fn env(&self, _: &str) -> Option<String> {
        None
    }
    fn is_setuid_root(&self, _: &Path) -> bool {
        false
    }
}

struct HostProbe;

impl Probe for HostProbe {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn find_executable(&self, name: &str) -> Option<PathBuf> {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .map(|directory| directory.join(name))
            .find(|candidate| {
                std::fs::metadata(candidate)
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            })
    }

    fn account(&self) -> (u32, String) {
        let uid = unsafe { libc::geteuid() };
        (uid, user_name(uid).unwrap_or_default())
    }

    fn can_open_read_write(&self, path: &Path) -> bool {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .is_ok()
    }

    fn env(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    fn is_setuid_root(&self, path: &Path) -> bool {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(path).is_ok_and(|m| m.uid() == 0 && m.mode() & 0o4000 != 0)
    }
}

fn user_name(uid: u32) -> Option<String> {
    let mut buffer = vec![0u8; 1024];
    loop {
        let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        let code = unsafe {
            libc::getpwuid_r(
                uid,
                &mut entry,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };
        if code == libc::ERANGE && buffer.len() < 1 << 20 {
            let larger = buffer.len() * 2;
            buffer.resize(larger, 0);
            continue;
        }
        if code != 0 || result.is_null() || entry.pw_name.is_null() {
            return None;
        }
        return Some(
            unsafe { std::ffi::CStr::from_ptr(entry.pw_name) }
                .to_string_lossy()
                .into_owned(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    struct Fake {
        files: HashMap<String, String>,
        existing: HashSet<String>,
        tools: HashSet<String>,
        setuid: bool,
        tun_open: bool,
        env: HashMap<String, String>,
        uid: u32,
        user: String,
    }

    impl Fake {
        fn good() -> Self {
            let mut fake = Self {
                files: HashMap::new(),
                existing: HashSet::new(),
                tools: HashSet::new(),
                setuid: true,
                tun_open: true,
                env: HashMap::new(),
                uid: 1000,
                user: "silo".into(),
            };
            for tool in [
                "crun",
                "skopeo",
                "umoci",
                "newuidmap",
                "newgidmap",
                "systemd-run",
            ] {
                fake.tools.insert(tool.into());
            }
            fake.existing.insert("/dev/net/tun".into());
            fake.existing
                .insert("/sys/fs/cgroup/cgroup.controllers".into());
            fake.files
                .insert("/etc/subuid".into(), "silo:100000:65536\n".into());
            fake.files
                .insert("/etc/subgid".into(), "silo:100000:65536\n".into());
            fake.files.insert(
                "/proc/sys/user/max_user_namespaces".into(),
                "15000\n".into(),
            );
            fake.files.insert(
                "/proc/sys/kernel/apparmor_restrict_unprivileged_userns".into(),
                "0\n".into(),
            );
            fake.files.insert(
                "/proc/filesystems".into(),
                "nodev\tsysfs\nnodev\toverlay\n\text4\n".into(),
            );
            fake.files.insert(
                "/sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/cgroup.controllers"
                    .into(),
                "cpuset cpu io memory pids\n".into(),
            );
            fake.files.insert(
                "/sys/kernel/security/lsm".into(),
                "lockdown,capability,landlock,yama,apparmor".into(),
            );
            fake.env
                .insert("XDG_RUNTIME_DIR".into(), "/run/user/1000".into());
            fake
        }

        fn without_file(mut self, path: &str) -> Self {
            self.files.remove(path);
            self
        }

        fn with_file(mut self, path: &str, text: &str) -> Self {
            self.files.insert(path.into(), text.into());
            self
        }
    }

    impl Probe for Fake {
        fn read_to_string(&self, path: &Path) -> io::Result<String> {
            self.files
                .get(path.to_str().unwrap())
                .cloned()
                .ok_or_else(|| io::ErrorKind::NotFound.into())
        }
        fn exists(&self, path: &Path) -> bool {
            self.existing.contains(path.to_str().unwrap())
        }
        fn find_executable(&self, name: &str) -> Option<PathBuf> {
            self.tools
                .contains(name)
                .then(|| PathBuf::from("/usr/bin").join(name))
        }
        fn account(&self) -> (u32, String) {
            (self.uid, self.user.clone())
        }
        fn can_open_read_write(&self, _: &Path) -> bool {
            self.tun_open
        }
        fn env(&self, name: &str) -> Option<String> {
            self.env.get(name).cloned()
        }
        fn is_setuid_root(&self, _: &Path) -> bool {
            self.setuid
        }
    }

    fn failing(report: &Report) -> Vec<&'static str> {
        report.problems().map(|finding| finding.id).collect()
    }

    fn linux(fake: &Fake) -> Report {
        check_on(true, fake)
    }

    #[test]
    fn a_good_host_is_ready() {
        let report = linux(&Fake::good());
        assert!(report.ready());
        assert!(failing(&report).is_empty(), "{:?}", failing(&report));
        assert_eq!(report.refusal(), None);
    }

    #[test]
    fn each_missing_tool_is_its_own_finding() {
        for tool in ["crun", "skopeo", "umoci", "systemd-run"] {
            let mut fake = Fake::good();
            fake.tools.remove(tool);
            assert_eq!(failing(&linux(&fake)), vec![tool], "{tool}");
        }
        for tool in ["newuidmap", "newgidmap"] {
            let mut fake = Fake::good();
            fake.tools.remove(tool);
            assert_eq!(failing(&linux(&fake)), vec!["uidmap"], "{tool}");
        }
    }

    #[test]
    fn uidmap_must_be_setuid_root() {
        let mut fake = Fake::good();
        fake.setuid = false;
        let report = linux(&fake);
        assert_eq!(failing(&report), vec!["uidmap"]);
        assert!(report.refusal().unwrap().contains("reinstall"));
    }

    #[test]
    fn systemd_run_needs_a_runtime_directory() {
        let mut fake = Fake::good();
        fake.env.clear();
        assert_eq!(failing(&linux(&fake)), vec!["systemd-run"]);
    }

    #[test]
    fn subordinate_ids_parse_by_name_and_uid() {
        let by_uid = Fake::good().with_file("/etc/subuid", "1000:100000:65536\n");
        assert!(linux(&by_uid).ready());
        let by_name =
            Fake::good().with_file("/etc/subgid", "# c\n\nother:1:65536\nsilo:200000:131072\n");
        assert!(linux(&by_name).ready());
    }

    #[test]
    fn subordinate_ids_fail_when_absent_small_or_commented() {
        let small = Fake::good().with_file("/etc/subuid", "silo:100000:1000\n");
        assert_eq!(failing(&linux(&small)), vec!["subuid"]);
        let other = Fake::good().with_file("/etc/subgid", "someone:100000:65536\n");
        assert_eq!(failing(&linux(&other)), vec!["subgid"]);
        let commented = Fake::good().with_file("/etc/subuid", "# silo:100000:65536\n\n");
        assert_eq!(failing(&linux(&commented)), vec!["subuid"]);
        let junk = Fake::good().with_file("/etc/subuid", "silo:100000:lots\nsilo\n");
        assert_eq!(failing(&linux(&junk)), vec!["subuid"]);
        let prefix = Fake::good().with_file("/etc/subuid", "silo2:100000:65536\n");
        assert_eq!(failing(&linux(&prefix)), vec!["subuid"]);
        let missing = Fake::good().without_file("/etc/subgid");
        assert_eq!(failing(&linux(&missing)), vec!["subgid"]);
    }

    #[test]
    fn tun_needs_the_device_and_access() {
        let mut gone = Fake::good();
        gone.existing.remove("/dev/net/tun");
        assert_eq!(failing(&linux(&gone)), vec!["tun"]);
        let mut closed = Fake::good();
        closed.tun_open = false;
        assert_eq!(failing(&linux(&closed)), vec!["tun"]);
    }

    #[test]
    fn user_namespaces_and_apparmor() {
        let zero = Fake::good().with_file("/proc/sys/user/max_user_namespaces", "0\n");
        assert_eq!(failing(&linux(&zero)), vec!["userns"]);
        let absent = Fake::good().without_file("/proc/sys/user/max_user_namespaces");
        assert_eq!(failing(&linux(&absent)), vec!["userns"]);
        let restricted = Fake::good().with_file(
            "/proc/sys/kernel/apparmor_restrict_unprivileged_userns",
            "1\n",
        );
        let report = linux(&restricted);
        assert_eq!(failing(&report), vec!["apparmor-userns"]);
        let message = report.refusal().unwrap();
        assert!(message.contains("sysctl -w kernel.apparmor_restrict_unprivileged_userns=0"));
        assert!(message.contains("/etc/sysctl.d"));
        assert!(message.contains("host-wide"));
        let no_apparmor =
            Fake::good().without_file("/proc/sys/kernel/apparmor_restrict_unprivileged_userns");
        assert!(linux(&no_apparmor).ready());
    }

    #[test]
    fn overlay_and_cgroup_v2() {
        let none = Fake::good().with_file("/proc/filesystems", "nodev\tsysfs\n\text4\n");
        assert_eq!(failing(&linux(&none)), vec!["overlayfs"]);
        let mut v1 = Fake::good();
        v1.existing.remove("/sys/fs/cgroup/cgroup.controllers");
        assert_eq!(failing(&linux(&v1)), vec!["cgroup-v2"]);
    }

    #[test]
    fn delegation_needs_memory_and_pids_and_advises_cpu() {
        let path = "/sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/cgroup.controllers";
        let no_pids = Fake::good().with_file(path, "cpu memory\n");
        assert_eq!(failing(&linux(&no_pids)), vec!["cgroup-delegation"]);
        let no_manager = Fake::good().without_file(path);
        let report = linux(&no_manager);
        assert_eq!(failing(&report), vec!["cgroup-delegation", "cpu-limit"]);
        assert!(report.refusal().unwrap().contains("loginctl enable-linger"));
        let no_cpu = Fake::good().with_file(path, "memory pids\n");
        let report = linux(&no_cpu);
        assert_eq!(failing(&report), vec!["cpu-limit"]);
        assert!(report.ready());
        assert_eq!(report.refusal(), None);
        assert!(report
            .findings
            .iter()
            .any(|f| f.what == "CPU limits are not enforced"));
    }

    #[test]
    fn landlock_is_advice_and_unknown_when_unreadable() {
        let off = Fake::good().with_file("/sys/kernel/security/lsm", "capability,yama,apparmor");
        let report = linux(&off);
        assert_eq!(failing(&report), vec!["landlock"]);
        assert!(report.ready());
        let unknown = Fake::good().without_file("/sys/kernel/security/lsm");
        let report = linux(&unknown);
        assert!(failing(&report).is_empty());
        assert!(report.findings.last().unwrap().what.contains("unknown"));
    }

    #[test]
    fn refusal_names_every_unmet_required_item_and_omits_advice() {
        let mut fake = Fake::good().with_file("/sys/kernel/security/lsm", "capability");
        fake.tools.remove("crun");
        fake.tools.remove("umoci");
        let report = linux(&fake);
        assert!(!report.ready());
        let message = report.refusal().unwrap();
        assert!(message.starts_with("This device cannot run container computers yet: "));
        assert!(message
            .contains("crun is installed — install it (Ubuntu 24.04: sudo apt install crun)"));
        assert!(message.contains("umoci is installed — "));
        assert!(!message.contains("skopeo"));
        assert!(!message.contains("Landlock"));
        assert_eq!(message.matches("; ").count(), 1, "{message}");
    }

    #[test]
    fn a_host_that_is_not_linux_gives_one_reason() {
        let report = check_on(false, &Fake::good());
        assert!(!report.ready());
        assert!(report.findings.iter().all(|f| !f.ok && f.fix == NOT_LINUX));
        assert_eq!(
            report.refusal().unwrap(),
            "Container computers run only on a Linux host."
        );
    }
}
