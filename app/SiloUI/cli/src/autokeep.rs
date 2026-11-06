//! `auto_keep` rules: keeping a fork's changes without asking when a rule covers every change
//! and the fork's tests pass. Matching is plain functions over the diff; [`attempt`] adds the
//! engine calls (the diff, the tests, the keep).

use std::collections::BTreeSet;

use serde_json::{json, Value};

use crate::{
    config::{AutoKeepRule, Bucket, Recipe, Settings, TestsRequirement},
    engine::{Engine, Progress},
    error::CliError,
    flows, folder,
    ops::{self, DiffArgs, ExecArgs, FoldOptions},
};

/// What a fork changed, reduced to what rules look at.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
    pub buckets: BTreeSet<Bucket>,
    /// A database changed in a way a migrate step cannot cover (one side could not be read).
    pub uncompared_data: bool,
    /// Things a keep leaves behind that a person should decide about: credential paths and
    /// service changes. Listening ports and processes are not among them.
    pub left_behind: Vec<String>,
    /// The digest a keep is pinned to; without one nothing can be kept without asking.
    pub digest: String,
}

fn filled(value: &Value) -> bool {
    match value {
        Value::Array(items) => !items.is_empty(),
        Value::Object(items) => !items.is_empty(),
        Value::Null => false,
        _ => true,
    }
}

impl Changes {
    /// Reads the diff document `silo diff --json` prints.
    pub fn from_diff(diff: &Value) -> Self {
        let mut buckets = BTreeSet::new();
        let mut note = |bucket: Bucket, present: bool| {
            if present {
                buckets.insert(bucket);
            }
        };
        note(Bucket::Code, filled(&diff["code"]));
        note(
            Bucket::Packages,
            diff["packages"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|entry| {
                    ["added", "removed", "upgraded"]
                        .iter()
                        .any(|key| filled(&entry[*key]))
                }),
        );
        note(
            Bucket::Settings,
            ["files", "cron", "services", "launch_agents"]
                .iter()
                .any(|key| filled(&diff["settings"][*key])),
        );
        note(Bucket::Database, filled(&diff["database"]));
        note(Bucket::Other, filled(&diff["other"]));
        let uncompared_data = diff["database"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|entry| filled(&entry["not_compared"]));
        let mut left_behind = Vec::new();
        if let Some(credentials) = diff["not_kept"]["credentials"].as_object() {
            left_behind.extend(credentials.keys().map(|path| format!("credentials {path}")));
        }
        if let Some(services) = diff["not_kept"]["services"].as_array() {
            left_behind.extend(
                services
                    .iter()
                    .map(|service| format!("service {}", service.as_str().unwrap_or("changed"))),
            );
        }
        Self {
            buckets,
            uncompared_data,
            left_behind,
            digest: diff["reviewDigest"].as_str().unwrap_or_default().to_owned(),
        }
    }

