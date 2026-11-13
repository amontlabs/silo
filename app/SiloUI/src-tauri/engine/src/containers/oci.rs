//! The OCI runtime configuration of a container computer: the hardening baseline.
//!
//! crun runs inside the holder's user namespace (see `silo-ctr run`), where id 0 is the account
//! that owns the computer and ids 1..65535 are its sub-id range. The container's own user
//! namespace maps its ids onto those so that the working account is the engine's own user
//! ([`super::idmap`]) and root is a sub-id. The configuration omits the network namespace: the container joins the holder's, which holds only a TAP device that the
//! computer's gateway serves.

use oci_spec::runtime::Spec;
use serde_json::{json, Value};
use std::path::Path;

/// Where the shim binary sits inside the container.
pub const INIT_PATH: &str = "/silo/init";

/// The system trust bundle (Debian and Ubuntu). It stays the guest's own writable file:
/// `update-ca-certificates` rebuilds it, with the gateway CA below included.
pub const CA_BUNDLE_PATH: &str = "/etc/ssl/certs/ca-certificates.crt";

/// Where the computer's gateway CA certificate is bound (read-only), in the directory
/// `update-ca-certificates` reads local certificates from. Silo runs it at start, so the guest
/// accepts the certificates the gateway issues toward hosts a secret is allowed for, and a guest
/// can add its own CA beside this one.
pub const GATEWAY_CA_PATH: &str = "/usr/local/share/ca-certificates/silo-gateway.crt";

