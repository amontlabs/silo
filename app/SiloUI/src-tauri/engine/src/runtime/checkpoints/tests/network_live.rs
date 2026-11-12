//! A live check of the network policy against the real runtime: what a guest can reach in each
//! mode, a stopped computer changed from one mode to another by the engine's own start, and a
//! restore and a fork of an allowlist computer. Needs the packaged runtime, a signed `msb`
//! (`SILO_TEST_MSB`, `SILO_TEST_LIBKRUNFW`), the guest image folder (`SILO_TEST_GUEST_IMAGE`)
//! and `SILO_LIVE_TEST_CONFIRM`. `SILO_T8_CONFIGS` names a folder that receives the runtime's
//! configuration of each computer, for the export check in `backup`.
use super::*;
use crate::network_policy::{store, use_test_grants, Grants};
use crate::network_policy::{NetworkPolicy, NetworkPolicyMode};
use crate::test_support::computer_use_live::Fixture;

const PROBE: &str = r#"
code() { curl -s -o /dev/null -m 12 -k -w '%{http_code}' "$1" 2>/dev/null || true; }
echo "allowed host, HTTPS (example.com): $(code https://example.com)"
echo "other host, HTTPS (example.org): $(code https://example.org)"
echo "secret-covered host, HTTPS (api.github.com): $(code https://api.github.com/zen)"
echo "name example.org through the gateway: $(getent hosts example.org | head -1 | cut -d' ' -f1)"
python3 - <<'PY'
import socket, struct
def ask(server, name):
    try:
        header = b'\x12\x34\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00'
        question = b''.join(bytes([len(p)]) + p.encode() for p in name.split('.')) + b'\x00\x00\x01\x00\x01'
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.settimeout(4)
        s.sendto(header + question, (server, 53))
        data = s.recv(512)
        return "rcode=%d answers=%d" % (data[3] & 15, struct.unpack('>H', data[6:8])[0])
    except Exception as e:
        return "no answer (%s)" % type(e).__name__
def connect(address, port):
    try:
        socket.create_connection((address, port), timeout=6).close()
        return "connected"
    except Exception as e:
        return "failed (%s)" % type(e).__name__
print("direct public IP, TCP connect (1.1.1.1:443):", connect("1.1.1.1", 443))
print("another resolver (1.1.1.1) for example.org:", ask("1.1.1.1", "example.org"))
def exchange(address, port, payload):
    try:
        c = socket.create_connection((address, port), timeout=6)
    except Exception as e:
        return "failed (%s)" % type(e).__name__
    try:
        c.settimeout(5)
        c.sendall(payload)
        data = c.recv(512)
        if not data:
            return "connected, closed with no data"
        if port == 53 and len(data) >= 14:
            return "connected, DNS reply rcode=%d answers=%d" % (data[5] & 15, struct.unpack('>H', data[8:10])[0])
        return "connected, %d bytes came back" % len(data)
    except Exception as e:
        return "connected, nothing came back (%s)" % type(e).__name__
    finally:
        c.close()
query = b'\x12\x34\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00\x07example\x03org\x00\x00\x01\x00\x01'
print("unlisted IP, TCP connect (1.1.1.1:853):", exchange("1.1.1.1", 853, b'\x16\x03\x01\x00\x01\x00'))
print("unlisted IP, TCP connect (8.8.8.8:53):", exchange("8.8.8.8", 53, struct.pack('>H', len(query)) + query))
PY
echo "unlisted IP, HTTP status (1.1.1.1): $(curl -s -o /dev/null -m 12 -w '%{http_code}' http://1.1.1.1/ 2>/dev/null || true)"
echo "unlisted IP, HTTP body (1.1.1.1): $(curl -s -m 12 http://1.1.1.1/ 2>/dev/null | head -c 300 | tr '\n' ' ' || true)"
"#;

#[derive(Debug)]
struct Reach {
    example_com: String,
    example_org: String,
    public_ip: String,
    github: String,
    gateway_name: String,
    resolver: String,
    http_status: String,
    http_body: String,
    port_853: String,
    port_53: String,
}

fn field(report: &str, label: &str) -> String {
    report
        .lines()
        .find_map(|line| line.strip_prefix(label))
        .unwrap_or_else(|| panic!("no {label:?} in {report}"))
        .trim()
        .trim_start_matches(':')
        .trim()
        .to_owned()
}