    /// A fork made from a host folder only ever has code changes.
    pub fn code(digest: String) -> Self {
        Self {
            buckets: BTreeSet::from([Bucket::Code]),
            digest,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Nothing changed; there is nothing to keep or to ask about.
    Nothing,
    /// Keep without asking: the index of the rule that matched.
    Keep(usize),
    /// Leave a review request, for this reason.
    Ask(String),
}

fn names(buckets: &BTreeSet<Bucket>) -> String {
    buckets
        .iter()
        .map(|bucket| bucket.name())
        .collect::<Vec<_>>()
        .join(", ")
}

fn covers(rule: &AutoKeepRule, changes: &Changes) -> bool {
    changes
        .buckets
        .iter()
        .all(|bucket| rule.when.only.contains(bucket))
}

/// Why no rule may apply whatever it says, or `None`.
fn forbidden(changes: &Changes, migrates: bool) -> Option<String> {
    if changes.digest.is_empty() {
        return Some("the changes have no review digest to pin a keep to".into());
    }
    if changes.buckets.contains(&Bucket::Database) && (!migrates || changes.uncompared_data) {
        return Some("database changes that no migrate step covers".into());
    }
    if !changes.left_behind.is_empty() {
        return Some(format!("not kept: {}", changes.left_behind.join(", ")));
    }
    None
}

/// Whether deciding needs the test result: some rule covers the changes and asks for tests.
pub fn needs_tests(rules: &[AutoKeepRule], changes: &Changes, migrates: bool) -> bool {
    !changes.buckets.is_empty()
        && forbidden(changes, migrates).is_none()
        && rules
            .iter()
            .any(|rule| covers(rule, changes) && rule.when.tests.is_some())
}

/// The first rule that covers every change and whose tests passed (`tests` is the result of
/// the project's test command, `None` when it was not run), unless the changes are ones no
/// rule may keep: an uncovered database change, something a keep leaves behind that
/// matters, or no digest to pin to.
pub fn decide(
    rules: &[AutoKeepRule],
    changes: &Changes,
    migrates: bool,
    tests: Option<bool>,
) -> Decision {
    if changes.buckets.is_empty() {
        return Decision::Nothing;
    }
    if let Some(reason) = forbidden(changes, migrates) {
        return Decision::Ask(reason);
    }
    let mut reason = format!(
        "no auto_keep rule covers the changes in {}",
        names(&changes.buckets)
    );
    for (index, rule) in rules.iter().enumerate() {
        if !covers(rule, changes) {
            continue;
        }
        match (rule.when.tests, tests) {
            (None, _) => return Decision::Keep(index),
            (Some(TestsRequirement::Pass), Some(true)) => return Decision::Keep(index),
            (Some(TestsRequirement::Pass), Some(false)) => reason = "the tests failed".into(),
            (Some(TestsRequirement::Pass), None) => {
                reason = "the rule needs tests, and the recipe has no test: command".into();
            }
        }
    }
    Decision::Ask(reason)
}

/// What an attempt came to.
pub enum Outcome {
    /// Kept without asking; carries the keep result with `autoKeep` added and the digest the
    /// keep was pinned to.
    Kept(Value, String),
    /// Nothing to keep.
    Nothing,
    /// Not kept, for this reason; the caller leaves a review request.
    Declined(String),
}

/// Runs the project's test command in the fork; true when it exits 0.
async fn run_tests(engine: &Engine, fork: &str, command: &str, cwd: Option<&str>) -> bool {
    let args = ExecArgs {
        computer: fork.to_owned(),
        argv: vec!["sh".into(), "-c".into(), command.to_owned()],
        cwd: cwd.map(str::to_owned),
        timeout_secs: ops::MAX_EXEC_SECS,
    };
    matches!(ops::exec_raw(engine, &args).await, Ok(output) if output.process_exit_code() == 0)
}

/// Keeps `fork` without asking when an `auto_keep` rule of `settings` covers every change.
/// The keep is pinned to the digest of the diff just taken, so a fork that moves meanwhile
/// is refused and left for review instead.
pub async fn attempt(
    engine: &Engine,
    fork: &str,
    into: Option<&str>,
    settings: Option<&Settings>,
    recipe: Option<&Recipe>,
    progress: Progress<'_>,
) -> Result<Outcome, CliError> {
    let rules = Settings::auto_keep_rules(settings);
    if rules.is_empty() {
        return Ok(Outcome::Declined("no auto_keep rule is set".into()));
    }
    let computer = engine.computer(fork).await?;
    let record = folder::record_for(engine, &computer.id);
    let (changes, cwd) = match &record {
        Some(record) => {
            let changed = folder::has_changes(engine, &computer, record).await?;
            let digest = folder::review_digest(engine, &computer, record).await?;
            let changes = if changed {
                Changes::code(digest)
            } else {
                Changes::default()
            };
            (changes, Some(record.guest_path.clone()))
        }
        None => {
            let seen = ops::diff(
                engine,
                &DiffArgs {
                    computer: computer.name.clone(),
                    max_lines: None,
                    no_baseline: false,
                    compute_baseline: false,
                },
                progress,
            )
            .await?;
            (Changes::from_diff(&seen), None)
        }
    };
    let migrates = recipe.is_some_and(|recipe| !recipe.keep.migrate.is_empty());
    let tests = match recipe.and_then(|recipe| recipe.test.as_deref()) {
        Some(command) if needs_tests(rules, &changes, migrates) => {
            progress(&format!("Running the tests in {}", computer.name));
            Some(run_tests(engine, &computer.name, command, cwd.as_deref()).await)
        }
        _ => None,
    };
    match decide(rules, &changes, migrates, tests) {
        Decision::Nothing => Ok(Outcome::Nothing),
        Decision::Ask(reason) => Ok(Outcome::Declined(reason)),
        Decision::Keep(index) => {
            let rule = rules[index].describe();
            let mut kept = flows::fold(
                engine,
                fork,
                into,
                ops::keep_hooks(recipe),
                FoldOptions {
                    dry_run: false,
                    data_ok: false,
                    reviewed: Some(changes.digest.clone()).filter(|digest| !digest.is_empty()),
                },
                false,
                progress,
            )
            .await?;
            if kept["status"] != "kept" {
                let reason = kept["refusal"]["message"]
                    .as_str()
                    .unwrap_or("the keep did not go ahead");
                return Ok(Outcome::Declined(format!("the keep was refused: {reason}")));
            }
            kept["autoKeep"] = json!({ "rule": rule, "tests": tests });
            Ok(Outcome::Kept(kept, changes.digest.clone()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{parse_settings, AutoKeepWhen};

    fn rule(only: &[Bucket], tests: bool) -> AutoKeepRule {
        AutoKeepRule {
            when: AutoKeepWhen {
                only: only.to_vec(),
                tests: tests.then_some(TestsRequirement::Pass),
            },
        }
    }

    fn changes(buckets: &[Bucket]) -> Changes {
        Changes {
            buckets: buckets.iter().copied().collect(),
            digest: "d".into(),
            ..Changes::default()
        }
    }

    #[test]
    fn a_rule_covers_changes_only_in_the_buckets_it_lists() {
        let rules = [rule(&[Bucket::Code], false)];
        assert_eq!(
            decide(&rules, &changes(&[Bucket::Code]), false, None),
            Decision::Keep(0)
        );
        for other in [Bucket::Packages, Bucket::Settings, Bucket::Other] {
            assert!(matches!(
                decide(&rules, &changes(&[Bucket::Code, other]), false, None),
                Decision::Ask(_)
            ));
            assert!(matches!(
                decide(&rules, &changes(&[other]), false, None),
                Decision::Ask(_)
            ));
        }
    }

    #[test]
    fn each_bucket_can_be_allowed() {
        for bucket in [
            Bucket::Code,
            Bucket::Packages,
            Bucket::Settings,
            Bucket::Other,
        ] {
            let rules = [rule(&[bucket], false)];
            assert_eq!(
                decide(&rules, &changes(&[bucket]), false, None),
                Decision::Keep(0)
            );
        }
        // A database change needs a migrate step as well as a rule that lists it.
        let rules = [rule(&[Bucket::Database], false)];
        let data = changes(&[Bucket::Database]);
        assert!(matches!(
            decide(&rules, &data, false, None),
            Decision::Ask(_)
        ));
        assert_eq!(decide(&rules, &data, true, None), Decision::Keep(0));
        let uncompared = Changes {
            uncompared_data: true,
            ..data
        };
        assert!(matches!(
            decide(&rules, &uncompared, true, None),
            Decision::Ask(_)
        ));
    }

    #[test]
    fn tests_must_pass_when_a_rule_asks() {
        let rules = [rule(&[Bucket::Code], true)];
        let code = changes(&[Bucket::Code]);
        assert!(needs_tests(&rules, &code, false));
        assert_eq!(decide(&rules, &code, false, Some(true)), Decision::Keep(0));
        assert_eq!(
            decide(&rules, &code, false, Some(false)),
            Decision::Ask("the tests failed".into())
        );
        assert!(
            matches!(decide(&rules, &code, false, None), Decision::Ask(reason) if reason.contains("test:"))
        );
        // A rule without tests is not held back by the ones that ask.
        let both = [
            rule(&[Bucket::Code], true),
            rule(&[Bucket::Code, Bucket::Other], false),
        ];
        assert_eq!(decide(&both, &code, false, Some(false)), Decision::Keep(1));
        assert!(!needs_tests(&[rule(&[Bucket::Code], false)], &code, false));
        assert!(!needs_tests(&rules, &changes(&[Bucket::Other]), false));
    }

    #[test]
    fn an_empty_diff_has_nothing_to_keep() {
        let rules = [rule(&[Bucket::Code], true)];
        assert_eq!(
            decide(&rules, &Changes::default(), false, None),
            Decision::Nothing
        );
        assert_eq!(
            decide(&rules, &changes(&[]), false, Some(true)),
            Decision::Nothing
        );
    }

    #[test]
    fn what_a_keep_leaves_behind_or_a_missing_digest_always_asks() {
        let rules = [rule(&[Bucket::Code], false)];
        let credentials = Changes {
            left_behind: vec!["credentials /root/.ssh/".into()],
            ..changes(&[Bucket::Code])
        };
        assert!(
            matches!(decide(&rules, &credentials, false, None), Decision::Ask(reason) if reason.contains(".ssh"))
        );
        let stale = Changes {
            digest: String::new(),
            ..changes(&[Bucket::Code])
        };
        assert!(matches!(
            decide(&rules, &stale, false, None),
            Decision::Ask(_)
        ));
        assert!(matches!(
            decide(&[], &changes(&[Bucket::Code]), false, None),
            Decision::Ask(_)
        ));
    }

    #[test]
    fn changes_are_read_from_the_diff_document() {
        let diff = json!({
            "code": [{"repo": "/root/app"}],
            "packages": [{"manager": "dpkg", "added": {"jq": "1"}, "removed": {}, "upgraded": {}}],
            "settings": {"files": [], "cron": [], "services": [], "launch_agents": []},
            "database": [],
            "other": [],
            "not_kept": {"credentials": {"/root/.ssh/": ["/root/.ssh/id"]}, "processes": ["node"], "services": []},
            "reviewDigest": "abc"
        });
        let found = Changes::from_diff(&diff);
        assert_eq!(
            found.buckets,
            BTreeSet::from([Bucket::Code, Bucket::Packages])
        );
        assert_eq!(found.left_behind, ["credentials /root/.ssh/"]);
        assert_eq!(found.digest, "abc");
        let quiet = Changes::from_diff(&json!({
            "settings": {"files": [{"path": "/etc/a"}]},
            "other": [{"path": "/usr/local/bin/x"}],
            "database": [{"name": "app", "not_compared": "unreachable"}],
            "not_kept": {"listening": [{"port": "3000"}], "processes": ["node"]},
            "reviewDigest": "d"
        }));
        assert_eq!(
            quiet.buckets,
            BTreeSet::from([Bucket::Settings, Bucket::Database, Bucket::Other])
        );
        assert!(quiet.uncompared_data && quiet.left_behind.is_empty());
        assert!(Changes::from_diff(&json!({"summary": []}))
            .buckets
            .is_empty());
    }

    #[test]
    fn auto_code_stands_for_the_code_only_rule() {
        let settings = Settings::resolve(Some(parse_settings("fold: auto-code\n").unwrap()), None);
        assert_eq!(
            Settings::auto_keep_rules(settings.as_ref()),
            [AutoKeepRule::auto_code()]
        );
        assert_eq!(
            AutoKeepRule::auto_code().describe(),
            "when: { only: [code], tests: pass }"
        );
    }
}
