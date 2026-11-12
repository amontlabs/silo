use serde_json::json;

use super::*;

fn allowlist(hosts: &[&str], presets: &[&str]) -> NetworkPolicy {
    NetworkPolicy {
        mode: NetworkPolicyMode::Allowlist,
        hosts: hosts.iter().map(|host| host.to_string()).collect(),
        presets: presets.iter().map(|preset| preset.to_string()).collect(),
    }
}

fn mode(mode: NetworkPolicyMode) -> NetworkPolicy {
    NetworkPolicy {
        mode,
        ..NetworkPolicy::default()
    }
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|arg| arg.to_string()).collect()
}

#[test]
fn hosts_are_validated_strictly() {
    for good in [
        "example.com",
        "*.example.com",
        "api.github.com",
        "a-b.c-d.example",
        "x1.example.org",
        "xn--bcher-kva.example",
    ] {
        assert!(validate_host(good).is_ok(), "{good}");
    }
    for bad in [
        "",
        "*.co.uk",
        "*.github.io",
        "*.s3.amazonaws.com",
        "Example.com",
        "example.com.",
        ".example.com",
        "com",
        "*.com",
        "*",
        "*.*.example.com",
        "a.*.example.com",
        "**.example.com",
        "example.com:443",
        "1.2.3.4",
        "10.0.0.1",
        "[::1]",
        "::1",
        "-a.example.com",
        "a-.example.com",
        "a..example.com",
        "host.microsandbox.internal",
        "printer.local",
        "localhost",
        "app.localhost",
        "1.0.0.127.in-addr.arpa",
        "exa mple.com",
        "exam_ple.com",
        "example.com/path",
        "user@example.com",
        "example.com,other.com",
    ] {
        assert!(validate_host(bad).is_err(), "{bad:?} was accepted");
    }
    let long_label = format!("{}.com", "a".repeat(64));
    assert!(validate_host(&long_label).is_err());
    let long_name = format!("{}.com", ["abcdefghij"; 26].join("."));
    assert!(long_name.len() > 253 && validate_host(&long_name).is_err());
}

#[test]
fn a_policy_is_stored_sorted_and_once_and_capped() {
    let policy = allowlist(
        &["b.example.com", "a.example.com", "b.example.com"],
        &["pypi", "npm", "npm"],
    );
    let stored = validate(&policy).unwrap();
    assert_eq!(stored.hosts, ["a.example.com", "b.example.com"]);
    assert_eq!(stored.presets, ["npm", "pypi"]);
    assert!(validate(&allowlist(&[], &["no-such-preset"])).is_err());
    assert!(validate(&allowlist(&["1.1.1.1"], &[])).is_err());
    let many: Vec<String> = (0..=MAX_HOSTS)
        .map(|n| format!("h{n}.example.com"))
        .collect();
    let refs: Vec<&str> = many.iter().map(String::as_str).collect();
    assert!(validate(&allowlist(&refs, &[])).is_err());
    assert!(validate(&allowlist(&refs[..MAX_HOSTS], &[])).is_ok());
}

#[test]
fn old_and_partial_records_load_as_open() {
    let policy: NetworkPolicy = serde_json::from_value(json!({"mode": "closed"})).unwrap();
    assert_eq!(policy, mode(NetworkPolicyMode::Closed));
    // A computer saved before policies existed has no entry: open, unless its recorded runtime
    // policy is the closed one an import starts from.
    assert_eq!(effective(None, None), NetworkPolicy::default());
    let github_default: Value =
        serde_json::from_str(include_str!("../../../guest/github-network-default.json")).unwrap();
    assert_eq!(
        effective(None, Some(&github_default["policy"])),
        NetworkPolicy::default()
    );
    let imported = json!({"default_egress": "deny", "default_ingress": "deny", "rules": []});
    assert_eq!(
        effective(None, Some(&imported)).mode,
        NetworkPolicyMode::Closed
    );
    let stored = allowlist(&["example.com"], &[]);
    assert_eq!(effective(Some(stored.clone()), Some(&imported)), stored);
}