fn probe(fixture: &Fixture, name: &str, label: &str) -> Reach {
    let report = fixture.exec(name, "root", PROBE).unwrap();
    eprintln!("\nPROBE {label} ({name}):\n{report}");
    Reach {
        example_com: field(&report, "allowed host, HTTPS (example.com)"),
        example_org: field(&report, "other host, HTTPS (example.org)"),
        public_ip: field(&report, "direct public IP, TCP connect (1.1.1.1:443)"),
        github: field(&report, "secret-covered host, HTTPS (api.github.com)"),
        gateway_name: field(&report, "name example.org through the gateway"),
        resolver: field(&report, "another resolver (1.1.1.1) for example.org"),
        http_status: field(&report, "unlisted IP, HTTP status (1.1.1.1)"),
        http_body: field(&report, "unlisted IP, HTTP body (1.1.1.1)"),
        port_853: field(&report, "unlisted IP, TCP connect (1.1.1.1:853)"),
        port_53: field(&report, "unlisted IP, TCP connect (8.8.8.8:53)"),
    }
}

fn reached(result: &str) -> bool {
    result != "000" && !result.is_empty() && !result.starts_with("failed")
}

/// A connection to an unlisted public IP is refused at once on every port (HTTP, HTTPS, DNS,
/// DNS-over-TLS): the program gets a reset, never a handshake, a gateway answer or a hang, and
/// nothing is sent to the address.
fn unlisted_ip_is_refused(reach: &Reach, label: &str) {
    const REFUSED: &str = "failed (ConnectionRefusedError)";
    assert!(
        reach.public_ip == REFUSED
            && reach.port_853 == REFUSED
            && reach.port_53 == REFUSED
            && reach.http_status == "000"
            && reach.http_body.trim().is_empty(),
        "{label}: a connection to an unlisted IP must be refused on every port: {reach:?}"
    );
}

fn answered(resolver: &str) -> bool {
    resolver.contains("rcode=0") && !resolver.contains("answers=0")
}

fn allowlist() -> NetworkPolicy {
    NetworkPolicy {
        mode: NetworkPolicyMode::Allowlist,
        hosts: vec!["example.com".into()],
        presets: Vec::new(),
    }
}

fn of_mode(mode: NetworkPolicyMode) -> NetworkPolicy {
    NetworkPolicy {
        mode,
        ..NetworkPolicy::default()
    }
}

fn keep_config(fixture: &Fixture, name: &str, label: &str) {
    let Some(directory) = std::env::var_os("SILO_T8_CONFIGS") else {
        return;
    };
    let inspected = inspect_computer(&ProcessRunner, &fixture.paths, name).unwrap();
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        std::path::Path::new(&directory).join(format!("{label}.json")),
        serde_json::to_vec_pretty(&inspected.config).unwrap(),
    )
    .unwrap();
}

fn create(fixture: &mut Fixture, name: &str, policy: &NetworkPolicy) -> ComputerConfiguration {
    fixture.track(name);
    let configuration = ComputerConfiguration {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.into(),
        cpus: 1,
        max_cpus: 1,
        memory_gib: 1,
        max_memory_gib: 1,
        workspace_storage_gib: 1,
        runtime_storage_gib: 2,
        runtime: None,
        desktop: None,
    };
    store::set(configuration.id(), policy).unwrap();
    let mut request = super::super::super::read_metadata(&fixture.paths.metadata).unwrap();
    request.computers.push(configuration.clone());
    super::super::super::apply_whole_configuration(
        &ProcessRunner,
        &fixture.paths,
        &super::super::super::device_resources().unwrap(),
        request,
    )
    .unwrap();
    configuration
}

fn stopped_changed(fixture: &Fixture, name: &str) -> bool {
    let configuration = fixture.computer_configuration(name);
    network_policy_changed(&ProcessRunner, &fixture.paths, &configuration).unwrap()
}

