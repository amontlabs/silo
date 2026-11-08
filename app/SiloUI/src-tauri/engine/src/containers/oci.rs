//! The OCI runtime configuration of a container computer: the hardening baseline.
//!
//! crun runs inside the holder's user namespace (see `silo-ctr run`), where container id 0 is
//! the account that owns the computer and ids 1..65535 are its sub-id range, so the container's
//! own user namespace maps ids to that range as two extents. The configuration omits the
//! network namespace: the container joins the holder's, which holds only a TAP device that the
//! computer's gateway serves.

use oci_spec::runtime::Spec;
use serde_json::{json, Value};
use std::path::Path;

/// Where the shim binary sits inside the container.
pub const INIT_PATH: &str = "/silo/init";

/// What Silo's own root commands (account setup, installs) may use inside the container's user
/// namespace; user processes and the container's first process have none.
pub const ROOT_EXEC_CAPABILITIES: [&str; 7] = [
    "CAP_CHOWN",
    "CAP_DAC_OVERRIDE",
    "CAP_FOWNER",
    "CAP_FSETID",
    "CAP_KILL",
    "CAP_SETGID",
    "CAP_SETUID",
];

const CLONE_NEWUSER: u64 = 0x1000_0000;
/// Calls the seccomp deny-list refuses with EPERM.
const DENIED_CALLS: [&str; 9] = [
    "io_uring_setup",
    "io_uring_enter",
    "io_uring_register",
    "keyctl",
    "add_key",
    "request_key",
    "bpf",
    "perf_event_open",
    "userfaultfd",
];

pub struct Params<'a> {
    pub name: &'a str,
    /// The mounted overlay root.
    pub rootfs: &'a Path,
    /// The container's cgroup, absolute below the cgroup root.
    pub cgroup_path: &'a str,
    pub resolv_conf: &'a Path,
    pub hosts: &'a Path,
    /// `std::env::consts::ARCH`.
    pub arch: &'a str,
}

fn architectures(arch: &str) -> Vec<&'static str> {
    match arch {
        "aarch64" => vec!["SCMP_ARCH_AARCH64", "SCMP_ARCH_ARM"],
        _ => vec!["SCMP_ARCH_X86_64", "SCMP_ARCH_X86", "SCMP_ARCH_X32"],
    }
}

fn new_user_rule(names: &[&str], index: u32) -> Value {
    json!({
        "names": names,
        "action": "SCMP_ACT_ERRNO",
        "errnoRet": 1,
        "args": [{"index": index, "value": CLONE_NEWUSER, "valueTwo": CLONE_NEWUSER, "op": "SCMP_CMP_MASKED_EQ"}],
    })
}