#[test]
fn presets_load_and_expand_to_their_hosts() {
    let names: Vec<&str> = presets()
        .iter()
        .map(|preset| preset.name.as_str())
        .collect();
    for expected in [
        "npm",
        "pypi",
        "crates",
        "ubuntu",
        "homebrew",
        "github",
        "anthropic",
        "openai",
    ] {
        assert!(names.contains(&expected), "{expected}");
    }
    let expanded = expand(&allowlist(&["example.com"], &["pypi", "ubuntu"]));
    for (host, port) in [
        ("example.com", 443),
        ("pypi.org", 443),
        ("files.pythonhosted.org", 443),
        ("*.archive.ubuntu.com", 80),
        ("*.archive.ubuntu.com", 443),
        ("ports.ubuntu.com", 80),
        ("security.ubuntu.com", 80),
    ] {
        assert!(
            expanded.contains(&Allowed::new(host, port)),
            "{host}:{port}"
        );
    }
    assert!(!expanded
        .iter()
        .any(|allowed| allowed.host == "registry.npmjs.org"));
    assert!(expand(&mode(NetworkPolicyMode::Open)).is_empty());
    assert!(load_presets(
        r#"{"version":2,"presets":[{"name":"x","title":"X","version":1,"hosts":["1.2.3.4"]}]}"#
    )
    .is_err());
    assert!(load_presets(
        r#"{"version":2,"presets":[{"name":"x","title":"X","version":1,"hosts":[]}]}"#
    )
    .is_err());
    assert!(load_presets(r#"{"version":2,"presets":[{"name":"x","title":"X","version":1,"hosts":["a.com"]},{"name":"x","title":"Y","version":1,"hosts":["b.com"]}]}"#).is_err());
    assert!(load_presets(r#"{"version":3,"presets":[]}"#).is_err());
    let listed = preset_list();
    let ubuntu = listed
        .iter()
        .find(|preset| preset.name == "ubuntu")
        .unwrap();
    assert_eq!(ubuntu.hosts.len(), 3);
}

/// The argument lists S1 tried against the real runtime (`docs/direction/evidence/S1.md`).
#[test]
fn rules_match_the_argument_lists_tried_in_s1() {
    let none = Grants::default();
    let open = plan(&mode(NetworkPolicyMode::Open), &none, &[]);
    assert_eq!(
        open.rule_args(),
        args(&[
            "--net-default-egress",
            "deny",
            "--net-default-ingress",
            "allow",
            "--net-rule",
            "allow@dns",
            "--net-rule",
            "allow@public",
        ])
    );
    assert!(open.strict);
    // An open computer with nothing extra is created exactly as before policies existed.
    assert_eq!(open.create_args(), args(&["--net-strict=true"]));

    let with_channel = plan(
        &mode(NetworkPolicyMode::Open),
        &none,
        &[ExtraRule::GatewayPort(47311)],
    );
    assert_eq!(
        with_channel.create_args(),
        args(&[
            "--net-default-egress",
            "deny",
            "--net-default-ingress",
            "allow",
            "--net-rule",
            "allow@dns",
            "--net-rule",
            "allow@host:tcp:47311",
            "--net-rule",
            "allow@public",
            "--net-strict=true",
        ])
    );

    let list = plan(
        &allowlist(&["example.com", "*.python.org"], &[]),
        &none,
        &[ExtraRule::GatewayPort(47311)],
    );
    assert_eq!(
        list.create_args(),
        args(&[
            "--net-default-egress",
            "deny",
            "--net-default-ingress",
            "allow",
            "--net-rule",
            "allow@dns",
            "--net-rule",
            "allow@example.com:tcp:443",
            "--net-rule",
            "allow@*.python.org:tcp:443",
            "--net-rule",
            "allow@host:tcp:47311",
            "--net-strict=false",
        ])
    );
    assert!(!list.strict);

    let closed = plan(&mode(NetworkPolicyMode::Closed), &none, &[]);
    assert_eq!(
        closed.create_args(),
        args(&[
            "--net-default-egress",
            "deny",
            "--net-default-ingress",
            "allow",
            "--net-strict=true",
        ])
    );
    let closed_channel = plan(
        &mode(NetworkPolicyMode::Closed),
        &none,
        &[ExtraRule::GatewayPort(47311)],
    );
    assert_eq!(
        closed_channel.rules,
        vec![Rule::GatewayPort(47311)],
        "closed has no name lookups, only Silo's own rules"
    );
}

#[test]
fn saved_shapes_match_the_runtime() {
    let github_default: Value =
        serde_json::from_str(include_str!("../../../guest/github-network-default.json")).unwrap();
    let open = plan(&mode(NetworkPolicyMode::Open), &Grants::default(), &[]);
    assert_eq!(open.policy_value(), github_default["policy"]);
    assert!(open.matches(&github_default));
    let mut relaxed = github_default.clone();
    relaxed["strict"] = json!(false);
    assert!(!open.matches(&relaxed));
    let list = plan(
        &allowlist(&["example.com", "*.python.org"], &[]),
        &Grants::default(),
        &[],
    );
    let rules = list.policy_value()["rules"].clone();
    assert_eq!(rules[1]["destination"], json!({"domain": "example.com"}));
    assert_eq!(
        rules[2]["destination"],
        json!({"domain_suffix": "python.org"})
    );
    assert_eq!(rules[1]["ports"], json!([{"start": 443, "end": 443}]));
    assert!(!list.matches(&github_default));
    let mut saved = github_default.clone();
    saved["policy"] = list.policy_value();
    saved["strict"] = json!(false);
    assert!(list.matches(&saved));
    // Protocol order and extra fields do not matter; rule order does.
    saved["policy"]["rules"][0]["protocols"] = json!(["tcp", "udp"]);
    saved["policy"]["rules"][0]["note"] = json!("ignored");
    assert!(list.matches(&saved));
    let rules = saved["policy"]["rules"].as_array().unwrap().clone();
    saved["policy"]["rules"] = json!([rules[0], rules[2], rules[1]]);
    assert!(!list.matches(&saved));
}

#[test]
fn grants_add_github_and_secret_hosts_only_in_allowlist_mode() {
    let grants = Grants {
        github: true,
        secrets: vec![
            ("OPENAI_KEY".into(), vec!["api.openai.com".into()]),
            ("ANY".into(), vec!["*".into()]),
            (
                "WILD".into(),
                vec!["*.example.net".into(), "10.0.0.1".into()],
            ),
        ],
    };
    let list = plan(&allowlist(&["example.com"], &[]), &grants, &[]);
    let tokens: Vec<String> = list.rules.iter().map(Rule::token).collect();
    for token in [
        "allow@dns",
        "allow@example.com:tcp:443",
        "allow@github.com:tcp:443",
        "allow@api.github.com:tcp:443",
        "allow@api.openai.com:tcp:443",
        "allow@*.example.net:tcp:443",
    ] {
        assert!(
            tokens.contains(&token.to_owned()),
            "{token} missing from {tokens:?}"
        );
    }
    assert!(!tokens
        .iter()
        .any(|token| token.contains("10.0.0.1") || token == "allow@*:tcp:443"));
    assert!(list
        .hosts
        .iter()
        .any(|host| host.host == "github.com" && host.from == "github"));
    assert!(list
        .hosts
        .iter()
        .any(|host| host.host == "api.openai.com" && host.from == "secret:OPENAI_KEY"));
    // A host the list and a grant both name is one rule.
    let twice = plan(&allowlist(&["api.openai.com"], &[]), &grants, &[]);
    assert_eq!(
        twice
            .rules
            .iter()
            .filter(|rule| rule.token() == "allow@api.openai.com:tcp:443")
            .count(),
        1
    );
    for other in [NetworkPolicyMode::Open, NetworkPolicyMode::Closed] {
        let plan = plan(&mode(other), &grants, &[]);
        assert!(plan.hosts.is_empty());
        assert!(!plan
            .rules
            .iter()
            .any(|rule| matches!(rule, Rule::Domain { .. })));
    }
}

#[test]
fn no_wider_orders_modes_and_compares_host_sets() {
    use NetworkPolicyMode::*;
    let open = mode(Open);
    let closed = mode(Closed);
    let empty = allowlist(&[], &[]);
    let a = allowlist(&["a.example.com"], &[]);
    let ab = allowlist(&["a.example.com", "b.example.com"], &[]);
    let wild = allowlist(&["*.example.com"], &[]);
    let deeper_wild = allowlist(&["*.a.example.com"], &[]);
    let apex = allowlist(&["example.com"], &[]);
    let pypi = allowlist(&[], &["pypi"]);
    let pypi_hosts = allowlist(&["pypi.org", "files.pythonhosted.org"], &[]);
    let ubuntu = allowlist(&[], &["ubuntu"]);
    let ubuntu_https_only = allowlist(
        &[
            "*.archive.ubuntu.com",
            "security.ubuntu.com",
            "ports.ubuntu.com",
        ],
        &[],
    );
    let cases: Vec<(&NetworkPolicy, &NetworkPolicy, bool)> = vec![
        (&closed, &closed, true),
        (&closed, &empty, true),
        (&closed, &open, true),
        (&open, &open, true),
        (&open, &closed, false),
        (&open, &a, false),
        (&a, &open, true),
        (&empty, &closed, false),
        (&a, &closed, false),
        (&empty, &a, true),
        (&a, &a, true),
        (&a, &ab, true),
        (&ab, &a, false),
        (&a, &wild, true),
        (&apex, &wild, true),
        (&deeper_wild, &wild, true),
        (&wild, &deeper_wild, false),
        (&wild, &a, false),
        (&wild, &apex, false),
        (&pypi, &pypi_hosts, true),
        (&pypi_hosts, &pypi, true),
        (&ubuntu_https_only, &ubuntu, true),
        (&ubuntu, &ubuntu_https_only, false),
        (&pypi, &a, false),
    ];
    for (new, current, expected) in cases {
        assert_eq!(no_wider(new, current), expected, "{new:?} vs {current:?}");
    }
    let unrelated = allowlist(&["example.org"], &[]);
    assert!(!no_wider(&unrelated, &wild));
    let sibling = allowlist(&["badexample.com"], &[]);
    assert!(
        !no_wider(&sibling, &wild),
        "a suffix covers only names under it"
    );
}

#[test]
fn a_widening_summary_names_the_hosts_added() {
    let current = allowlist(&["a.example.com"], &[]);
    let new = allowlist(&["a.example.com", "b.example.com"], &["pypi"]);
    let summary = widening_summary("main", &current, &new);
    assert!(
        summary.starts_with("Widen the network of main"),
        "{summary}"
    );
    for host in ["b.example.com", "pypi.org", "files.pythonhosted.org"] {
        assert!(summary.contains(host), "{summary}");
    }
    assert!(!summary.contains("a.example.com"), "{summary}");
    let summary = widening_summary(
        "main",
        &mode(NetworkPolicyMode::Closed),
        &mode(NetworkPolicyMode::Open),
    );
    assert_eq!(
        summary,
        "Change the network of main from closed to open (any public address)."
    );
    let summary = widening_summary(
        "main",
        &mode(NetworkPolicyMode::Closed),
        &allowlist(&[], &["ubuntu"]),
    );
    assert!(summary.contains("ports.ubuntu.com (port 80)"), "{summary}");
    let many: Vec<String> = (0..30).map(|n| format!("h{n}.example.com")).collect();
    let refs: Vec<&str> = many.iter().map(String::as_str).collect();
    let summary = widening_summary(
        "main",
        &mode(NetworkPolicyMode::Closed),
        &allowlist(&refs, &[]),
    );
    assert!(summary.contains("and 10 more"), "{summary}");
}

#[test]
fn the_store_keeps_policies_by_computer_and_forgets_removed_ones() {
    let directory = tempfile::tempdir().unwrap();
    store::use_test_path(Some(directory.path().join("network-policy.json")));
    assert_eq!(store::get("c1").unwrap(), None);
    let list = validate(&allowlist(&["example.com"], &["npm"])).unwrap();
    store::set("c1", &list).unwrap();
    assert_eq!(store::get("c1").unwrap(), Some(list.clone()));
    store::inherit("c1", "c2", None).unwrap();
    assert_eq!(store::get("c2").unwrap(), Some(list.clone()));
    let closed = mode(NetworkPolicyMode::Closed);
    store::inherit("c1", "c3", Some(&closed)).unwrap();
    assert_eq!(store::get("c3").unwrap(), Some(closed));
    store::inherit("unknown", "c4", None).unwrap();
    assert_eq!(store::get("c4").unwrap(), None);
    computer_removed("c1");
    assert_eq!(store::get("c1").unwrap(), None);
    assert_eq!(store::get("c2").unwrap(), Some(list));
    store::use_test_path(None);
}

#[test]
fn an_import_keeps_its_deny_all_policy_until_the_owner_sets_one() {
    let directory = tempfile::tempdir().unwrap();
    store::use_test_path(Some(directory.path().join("network-policy.json")));
    let imported = json!({"default_egress": "deny", "default_ingress": "deny", "rules": []});
    let plan = plan_for("imported", "imported", Some(&imported)).unwrap();
    assert_eq!(plan.mode, NetworkPolicyMode::Closed);
    assert_eq!(plan.policy_value(), imported);
    assert!(plan
        .rule_args()
        .windows(2)
        .any(|pair| pair == ["--net-default-ingress", "deny"]));
    // Once the owner sets closed, published ports answer again.
    store::set("imported", &mode(NetworkPolicyMode::Closed)).unwrap();
    let plan = plan_for("imported", "imported", Some(&imported)).unwrap();
    assert_eq!(plan.policy_value()["default_ingress"], "allow");
    // A computer saved before policies existed stays open.
    let github_default: Value =
        serde_json::from_str(include_str!("../../../guest/github-network-default.json")).unwrap();
    let plan = plan_for("old", "old", Some(&github_default["policy"])).unwrap();
    assert_eq!(plan.policy_value(), github_default["policy"]);
    store::use_test_path(None);
}

#[test]
fn the_channel_rule_is_the_computers_own_port_in_every_mode_and_no_other_computers() {
    let directory = tempfile::tempdir().unwrap();
    store::use_test_path(Some(directory.path().join("network-policy.json")));
    crate::guest_client::tokens::use_test_directory(Some(directory.path().join("guest-client")));
    let tokens = crate::guest_client::TokenStore::current().unwrap();
    tokens.enable("a", 47311).unwrap();
    tokens.enable("b", 47312).unwrap();
    assert_eq!(extra_rules("a"), vec![ExtraRule::GatewayPort(47311)]);
    assert_eq!(extra_rules("b"), vec![ExtraRule::GatewayPort(47312)]);
    assert!(
        extra_rules("c").is_empty(),
        "a computer without tools has no channel rule"
    );
    let opens = |plan: &Plan, port: u16| {
        plan.rules.contains(&Rule::GatewayPort(port))
            && plan.rule_args().contains(&format!("allow@host:tcp:{port}"))
            && plan.policy_value()["rules"]
                .as_array()
                .unwrap()
                .contains(&Rule::GatewayPort(port).saved())
    };
    for policy in [
        mode(NetworkPolicyMode::Open),
        allowlist(&["example.com"], &["npm"]),
        mode(NetworkPolicyMode::Closed),
    ] {
        store::set("a", &policy).unwrap();
        store::set("b", &policy).unwrap();
        store::set("c", &policy).unwrap();
        let (a, b, c) = (
            plan_for("a", "a", None).unwrap(),
            plan_for("b", "b", None).unwrap(),
            plan_for("c", "c", None).unwrap(),
        );
        assert!(opens(&a, 47311), "{:?}", policy.mode);
        assert!(!opens(&a, 47312));
        assert!(opens(&b, 47312) && !opens(&b, 47311));
        assert!(!opens(&c, 47311) && !opens(&c, 47312));
        let (gateway, _) = gateway_policy(&a);
        assert!(gateway["rules"]
            .as_array()
            .unwrap()
            .contains(&Rule::GatewayPort(47311).saved()));
        // No other rule of any mode reaches the gateway beyond name lookups.
        for rule in &a.rules {
            if let Rule::GatewayPort(port) = rule {
                assert_eq!(*port, 47311);
            }
        }
    }
    crate::guest_client::tokens::use_test_directory(None);
    store::use_test_path(None);
}

fn view(
    network: Option<Value>,
    recorded: Option<Value>,
) -> crate::runtime::checkpoints::NetworkPolicyView {
    crate::runtime::checkpoints::NetworkPolicyView {
        name: "dev".into(),
        container: false,
        running: false,
        pending: false,
        network,
        recorded,
    }
}

fn deny_all() -> Value {
    json!({"default_egress": "deny", "default_ingress": "deny", "rules": []})
}

#[test]
fn the_judge_reads_a_computer_as_the_service_does() {
    let directory = tempfile::tempdir().unwrap();
    store::use_test_path(Some(directory.path().join("network-policy.json")));
    let github_default: Value =
        serde_json::from_str(include_str!("../../../guest/github-network-default.json")).unwrap();
    // An import that runs closed on deny-all rules and has no entry is closed, not open.
    let imported = view(Some(json!({"policy": deny_all()})), Some(deny_all()));
    let held = service::judged_from_view("imported", None, imported);
    assert_eq!(held.policy.mode, NetworkPolicyMode::Closed);
    let none = Grants::default();
    assert!(!no_wider_held(
        &Held::latest(mode(NetworkPolicyMode::Open)),
        &none,
        &held,
        &none
    ));
    // An older computer with the open rules and no entry is still open.
    let older = view(Some(github_default.clone()), None);
    assert_eq!(
        service::judged_from_view("older", None, older).policy.mode,
        NetworkPolicyMode::Open
    );
    // A stored policy wins over the rules.
    let list = allowlist(&["example.com"], &[]);
    let stored = store::Stored::approved(&list);
    let held = service::judged_from_view("c", Some(stored), view(Some(github_default), None));
    assert_eq!(held.policy, list);
    store::use_test_path(None);
}

#[test]
fn a_blocked_container_is_judged_closed_whatever_is_stored() {
    let stored = store::Stored::approved(&mode(NetworkPolicyMode::Open));
    let mut blocked = view(
        Some(json!({"policy": crate::containers::secrets::blocked_policy()})),
        None,
    );
    blocked.container = true;
    let held = service::judged_from_view("c", Some(stored), blocked);
    assert_eq!(held.policy.mode, NetworkPolicyMode::Closed);
}

#[test]
fn an_import_saves_a_closed_entry_that_keeps_incoming_denied_until_the_owner_sets_a_policy() {
    let directory = tempfile::tempdir().unwrap();
    store::use_test_path(Some(directory.path().join("network-policy.json")));
    store::set_stored("imported", store::Stored::closed_without_incoming()).unwrap();
    assert_eq!(
        store::get("imported").unwrap(),
        Some(mode(NetworkPolicyMode::Closed))
    );
    let plan = plan_for("imported", "imported", Some(&deny_all())).unwrap();
    assert_eq!(plan.mode, NetworkPolicyMode::Closed);
    assert_eq!(plan.policy_value(), deny_all());
    // The stored entry is the truth, whatever the recorded rules say.
    let plan = plan_for("imported", "imported", None).unwrap();
    assert_eq!(plan.policy_value(), deny_all());
    // A gateway always lets its published ports in.
    assert_eq!(
        plan_for_gateway("imported", "imported")
            .unwrap()
            .policy_value()["default_ingress"],
        "allow"
    );
    store::set("imported", &mode(NetworkPolicyMode::Closed)).unwrap();
    assert_eq!(
        plan_for("imported", "imported", None)
            .unwrap()
            .policy_value()["default_ingress"],
        "allow"
    );
    store::use_test_path(None);
}

#[test]
fn a_fork_of_an_import_is_closed_and_a_closed_network_copy_denies_incoming() {
    let directory = tempfile::tempdir().unwrap();
    store::use_test_path(Some(directory.path().join("network-policy.json")));
    store::set_stored("imported", store::Stored::closed_without_incoming()).unwrap();
    store::inherit("imported", "fork", None).unwrap();
    assert_eq!(
        store::get("fork").unwrap(),
        Some(mode(NetworkPolicyMode::Closed))
    );
    assert!(store::get_stored("fork").unwrap().unwrap().incoming_denied);
    store::use_test_path(None);
}

#[test]
fn a_policy_file_that_cannot_be_read_refuses_instead_of_running_open() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("network-policy.json");
    store::use_test_path(Some(file.clone()));
    for contents in [
        "{ not json".to_owned(),
        r#"{"version": 2, "computers": {}}"#.to_owned(),
        r#"{"version": 1, "computers": {"c": {"mode": "from-the-future"}}}"#.to_owned(),
    ] {
        std::fs::write(&file, &contents).unwrap();
        assert!(plan_for("c", "c", None).is_err(), "{contents}");
        assert!(plan_for_gateway("c", "c").is_err(), "{contents}");
        assert!(held_for("c", None).is_err(), "{contents}");
        assert!(store::set("c", &mode(NetworkPolicyMode::Closed)).is_err());
        assert!(store::inherit("a", "c", None).is_err());
        assert!(store::remove("c").is_err());
        // Nothing was overwritten.
        assert_eq!(std::fs::read_to_string(&file).unwrap(), contents);
    }
    store::use_test_path(None);
}

fn version_two_presets() -> Vec<Preset> {
    let at = |host: &str| Allowed::new(host, HTTPS_PORT);
    vec![Preset {
        name: "tools".into(),
        title: "Tools".into(),
        version: 2,
        hosts: vec![at("a.example.com"), at("b.example.com")],
        history: vec![(1, vec![at("a.example.com")])],
    }]
}

#[test]
fn a_preset_update_waits_for_the_owner_to_accept_it() {
    let directory = tempfile::tempdir().unwrap();
    store::use_test_path(Some(directory.path().join("network-policy.json")));
    use_test_presets(Some(version_two_presets()));
    let policy = allowlist(&[], &["tools"]);
    // Approved at version 1 (no pin saved is version 1).
    let held = Held {
        policy: policy.clone(),
        pins: Pins::new(),
    };
    let hosts: Vec<String> = expand_held(&held)
        .into_iter()
        .map(|allowed| allowed.host)
        .collect();
    assert_eq!(hosts, ["a.example.com"]);
    let updates = held.preset_updates();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].name, "tools");
    assert_eq!(updates[0].added_hosts, ["b.example.com"]);
    let rules: Vec<String> = plan_held(&held, &Grants::default(), &[])
        .rules
        .iter()
        .map(Rule::token)
        .collect();
    assert!(rules.contains(&"allow@a.example.com:tcp:443".to_owned()));
    assert!(
        !rules.iter().any(|rule| rule.contains("b.example.com")),
        "{rules:?}"
    );
    // The same presets at the latest version are wider, so a computer's agent that sends them
    // asks, and the summary names the new host.
    let latest = Held::latest(policy.clone());
    let none = Grants::default();
    assert!(!no_wider_held(&latest, &none, &held, &none));
    assert!(widening_summary_held("main", &held, &none, &latest, &none).contains("b.example.com"));
    assert!(no_wider_held(&held, &none, &latest, &none));
    // Setting the policy again approves the running version.
    store::set("c", &policy).unwrap();
    let stored = store::get_stored("c").unwrap().unwrap();
    assert_eq!(stored.preset_versions.get("tools"), Some(&2));
    assert!(stored.held().preset_updates().is_empty());
    use_test_presets(None);
    store::use_test_path(None);
}

#[test]
fn saving_policy_keeps_current_pins_and_only_explicit_accept_repins() {
    let directory = tempfile::tempdir().unwrap();
    store::use_test_path(Some(directory.path().join("network-policy.json")));
    use_test_presets(Some(version_two_presets()));
    let target = || service::Target::Macos {
        name: "dev".into(),
        running: false,
    };
    let policy = allowlist(&[], &["tools"]);
    // Stored initially at version 1
    store::set_stored(
        "c",
        store::Stored {
            mode: NetworkPolicyMode::Allowlist,
            hosts: Vec::new(),
            presets: vec!["tools".into()],
            preset_versions: Pins::from([("tools".to_owned(), 1)]),
            incoming_denied: false,
        },
    )
    .unwrap();
    let initial = service::get("c", target()).unwrap();
    assert_eq!(initial.preset_updates.len(), 1);

    // Normal save with expected keeps version 1 pinned (update still pending)
    let saved = service::set("c", &policy, Some(&policy), None, target()).unwrap();
    assert_eq!(saved.preset_updates.len(), 1);
    let stored = store::get_stored("c").unwrap().unwrap();
    assert_eq!(stored.preset_versions.get("tools"), Some(&1));

    // An entry saved with no pin (the first version) keeps waiting for an accept, with or
    // without `expected`.
    store::set_stored(
        "c",
        store::Stored {
            mode: NetworkPolicyMode::Allowlist,
            hosts: Vec::new(),
            presets: vec!["tools".into()],
            preset_versions: Pins::new(),
            incoming_denied: false,
        },
    )
    .unwrap();
    let saved = service::set("c", &policy, None, None, target()).unwrap();
    assert_eq!(saved.preset_updates.len(), 1);
    assert!(store::get_stored("c")
        .unwrap()
        .unwrap()
        .preset_versions
        .is_empty());

    // Explicit accept re-pins to version 2
    let accepted = service::set("c", &policy, Some(&policy), Some(true), target()).unwrap();
    assert!(accepted.preset_updates.is_empty());
    let stored = store::get_stored("c").unwrap().unwrap();
    assert_eq!(stored.preset_versions.get("tools"), Some(&2));

    use_test_presets(None);
    store::use_test_path(None);
}

#[test]
fn a_preset_version_this_silo_does_not_know_allows_nothing() {
    use_test_presets(Some(version_two_presets()));
    let policy = allowlist(&["x.example.com"], &["tools"]);
    for pin in [0, 3] {
        let held = Held {
            policy: policy.clone(),
            pins: Pins::from([("tools".to_owned(), pin)]),
        };
        let hosts: Vec<String> = expand_held(&held)
            .into_iter()
            .map(|allowed| allowed.host)
            .collect();
        assert_eq!(hosts, ["x.example.com"], "pin {pin}");
    }
    use_test_presets(None);
}

#[test]
fn every_checked_in_preset_keeps_the_versions_it_replaced() {
    for preset in presets() {
        assert!(preset.version >= 1);
        for (version, hosts) in &preset.history {
            assert!(*version < preset.version && !hosts.is_empty());
        }
        for version in 1..preset.version {
            assert!(
                preset.history.iter().any(|(kept, _)| *kept == version),
                "{} lost the list of version {version}",
                preset.name
            );
        }
    }
}

#[test]
fn a_narrowing_is_saved_only_against_the_policy_it_was_judged_against() {
    let directory = tempfile::tempdir().unwrap();
    store::use_test_path(Some(directory.path().join("network-policy.json")));
    let target = || service::Target::Macos {
        name: "dev".into(),
        running: false,
    };
    let wide = allowlist(&["a.example.com", "b.example.com"], &[]);
    let narrow = allowlist(&["a.example.com"], &[]);
    store::set("c", &wide).unwrap();
    // The owner narrows to nothing meanwhile: the agent's narrowing, judged against `wide`,
    // does not put a list back.
    store::set("c", &mode(NetworkPolicyMode::Closed)).unwrap();
    let refused = service::set("c", &narrow, Some(&wide), None, target()).unwrap_err();
    assert!(
        refused.contains("try") || refused.contains("retry"),
        "{refused}"
    );
    assert_eq!(
        store::get("c").unwrap(),
        Some(mode(NetworkPolicyMode::Closed))
    );
    // Against the policy it was judged against, it is saved.
    store::set("c", &wide).unwrap();
    let state = service::set("c", &narrow, Some(&wide), None, target()).unwrap();
    assert_eq!(state.policy, narrow);
    assert_eq!(store::get("c").unwrap(), Some(narrow));
    store::use_test_path(None);
}

#[test]
fn a_fork_gets_its_own_port_and_no_token_and_an_import_gets_none_until_turned_on() {
    let directory = tempfile::tempdir().unwrap();
    store::use_test_path(Some(directory.path().join("network-policy.json")));
    crate::guest_client::tokens::use_test_directory(Some(directory.path().join("guest-client")));
    let tokens = crate::guest_client::TokenStore::current().unwrap();
    crate::guest_client::turn_on("parent").unwrap();
    let parent_token = tokens.mint("parent").unwrap().token;
    let parent_port = tokens.port_of("parent").unwrap();
    crate::guest_client::inherit("parent", "child").unwrap();
    let child_port = tokens.port_of("child").unwrap();
    assert_ne!(child_port, parent_port);
    assert!(
        !crate::guest_client::tokens::tests::accepts(&tokens, &parent_token, "child", child_port),
        "a copied token proves nothing"
    );
    assert!(tokens.status("child").unwrap().unwrap().hash.is_empty());
    assert_eq!(
        plan_for("child", "child", None).unwrap().rules,
        vec![Rule::Dns, Rule::GatewayPort(child_port), Rule::Public]
    );
    // A fork of a computer without tools has none.
    crate::guest_client::inherit("nothing", "other-child").unwrap();
    assert!(tokens.status("other-child").unwrap().is_none());
    // An import starts closed with no channel; turned on, it keeps its deny-all ingress.
    let imported = json!({"default_egress": "deny", "default_ingress": "deny", "rules": []});
    assert!(extra_rules("imported").is_empty());
    crate::guest_client::turn_on("imported").unwrap();
    let port = tokens.port_of("imported").unwrap();
    let plan = plan_for("imported", "imported", Some(&imported)).unwrap();
    assert_eq!(plan.rules, vec![Rule::GatewayPort(port)]);
    assert_eq!(plan.policy_value()["default_ingress"], "deny");
    // The import's saved closed entry plans the same: the channel and nothing else, incoming
    // still denied.
    store::set_stored("imported", store::Stored::closed_without_incoming()).unwrap();
    let plan = plan_for("imported", "imported", None).unwrap();
    assert_eq!(plan.rules, vec![Rule::GatewayPort(port)]);
    assert_eq!(plan.policy_value()["default_ingress"], "deny");
    crate::guest_client::computer_removed("imported");
    assert!(extra_rules("imported").is_empty());
    crate::guest_client::tokens::use_test_directory(None);
    store::use_test_path(None);
}

#[test]
fn a_secret_wildcard_over_a_public_suffix_is_not_a_network_rule() {
    let grants = Grants {
        github: false,
        secrets: vec![(
            "WIDE".into(),
            vec!["*.co.uk".into(), "*.com".into(), "api.example.com".into()],
        )],
    };
    let list = plan(&allowlist(&[], &[]), &grants, &[]);
    let tokens: Vec<String> = list.rules.iter().map(Rule::token).collect();
    assert!(tokens.contains(&"allow@api.example.com:tcp:443".to_owned()));
    assert!(!tokens
        .iter()
        .any(|token| token.contains("co.uk") || token.contains("*.com")));
    assert_eq!(
        grants.too_wide(),
        [
            ("WIDE".to_owned(), "*.co.uk".to_owned()),
            ("WIDE".to_owned(), "*.com".to_owned())
        ]
    );
    assert!(validate_host("*.example.co.uk").is_ok());
    assert!(validate(&allowlist(&["*.co.uk"], &[])).is_err());
}

fn github_version_two() -> Vec<Preset> {
    let at = |host: &str| Allowed::new(host, HTTPS_PORT);
    vec![Preset {
        name: "github".into(),
        title: "GitHub".into(),
        version: 2,
        hosts: vec![at("github.com"), at("new.github.example")],
        history: vec![(1, vec![at("github.com")])],
    }]
}

fn github_grant() -> Grants {
    Grants {
        github: true,
        secrets: Vec::new(),
    }
}

fn rule_tokens(held: &Held, grants: &Grants) -> Vec<String> {
    plan_held(held, grants, &[])
        .rules
        .iter()
        .map(Rule::token)
        .collect()
}

#[test]
fn a_github_grant_holds_the_list_it_was_approved_with_until_the_owner_accepts_the_update() {
    use_test_presets(Some(github_version_two()));
    let policy = allowlist(&["example.com"], &[]);
    // A policy saved before grants were pinned has no pin: it holds the first list.
    let old = Held {
        policy: policy.clone(),
        pins: Pins::new(),
    };
    let tokens = rule_tokens(&old, &github_grant());
    assert!(
        tokens.contains(&"allow@github.com:tcp:443".to_owned()),
        "{tokens:?}"
    );
    assert!(
        !tokens
            .iter()
            .any(|token| token.contains("new.github.example")),
        "{tokens:?}"
    );
    let updates = old.grant_updates(&github_grant());
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].name, GITHUB_GRANT_PIN);
    assert_eq!(updates[0].added_hosts, ["new.github.example"]);
    // Without a GitHub grant there is nothing to approve, and nothing is added.
    assert!(old.grant_updates(&Grants::default()).is_empty());
    assert!(!rule_tokens(&old, &Grants::default())
        .iter()
        .any(|token| token.contains("github")));
    // Approved now, the grant gets the running list.
    let latest = Held::latest(policy.clone());
    assert_eq!(latest.pins.get(GITHUB_GRANT_PIN), Some(&2));
    assert!(latest.grant_updates(&github_grant()).is_empty());
    assert!(rule_tokens(&latest, &github_grant())
        .contains(&"allow@new.github.example:tcp:443".to_owned()));
    // A pin this Silo does not know allows no GitHub hosts.
    let unknown = Held {
        policy,
        pins: Pins::from([(GITHUB_GRANT_PIN.to_owned(), 7)]),
    };
    assert!(!rule_tokens(&unknown, &github_grant())
        .iter()
        .any(|token| token.contains("github")));
    use_test_presets(None);
}

