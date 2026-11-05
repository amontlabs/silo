//! Golden tests against the recorded validation manifests, and one test per rule.
//!
//! The fixtures in `fixtures/` are the recorded `t2` manifests (Linux `l1`..`l5`, `s1`,
//! `s2`; macOS steps; the live agent task) reduced to the records that differ between
//! the two sides, plus the records that give them context (`META`, `GIT`, `CRON`,
//! `PKGM`). Unchanged files and packages cannot affect a diff, so the output is the same
//! as for the full manifests.

use std::io::Read;

use super::diff::Change;
use super::*;

fn fixture(name: &str) -> Manifest {
    let path = format!("{}/src/changes/fixtures/{name}.tsv.gz", env!("CARGO_MANIFEST_DIR"));
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut text = String::new();
    flate2::read::GzDecoder::new(&bytes[..]).read_to_string(&mut text).unwrap();
    Manifest::parse(&text).unwrap()
}

fn golden(name: &str) -> Vec<String> {
    let d = diff(
        &fixture(&format!("{name}.parent")),
        &fixture(&format!("{name}.fork")),
        None,
        &IgnoreList::builtin(),
    );
    d.render_lines(15)
}

fn lines(s: &str) -> Vec<String> {
    s.lines().map(str::to_owned).collect()
}

fn m(text: &str) -> Manifest {
    Manifest::parse(&format!("META\tformat\t2\nMETA\thome\t/root\n{text}")).unwrap()
}

fn run(a: &str, b: &str) -> Diff {
    diff(&m(a), &m(b), None, &IgnoreList::builtin())
}