pub fn build(params: &Params<'_>) -> Result<Spec, String> {
    let empty: [&str; 0] = [];
    let namespaces: Vec<Value> = ["pid", "ipc", "uts", "mount", "user", "cgroup"]
        .iter()
        .map(|kind| json!({"type": kind}))
        .collect();
    let value = json!({
        "ociVersion": "1.0.2",
        "process": {
            "terminal": false,
            "user": {"uid": 0, "gid": 0},
            "args": [INIT_PATH, "--", "/usr/bin/sleep", "infinity"],
            "env": [
                "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
                "HOME=/root",
                "TERM=xterm",
                "LANG=C.UTF-8",
                "SILO_RUNTIME=container",
            ],
            "cwd": "/",
            "capabilities": {
                "bounding": empty, "effective": empty, "inheritable": empty,
                "permitted": empty, "ambient": empty,
            },
            "noNewPrivileges": true,
        },
        "root": {"path": params.rootfs, "readonly": false},
        "hostname": params.name,
        "mounts": [
            {"destination": "/proc", "type": "proc", "source": "proc", "options": ["nosuid", "noexec", "nodev"]},
            {"destination": "/dev", "type": "tmpfs", "source": "tmpfs",
             "options": ["nosuid", "strictatime", "mode=755", "size=65536k"]},
            {"destination": "/dev/pts", "type": "devpts", "source": "devpts",
             "options": ["nosuid", "noexec", "newinstance", "ptmxmode=0666", "mode=0620", "gid=5"]},
            {"destination": "/dev/shm", "type": "tmpfs", "source": "shm",
             "options": ["nosuid", "noexec", "nodev", "mode=1777", "size=1073741824"]},
            {"destination": "/dev/mqueue", "type": "mqueue", "source": "mqueue",
             "options": ["nosuid", "noexec", "nodev"]},
            {"destination": "/sys", "type": "sysfs", "source": "sysfs",
             "options": ["nosuid", "noexec", "nodev", "ro"]},
            {"destination": "/sys/fs/cgroup", "type": "cgroup", "source": "cgroup",
             "options": ["nosuid", "noexec", "nodev", "relatime", "ro"]},
            {"destination": "/etc/resolv.conf", "type": "bind", "source": params.resolv_conf,
             "options": ["rbind", "ro", "nosuid", "nodev", "noexec"]},
            {"destination": "/etc/hosts", "type": "bind", "source": params.hosts,
             "options": ["rbind", "ro", "nosuid", "nodev", "noexec"]},
        ],
        "linux": {
            "namespaces": namespaces,
            "uidMappings": [
                {"containerID": 0, "hostID": 0, "size": 1},
                {"containerID": 1, "hostID": 1, "size": 65535},
            ],
            "gidMappings": [
                {"containerID": 0, "hostID": 0, "size": 1},
                {"containerID": 1, "hostID": 1, "size": 65535},
            ],
            "cgroupsPath": params.cgroup_path,
            "seccomp": {
                "defaultAction": "SCMP_ACT_ALLOW",
                "architectures": architectures(params.arch),
                "syscalls": [
                    {"names": DENIED_CALLS, "action": "SCMP_ACT_ERRNO", "errnoRet": 1},
                    new_user_rule(&["clone", "unshare"], 0),
                    new_user_rule(&["setns"], 1),
                    // clone3 keeps its flags in a struct seccomp cannot read; ENOSYS makes libc use clone.
                    {"names": ["clone3"], "action": "SCMP_ACT_ERRNO", "errnoRet": 38},
                ],
            },
            "maskedPaths": [
                "/proc/acpi", "/proc/asound", "/proc/kcore", "/proc/keys", "/proc/latency_stats",
                "/proc/timer_list", "/proc/timer_stats", "/proc/sched_debug", "/proc/scsi",
                "/sys/firmware", "/sys/devices/virtual/powercap",
            ],
            "readonlyPaths": ["/proc/bus", "/proc/fs", "/proc/irq", "/proc/sys", "/proc/sysrq-trigger"],
        },
    });
    serde_json::from_value(value)
        .map_err(|error| format!("The container configuration is invalid: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use oci_spec::runtime::LinuxNamespaceType;
    use std::path::PathBuf;

    fn spec() -> Spec {
        build(&Params {
            name: "dev",
            rootfs: Path::new("/r/merged"),
            cgroup_path: "/user.slice/x.scope/ctr",
            resolv_conf: Path::new("/r/state/resolv.conf"),
            hosts: Path::new("/r/state/hosts"),
            arch: "x86_64",
        })
        .unwrap()
    }

    #[test]
    fn the_first_process_has_no_privilege_and_no_capability() {
        let spec = spec();
        let process = spec.process().as_ref().unwrap();
        assert_eq!(process.no_new_privileges(), Some(true));
        let caps = process.capabilities().as_ref().unwrap();
        for set in [
            caps.bounding(),
            caps.effective(),
            caps.permitted(),
            caps.inheritable(),
            caps.ambient(),
        ] {
            assert!(set.as_ref().is_none_or(|set| set.is_empty()));
        }
        assert_eq!(process.args().as_ref().unwrap()[0], INIT_PATH);
        assert_eq!(process.user().uid(), 0);
    }

    #[test]
    fn the_container_has_its_own_namespaces_except_the_network() {
        let spec = spec();
        let kinds: Vec<_> = spec
            .linux()
            .as_ref()
            .unwrap()
            .namespaces()
            .as_ref()
            .unwrap()
            .iter()
            .map(|namespace| namespace.typ())
            .collect();
        for wanted in [
            LinuxNamespaceType::Pid,
            LinuxNamespaceType::Ipc,
            LinuxNamespaceType::Uts,
            LinuxNamespaceType::Mount,
            LinuxNamespaceType::User,
            LinuxNamespaceType::Cgroup,
        ] {
            assert!(kinds.contains(&wanted), "{wanted:?}");
        }
        assert!(!kinds.contains(&LinuxNamespaceType::Network));
    }

    #[test]
    fn the_ids_map_as_two_extents_of_the_holders_namespace() {
        let spec = spec();
        let linux = spec.linux().as_ref().unwrap();
        let sizes: Vec<_> = linux
            .uid_mappings()
            .as_ref()
            .unwrap()
            .iter()
            .map(|m| (m.container_id(), m.host_id(), m.size()))
            .collect();
        assert_eq!(sizes, [(0, 0, 1), (1, 1, 65535)]);
        assert_eq!(linux.gid_mappings().as_ref().unwrap().len(), 2);
    }

    #[test]
    fn the_seccomp_deny_list_names_the_dangerous_calls() {
        let spec = spec();
        let seccomp = spec.linux().as_ref().unwrap().seccomp().as_ref().unwrap();
        let text = serde_json::to_string(seccomp).unwrap();
        for call in DENIED_CALLS {
            assert!(text.contains(call), "{call}");
        }
        for call in ["clone", "unshare", "setns", "clone3"] {
            assert!(text.contains(call), "{call}");
        }
        assert!(text.contains("SCMP_ARCH_X86_64"));
        assert!(!text.contains("AARCH64"));
    }

    #[test]
    fn arm_hosts_filter_their_own_architectures() {
        let mut value = serde_json::to_value(spec()).unwrap();
        value["linux"]["seccomp"]["architectures"] = json!(architectures("aarch64"));
        assert!(value.to_string().contains("SCMP_ARCH_AARCH64"));
    }

    #[test]
    fn proc_and_sys_are_masked_or_read_only_and_only_two_files_are_bound_from_the_host() {
        let spec = spec();
        let linux = spec.linux().as_ref().unwrap();
        assert!(linux
            .masked_paths()
            .as_ref()
            .unwrap()
            .contains(&"/proc/kcore".to_owned()));
        assert!(linux
            .readonly_paths()
            .as_ref()
            .unwrap()
            .contains(&"/proc/sys".to_owned()));
        let mounts = spec.mounts().as_ref().unwrap();
        let sys = mounts
            .iter()
            .find(|m| m.destination() == &PathBuf::from("/sys"))
            .unwrap();
        assert!(sys.options().as_ref().unwrap().contains(&"ro".to_owned()));
        let bound: Vec<_> = mounts
            .iter()
            .filter(|m| m.typ().as_deref() == Some("bind"))
            .map(|m| m.destination().to_string_lossy().into_owned())
            .collect();
        assert_eq!(bound, ["/etc/resolv.conf", "/etc/hosts"]);
        let text = serde_json::to_string(&spec).unwrap();
        assert!(
            !text.contains("docker.sock")
                && !text.contains("podman.sock")
                && !text.contains("silod")
        );
    }

    #[test]
    fn root_commands_get_seven_capabilities_and_no_more() {
        assert_eq!(ROOT_EXEC_CAPABILITIES.len(), 7);
        assert!(!ROOT_EXEC_CAPABILITIES.contains(&"CAP_SYS_ADMIN"));
        assert!(!ROOT_EXEC_CAPABILITIES.contains(&"CAP_NET_RAW"));
    }

    #[test]
    fn the_cgroup_and_hostname_are_set() {
        let spec = spec();
        assert_eq!(spec.hostname().as_deref(), Some("dev"));
        assert_eq!(
            spec.linux()
                .as_ref()
                .unwrap()
                .cgroups_path()
                .as_ref()
                .unwrap(),
            &PathBuf::from("/user.slice/x.scope/ctr")
        );
    }
}