#[test]
fn saving_a_policy_keeps_the_github_grant_pin_and_only_an_accept_moves_it() {
    let directory = tempfile::tempdir().unwrap();
    store::use_test_path(Some(directory.path().join("network-policy.json")));
    use_test_presets(Some(github_version_two()));
    use_test_grants(Some(github_grant()));
    let target = || service::Target::Macos {
        name: "dev".into(),
        running: false,
    };
    let policy = allowlist(&["example.com"], &[]);
    // An entry saved before grants were pinned.
    std::fs::write(
        directory.path().join("network-policy.json"),
        r#"{"version": 1, "computers": {"c": {"mode": "allowlist", "hosts": ["example.com"]}}}"#,
    )
    .unwrap();
    let state = service::get("c", target()).unwrap();
    assert_eq!(state.preset_updates.len(), 1);
    assert_eq!(state.preset_updates[0].added_hosts, ["new.github.example"]);
    assert!(!state
        .allowed_hosts
        .iter()
        .any(|host| host.host == "new.github.example"));
    assert!(state
        .allowed_hosts
        .iter()
        .any(|host| host.host == "github.com" && host.from == "github"));
    // An ordinary save keeps waiting for the owner.
    let saved = service::set("c", &policy, None, None, target()).unwrap();
    assert_eq!(saved.preset_updates.len(), 1);
    assert!(store::get_stored("c")
        .unwrap()
        .unwrap()
        .preset_versions
        .is_empty());
    // A computer with no entry is unpinned like an old one: its first save keeps waiting too.
    let first = service::set("d", &policy, None, None, target()).unwrap();
    assert_eq!(first.preset_updates.len(), 1);
    assert!(store::get_stored("d")
        .unwrap()
        .unwrap()
        .preset_versions
        .is_empty());
    assert!(!first
        .allowed_hosts
        .iter()
        .any(|host| host.host == "new.github.example"));
    // A computer created now is approved at the running list.
    store::set("e", &policy).unwrap();
    let fresh = service::get("e", target()).unwrap();
    assert!(fresh.preset_updates.is_empty());
    assert_eq!(
        store::get_stored("e")
            .unwrap()
            .unwrap()
            .preset_versions
            .get(GITHUB_GRANT_PIN),
        Some(&2)
    );
    assert!(fresh
        .allowed_hosts
        .iter()
        .any(|host| host.host == "new.github.example"));
    // Accepting moves the pin.
    let accepted = service::set("c", &policy, None, Some(true), target()).unwrap();
    assert!(accepted.preset_updates.is_empty());
    assert!(accepted
        .allowed_hosts
        .iter()
        .any(|host| host.host == "new.github.example"));
    assert_eq!(
        store::get_stored("c")
            .unwrap()
            .unwrap()
            .preset_versions
            .get(GITHUB_GRANT_PIN),
        Some(&2)
    );
    use_test_grants(None);
    use_test_presets(None);
    store::use_test_path(None);
}