fn section<'a>(d: &'a Diff, bucket: &str) -> Vec<&'a str> {
    d.summary
        .iter()
        .find(|s| s.bucket == bucket)
        .map(|s| s.lines.iter().map(String::as_str).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------- golden: Linux

#[test]
fn golden_l1_endpoint_is_3_lines() {
    assert_eq!(
        golden("l1"),
        lines("Code\n  ~/app: 46cc034..e710c1a\nIgnored: 4 paths (caches, logs, build output)")
    );
}

#[test]
fn golden_l2_dependency_is_4_lines() {
    assert_eq!(
        golden("l2"),
        lines(
            "Code\n  ~/app: 46cc034..5ff6e65\n  ~/app: 2 dirty/untracked: routes.js, validate.js\n\
Ignored: 11 paths (caches, logs, build output)"
        )
    );
}

#[test]
fn golden_l3_cli_tools_is_7_lines() {
    // Same 7 lines as the prototype; the apt line now names the two manual installs and
    // folds libjq1 and libonig5 into "+2 dependencies" (was 4 packages with versions).
    assert_eq!(
        golden("l3"),
        lines(
            "Code\n  ~/app: 46cc034..8daf86c\nPackages\n  apt: jq, ripgrep (+2 dependencies)\n\
Settings & files\n  ~ ~/.bashrc\nIgnored: 5 paths (caches, logs, build output)"
        )
    );
}

#[test]
fn golden_l4_python_tooling_is_15_lines() {
    // Same 15 lines. Differences from the recorded output: the pip line shows 3 names and
    // a count instead of 6 names and "+8 more" (this manifest has no pip dependency data),
    // and /usr/local/lib/ counts 834 files, the same as before (certifi's cacert.pem is a
    // package file, not a credential).
    assert_eq!(
        golden("l4"),
        lines(
            "Code\n  ~/app: 46cc034..a5334d1\n  ~/app: 1 dirty/untracked: scripts-smoke.sh\n\
Packages\n  npm -g: typescript\n  pip: PySocks, Pygments, certifi (+11 more)\n\
Settings & files\n  + ~/.config/httpie/config.json\n\
Other changed\n  /usr/local/bin/ (9 files: +9)\n  /usr/local/lib/ (834 files: +834)\n\
\x20\x20+ /usr/local/share/man/man1/http.1\n  + /usr/local/share/man/man1/httpie.1\n  + /usr/local/share/man/man1/https.1\n\
Ignored: 794 paths (caches, logs, build output)"
        )
    );
}

#[test]
fn golden_l5_db_migration_is_9_lines() {
    // Unchanged: the manifest predates CRONF and DB records, so the crontab file and the
    // cron line both still show.
    assert_eq!(
        golden("l5"),
        lines(
            "Code\n  ~/app: 46cc034..85053a5\nSettings & files\n  ~ /etc/postgresql/16/main/postgresql.conf\n\
\x20\x20+ /var/spool/cron/crontabs/root\n  cron root: added\nNot kept\n  listening: :3001\n\
Ignored: 6 paths (caches, logs, build output)"
        )
    );
}

#[test]
fn golden_s1_openclaw_weather_is_8_lines() {
    assert_eq!(
        golden("s1"),
        lines(
            "Packages\n  apt: jq (+2 dependencies)\nSettings & files\n  + ~/.openclaw/openclaw.json\n\
\x20\x20+ ~/.openclaw/workspace/.clawhub/lock.json\n  + ~/.openclaw/workspace/skills/weather/SKILL.md\n\
\x20\x20+ ~/.openclaw/workspace/skills/weather/scripts/wx.sh\nIgnored: 2 paths (caches, logs, build output)"
        )
    );
}

#[test]
fn golden_s2_openclaw_pdf_is_11_lines_one_more_because_credentials_move_to_not_kept() {
    // Was 10 lines: ~/.openclaw/credentials/pdf-tools.json sat under Settings & files. It is
    // a credential, so Settings & files loses a line and Not kept (header + one line) is new.
    // The packages lines drop their versions.
    assert_eq!(
        golden("s2"),
        lines(
            "Packages\n  npm -g: pdf-parse\n  pip: pypdf\nSettings & files\n  + ~/.openclaw/openclaw.json\n\
\x20\x20+ ~/.openclaw/workspace/skills/pdf-tools/SKILL.md\nOther changed\n  /usr/local/lib/ (59 files: +59)\n\
Not kept\n  credentials: ~/.openclaw/credentials/ (1 file)\nIgnored: 81 paths (caches, logs, build output)"
        )
    );
}

// ---------------------------------------------------------------- golden: macOS and live

#[test]
fn golden_macos_brew_step_is_14_lines() {
    let l = golden("mac-m0-m1");
    assert_eq!(l.len(), 14);
    assert_eq!(l[0..2], lines("Packages\n  brew: jq, ripgrep (+2 dependencies)")[..]);
    assert!(l.iter().any(|x| x.starts_with("  processes: AUCrashHandlerService")));
    assert_eq!(l.last().unwrap(), "Ignored: 27 paths (caches, logs, build output)");
}

#[test]
fn golden_macos_launchagent_step_is_12_lines() {
    // The recorded manifests have no LAGENT records, so the plist still shows as a file.
    assert_eq!(
        golden("mac-m1-m2"),
        lines(
            "Code\n  new repo ~/code/dotfiles @ 53a2a53 (main)\nSettings & files\n  + ~/.zshrc\n\
\x20\x20+ ~/Library/LaunchAgents/dev.example.clean-shots.plist\n  ~/Library/Preferences/ (5 files: ~5)\n\
Other changed\n  + ~/bin/clean-shots\nNot kept\n  processes: ModelCatalogAgent, sysmond\n\
\x20\x20services: dev.example.clean-shots\nIgnored: 38 paths (caches, logs, build output)"
        )
    );
}

#[test]
fn golden_macos_python_editor_step_is_12_lines() {
    assert_eq!(
        golden("mac-m2-m3"),
        lines(
            "Code\n  new repo ~/code/api @ 7839bee (main)\n  ~/code/api: 1 dirty/untracked: app.py\n\
Packages\n  pip: ruff\nSettings & files\n  + ~/.gitconfig\n\
Other changed\n  + ~/Library/Application Support/Code/User/settings.json\n\
Not kept\n  processes: intelligenceplatformd, knowledgeconstructiond, ndoagent\n\
Ignored: 50 paths (caches, logs, build output)"
        )
    );
}

#[test]
fn golden_live_idle_agent_task_is_3_lines() {
    assert_eq!(
        golden("live"),
        lines("Code\n  ~/app: 5118402..21b644b\nIgnored: 8 paths (caches, logs, build output)")
    );
}

#[test]
fn golden_json_round_trips_and_is_complete() {
    let d = diff(&fixture("l4.parent"), &fixture("l4.fork"), None, &IgnoreList::builtin());
    let json = serde_json::to_string(&d).unwrap();
    let back: Diff = serde_json::from_str(&json).unwrap();
    assert_eq!(back, d);
    // the full list stays in the JSON although the text rolls it up
    assert_eq!(d.other.iter().filter(|c| c.path.starts_with("/usr/local/lib/")).count(), 834);
    assert!(d.render_lines(15).len() <= 15);
}

// ---------------------------------------------------------------- rules

#[test]
fn json_shape_is_stable() {
    let d = run("", "F\t/etc/a.conf\t1\t1\tx\n");
    let v = serde_json::to_value(&d).unwrap();
    let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(
        keys,
        ["code", "database", "format", "home", "ignored", "not_kept", "other", "packages", "repo_files", "settings", "summary"]
    );
    assert_eq!(v["format"], 1);
    assert_eq!(v["settings"]["files"][0], serde_json::json!({"path": "/etc/a.conf", "change": "added"}));
    assert_eq!(v["summary"][0], serde_json::json!({"bucket": "Settings & files", "lines": ["+ /etc/a.conf"]}));
    let mut nk: Vec<&str> = v["not_kept"].as_object().unwrap().keys().map(String::as_str).collect();
    nk.sort();
    assert_eq!(nk, ["credentials", "listening", "processes", "services"]);
}

#[test]
fn changed_files_in_a_repo_are_code_only() {
    let repo = "GIT\t/root/app\taaaaaaa1\tmain\t0\t0\n";
    let d = run(
        &format!("{repo}F\t/root/app/src/a.rs\t1\t1\tx\n"),
        &format!("{repo}F\t/root/app/src/a.rs\t2\t2\ty\nF\t/root/app/src/b.rs\t2\t2\ty\n"),
    );
    assert!(section(&d, "Other changed").is_empty());
    assert!(section(&d, "Settings & files").is_empty());
    assert_eq!(d.repo_files.len(), 2);
}

#[test]
fn gitignored_changes_show_under_code() {
    let git = "GIT\t/root/app\taaaaaaa1\tmain\t0\t0\n";
    let d = run(
        &format!("{git}GIG\t/root/app\t/root/app/.env\t5\t1\taaa\nGIG\t/root/app\t/root/app/.env.keep\t5\t1\tkkk\n"),
        &format!("{git}GIG\t/root/app\t/root/app/.env\t6\t2\tbbb\nGIG\t/root/app\t/root/app/.env.keep\t5\t1\tkkk\n"),
    );
    assert_eq!(section(&d, "Code"), ["~/app: .env changed (gitignored)"]);
    assert_eq!(d.code[0].gitignored[0].change, Change::Modified);
}

#[test]
fn gitignored_files_in_ignored_trees_and_credentials_are_not_code() {
    let git = "GIT\t/root/app\taaaaaaa1\tmain\t0\t0\n";
    let d = run(
        git,
        &format!(
            "{git}GIG\t/root/app\t/root/app/debug.log\t5\t1\taaa\nGIG\t/root/app\t/root/app/certs/dev.pem\t5\t1\tbbb\n"
        ),
    );
    assert!(section(&d, "Code").is_empty());
    assert_eq!(d.ignored.paths, ["/root/app/debug.log"]);
    assert_eq!(section(&d, "Not kept"), ["credentials: ~/app/certs/dev.pem"]);
}

#[test]
fn baseline_removes_matching_entries_only() {
    let a = "F\t/etc/motd\t1\t1\tx\nPKG\tdpkg\tcurl\t1\nSVC\tcron.service\tenabled\n";
    let b_idle = "F\t/etc/motd\t2\t2\ty\nPKG\tdpkg\tcurl\t2\nSVC\tcron.service\tdisabled\nPROC\tsshd\nLISTEN\ttcp\t22\tsshd\nP\t/root/.npm\t3\n";
    let baseline = run(a, b_idle);
    assert!(!baseline.is_empty());
    let b_task = format!(
        "{b_idle}F\t/etc/app.conf\t1\t1\tz\nPKG\tdpkg\tjq\t1\nPKGM\tdpkg\tjq\nPROC\tnode\nLISTEN\ttcp\t3000\tnode\nP\t/root/.npm\t9\n"
    );
    let task = diff(&m(a), &m(&b_task), Some(&baseline), &IgnoreList::builtin());
    assert_eq!(section(&task, "Settings & files"), ["+ /etc/app.conf"]);
    assert_eq!(section(&task, "Packages"), ["apt: jq"]);
    assert_eq!(section(&task, "Not kept"), ["listening: :3000 (node)", "processes: node"]);
    // the pruned-tree change differs (3 -> 9 vs 3), so it stays counted
    assert_eq!(task.ignored.count(), 6);
    // an identical run leaves nothing
    let none = diff(&m(a), &m(b_idle), Some(&baseline), &IgnoreList::builtin());
    assert!(none.is_empty());
    assert_eq!(none.render(15), "No changes");
}

#[test]
fn baseline_survives_a_json_round_trip() {
    let a = "GIT\t/root/app\taaaaaaa1\tmain\t0\t0\n";
    let b = "GIT\t/root/app\tbbbbbbb2\tmain\t0\t0\n";
    let baseline: Diff = serde_json::from_str(&serde_json::to_string(&run(a, b)).unwrap()).unwrap();
    assert!(diff(&m(a), &m(b), Some(&baseline), &IgnoreList::builtin()).is_empty());
}

#[test]
fn package_files_collapse_into_the_package() {
    let a = "PKG\tdpkg\tcurl\t1\n";
    let b = "PKG\tdpkg\tcurl\t1\nPKG\tdpkg\tjq\t1.7\nPKGM\tdpkg\tjq\nF\t/usr/bin/jq\t1\t1\ta\nF\t/usr/lib/libjq.so\t1\t1\tb\nF\t/usr/local/bin/mine\t1\t1\tc\n\
OWN\tdpkg\tjq\t/usr/bin/jq\nOWN\tdpkg\tjq\t/usr/lib/libjq.so\n";
    let d = run(a, b);
    assert_eq!(section(&d, "Packages"), ["apt: jq"]);
    assert_eq!(section(&d, "Other changed"), ["+ /usr/local/bin/mine"]);
    assert_eq!(d.packages[0].owned_files["jq"].len(), 2);
}

#[test]
fn owned_files_of_an_unchanged_package_are_named_in_the_package_line() {
    let a = "PKG\tdpkg\tnginx\t1\nF\t/etc/nginx/nginx.conf\t1\t1\ta\n";
    let b = "PKG\tdpkg\tnginx\t1\nF\t/etc/nginx/nginx.conf\t2\t2\tb\nOWN\tdpkg\tnginx\t/etc/nginx/nginx.conf\n";
    let d = run(a, b);
    assert_eq!(section(&d, "Packages"), ["apt: files changed in nginx (1 file)"]);
    assert!(section(&d, "Settings & files").is_empty());
}

#[test]
fn postgres_schema_and_table_changes_with_row_counts() {
    let a = "DB\tpostgres\tapp\tschema\ts1\nDBT\tpostgres\tapp\tusers\tt1\t10\nDBT\tpostgres\tapp\torders\to1\t4\nDBT\tpostgres\tapp\tsame\tx\t1\n";
    let b = "DB\tpostgres\tapp\tschema\ts2\nDBT\tpostgres\tapp\tusers\tt2\t12\nDBT\tpostgres\tapp\torders\to1\t4\nDBT\tpostgres\tapp\tsame\tx\t1\nDBT\tpostgres\tapp\taudit\ta1\t3\n";
    let d = run(a, b);
    assert_eq!(
        section(&d, "Database changed"),
        ["postgres app: schema changed; data changed in audit (new, 3 rows), users (10→12 rows)"]
    );
}

#[test]
fn postgres_database_added_and_dropped() {
    let d = run("", "DB\tpostgres\tnew\tschema\ts\nDBT\tpostgres\tnew\tt\td\t5\n");
    assert_eq!(section(&d, "Database changed"), ["postgres new: new database (1 table)"]);
    let d = run("DB\tpostgres\told\tschema\ts\n", "");
    assert_eq!(section(&d, "Database changed"), ["postgres old: dropped"]);
}

#[test]
fn sqlite_files_report_digests_and_not_as_other_changes() {
    let a = "DB\tsqlite\t/srv/dev.db\taaaaaaaaaa\nF\t/srv/dev.db\t1\t1\t-\n";
    let b = "DB\tsqlite\t/srv/dev.db\tbbbbbbbbbb\nF\t/srv/dev.db\t2\t2\t-\n";
    let d = run(a, b);
    assert_eq!(section(&d, "Database changed"), ["sqlite /srv/dev.db: changed (aaaaaaa→bbbbbbb)"]);
    assert!(section(&d, "Other changed").is_empty());
}

#[test]
fn credential_paths_go_to_not_kept() {
    let files = [
        "/root/.openclaw/credentials/pdf.json",
        "/root/.aws/credentials",
        "/root/.config/gh/hosts.yml",
        "/root/.ssh/id_ed25519",
        "/root/.ssh/id_ed25519.pub",
        "/srv/tls/site.pem",
        "/Users/a/Library/Keychains/login.keychain-db",
    ];
    let b: String = files.iter().map(|f| format!("F\t{f}\t1\t1\tx\n")).collect();
    let d = run("", &b);
    assert!(section(&d, "Settings & files").is_empty());
    assert!(section(&d, "Other changed").is_empty());
    assert_eq!(
        section(&d, "Not kept"),
        [
            "credentials: /Users/a/Library/Keychains/ (1 file), ~/.aws/credentials, ~/.config/gh/hosts.yml, \
~/.openclaw/credentials/ (1 file), ~/.ssh/ (2 files), /srv/tls/site.pem"
        ]
    );
}

#[test]
fn dirty_credential_in_a_repo_is_not_listed_as_code() {
    let git = "GIT\t/root/app\taaaaaaa1\tmain\t1\t0\n";
    let d = run(git, &format!("{git}GITF\t/root/app\t??\tid_rsa\nGITF\t/root/app\t_M\tsrc/a.rs\n"));
    assert_eq!(section(&d, "Code"), ["~/app: 1 dirty/untracked: a.rs"]);
    assert_eq!(section(&d, "Not kept"), ["credentials: ~/app/id_rsa"]);
}

#[test]
fn apt_transitive_dependencies_collapse_into_the_manual_parent() {
    let b = "PKG\tdpkg\tripgrep\t1\nPKG\tdpkg\tjq\t1\nPKG\tdpkg\tlibjq1\t1\nPKG\tdpkg\tlibonig5\t1\nPKG\tdpkg\tlibpcre2\t1\nPKGM\tdpkg\tripgrep\nPKGM\tdpkg\tjq\n";
    let d = run("", b);
    assert_eq!(section(&d, "Packages"), ["apt: jq, ripgrep (+3 dependencies)"]);
    assert_eq!(d.packages[0].dependencies, ["libjq1", "libonig5", "libpcre2"]);
}

#[test]
fn dependency_data_attributes_dependencies_per_parent() {
    let b = "PKG\tbrew\tjq\t1\nPKG\tbrew\toniguruma\t1\nPKG\tbrew\tripgrep\t1\nPKG\tbrew\tpcre2\t1\nPKGM\tbrew\tjq\nPKGM\tbrew\tripgrep\n\
PKGD\tbrew\tjq\toniguruma\nPKGD\tbrew\tripgrep\tpcre2\nPKGD\tbrew\toniguruma\t\n";
    let d = run("", b);
    assert_eq!(section(&d, "Packages"), ["brew: jq (+1 dependencies), ripgrep (+1 dependencies)"]);
}

#[test]
fn pip_without_dependency_data_collapses_beyond_three_with_a_count() {
    let b: String = ["a", "b", "c", "d", "e"].iter().map(|n| format!("PKG\tpip\t{n}\t1\n")).collect();
    let d = run("", &b);
    assert_eq!(section(&d, "Packages"), ["pip: a, b, c (+2 more)"]);
    assert_eq!(d.packages[0].top_level, None);
}

#[test]
fn pip_with_dependency_data_keeps_the_packages_nothing_requires() {
    let b = "PKG\tpip\thttpie\t1\nPKG\tpip\trequests\t1\nPKG\tpip\turllib3\t1\nPKG\tpip\tcertifi\t1\nPKG\tpip\tpygments\t1\n\
PKGD\tpip\thttpie\trequests,pygments\nPKGD\tpip\trequests\turllib3,certifi\nPKGD\tpip\turllib3\t\n";
    let d = run("", b);
    assert_eq!(section(&d, "Packages"), ["pip: httpie (+4 dependencies)"]);
    assert_eq!(d.packages[0].top_level.as_deref(), Some(&["httpie".to_owned()][..]));
}

#[test]
fn upgrades_and_removals_are_listed() {
    let d = run("PKG\tnpm\tx\t1\nPKG\tnpm\ty\t1\n", "PKG\tnpm\tx\t2\n");
    assert_eq!(section(&d, "Packages"), ["npm -g: upgraded x 1→2; removed y"]);
}

#[test]
fn crontab_file_and_cron_line_are_one_change() {
    let path = "/var/spool/cron/crontabs/root";
    let a = "CRON\troot\t-\n";
    let b = format!("CRON\troot\tabc\nCRONF\troot\t{path}\nF\t{path}\t10\t1\tdef\n");
    let d = run(a, &b);
    assert_eq!(section(&d, "Settings & files"), ["cron root: added"]);
}

#[test]
fn launch_agent_file_and_record_are_one_change() {
    let plist = "/Users/a/Library/LaunchAgents/dev.x.plist";
    let b = format!("LAGENT\tuser\tdev.x\tabc\t{plist}\nF\t{plist}\t10\t1\tdef\n");
    let d = diff(
        &Manifest::parse("META\thome\t/Users/a\n").unwrap(),
        &Manifest::parse(&format!("META\thome\t/Users/a\n{b}")).unwrap(),
        None,
        &IgnoreList::builtin(),
    );
    assert_eq!(section(&d, "Settings & files"), ["launch agent dev.x (user): added"]);
}

#[test]
fn rollup_collapses_more_than_three_files_per_directory() {
    let three: String = (0..3).map(|i| format!("F\t/etc/app/{i}.conf\t1\t1\tx\n")).collect();
    assert_eq!(section(&run("", &three), "Settings & files").len(), 3);
    let four: String = (0..4).map(|i| format!("F\t/etc/app/{i}.conf\t1\t1\tx\n")).collect();
    assert_eq!(section(&run("", &four), "Settings & files"), ["/etc/app/ (4 files: +4)"]);
}

#[test]
fn ignored_paths_are_counted_not_listed() {
    let d = run("", "F\t/var/log/syslog\t1\t1\tx\nF\t/root/.bash_history\t1\t1\ty\nP\t/root/.cache\t5\n");
    assert_eq!(d.ignored.count(), 7);
    assert_eq!(d.render(15), "Ignored: 7 paths (caches, logs, build output)");
}

#[test]
fn render_limit_hides_items_behind_a_more_line() {
    let files: String = (0..9).map(|i| format!("F\t/etc/d{i}/a.conf\t1\t1\tx\n")).collect();
    let d = run("", &files);
    let l = d.render_lines(5);
    assert_eq!(l, lines("Settings & files\n  + /etc/d0/a.conf\n  + /etc/d1/a.conf\n  + /etc/d2/a.conf\n  … +6 more (see --json)"));
}

#[test]
fn typical_task_fits_in_fifteen_lines() {
    for n in ["l1", "l2", "l3", "l4", "l5", "s1", "mac-m1-m2", "mac-m2-m3", "live"] {
        assert!(golden(n).len() <= 15, "{n}");
    }
}
