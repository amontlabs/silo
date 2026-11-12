//! A computer's network policy: `open` (any public address), `allowlist` (named hosts only) or
//! `closed` (nothing out), set by the owner and stored per computer ([`store`]).
//!
//! The policy is turned into the runtime's own rules ([`Plan`]): `--net-rule` arguments for a
//! Linux virtual machine and the saved-policy shape for the gateway of a macOS or container
//! computer. In allowlist mode the engine also lets through the hosts the computer needs for
//! what the owner already granted (its GitHub access and its secrets), and every mode carries
//! the rules Silo needs for itself ([`extra_rules`]).
//!
//! Hostname rules for hosts no secret covers work only with the runtime's strict mode off: under
//! strict, a TLS connection the runtime does not intercept is refused when only a hostname rule
//! admitted it. An allowlist computer therefore runs with strict off; the cost is in the threat
//! model (a host sharing an address with an allowed host may be reachable).

pub mod service;
pub mod store;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::{json, Value};
pub use silo_api::methods::network::{
    NetworkPolicy, NetworkPolicyHost, NetworkPolicyMode, NetworkPreset, NetworkPresetUpdate,
};
use silo_api::plain_text_within;

/// At most this many hosts on a computer's own list.
pub const MAX_HOSTS: usize = 256;
/// At most this many presets on a computer's policy.
pub const MAX_PRESETS: usize = 32;
/// The longest host a summary shows whole.
const HOST_SHOWN_MAX_CHARS: usize = 80;
/// The TCP port an owner's host entry allows.
pub const HTTPS_PORT: u16 = 443;
/// Top-level names that never leave the device or that the runtime answers itself
/// (`host.microsandbox.internal` is the gateway).
const RESERVED_TOP_LEVEL: [&str; 10] = [
    "internal",
    "local",
    "localhost",
    "arpa",
    "invalid",
    "test",
    "onion",
    "lan",
    "home",
    "localdomain",
];

/// One host pattern and one TCP port it may be reached on.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Allowed {
    pub host: String,
    pub port: u16,
}

impl Allowed {
    fn new(host: &str, port: u16) -> Self {
        Self {
            host: host.to_owned(),
            port,
        }
    }

    /// The name a `*.` pattern covers (the pattern without `*.`), or the exact name.
    fn base(&self) -> &str {
        self.host.strip_prefix("*.").unwrap_or(&self.host)
    }

    fn is_wildcard(&self) -> bool {
        self.host.starts_with("*.")
    }

    /// Whether every connection `other` admits is admitted by `self`. A `*.` pattern covers its
    /// base name too, as the runtime's suffix rule does.
    pub fn covers(&self, other: &Allowed) -> bool {
        if self.port != other.port {
            return false;
        }
        if !self.is_wildcard() {
            return !other.is_wildcard() && self.host == other.host;
        }
        let (base, theirs) = (self.base(), other.base());
        theirs == base || theirs.ends_with(&format!(".{base}"))
    }
}

/// The preset version a policy saved before presets had versions was approved at.
const FIRST_PRESET_VERSION: u32 = 1;

/// The version of each preset a policy was approved at, by preset name. A preset without an
/// entry was approved at the first version.
pub type Pins = BTreeMap<String, u32>;