#[test]
fn a_computer_with_no_policy_entry_holds_the_first_github_list_until_the_owner_accepts() {
    let directory = tempfile::tempdir().unwrap();
    store::use_test_path(Some(directory.path().join("network-policy.json")));
    use_test_presets(Some(github_version_two()));
    use_test_grants(Some(github_grant()));
    let macos = || service::Target::Macos {
        name: "dev".into(),
        running: false,
    };
    // Shown and planned the same way: the newer list waits for the owner.
    let state = service::get("legacy", macos()).unwrap();
    assert_eq!(state.preset_updates.len(), 1);
    assert_eq!(state.preset_updates[0].added_hosts, ["new.github.example"]);
    assert!(!state
        .allowed_hosts
        .iter()
        .any(|host| host.host == "new.github.example"));
    let (held, _) = held_for("legacy", None).unwrap();
    assert!(held.pins.is_empty());
    assert!(!rule_tokens(&held, &github_grant())
        .iter()
        .any(|token| token.contains("new.github.example")));
    // A Linux computer is resolved the same way, and the judge sees the same policy.
    let resolved = service::judged_from_view("legacy", None, view(None, None));
    assert!(resolved.pins.is_empty());
    // A copy of a computer with no entry has none either.
    store::inherit("legacy", "copy", None).unwrap();
    assert!(store::get_stored("copy").unwrap().is_none());
    assert_eq!(
        service::get("copy", macos()).unwrap().preset_updates.len(),
        1
    );
    // A copy given a policy holds its source's versions too: handing a policy on takes no
    // update.
    store::inherit("legacy", "chosen", Some(&allowlist(&["example.com"], &[]))).unwrap();
    assert_eq!(
        service::get("chosen", macos())
            .unwrap()
            .preset_updates
            .len(),
        1
    );
    use_test_grants(None);
    use_test_presets(None);
    store::use_test_path(None);
}

