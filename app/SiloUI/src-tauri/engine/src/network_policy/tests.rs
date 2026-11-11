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
    let policy = allowlist(&["b.example.com", "a.example.com", "b.example.com"], &["pypi", "npm", "npm"]);
    let stored = validate(&policy).unwrap();
    assert_eq!(stored.hosts, ["a.example.com", "b.example.com"]);
    assert_eq!(stored.presets, ["npm", "pypi"]);
    assert!(validate(&allowlist(&[], &["no-such-preset"])).is_err());
    assert!(validate(&allowlist(&["1.1.1.1"], &[])).is_err());
    let many: Vec<String> = (0..=MAX_HOSTS).map(|n| format!("h{n}.example.com")).collect();
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
    let names: Vec<&str> = presets().iter().map(|preset| preset.name.as_str()).collect();
    for expected in ["npm", "pypi", "crates", "ubuntu", "homebrew", "github", "anthropic", "openai"] {
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
        assert!(expanded.contains(&Allowed::new(host, port)), "{host}:{port}");
    }
    assert!(!expanded.iter().any(|allowed| allowed.host == "registry.npmjs.org"));
    assert!(expand(&mode(NetworkPolicyMode::Open)).is_empty());
    assert!(load_presets(r#"{"version":1,"presets":[{"name":"x","title":"X","hosts":["1.2.3.4"]}]}"#).is_err());
    assert!(load_presets(r#"{"version":1,"presets":[{"name":"x","title":"X","hosts":[]}]}"#).is_err());
    assert!(load_presets(r#"{"version":1,"presets":[{"name":"x","title":"X","hosts":["a.com"]},{"name":"x","title":"Y","hosts":["b.com"]}]}"#).is_err());
    assert!(load_presets(r#"{"version":2,"presets":[]}"#).is_err());
    let listed = preset_list();
    let ubuntu = listed.iter().find(|preset| preset.name == "ubuntu").unwrap();
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
            "--net-default-egress", "deny", "--net-default-ingress", "allow",
            "--net-rule", "allow@dns", "--net-rule", "allow@public",
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
            "--net-default-egress", "deny", "--net-default-ingress", "allow",
            "--net-rule", "allow@dns", "--net-rule", "allow@host:tcp:47311",
            "--net-rule", "allow@public", "--net-strict=true",
        ])
    );

    let list = plan(&allowlist(&["example.com", "*.python.org"], &[]), &none, &[ExtraRule::GatewayPort(47311)]);
    assert_eq!(
        list.create_args(),
        args(&[
            "--net-default-egress", "deny", "--net-default-ingress", "allow",
            "--net-rule", "allow@dns", "--net-rule", "allow@example.com:tcp:443",
            "--net-rule", "allow@*.python.org:tcp:443", "--net-rule", "allow@host:tcp:47311",
            "--net-strict=false",
        ])
    );
    assert!(!list.strict);

    let closed = plan(&mode(NetworkPolicyMode::Closed), &none, &[]);
    assert_eq!(
        closed.create_args(),
        args(&[
            "--net-default-egress", "deny", "--net-default-ingress", "allow",
            "--net-strict=true",
        ])
    );
    let closed_channel = plan(&mode(NetworkPolicyMode::Closed), &none, &[ExtraRule::GatewayPort(47311)]);
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
    let list = plan(&allowlist(&["example.com", "*.python.org"], &[]), &Grants::default(), &[]);
    let rules = list.policy_value()["rules"].clone();
    assert_eq!(rules[1]["destination"], json!({"domain": "example.com"}));
    assert_eq!(rules[2]["destination"], json!({"domain_suffix": "python.org"}));
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
            ("WILD".into(), vec!["*.example.net".into(), "10.0.0.1".into()]),
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
        assert!(tokens.contains(&token.to_owned()), "{token} missing from {tokens:?}");
    }
    assert!(!tokens.iter().any(|token| token.contains("10.0.0.1") || token == "allow@*:tcp:443"));
    assert!(list.hosts.iter().any(|host| host.host == "github.com" && host.from == "github"));
    assert!(list.hosts.iter().any(|host| host.host == "api.openai.com" && host.from == "secret:OPENAI_KEY"));
    // A host the list and a grant both name is one rule.
    let twice = plan(&allowlist(&["api.openai.com"], &[]), &grants, &[]);
    assert_eq!(
        twice.rules.iter().filter(|rule| rule.token() == "allow@api.openai.com:tcp:443").count(),
        1
    );
    for other in [NetworkPolicyMode::Open, NetworkPolicyMode::Closed] {
        let plan = plan(&mode(other), &grants, &[]);
        assert!(plan.hosts.is_empty());
        assert!(!plan.rules.iter().any(|rule| matches!(rule, Rule::Domain { .. })));
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
    let ubuntu_https_only = allowlist(&["*.archive.ubuntu.com", "security.ubuntu.com", "ports.ubuntu.com"], &[]);
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
    assert!(!no_wider(&sibling, &wild), "a suffix covers only names under it");
}

#[test]
fn a_widening_summary_names_the_hosts_added() {
    let current = allowlist(&["a.example.com"], &[]);
    let new = allowlist(&["a.example.com", "b.example.com"], &["pypi"]);
    let summary = widening_summary("main", &current, &new);
    assert!(summary.starts_with("Widen the network of main"), "{summary}");
    for host in ["b.example.com", "pypi.org", "files.pythonhosted.org"] {
        assert!(summary.contains(host), "{summary}");
    }
    assert!(!summary.contains("a.example.com"), "{summary}");
    let summary = widening_summary("main", &mode(NetworkPolicyMode::Closed), &mode(NetworkPolicyMode::Open));
    assert_eq!(summary, "Change the network of main from closed to open (any public address).");
    let summary = widening_summary("main", &mode(NetworkPolicyMode::Closed), &allowlist(&[], &["ubuntu"]));
    assert!(summary.contains("ports.ubuntu.com (port 80)"), "{summary}");
    let many: Vec<String> = (0..30).map(|n| format!("h{n}.example.com")).collect();
    let refs: Vec<&str> = many.iter().map(String::as_str).collect();
    let summary = widening_summary("main", &mode(NetworkPolicyMode::Closed), &allowlist(&refs, &[]));
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
    let plan = plan_for("imported", "imported", Some(&imported));
    assert_eq!(plan.mode, NetworkPolicyMode::Closed);
    assert_eq!(plan.policy_value(), imported);
    assert!(plan.rule_args().windows(2).any(|pair| pair == ["--net-default-ingress", "deny"]));
    // Once the owner sets closed, published ports answer again.
    store::set("imported", &mode(NetworkPolicyMode::Closed)).unwrap();
    let plan = plan_for("imported", "imported", Some(&imported));
    assert_eq!(plan.policy_value()["default_ingress"], "allow");
    // A computer saved before policies existed stays open.
    let github_default: Value =
        serde_json::from_str(include_str!("../../../guest/github-network-default.json")).unwrap();
    let plan = plan_for("old", "old", Some(&github_default["policy"]));
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
            plan_for("a", "a", None),
            plan_for("b", "b", None),
            plan_for("c", "c", None),
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
        tokens.verify(&parent_token, "child").is_none(),
        "a copied token proves nothing"
    );
    assert!(tokens.status("child").unwrap().unwrap().hash.is_empty());
    assert_eq!(
        plan_for("child", "child", None).rules,
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
    let plan = plan_for("imported", "imported", Some(&imported));
    assert_eq!(plan.rules, vec![Rule::GatewayPort(port)]);
    assert_eq!(plan.policy_value()["default_ingress"], "deny");
    crate::guest_client::computer_removed("imported");
    assert!(extra_rules("imported").is_empty());
    crate::guest_client::tokens::use_test_directory(None);
    store::use_test_path(None);
}