/// A named host list from `presets.json`.
#[derive(Clone, Debug)]
pub struct Preset {
    pub name: String,
    pub title: String,
    /// Raised whenever the host list changes in a way that can add a host.
    pub version: u32,
    pub hosts: Vec<Allowed>,
    /// The host lists of earlier versions, kept for as long as a saved policy may have been
    /// approved at one.
    pub history: Vec<(u32, Vec<Allowed>)>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PresetFile {
    version: u8,
    presets: Vec<PresetEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PresetEntry {
    name: String,
    title: String,
    version: u32,
    hosts: Vec<PresetHost>,
    #[serde(default)]
    history: Vec<PresetHistory>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PresetHistory {
    version: u32,
    hosts: Vec<PresetHost>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum PresetHost {
    Host(String),
    WithPorts { host: String, ports: Vec<u16> },
}

fn preset_hosts(name: &str, listed: Vec<PresetHost>) -> Result<Vec<Allowed>, String> {
    let mut hosts = Vec::new();
    for host in listed {
        let (host, ports) = match host {
            PresetHost::Host(host) => (host, vec![HTTPS_PORT]),
            PresetHost::WithPorts { host, ports } => (host, ports),
        };
        validate_host(&host)?;
        if ports.is_empty() || ports.contains(&0) {
            return Err(format!("preset {name} lists {host} without a port"));
        }
        hosts.extend(ports.into_iter().map(|port| Allowed::new(&host, port)));
    }
    if hosts.is_empty() {
        return Err(format!("preset {name} has no hosts"));
    }
    Ok(hosts)
}

fn load_presets(text: &str) -> Result<Vec<Preset>, String> {
    let file: PresetFile = serde_json::from_str(text).map_err(|error| error.to_string())?;
    if file.version != 2 {
        return Err("unknown preset file version".into());
    }
    let mut names = BTreeSet::new();
    file.presets
        .into_iter()
        .map(|entry| {
            if !names.insert(entry.name.clone())
                || entry.name.is_empty()
                || !entry
                    .name
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            {
                return Err(format!("preset name {:?} is invalid or repeated", entry.name));
            }
            if entry.version < FIRST_PRESET_VERSION {
                return Err(format!("preset {} has no version", entry.name));
            }
            let mut history = Vec::new();
            for earlier in entry.history {
                if earlier.version >= entry.version
                    || history.iter().any(|(version, _)| *version == earlier.version)
                {
                    return Err(format!(
                        "preset {} lists version {} twice or after its own",
                        entry.name, earlier.version
                    ));
                }
                history.push((earlier.version, preset_hosts(&entry.name, earlier.hosts)?));
            }
            Ok(Preset {
                hosts: preset_hosts(&entry.name, entry.hosts)?,
                name: entry.name,
                title: entry.title,
                version: entry.version,
                history,
            })
        })
        .collect()
}

#[cfg(test)]
thread_local! {
    static TEST_PRESETS: std::cell::RefCell<Option<&'static [Preset]>> = const { std::cell::RefCell::new(None) };
}

/// Tests on this thread see `presets` instead of the data file.
#[cfg(test)]
pub(crate) fn use_test_presets(presets: Option<Vec<Preset>>) {
    TEST_PRESETS.with(|test| {
        *test.borrow_mut() = presets.map(|presets| &*Box::leak(presets.into_boxed_slice()));
    });
}

/// The presets the engine knows, from `presets.json`.
pub fn presets() -> &'static [Preset] {
    #[cfg(test)]
    if let Some(presets) = TEST_PRESETS.with(|test| *test.borrow()) {
        return presets;
    }
    static PRESETS: OnceLock<Vec<Preset>> = OnceLock::new();
    PRESETS.get_or_init(|| {
        load_presets(include_str!("presets.json")).expect("checked-in network presets are valid")
    })
}

pub fn preset(name: &str) -> Option<&'static Preset> {
    presets().iter().find(|preset| preset.name == name)
}

impl Preset {
    /// The hosts a policy approved at `pin` gets from this preset, and the hosts the running
    /// Silo's version adds that it does not get. The approved list stays in force until the
    /// owner accepts the update; a version this Silo does not know allows nothing.
    fn hosts_at(&self, pin: Option<u32>) -> (Vec<Allowed>, Vec<Allowed>) {
        let pin = pin.unwrap_or(FIRST_PRESET_VERSION);
        if pin == self.version {
            return (self.hosts.clone(), Vec::new());
        }
        let approved = self
            .history
            .iter()
            .find(|(version, _)| *version == pin)
            .map(|(_, hosts)| hosts.as_slice());
        let Some(approved) = approved else {
            return (Vec::new(), Vec::new());
        };
        let approved: BTreeSet<Allowed> = approved.iter().cloned().collect();
        let current: BTreeSet<Allowed> = self.hosts.iter().cloned().collect();
        let added = uncovered(&current, &approved);
        if added.is_empty() {
            (self.hosts.clone(), Vec::new())
        } else {
            (approved.into_iter().collect(), added)
        }
    }
}

/// A policy with the preset versions it was approved at.
#[derive(Clone, Debug, PartialEq)]
pub struct Held {
    pub policy: NetworkPolicy,
    pub pins: Pins,
}

impl Held {
    /// `policy` approved now: its presets at the versions this Silo has.
    pub fn latest(policy: NetworkPolicy) -> Self {
        let pins = policy
            .presets
            .iter()
            .filter_map(|name| preset(name).map(|preset| (name.clone(), preset.version)))
            .collect();
        Self { policy, pins }
    }

    /// The hosts of every preset of the policy as approved, and per preset the hosts an update
    /// would add: `(name, title, added)`.
    fn preset_hosts(&self) -> (Vec<(String, Vec<Allowed>)>, Vec<(String, String, Vec<Allowed>)>) {
        let mut hosts = Vec::new();
        let mut updates = Vec::new();
        for name in &self.policy.presets {
            if let Some(preset) = preset(name) {
                let (approved, added) = preset.hosts_at(self.pins.get(name).copied());
                hosts.push((name.clone(), approved));
                if !added.is_empty() {
                    updates.push((name.clone(), preset.title.clone(), added));
                }
            }
        }
        (hosts, updates)
    }

    /// The presets whose running version has hosts this policy was not approved with.
    pub fn preset_updates(&self) -> Vec<NetworkPresetUpdate> {
        self.preset_hosts()
            .1
            .into_iter()
            .map(|(name, title, added)| NetworkPresetUpdate {
                name,
                title,
                added_hosts: added
                    .iter()
                    .map(|allowed| allowed.host.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
            })
            .collect()
    }
}

/// The presets as the API lists them.
pub fn preset_list() -> Vec<NetworkPreset> {
    presets()
        .iter()
        .map(|preset| NetworkPreset {
            name: preset.name.clone(),
            title: preset.title.clone(),
            hosts: preset
                .hosts
                .iter()
                .map(|allowed| allowed.host.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
        })
        .collect()
}

/// Checks one host pattern: a lowercase DNS name of at least two labels, or `*.` and such a
/// name of at least two labels. No ports, no addresses, no bare top-level names, and no names
/// that stay on the device.
pub fn validate_host(host: &str) -> Result<(), String> {
    let refuse = |why: &str| Err(format!("{host:?} is not a host Silo can allow: {why}."));
    let name = host.strip_prefix("*.").unwrap_or(host);
    if host.is_empty() || host.len() > 253 {
        return refuse("a host name has 1 to 253 characters");
    }
    if name.contains('*') {
        return refuse("only one leading `*.` is allowed");
    }
    if host.contains(':') {
        return refuse("give a name without a port; port 443 is allowed");
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'.')
    {
        return refuse("use lowercase letters, digits, hyphens and dots only");
    }
    let labels: Vec<&str> = name.split('.').collect();
    if labels.iter().any(|label| {
        label.is_empty() || label.len() > 63 || label.starts_with('-') || label.ends_with('-')
    }) {
        return refuse("each part between dots has 1 to 63 characters and does not start or end with a hyphen");
    }
    if labels.len() < 2 {
        return refuse("a top-level name alone would allow a whole domain");
    }
    let top = labels[labels.len() - 1];
    if top.bytes().all(|byte| byte.is_ascii_digit()) {
        return refuse("addresses are not allowed, only names");
    }
    if RESERVED_TOP_LEVEL.contains(&top) {
        return refuse("that name stays on this device or its network");
    }
    if host.starts_with("*.") && is_public_suffix(name) {
        return refuse("a name under which anyone can get a host would allow all of them");
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SuffixFile {
    version: u8,
    #[allow(dead_code)]
    note: String,
    suffixes: Vec<String>,
}

/// The common multi-label public suffixes from `public_suffixes.json`: not the full public
/// suffix list.
fn public_suffixes() -> &'static BTreeSet<String> {
    static SUFFIXES: OnceLock<BTreeSet<String>> = OnceLock::new();
    SUFFIXES.get_or_init(|| {
        let file: SuffixFile = serde_json::from_str(include_str!("public_suffixes.json"))
            .expect("checked-in public suffixes are valid");
        assert_eq!(file.version, 1, "unknown public suffix file version");
        file.suffixes.into_iter().collect()
    })
}

/// Whether `name` is one of the listed public suffixes.
fn is_public_suffix(name: &str) -> bool {
    public_suffixes().contains(name)
}

/// Whether `host` is a `*.` pattern a network rule refuses for being too wide, whatever else is
/// wrong with it.
pub fn is_too_wide_wildcard(host: &str) -> bool {
    host.strip_prefix("*.")
        .is_some_and(|name| !name.contains('.') || is_public_suffix(name))
}

/// Checks a policy and returns it in its stored form: hosts and presets sorted, each once.
pub fn validate(policy: &NetworkPolicy) -> Result<NetworkPolicy, String> {
    if policy.hosts.len() > MAX_HOSTS {
        return Err(format!("A network policy lists at most {MAX_HOSTS} hosts."));
    }
    if policy.presets.len() > MAX_PRESETS {
        return Err(format!("A network policy uses at most {MAX_PRESETS} presets."));
    }
    for host in &policy.hosts {
        validate_host(host)?;
    }
    for name in &policy.presets {
        if preset(name).is_none() {
            return Err(format!(
                "{name:?} is not a network preset. Known presets: {}.",
                presets()
                    .iter()
                    .map(|preset| preset.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    let hosts: BTreeSet<String> = policy.hosts.iter().cloned().collect();
    let presets: BTreeSet<String> = policy.presets.iter().cloned().collect();
    Ok(NetworkPolicy {
        mode: policy.mode,
        hosts: hosts.into_iter().collect(),
        presets: presets.into_iter().collect(),
    })
}

/// What an allowlist policy lets through by itself: its own hosts on port 443 and its presets'
/// hosts. Empty for the other modes. Presets are taken at the latest version.
pub fn expand(policy: &NetworkPolicy) -> BTreeSet<Allowed> {
    expand_held(&Held::latest(policy.clone()))
}

/// What an allowlist policy lets through by itself, with each preset at the version the policy
/// was approved at.
pub fn expand_held(held: &Held) -> BTreeSet<Allowed> {
    if held.policy.mode != NetworkPolicyMode::Allowlist {
        return BTreeSet::new();
    }
    let mut allowed: BTreeSet<Allowed> = held
        .policy
        .hosts
        .iter()
        .map(|host| Allowed::new(host, HTTPS_PORT))
        .collect();
    for (_, hosts) in held.preset_hosts().0 {
        allowed.extend(hosts);
    }
    allowed
}

/// What a policy keeps a computer from reaching among some hosts, each over https.
#[derive(Debug, PartialEq, Eq)]
pub enum Reach {
    /// All of them can be reached.
    All,
    /// Nothing can be reached: the policy is closed.
    Nothing,
    /// These cannot: the policy is an allowlist that does not cover them.
    Missing(Vec<String>),
}

/// Whether `held` lets a computer reach `hosts`. The hosts its grants need are not counted.
pub fn reach_of(held: &Held, hosts: &[&str]) -> Reach {
    match held.policy.mode {
        NetworkPolicyMode::Open => Reach::All,
        NetworkPolicyMode::Closed => Reach::Nothing,
        NetworkPolicyMode::Allowlist => {
            let allowed = expand_held(held);
            let missing: Vec<String> = hosts
                .iter()
                .filter(|host| {
                    let wanted = Allowed::new(host, HTTPS_PORT);
                    !allowed.iter().any(|have| have.covers(&wanted))
                })
                .map(|host| (*host).to_owned())
                .collect();
            if missing.is_empty() {
                Reach::All
            } else {
                Reach::Missing(missing)
            }
        }
    }
}

/// Entries of `new` that nothing in `current` covers.
fn uncovered(new: &BTreeSet<Allowed>, current: &BTreeSet<Allowed>) -> Vec<Allowed> {
    new.iter()
        .filter(|entry| !current.iter().any(|have| have.covers(entry)))
        .cloned()
        .collect()
}

/// Whether `new` lets a computer reach nothing that `current` does not. Closed is the
/// narrowest, open the widest; an allowlist is no wider than another when every host it allows
/// is covered by the other. An allowlist is wider than closed: it resolves names.
pub fn no_wider(new: &NetworkPolicy, current: &NetworkPolicy) -> bool {
    no_wider_held(
        &Held::latest(new.clone()),
        &Held::latest(current.clone()),
    )
}

/// [`no_wider`] with the preset versions each policy was approved at.
pub fn no_wider_held(new: &Held, current: &Held) -> bool {
    use NetworkPolicyMode::*;
    match (new.policy.mode, current.policy.mode) {
        (Closed, _) | (_, Open) => true,
        (Open, _) | (Allowlist, Closed) => false,
        (Allowlist, Allowlist) => uncovered(&expand_held(new), &expand_held(current)).is_empty(),
    }
}

fn mode_words(mode: NetworkPolicyMode) -> &'static str {
    match mode {
        NetworkPolicyMode::Open => "open (any public address)",
        NetworkPolicyMode::Allowlist => "a list of allowed hosts",
        NetworkPolicyMode::Closed => "closed",
    }
}

/// A host as a summary shows it: plain and bounded.
fn shown_host(host: &str) -> String {
    plain_text_within(host, HOST_SHOWN_MAX_CHARS)
}

fn host_words(allowed: &[Allowed]) -> String {
    const SHOWN: usize = 20;
    let mut names: Vec<String> = allowed
        .iter()
        .map(|entry| {
            if entry.port == HTTPS_PORT {
                shown_host(&entry.host)
            } else {
                format!("{} (port {})", shown_host(&entry.host), entry.port)
            }
        })
        .collect();
    let more = names.len().saturating_sub(SHOWN);
    names.truncate(SHOWN);
    let mut text = names.join(", ");
    if more > 0 {
        text.push_str(&format!(" and {more} more"));
    }
    text
}

/// The summary of a request to widen `computer`'s policy from `current` to `new`, written from
/// the two policies: the mode change and every host that is added.
pub fn widening_summary(computer: &str, current: &NetworkPolicy, new: &NetworkPolicy) -> String {
    widening_summary_held(
        computer,
        &Held::latest(current.clone()),
        &Held::latest(new.clone()),
    )
}

/// [`widening_summary`] with the preset versions each policy was approved at.
pub fn widening_summary_held(computer: &str, current: &Held, new: &Held) -> String {
    let mut summary = if new.policy.mode == current.policy.mode {
        format!("Widen the network of {computer}")
    } else {
        format!(
            "Change the network of {computer} from {} to {}",
            mode_words(current.policy.mode),
            mode_words(new.policy.mode)
        )
    };
    if new.policy.mode == NetworkPolicyMode::Allowlist {
        let added = uncovered(&expand_held(new), &expand_held(current));
        if !added.is_empty() {
            summary.push_str(&format!(", adding {}", host_words(&added)));
        } else if current.policy.mode == NetworkPolicyMode::Closed {
            summary.push_str(", which lets it look up names");
        }
    }
    summary.push('.');
    summary
}

/// What a computer was granted that its allowlist must let through.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Grants {
    pub github: bool,
    /// Each assigned secret's name and the hosts it may be sent to.
    pub secrets: Vec<(String, Vec<String>)>,
}

#[cfg(test)]
thread_local! {
    static TEST_GRANTS: std::cell::RefCell<Option<Grants>> = const { std::cell::RefCell::new(None) };
}

/// Tests on this thread see `grants` for every computer instead of the GitHub and secret stores.
#[cfg(test)]
pub(crate) fn use_test_grants(grants: Option<Grants>) {
    TEST_GRANTS.with(|test| *test.borrow_mut() = grants);
}

impl Grants {
    /// The grants of the computer named `name`, read from the GitHub and secret stores.
    pub fn of_computer(name: &str) -> Self {
        #[cfg(test)]
        if let Some(grants) = TEST_GRANTS.with(|test| test.borrow().clone()) {
            return grants;
        }
        let access = crate::policy::access::of_computer(name);
        Self {
            github: access.github.is_some(),
            secrets: crate::secrets::assigned_hosts(name).unwrap_or_default(),
        }
    }

    /// The secrets' wildcard hosts too wide to be a network rule (over a public suffix), as
    /// `(secret, host)`. The secret still works for those hosts; the computer just cannot reach
    /// them by it.
    pub fn too_wide(&self) -> Vec<(String, String)> {
        self.secrets
            .iter()
            .flat_map(|(name, domains)| {
                domains
                    .iter()
                    .filter(|domain| is_too_wide_wildcard(domain))
                    .map(|domain| (name.clone(), domain.clone()))
            })
            .collect()
    }

    /// The hosts these grants add, with where each comes from. A secret allowed to any host
    /// adds nothing: an allowlist computer can send it only to hosts on its list. A wildcard
    /// over a public suffix adds nothing either ([`Self::too_wide`]).
    fn hosts(&self) -> Vec<(Allowed, String)> {
        let mut hosts = Vec::new();
        if self.github {
            if let Some(preset) = preset("github") {
                hosts.extend(
                    preset
                        .hosts
                        .iter()
                        .map(|allowed| (allowed.clone(), "github".to_owned())),
                );
            }
        }
        for (name, domains) in &self.secrets {
            for domain in domains {
                if validate_host(domain).is_ok() {
                    hosts.push((Allowed::new(domain, HTTPS_PORT), format!("secret:{name}")));
                }
            }
        }
        hosts
    }
}

/// A rule the engine adds for Silo itself in every mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtraRule {
    /// One TCP port on the gateway address (`allow@host:tcp:<port>`), which the runtime maps to
    /// the device's loopback.
    GatewayPort(u16),
}

/// The rules Silo needs for itself in `computer`'s network, in every mode: the port of its
/// channel to Silo's tools when they are on, and nothing else. Only the computer's own port.
pub fn extra_rules(computer_id: &str) -> Vec<ExtraRule> {
    crate::guest_client::TokenStore::current()
        .and_then(|store| store.port_of(computer_id))
        .map(ExtraRule::GatewayPort)
        .into_iter()
        .collect()
}

/// One rule in the runtime's terms.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rule {
    /// Name lookups through the gateway's resolver (host port 53, UDP and TCP).
    Dns,
    /// Every public address, any port.
    Public,
    /// An exact host name on one TCP port.
    Domain { host: String, port: u16 },
    /// A name and every name under it, on one TCP port.
    Suffix { base: String, port: u16 },
    /// One TCP port on the gateway address.
    GatewayPort(u16),
}

impl Rule {
    fn from_allowed(allowed: &Allowed) -> Self {
        if allowed.is_wildcard() {
            Rule::Suffix {
                base: allowed.base().to_owned(),
                port: allowed.port,
            }
        } else {
            Rule::Domain {
                host: allowed.host.clone(),
                port: allowed.port,
            }
        }
    }

    /// The `--net-rule` token.
    pub fn token(&self) -> String {
        match self {
            Rule::Dns => "allow@dns".into(),
            Rule::Public => "allow@public".into(),
            Rule::Domain { host, port } => format!("allow@{host}:tcp:{port}"),
            Rule::Suffix { base, port } => format!("allow@*.{base}:tcp:{port}"),
            Rule::GatewayPort(port) => format!("allow@host:tcp:{port}"),
        }
    }

    /// The rule as the runtime saves it.
    pub fn saved(&self) -> Value {
        let tcp_port = |destination: Value, port: u16| {
            json!({
                "action": "allow",
                "destination": destination,
                "direction": "egress",
                "ports": [{"start": port, "end": port}],
                "protocols": ["tcp"],
            })
        };
        match self {
            Rule::Dns => json!({
                "action": "allow",
                "destination": {"group": "host"},
                "direction": "egress",
                "ports": [{"start": 53, "end": 53}],
                "protocols": ["udp", "tcp"],
            }),
            Rule::Public => json!({
                "action": "allow",
                "destination": {"group": "public"},
                "direction": "egress",
                "ports": [],
                "protocols": [],
            }),
            Rule::Domain { host, port } => tcp_port(json!({"domain": host}), *port),
            Rule::Suffix { base, port } => tcp_port(json!({"domain_suffix": base}), *port),
            Rule::GatewayPort(port) => tcp_port(json!({"group": "host"}), *port),
        }
    }
}

/// A computer's network in the runtime's terms: egress denied by default, ingress allowed (so
/// the ports the owner publishes still answer), the rules, and the strict setting.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub mode: NetworkPolicyMode,
    /// Whether connections to the computer's published ports are let in. An import, and the
    /// copy an export makes, keep the deny-all they start with until the owner sets a policy.
    pub ingress_allowed: bool,
    pub rules: Vec<Rule>,
    pub strict: bool,
    /// Every host an allowlist lets through, with where it comes from.
    pub hosts: Vec<NetworkPolicyHost>,
}

/// Builds the runtime rules for `policy`. Open: name lookups, Silo's rules, every public
/// address. Allowlist: name lookups, each allowed host, Silo's rules, with strict off. Closed:
/// Silo's rules only, so not even name lookups.
pub fn plan(policy: &NetworkPolicy, grants: &Grants, extras: &[ExtraRule]) -> Plan {
    plan_held(&Held::latest(policy.clone()), grants, extras)
}

/// [`plan`] with each preset at the version the policy was approved at.
pub fn plan_held(held: &Held, grants: &Grants, extras: &[ExtraRule]) -> Plan {
    let policy = &held.policy;
    let extra = extras.iter().map(|rule| match rule {
        ExtraRule::GatewayPort(port) => Rule::GatewayPort(*port),
    });
    match policy.mode {
        NetworkPolicyMode::Open => Plan {
            mode: policy.mode,
            ingress_allowed: true,
            rules: std::iter::once(Rule::Dns)
                .chain(extra)
                .chain(std::iter::once(Rule::Public))
                .collect(),
            strict: true,
            hosts: Vec::new(),
        },
        NetworkPolicyMode::Closed => Plan {
            mode: policy.mode,
            ingress_allowed: true,
            rules: extra.collect(),
            strict: true,
            hosts: Vec::new(),
        },
        NetworkPolicyMode::Allowlist => {
            let mut sources: Vec<(Allowed, String)> = Vec::new();
            for host in &policy.hosts {
                sources.push((Allowed::new(host, HTTPS_PORT), "list".into()));
            }
            for (name, hosts) in held.preset_hosts().0 {
                sources.extend(
                    hosts
                        .into_iter()
                        .map(|allowed| (allowed, format!("preset:{name}"))),
                );
            }
            sources.extend(grants.hosts());
            let mut seen = BTreeSet::new();
            let mut rules = vec![Rule::Dns];
            let mut hosts: Vec<NetworkPolicyHost> = Vec::new();
            for (allowed, from) in sources {
                if seen.insert(allowed.clone()) {
                    rules.push(Rule::from_allowed(&allowed));
                }
                match hosts
                    .iter_mut()
                    .find(|entry| entry.host == allowed.host && entry.from == from)
                {
                    Some(entry) if !entry.ports.contains(&allowed.port) => {
                        entry.ports.push(allowed.port)
                    }
                    Some(_) => {}
                    None => hosts.push(NetworkPolicyHost {
                        host: allowed.host.clone(),
                        ports: vec![allowed.port],
                        from,
                    }),
                }
            }
            rules.extend(extra);
            Plan {
                mode: policy.mode,
                ingress_allowed: true,
                rules,
                strict: false,
                hosts,
            }
        }
    }
}

impl Plan {
    fn ingress(&self) -> &'static str {
        if self.ingress_allowed {
            "allow"
        } else {
            "deny"
        }
    }

    /// The policy as the runtime saves it (`config.network.policy`).
    pub fn policy_value(&self) -> Value {
        json!({
            "default_egress": "deny",
            "default_ingress": self.ingress(),
            "rules": self.rules.iter().map(Rule::saved).collect::<Vec<_>>(),
        })
    }

    /// `--net-default-*` and every `--net-rule`, for `msb create` and `msb restore`.
    pub fn rule_args(&self) -> Vec<String> {
        let mut args: Vec<String> = vec![
            "--net-default-egress".into(),
            "deny".into(),
            "--net-default-ingress".into(),
            self.ingress().into(),
        ];
        for rule in &self.rules {
            args.extend(["--net-rule".into(), rule.token()]);
        }
        args
    }

    pub fn strict_arg(&self) -> String {
        format!("--net-strict={}", self.strict)
    }

    /// The network arguments of `msb create`. The runtime's own default is exactly the open
    /// rules, so an open computer with nothing extra passes only the strict setting, as Silo
    /// always has.
    pub fn create_args(&self) -> Vec<String> {
        let mut args = if self.rules == [Rule::Dns, Rule::Public] && self.ingress_allowed {
            Vec::new()
        } else {
            self.rule_args()
        };
        args.push(self.strict_arg());
        args
    }

    /// Whether a runtime's `config.network` holds exactly these rules and this strict setting.
    pub fn matches(&self, network: &Value) -> bool {
        let strict = network
            .get("strict")
            .map_or(Some(true), Value::as_bool)
            .unwrap_or(!self.strict);
        strict == self.strict
            && network
                .get("policy")
                .is_some_and(|policy| same_policy(policy, &self.policy_value()))
    }
}

/// Whether two saved policies admit the same traffic in the same order: defaults, then each
/// rule's action, direction, destination, protocols and ports, ignoring the order of protocols
/// and of ports within a rule and any other field.
pub fn same_policy(left: &Value, right: &Value) -> bool {
    fn normalized(policy: &Value) -> Option<Value> {
        let rules = policy.get("rules")?.as_array()?;
        let rules = rules
            .iter()
            .map(|rule| {
                let mut protocols: Vec<String> = rule
                    .get("protocols")?
                    .as_array()?
                    .iter()
                    .map(|value| value.as_str().map(str::to_owned))
                    .collect::<Option<_>>()?;
                protocols.sort();
                let mut ports: Vec<(u64, u64)> = rule
                    .get("ports")?
                    .as_array()?
                    .iter()
                    .map(|port| Some((port.get("start")?.as_u64()?, port.get("end")?.as_u64()?)))
                    .collect::<Option<_>>()?;
                ports.sort();
                Some(json!({
                    "action": rule.get("action")?,
                    "direction": rule.get("direction")?,
                    "destination": rule.get("destination")?,
                    "protocols": protocols,
                    "ports": ports,
                }))
            })
            .collect::<Option<Vec<_>>>()?;
        Some(json!({
            "default_egress": policy.get("default_egress")?,
            "default_ingress": policy.get("default_ingress")?,
            "rules": rules,
        }))
    }
    match (normalized(left), normalized(right)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

/// Whether a saved runtime policy is closed in Silo's sense: egress denied and no rule that
/// allows name lookups or public addresses. An import carries such a policy before it first
/// starts.
pub fn is_closed_runtime_policy(policy: &Value) -> bool {
    policy.get("default_egress").and_then(Value::as_str) == Some("deny")
        && policy
            .get("rules")
            .and_then(Value::as_array)
            .is_some_and(|rules| {
                rules.iter().all(|rule| {
                    rule.get("action").and_then(Value::as_str) != Some("allow")
                        || rule.get("direction").and_then(Value::as_str) == Some("ingress")
                        || (rule.pointer("/destination/group").and_then(Value::as_str)
                            == Some("host")
                            && !rule
                                .get("ports")
                                .and_then(Value::as_array)
                                .is_some_and(|ports| {
                                    ports.iter().any(|port| port["start"].as_u64() == Some(53))
                                }))
                })
            })
}

/// The policy a computer runs with: the stored one, or for a computer saved before policies
/// existed, closed when its recorded runtime policy is closed (an import that has not started
/// yet) and open otherwise.
pub fn effective(stored: Option<NetworkPolicy>, recorded: Option<&Value>) -> NetworkPolicy {
    stored.unwrap_or_else(|| {
        if recorded.is_some_and(is_closed_runtime_policy) {
            NetworkPolicy {
                mode: NetworkPolicyMode::Closed,
                ..NetworkPolicy::default()
            }
        } else {
            NetworkPolicy::default()
        }
    })
}

/// What a computer runs with and the preset versions it was approved at: the stored policy,
/// else what the runtime's recorded rules mean (see [`effective`]). The error is a stored
/// policy that cannot be read: the computer must not start on a guess.
pub fn held_for(
    computer_id: &str,
    recorded: Option<&Value>,
) -> Result<(Held, bool), String> {
    match store::get_stored(computer_id)? {
        Some(stored) => Ok((stored.held(), stored.incoming_denied)),
        None => Ok(held_from_recorded(recorded)),
    }
}

/// What the runtime's recorded rules mean for a computer with no stored policy (see
/// [`effective`]), and whether its incoming connections are denied.
pub fn held_from_recorded(recorded: Option<&Value>) -> (Held, bool) {
    let policy = effective(None, recorded);
    // An import saved before policies were stored keeps the deny-all it was imported with
    // until the owner sets one.
    let denied = policy.mode == NetworkPolicyMode::Closed
        && recorded
            .and_then(|recorded| recorded.get("default_ingress"))
            .and_then(Value::as_str)
            == Some("deny");
    (Held::latest(policy), denied)
}

/// The plan for a computer: its stored policy (see [`held_for`]), its grants and Silo's own
/// rules. Refused when the stored policies cannot be read.
pub fn plan_for(
    computer_id: &str,
    computer_name: &str,
    recorded: Option<&Value>,
) -> Result<Plan, String> {
    let (held, incoming_denied) = held_for(computer_id, recorded)?;
    let mut plan = plan_held(
        &held,
        &Grants::of_computer(computer_name),
        &extra_rules(computer_id),
    );
    if incoming_denied {
        plan.ingress_allowed = false;
    }
    Ok(plan)
}

/// [`plan_for`] for a computer behind a gateway, whose published ports are always let in.
pub fn plan_for_gateway(computer_id: &str, computer_name: &str) -> Result<Plan, String> {
    plan_for(computer_id, computer_name, None).map(|mut plan| {
        plan.ingress_allowed = true;
        plan
    })
}

/// The policy as a macOS or container computer's gateway takes it at start, with its strict
/// setting.
pub fn gateway_policy(plan: &Plan) -> (Value, bool) {
    (plan.policy_value(), plan.strict)
}

/// Forgets a removed computer's policy.
pub fn computer_removed(computer_id: &str) {
    if let Err(error) = store::remove(computer_id) {
        eprintln!("silo-engine: {error}");
    }
}