#[test]
fn the_checked_in_github_list_is_what_an_unpinned_grant_has_always_added() {
    let github = preset("github").unwrap();
    let held = Held {
        policy: allowlist(&[], &[]),
        pins: Pins::new(),
    };
    let unpinned: BTreeSet<Allowed> = github
        .hosts_at(held.pins.get(GITHUB_GRANT_PIN).copied())
        .0
        .into_iter()
        .collect();
    let current: BTreeSet<Allowed> = github.hosts.iter().cloned().collect();
    assert_eq!(
        unpinned, current,
        "an unpinned grant would widen or narrow on load"
    );
}

#[test]
fn a_wildcard_over_any_public_suffix_is_refused_and_ordinary_domains_are_not() {
    for shared in [
        "*.duckdns.org",
        "*.blogspot.com",
        "*.github.io",
        "*.s3.amazonaws.com",
        "*.herokuapp.com",
        "*.co.uk",
        "*.pages.dev",
        "*.githubusercontent.com",
        "*.ck",
        "*.compute.amazonaws.com",
        "*.sch.uk",
        "*.kobe.jp",
    ] {
        assert!(validate_host(shared).is_err(), "{shared}");
        assert!(is_too_wide_wildcard(shared), "{shared}");
        assert!(validate(&allowlist(&[shared], &[])).is_err(), "{shared}");
    }
    // The same names as exact hosts, and names under them, are fine.
    for fine in [
        "duckdns.org",
        "example.duckdns.org",
        "*.example.duckdns.org",
        "*.example.co.uk",
        "*.example.com",
        "*.archive.ubuntu.com",
        "*.www.ck",
        "*.city.kobe.jp",
        "*.example.unknowntld",
    ] {
        assert!(validate_host(fine).is_ok(), "{fine}");
        assert!(!is_too_wide_wildcard(fine), "{fine}");
    }
}