/// Where the computer's per-computer share (the in-guest client's bridge folder) is bound.
pub const SHARE_PATH: &str = "/.msb";

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
/// Calls the seccomp deny-list refuses with EPERM. `setns` is refused outright: nothing in a
/// container joins another namespace (`crun exec` joins from outside, before the filter applies).
const DENIED_CALLS: [&str; 10] = [
    "setns",
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
    /// The computer's gateway CA certificate (PEM), bound at [`GATEWAY_CA_PATH`].
    pub ca_cert: &'a Path,
    /// The host folder bound read-write at [`SHARE_PATH`].
    pub share: &'a Path,
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

/// The OCI process `crun exec --process` runs: the same as the `--user`, `--env`, `--cwd`,
/// `--cap` and `--no-new-privs` flags give, plus the user's supplementary groups, which the flags
/// cannot carry. `argv` is the command with its wrapper already in front.
pub fn exec_process(
    uid: u32,
    gid: u32,
    additional_gids: &[u32],
    cwd: &str,
    env: &[(String, String)],
    argv: &[String],
    terminal: bool,
    capabilities: &[&str],
) -> Value {
    let mut process = json!({
        "terminal": terminal,
        "user": {"uid": uid, "gid": gid, "additionalGids": additional_gids},
        "args": argv,
        "env": env.iter().map(|(key, value)| format!("{key}={value}")).collect::<Vec<_>>(),
        "cwd": cwd,
        "noNewPrivileges": true,
    });
    if !capabilities.is_empty() {
        process["capabilities"] = json!({
            "effective": capabilities,
            "bounding": capabilities,
            "inheritable": capabilities,
            "ambient": capabilities,
            "permitted": capabilities,
        });
    }
    process
}

/// The container's `uidMappings` and `gidMappings`: the keep-id layout of [`super::idmap`].
fn id_mappings() -> Vec<Value> {
    super::idmap::extents()
        .iter()
        .map(|e| json!({"containerID": e.container, "hostID": e.holder, "size": e.size}))
        .collect()
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
            // As a virtual machine's /run is: memory, empty at every start, and not part of the
            // computer's files, so what the desktop keeps there (the display's authority cookie,
            // the session's environment) is not carried into a checkpoint or a fork.
            {"destination": "/run", "type": "tmpfs", "source": "tmpfs",
             "options": ["nosuid", "nodev", "mode=755", "size=268435456"]},
            {"destination": "/etc/resolv.conf", "type": "bind", "source": params.resolv_conf,
             "options": ["rbind", "ro", "nosuid", "nodev", "noexec"]},
            {"destination": "/etc/hosts", "type": "bind", "source": params.hosts,
             "options": ["rbind", "ro", "nosuid", "nodev", "noexec"]},
            {"destination": GATEWAY_CA_PATH, "type": "bind", "source": params.ca_cert,
             "options": ["rbind", "ro", "nosuid", "nodev", "noexec"]},
            {"destination": SHARE_PATH, "type": "bind", "source": params.share,
             "options": ["rbind", "rw", "nosuid", "nodev", "noexec"]},
        ],
        "linux": {
            "namespaces": namespaces,
            "uidMappings": id_mappings(),
            "gidMappings": id_mappings(),
            "cgroupsPath": params.cgroup_path,
            "seccomp": {
                "defaultAction": "SCMP_ACT_ALLOW",
                "architectures": architectures(params.arch),
                "syscalls": [
                    {"names": DENIED_CALLS, "action": "SCMP_ACT_ERRNO", "errnoRet": 1},
                    new_user_rule(&["clone", "unshare"], 0),
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
            ca_cert: Path::new("/r/state/gateway-ca.crt"),
            share: Path::new("/r/runtime"),
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
    fn setns_is_denied_outright_and_unshare_only_for_user_namespaces() {
        let spec = spec();
        let seccomp = spec.linux().as_ref().unwrap().seccomp().as_ref().unwrap();
        let rules = serde_json::to_value(seccomp).unwrap()["syscalls"].clone();
        let rule = |call: &str| {
            rules
                .as_array()
                .unwrap()
                .iter()
                .find(|rule| rule["names"].as_array().unwrap().iter().any(|n| n == call))
                .unwrap()
                .clone()
        };
        let setns = rule("setns");
        assert_eq!(setns["action"], "SCMP_ACT_ERRNO");
        assert!(setns
            .get("args")
            .is_none_or(|args| args.as_array().unwrap().is_empty()));
        assert!(!rule("unshare")["args"].as_array().unwrap().is_empty());
    }

    #[test]
    fn an_exec_process_carries_the_supplementary_groups_and_the_flags_meaning() {
        let process = exec_process(
            1001,
            1001,
            &[27, 44],
            "/workspace",
            &[("A".into(), "b=c".into())],
            &["/silo/init".into(), "--".into(), "id".into()],
            false,
            &[],
        );
        assert_eq!(process["user"]["additionalGids"], json!([27, 44]));
        assert_eq!(process["user"]["uid"], 1001);
        assert_eq!(process["env"], json!(["A=b=c"]));
        assert_eq!(process["noNewPrivileges"], true);
        assert_eq!(process["terminal"], false);
        assert!(process.get("capabilities").is_none());
        let root = exec_process(
            0,
            0,
            &[],
            "/",
            &[],
            &["x".into()],
            false,
            &ROOT_EXEC_CAPABILITIES,
        );
        let terminal = exec_process(1001, 1001, &[], "/", &[], &["x".into()], true, &[]);
        assert_eq!(terminal["terminal"], true);
        assert_eq!(
            root["capabilities"]["effective"].as_array().unwrap().len(),
            7
        );
        // The kernel only raises an ambient capability that is also permitted and inheritable.
        for set in ["ambient", "inheritable", "permitted", "bounding"] {
            assert_eq!(
                root["capabilities"][set], root["capabilities"]["effective"],
                "{set}"
            );
        }
        // The shape is a valid OCI process.
        let _: oci_spec::runtime::Process = serde_json::from_value(root).unwrap();
        let _: oci_spec::runtime::Process = serde_json::from_value(process).unwrap();
    }

    #[test]
    fn the_working_account_is_the_engines_user_and_root_is_a_sub_id() {
        let value = serde_json::to_value(spec()).unwrap();
        let linux = &value["linux"];
        for key in ["uidMappings", "gidMappings"] {
            let maps = linux[key].as_array().unwrap();
            let find = |container: u64| {
                maps.iter()
                    .find(|m| m["containerID"] == container)
                    .unwrap()
                    .clone()
            };
            // Container 0 (root) is holder id 1, never the holder's own root (the engine's user).
            assert_eq!(find(0)["hostID"], 1, "{key}");
            // The working account is the holder's root, the engine's own user.
            assert_eq!(
                (find(1001)["hostID"].as_u64(), find(1001)["size"].as_u64()),
                (Some(0), Some(1))
            );
            assert_eq!(
                maps.iter()
                    .map(|m| m["size"].as_u64().unwrap())
                    .sum::<u64>(),
                65536
            );
        }
    }

    #[test]
    fn the_computers_share_is_bound_read_write_without_exec_or_devices() {
        let value = serde_json::to_value(spec()).unwrap();
        let mount = value["mounts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["destination"] == SHARE_PATH)
            .unwrap();
        assert_eq!(mount["source"], "/r/runtime");
        let options: Vec<&str> = mount["options"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o.as_str().unwrap())
            .collect();
        for wanted in ["rw", "nosuid", "nodev", "noexec"] {
            assert!(options.contains(&wanted), "{wanted}");
        }
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
    fn the_ids_map_as_three_extents_of_the_holders_namespace() {
        let spec = spec();
        let linux = spec.linux().as_ref().unwrap();
        let sizes: Vec<_> = linux
            .uid_mappings()
            .as_ref()
            .unwrap()
            .iter()
            .map(|m| (m.container_id(), m.host_id(), m.size()))
            .collect();
        assert_eq!(sizes, [(0, 1, 1001), (1001, 0, 1), (1002, 1002, 64534)]);
        assert_eq!(linux.gid_mappings().as_ref().unwrap().len(), 3);
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
    fn proc_and_sys_are_masked_or_read_only_and_only_three_files_and_the_share_are_bound_from_the_host(
    ) {
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
        let run = mounts
            .iter()
            .find(|m| m.destination() == &PathBuf::from("/run"))
            .unwrap();
        assert_eq!(run.typ().as_deref(), Some("tmpfs"));
        let options = run.options().as_ref().unwrap();
        assert!(options.contains(&"nosuid".to_owned()) && options.contains(&"nodev".to_owned()));
        let bound: Vec<_> = mounts
            .iter()
            .filter(|m| m.typ().as_deref() == Some("bind"))
            .map(|m| m.destination().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            bound,
            [
                "/etc/resolv.conf",
                "/etc/hosts",
                "/usr/local/share/ca-certificates/silo-gateway.crt",
                "/.msb"
            ]
        );
        // The system bundle stays the guest's own writable file: `update-ca-certificates` replaces
        // it by renaming over it, which fails on a mount point.
        assert!(!bound.contains(&CA_BUNDLE_PATH.to_owned()));
        let gateway_ca = mounts
            .iter()
            .find(|m| m.destination() == &PathBuf::from(GATEWAY_CA_PATH))
            .unwrap();
        let options = gateway_ca.options().as_ref().unwrap();
        assert!(options.contains(&"ro".to_owned()), "{options:?}");
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