#[test]
#[ignore = "requires the packaged runtime, the guest image, network access and hardware virtualization"]
fn live_network_policy_modes_restore_and_fork() {
    let _test_state = crate::test_support::global_state();
    let mut fixture = Fixture::new("t8-", None);
    let directory = fixture.paths.metadata.parent().unwrap().to_owned();
    store::use_test_path(Some(directory.join("network-policy.json")));
    // GitHub access is granted to every computer of this check, so its hosts are on the list
    // in allowlist mode: api.github.com stands for a host a secret covers.
    use_test_grants(Some(Grants {
        github: true,
        ..Grants::default()
    }));
    let source = "e2e-t8-main";
    let configuration = create(&mut fixture, source, &of_mode(NetworkPolicyMode::Open));
    let id = configuration.id().to_owned();

    // Open: created with the engine's rules for a new computer.
    super::super::super::start_disposable_test_computer(&fixture.paths, source).unwrap();
    let open = probe(&fixture, source, "open");
    assert!(reached(&open.example_com) && reached(&open.example_org), "{open:?}");
    assert!(reached(&open.public_ip) && reached(&open.github), "{open:?}");
    assert!(answered(&open.resolver) && !open.gateway_name.is_empty(), "{open:?}");
    keep_config(&fixture, source, "open");

    // Open to allowlist on a stopped computer: the next start restores it in place.
    fixture.stop(source);
    store::set(&id, &allowlist()).unwrap();
    assert!(stopped_changed(&fixture, source), "the runtime still holds the open rules");
    super::super::super::start_disposable_test_computer(&fixture.paths, source).unwrap();
    assert!(!stopped_changed(&fixture, source), "the start applied the stored policy");
    let listed = probe(&fixture, source, "allowlist after the change");
    assert!(reached(&listed.example_com) && reached(&listed.github), "{listed:?}");
    assert!(!reached(&listed.example_org), "{listed:?}");
    unlisted_ip_is_refused(&listed, "allowlist");
    assert!(!answered(&listed.resolver), "{listed:?}");
    assert!(!listed.gateway_name.is_empty(), "names still resolve through the gateway: {listed:?}");
    keep_config(&fixture, source, "allowlist");

    // A running computer keeps its rules until it restarts.
    store::set(&id, &of_mode(NetworkPolicyMode::Closed)).unwrap();
    assert!(stopped_changed(&fixture, source));
    let still = probe(&fixture, source, "allowlist while the stored policy is closed");
    assert!(reached(&still.example_com), "{still:?}");

    // Closed.
    fixture.stop(source);
    super::super::super::start_disposable_test_computer(&fixture.paths, source).unwrap();
    let closed = probe(&fixture, source, "closed");
    assert!(
        !reached(&closed.example_com) && !reached(&closed.example_org),
        "{closed:?}"
    );
    unlisted_ip_is_refused(&closed, "closed");
    assert!(!reached(&closed.github) && !answered(&closed.resolver), "{closed:?}");
    assert!(closed.gateway_name.is_empty(), "{closed:?}");
    keep_config(&fixture, source, "closed");

    // Back to open.
    fixture.stop(source);
    store::set(&id, &of_mode(NetworkPolicyMode::Open)).unwrap();
    super::super::super::start_disposable_test_computer(&fixture.paths, source).unwrap();
    let again = probe(&fixture, source, "open again");
    assert!(reached(&again.example_org) && reached(&again.public_ip), "{again:?}");
    assert!(answered(&again.resolver), "{again:?}");

    // Allowlist, then a checkpoint of the running computer, a fork of it and a restore.
    fixture.stop(source);
    store::set(&id, &allowlist()).unwrap();
    super::super::super::start_disposable_test_computer(&fixture.paths, source).unwrap();
    let checkpoint = capture_for_test(&fixture.paths, &id, "Allowlist").unwrap();
    let fork_name = "e2e-t8-fork";
    fixture.track(fork_name);
    let fork = fork_source(&ProcessRunner, &fixture.paths, &id, Some(&checkpoint), fork_name).unwrap();
    fork_commit(&ProcessRunner, &fixture.paths, &FakeAssignments::new(&[]), &fork, fork_name).unwrap();
    let fork_id = fixture.computer_configuration(fork_name).id().to_owned();
    assert_eq!(
        store::get(&fork_id).unwrap(),
        Some(allowlist()),
        "a fork inherits the policy"
    );
    super::super::super::start_disposable_test_import(&fixture.paths, fork_name).unwrap();
    let forked = probe(&fixture, fork_name, "fork of an allowlist computer");
    assert!(reached(&forked.example_com) && reached(&forked.github), "{forked:?}");
    assert!(!reached(&forked.example_org), "{forked:?}");
    unlisted_ip_is_refused(&forked, "fork");
    keep_config(&fixture, fork_name, "allowlist-fork");
    fixture.stop(fork_name);

    fixture.stop(source);
    restore_with(&ProcessRunner, &fixture.paths, &id, &checkpoint).unwrap();
    super::super::super::start_disposable_test_import(&fixture.paths, source).unwrap();
    let restored = probe(&fixture, source, "allowlist computer restored in place");
    assert!(reached(&restored.example_com) && reached(&restored.github), "{restored:?}");
    assert!(!reached(&restored.example_org), "{restored:?}");
    unlisted_ip_is_refused(&restored, "restore");
    keep_config(&fixture, source, "allowlist-restored");
    fixture.stop(source);
    use_test_grants(None);
    eprintln!("\nVerified the network policy modes, the change on start, a fork and a restore.");
}