#[test]
fn cloud_bases_that_host_tenant_subdomains_are_refused_for_a_wildcard() {
    for base in [
        "azure.com",
        "azure.net",
        "windows.net",
        "core.windows.net",
        "database.windows.net",
        "search.windows.net",
        "scm.azurewebsites.net",
        "vault.azure.net",
        "openai.azure.com",
        "cognitiveservices.azure.com",
        "googleusercontent.com",
        "firebaseio.com",
        "firebasestorage.app",
        "oraclecloud.com",
        "aliyuncs.com",
        "myqcloud.com",
        "myhuaweicloud.com",
        "herokudns.com",
        "scw.cloud",
        "backblazeb2.com",
        "r2.cloudflarestorage.com",
        "netlify.com",
        "glitch.com",
        "webcontainer.io",
        "codesandbox.io",
        "gitpod.io",
        "fastly.net",
        "b-cdn.net",
        "visualstudio.com",
        "appdomain.cloud",
        "mybluemix.net",
        "salesforce.com",
        "force.com",
    ] {
        let pattern = format!("*.{base}");
        assert!(validate_host(&pattern).is_err(), "{pattern}");
        assert!(is_too_wide_wildcard(&pattern), "{pattern}");
        // A tenant's own name below it stays allowed, and so does the base as an exact host.
        assert!(validate_host(&format!("*.tenant.{base}")).is_ok(), "{base}");
        assert!(validate_host(base).is_ok(), "{base}");
    }
}

#[test]
fn the_names_silo_always_refused_stay_refused_whether_or_not_the_list_carries_them() {
    let file: Value = serde_json::from_str(include_str!("public_suffixes.json")).unwrap();
    for suffix in file["suffixes"].as_array().unwrap() {
        let suffix = suffix.as_str().unwrap();
        assert!(validate_host(&format!("*.{suffix}")).is_err(), "{suffix}");
        assert!(is_too_wide_wildcard(&format!("*.{suffix}")), "{suffix}");
    }
    assert!(validate_host("*.amazonaws.com").is_err());
}

#[test]
fn counting_what_a_policy_reaches_includes_the_hosts_its_github_grant_adds() {
    use_test_presets(Some(github_version_two()));
    let policy = allowlist(&["example.com"], &[]);
    let at = |version: u32| Held {
        policy: policy.clone(),
        pins: Pins::from([(GITHUB_GRANT_PIN.to_owned(), version)]),
    };
    let github = github_grant();
    let none = Grants::default();
    // Holding the newer list reaches more than holding the first one, but only for a computer
    // that has the grant.
    assert!(!no_wider_held(&at(2), &github, &at(1), &github));
    assert!(no_wider_held(&at(1), &github, &at(2), &github));
    assert!(no_wider_held(&at(2), &none, &at(1), &github));
    assert!(no_wider_held(&at(2), &none, &at(1), &none));
    let summary = widening_summary_held("copy", &at(1), &github, &at(2), &github);
    assert!(summary.contains("new.github.example"), "{summary}");
    use_test_presets(None);
}
