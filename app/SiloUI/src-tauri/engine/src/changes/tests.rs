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
    let path = format!(
        "{}/src/changes/fixtures/{name}.tsv.gz",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut text = String::new();
    flate2::read::GzDecoder::new(&bytes[..])
        .read_to_string(&mut text)
        .unwrap();
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
Other changed\n  + /usr/local/share/man/man1/http.1\n  + /usr/local/share/man/man1/httpie.1\n  + /usr/local/share/man/man1/https.1\n\
\x20\x20/usr/local/bin/ (9 files: +9)\n  /usr/local/lib/ (834 files: +834)\n\
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
fn golden_macos_brew_step_is_11_lines() {
    let l = golden("mac-m0-m1");
    assert_eq!(l.len(), 11);
    assert_eq!(
        l[0..2],
        lines("Packages\n  brew: jq, ripgrep (+2 dependencies)")[..]
    );
    assert!(l
        .iter()
        .any(|x| x.starts_with("  processes: AUCrashHandlerService")));
    assert_eq!(
        l.last().unwrap(),
        "Ignored: 57 paths (caches, logs, build output)"
    );
}

#[test]
fn golden_macos_launchagent_step_is_12_lines() {
    // The recorded manifests have no LAGENT records, so the plist still shows as a file. The
    // five `~/Library/Preferences` files that used to roll up here are macOS churn, now ignored.
    assert_eq!(
        golden("mac-m1-m2"),
        lines(
            "Code\n  new repo ~/code/dotfiles @ 53a2a53 (main)\nSettings & files\n  + ~/.zshrc\n\
\x20\x20+ ~/Library/LaunchAgents/dev.example.clean-shots.plist\n\
Other changed\n  + ~/bin/clean-shots\nNot kept\n  processes: ModelCatalogAgent, sysmond\n\
\x20\x20services: dev.example.clean-shots\nIgnored: 43 paths (caches, logs, build output)"
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
    let d = diff(
        &fixture("l4.parent"),
        &fixture("l4.fork"),
        None,
        &IgnoreList::builtin(),
    );
    let json = serde_json::to_string(&d).unwrap();
    let back: Diff = serde_json::from_str(&json).unwrap();
    assert_eq!(back, d);
    // the full list stays in the JSON although the text rolls it up
    assert_eq!(
        d.other
            .iter()
            .filter(|c| c.path.starts_with("/usr/local/lib/"))
            .count(),
        834
    );
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
        [
            "code",
            "database",
            "format",
            "home",
            "idle_noise",
            "ignored",
            "not_kept",
            "other",
            "packages",
            "repo_changes",
            "repo_files",
            "settings",
            "summary"
        ]
    );
    assert_eq!(v["format"], 1);
    assert_eq!(
        v["settings"]["files"][0],
        serde_json::json!({
            "id": "file:/etc/a.conf",
            "path": "/etc/a.conf",
            "change": "added",
            "before": null,
            "after": {"kind": "file", "size": "1", "hash": "x"}
        })
    );
    assert_eq!(
        v["summary"][0],
        serde_json::json!({"bucket": "Settings & files", "lines": ["+ /etc/a.conf"]})
    );
    let mut nk: Vec<&str> = v["not_kept"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    nk.sort();
    assert_eq!(
        nk,
        [
            "credential_details",
            "credentials",
            "listening",
            "processes",
            "services"
        ]
    );
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
    assert_eq!(
        section(&d, "Not kept"),
        ["credentials: ~/app/certs/dev.pem"]
    );
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
    assert_eq!(
        section(&task, "Not kept"),
        ["listening: :3000 (node)", "processes: node"]
    );
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
    assert_eq!(
        section(&d, "Packages"),
        ["apt: files changed in nginx (1 file)"]
    );
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
    let d = run(
        "",
        "DB\tpostgres\tnew\tschema\ts\nDBT\tpostgres\tnew\tt\td\t5\n",
    );
    assert_eq!(
        section(&d, "Database changed"),
        ["postgres new: new database (1 table)"]
    );
    let d = run("DB\tpostgres\told\tschema\ts\n", "");
    assert_eq!(section(&d, "Database changed"), ["postgres old: dropped"]);
}

#[test]
fn sqlite_files_report_digests_and_not_as_other_changes() {
    let a = "DB\tsqlite\t/srv/dev.db\taaaaaaaaaa\nF\t/srv/dev.db\t1\t1\t-\n";
    let b = "DB\tsqlite\t/srv/dev.db\tbbbbbbbbbb\nF\t/srv/dev.db\t2\t2\t-\n";
    let d = run(a, b);
    assert_eq!(
        section(&d, "Database changed"),
        ["sqlite /srv/dev.db: changed (aaaaaaa→bbbbbbb)"]
    );
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
    let d = run(
        git,
        &format!("{git}GITF\t/root/app\t??\tid_rsa\nGITF\t/root/app\t_M\tsrc/a.rs\n"),
    );
    assert_eq!(section(&d, "Code"), ["~/app: 1 dirty/untracked: a.rs"]);
    assert_eq!(section(&d, "Not kept"), ["credentials: ~/app/id_rsa"]);
}

#[test]
fn apt_transitive_dependencies_collapse_into_the_manual_parent() {
    let b = "PKG\tdpkg\tripgrep\t1\nPKG\tdpkg\tjq\t1\nPKG\tdpkg\tlibjq1\t1\nPKG\tdpkg\tlibonig5\t1\nPKG\tdpkg\tlibpcre2\t1\nPKGM\tdpkg\tripgrep\nPKGM\tdpkg\tjq\n";
    let d = run("", b);
    assert_eq!(
        section(&d, "Packages"),
        ["apt: jq, ripgrep (+3 dependencies)"]
    );
    assert_eq!(
        d.packages[0].dependencies,
        ["libjq1", "libonig5", "libpcre2"]
    );
}

#[test]
fn dependency_data_attributes_dependencies_per_parent() {
    let b = "PKG\tbrew\tjq\t1\nPKG\tbrew\toniguruma\t1\nPKG\tbrew\tripgrep\t1\nPKG\tbrew\tpcre2\t1\nPKGM\tbrew\tjq\nPKGM\tbrew\tripgrep\n\
PKGD\tbrew\tjq\toniguruma\nPKGD\tbrew\tripgrep\tpcre2\nPKGD\tbrew\toniguruma\t\n";
    let d = run("", b);
    assert_eq!(
        section(&d, "Packages"),
        ["brew: jq (+1 dependencies), ripgrep (+1 dependencies)"]
    );
}

#[test]
fn pip_without_dependency_data_collapses_beyond_three_with_a_count() {
    let b: String = ["a", "b", "c", "d", "e"]
        .iter()
        .map(|n| format!("PKG\tpip\t{n}\t1\n"))
        .collect();
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
    assert_eq!(
        d.packages[0].top_level.as_deref(),
        Some(&["httpie".to_owned()][..])
    );
}

#[test]
fn upgrades_and_removals_are_listed() {
    let d = run("PKG\tnpm\tx\t1\nPKG\tnpm\ty\t1\n", "PKG\tnpm\tx\t2\n");
    assert_eq!(
        section(&d, "Packages"),
        ["npm -g: upgraded x 1→2; removed y"]
    );
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
    assert_eq!(
        section(&d, "Settings & files"),
        ["launch agent dev.x (user): added"]
    );
}

#[test]
fn rollup_collapses_more_than_three_files_per_directory() {
    let three: String = (0..3)
        .map(|i| format!("F\t/etc/app/{i}.conf\t1\t1\tx\n"))
        .collect();
    assert_eq!(section(&run("", &three), "Settings & files").len(), 3);
    let four: String = (0..4)
        .map(|i| format!("F\t/etc/app/{i}.conf\t1\t1\tx\n"))
        .collect();
    assert_eq!(
        section(&run("", &four), "Settings & files"),
        ["/etc/app/ (4 files: +4)"]
    );
}

#[test]
fn ignored_paths_are_counted_not_listed() {
    let d = run(
        "",
        "F\t/var/log/syslog\t1\t1\tx\nF\t/root/.bash_history\t1\t1\ty\nP\t/root/.cache\t5\n",
    );
    assert_eq!(d.ignored.count(), 7);
    assert_eq!(
        d.render(15),
        "Ignored: 7 paths (caches, logs, build output)"
    );
}

#[test]
fn render_limit_hides_items_behind_a_more_line() {
    let files: String = (0..9)
        .map(|i| format!("F\t/etc/d{i}/a.conf\t1\t1\tx\n"))
        .collect();
    let d = run("", &files);
    let l = d.render_lines(5);
    assert_eq!(l, lines("Settings & files\n  + /etc/d0/a.conf\n  + /etc/d1/a.conf\n  + /etc/d2/a.conf\n  … +6 more (see --json)"));
}

#[test]
fn typical_task_fits_in_fifteen_lines() {
    for n in [
        "l1",
        "l2",
        "l3",
        "l4",
        "l5",
        "s1",
        "mac-m1-m2",
        "mac-m2-m3",
        "live",
    ] {
        assert!(golden(n).len() <= 15, "{n}");
    }
}

// ---------------------------------------------------------------- review fixes

fn with_baseline(a: &str, idle: &str, task: &str) -> Diff {
    let baseline = run(a, idle);
    diff(&m(a), &m(task), Some(&baseline), &IgnoreList::builtin())
}

#[test]
fn settings_files_the_idle_computer_also_changed_collapse_whatever_they_hold() {
    let a = "F\t/etc/app.conf\t1\t1\th0\nL\t/etc/l\t1\t1\t/x\nF\t/etc/own.conf\t1\t1\th0\n";
    let idle = "F\t/etc/app.conf\t1\t2\th1\nL\t/etc/l\t1\t1\t/y\nF\t/etc/own.conf\t1\t1\th0\n";
    let task = "F\t/etc/app.conf\t1\t3\th2\nL\t/etc/l\t1\t1\t/z\nF\t/etc/own.conf\t1\t3\th5\n";
    let d = with_baseline(a, idle, task);
    assert_eq!(section(&d, "Settings & files"), ["~ /etc/own.conf"]);
    assert_eq!(
        d.idle_noise
            .iter()
            .map(|c| c.path.as_str())
            .collect::<Vec<_>>(),
        ["/etc/app.conf", "/etc/l"]
    );
    // identities stay in the JSON
    let f = &d.idle_noise[0];
    assert_eq!(f.before.as_ref().unwrap().hash.as_deref(), Some("h0"));
    assert_eq!(f.after.as_ref().unwrap().hash.as_deref(), Some("h2"));
    // the same resulting content is removed outright
    assert!(with_baseline(a, idle, idle).is_empty());
}

#[test]
fn a_different_change_type_at_an_idle_path_is_also_idle_noise() {
    let a = "F\t/etc/app.conf\t1\t1\th0\n";
    let d = with_baseline(a, "F\t/etc/app.conf\t1\t1\th1\n", "");
    assert!(section(&d, "Settings & files").is_empty());
    assert_eq!(d.idle_noise.len(), 1);
}

#[test]
fn added_databases_with_other_digests_are_not_hidden_by_the_baseline() {
    let idle =
        "DB\tsqlite\t/srv/a.db\tdig1\nDB\tpostgres\tnew\tschema\ts\nDBT\tpostgres\tnew\tt\td1\t5\n";
    let task =
        "DB\tsqlite\t/srv/a.db\tdig2\nDB\tpostgres\tnew\tschema\ts\nDBT\tpostgres\tnew\tt\td2\t5\n";
    let d = with_baseline("", idle, task);
    assert_eq!(d.database.len(), 2, "{:?}", d.database);
    assert!(with_baseline("", idle, idle).is_empty());
}

#[test]
fn table_digests_not_row_counts_decide_what_the_baseline_removes() {
    let a = "DB\tpostgres\tapp\tschema\ts\nDBT\tpostgres\tapp\tusers\tt0\t10\n";
    let idle = "DB\tpostgres\tapp\tschema\ts\nDBT\tpostgres\tapp\tusers\tt1\t12\n";
    let task = "DB\tpostgres\tapp\tschema\ts\nDBT\tpostgres\tapp\tusers\tt2\t12\n";
    assert_eq!(
        section(&with_baseline(a, idle, task), "Database changed"),
        ["postgres app: data changed in users (10→12 rows)"]
    );
    assert!(with_baseline(a, idle, idle).is_empty());
    let d = run(a, task);
    let t = &d.database[0].tables[0];
    assert_eq!(
        (t.digest_before.as_deref(), t.digest_after.as_deref()),
        (Some("t0"), Some("t2"))
    );
}

#[test]
fn sqlite_records_follow_credential_and_ignore_rules() {
    let d = run(
        "",
        "DB\tsqlite\t/root/.ssh/store.db\td1\nDB\tsqlite\t/root/.cache/x.db\td2\nDB\tsqlite\t/srv/app.db\td3\n",
    );
    assert_eq!(d.database.len(), 1);
    assert_eq!(d.database[0].name, "/srv/app.db");
    assert_eq!(section(&d, "Not kept"), ["credentials: ~/.ssh/ (1 file)"]);
    assert_eq!(d.ignored.paths, ["/root/.cache/x.db"]);
    assert_eq!(d.ignored.count(), 1);
}

#[test]
fn system_secrets_are_not_kept() {
    let d = run(
        "",
        "F\t/etc/shadow\t1\t1\ta\nF\t/etc/ssh/ssh_host_ed25519_key\t1\t1\tb\nF\t/etc/ssh/ssh_host_ed25519_key.pub\t1\t1\tc\n\
F\t/usr/local/share/x/private.pem\t1\t1\td\nF\t/usr/share/ca/cacert.pem\t1\t1\te\nOWN\tdpkg\tca\t/usr/share/ca/cacert.pem\n",
    );
    assert_eq!(
        section(&d, "Not kept"),
        ["credentials: /etc/shadow, /etc/ssh/ssh_host_ed25519_key, /usr/local/share/x/private.pem"]
    );
    assert_eq!(
        section(&d, "Settings & files"),
        ["+ /etc/ssh/ssh_host_ed25519_key.pub"]
    );
    assert_eq!(
        section(&d, "Packages"),
        ["apt: files changed in ca (1 file)"]
    );
}

#[test]
fn git_paths_are_unquoted_and_renames_read_from_rename_records() {
    let git = "GIT\t/root/app\taaaaaaa1\tmain\t0\t0\n";
    let b = format!(
        "{git}GITF\t/root/app\t??\t\"caf\\303\\251.txt\"\nGITF\t/root/app\tR_\told.txt -> new.txt\n\
GITF\t/root/app\t??\ta -> b.txt\nGITF\t/root/app\t??\t\"sec\\303\\251.pem\"\n\
GITF\t/root/app\tR_\t\"x y.rs\" -> \"z \\\"q\\\".rs\"\n"
    );
    let d = run(git, &b);
    let rc = &d.code[0];
    let keys: Vec<&str> = rc.dirty_new.keys().map(String::as_str).collect();
    assert_eq!(keys, ["a -> b.txt", "café.txt", "new.txt", "z \"q\".rs"]);
    assert_eq!(rc.renamed_from["new.txt"], "old.txt");
    assert_eq!(rc.renamed_from["z \"q\".rs"], "x y.rs");
    assert_eq!(
        d.not_kept.credentials["/root/app/secé.pem"],
        ["/root/app/secé.pem"]
    );
}

#[test]
fn editing_an_already_dirty_file_shows_under_code() {
    let git = "GIT\t/root/app\taaaaaaa1\tmain\t1\t0\nGITF\t/root/app\t_M\tsrc/a.rs\n";
    let a = format!("{git}F\t/root/app/src/a.rs\t1\t1\th1\n");
    let idle = format!("{git}F\t/root/app/src/a.rs\t2\t2\th2\n");
    let other = format!("{git}F\t/root/app/src/a.rs\t3\t3\th3\n");
    let d = run(&a, &idle);
    assert_eq!(section(&d, "Code"), ["~/app: 1 dirty/untracked: a.rs"]);
    assert_eq!(
        d.code[0].dirty_identity["src/a.rs"].hash.as_deref(),
        Some("h2")
    );
    // an identical edit in the baseline is background, a different one is not
    assert!(with_baseline(&a, &idle, &idle).is_empty());
    assert_eq!(
        section(&with_baseline(&a, &idle, &other), "Code"),
        ["~/app: 1 dirty/untracked: a.rs"]
    );
}

#[test]
fn json_carries_keep_details() {
    let b = "PKG\tdpkg\tca\t1\nF\t/usr/share/ca/x\t1\t1\th\nOWN\tdpkg\tca\t/usr/share/ca/x\n\
CRON\troot\tc1\nCRONF\troot\t/var/spool/cron/crontabs/root\n\
LAGENT\tuser\tdev.x\td9\t/Users/a/Library/LaunchAgents/dev.x.plist\n";
    let d = run("", b);
    let v = serde_json::to_value(&d).unwrap();
    assert_eq!(
        v["packages"][0]["owned_changes"]["ca"][0]["change"],
        "added"
    );
    assert_eq!(
        v["settings"]["cron"][0]["path"],
        "/var/spool/cron/crontabs/root"
    );
    assert_eq!(v["settings"]["cron"][0]["after"], "c1");
    assert_eq!(
        v["settings"]["launch_agents"][0]["path"],
        "/Users/a/Library/LaunchAgents/dev.x.plist"
    );
    let d = run("GIT\t/root/gone\tdeadbeef1\tmain\t0\t0\n", "");
    assert_eq!(
        d.code[0].removed_from,
        Some(("deadbeef1".into(), "main".into()))
    );
}

#[test]
fn counts_saturate() {
    let max = u64::MAX;
    let a = "GIT\t/root/app\taaaaaaa1\tmain\t0\t0\nP\t/root/t1\t0\nP\t/root/t2\t0\n";
    let b = format!(
        "GIT\t/root/app\taaaaaaa1\tmain\t{max}\t{max}\nP\t/root/t1\t{max}\nP\t/root/t2\t{max}\n"
    );
    let d = run(a, &b);
    assert_eq!(d.ignored.count(), u64::MAX);
    assert!(!d.render(15).is_empty());
    assert_eq!(d.code[0].dirty_delta, i64::MAX);
}

#[test]
fn large_baselines_subtract_in_linear_time() {
    // Four times the files must cost about four times as much, not sixteen: comparing the
    // two sizes on the same run keeps the check independent of how fast the machine is.
    let time = |n: usize| {
        let files = |h: &str| {
            (0..n)
                .map(|i| format!("F\t/etc/f{i}\t1\t1\t{h}\n"))
                .collect::<String>()
        };
        let (idle, task) = (files("a"), files("b"));
        let started = std::time::Instant::now();
        let d = with_baseline("", &idle, &task);
        let elapsed = started.elapsed();
        assert_eq!(d.idle_noise.len(), n);
        elapsed.as_secs_f64()
    };
    let small = time(10_000);
    let large = time(40_000);
    let ratio = large / small.max(1e-6);
    assert!(
        ratio < 9.0,
        "4x the files took {ratio:.1}x as long ({small:.3}s -> {large:.3}s); linear is 4x, quadratic 16x"
    );
}

#[test]
fn idle_files_with_other_content_collapse_into_one_line_and_stay_in_the_json() {
    let a = "F\t/srv/x/a\t1\t1\th0\nF\t/srv/x/b\t1\t1\th0\n";
    let idle = "F\t/srv/x/a\t1\t2\th1\nF\t/srv/x/b\t1\t2\th1\n";
    let task = "F\t/srv/x/a\t1\t3\th2\nF\t/srv/x/b\t1\t2\th1\nF\t/srv/x/c\t1\t3\th3\n";
    let d = with_baseline(a, idle, task);
    assert_eq!(d.idle_noise.len(), 1);
    assert_eq!(d.idle_noise[0].path, "/srv/x/a");
    assert_eq!(
        section(&d, "Also changes when idle"),
        ["1 files (/srv/x/a)"]
    );
    assert_eq!(
        d.other.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(),
        ["/srv/x/c"]
    );
}

// ---------------------------------------------------------------- unreadable sides, quiet parks

#[test]
fn a_database_unreadable_at_the_base_is_not_compared_and_not_called_changed() {
    let base = "DBU\tpostgres\tthe server did not answer\n";
    let now = "DB\tpostgres\tappdb\tschema\ts1\nDBT\tpostgres\tappdb\tpublic.items\td1\t5\n";
    let d = run(base, now);
    assert_eq!(section(&d, "Database changed"), Vec::<&str>::new());
    assert_eq!(
        section(&d, "Database not compared"),
        ["postgres appdb: database not compared: base unreadable (the server did not answer)"]
    );
    assert!(!d.render(15).contains("new database"));
    // The same server answering at both ends still reports a created database.
    let readable = run("", now);
    assert_eq!(
        section(&readable, "Database changed"),
        ["postgres appdb: new database (1 table)"]
    );
    // And one that stopped answering afterwards does not read as dropped.
    let gone = run(now, "DBU\tpostgres\tthe server did not answer\n");
    assert_eq!(section(&gone, "Database changed"), Vec::<&str>::new());
    assert_eq!(section(&gone, "Database not compared").len(), 1);
}

#[test]
fn an_unreadable_crontab_is_never_compared() {
    let d = run(
        "CRON\troot\t?\n",
        "CRON\troot\tabc\nCRONT\troot\t* * * * * x\\n\n",
    );
    assert!(d.settings.cron.is_empty());
    let d = run("CRON\troot\tabc\n", "CRON\troot\t?\n");
    assert!(d.settings.cron.is_empty());
}

#[test]
fn a_parked_crontab_is_the_base_of_the_fork_crontab() {
    let parked = "QCRON\troot\th0\t0 3 * * * backup\\n# keep\\n\n";
    // Quiet forks start with no crontab; it stays parked: nothing changed.
    let still = run(&format!("CRON\troot\t-\n{parked}"), "CRON\troot\t-\n");
    assert!(still.settings.cron.is_empty());
    assert!(still.is_empty());
    // The fork adds one line to its own crontab.
    let d = run(
        &format!("CRON\troot\t-\n{parked}"),
        "CRON\troot\th1\nCRONF\troot\t/var/spool/cron/crontabs/root\nCRONT\troot\t0 3 * * * backup\\n# keep\\n* * * * * tick\\n\n",
    );
    assert_eq!(d.settings.cron.len(), 1);
    let c = &d.settings.cron[0];
    assert_eq!(c.change, Change::Modified);
    assert_eq!(c.before.as_deref(), Some("h0"));
    assert_eq!((c.lines_added, c.lines_removed), (Some(1), Some(0)));
    assert_eq!(section(&d, "Settings & files"), ["cron root: 1 line added"]);
    // The same crontab back unchanged is no change.
    let same = run(
        &format!("CRON\troot\t-\n{parked}"),
        "CRON\troot\th0\nCRONT\troot\t0 3 * * * backup\\n# keep\\n\n",
    );
    assert!(same.settings.cron.is_empty());
    // Without a parked original the crontab is a new one.
    let fresh = run("CRON\troot\t-\n", "CRON\troot\th1\nCRONT\troot\tx\\n\n");
    assert_eq!(section(&fresh, "Settings & files"), ["cron root: added"]);
}

#[test]
fn crontab_edits_count_lines_added_and_removed() {
    let d = run(
        "CRON\troot\ta\nCRONT\troot\tone\\ntwo\\n\n",
        "CRON\troot\tb\nCRONT\troot\ttwo\\nthree\\nfour\\n\n",
    );
    assert_eq!(
        section(&d, "Settings & files"),
        ["cron root: 2 lines added, 1 removed"]
    );
}

#[test]
fn pulse_audio_churn_is_ignored() {
    let d = run(
        "",
        "F\t/home/silo/.config/pulse/8f4a-default-sink\t5\t1\tabc\nF\t/home/silo/.config/pulse/8f4a-default-source\t5\t1\tabc\nF\t/home/silo/.config/pulse/cookie\t256\t1\tabc\n",
    );
    assert!(d.settings.files.is_empty(), "{:?}", d.settings.files);
    assert_eq!(d.ignored.paths.len(), 3);
    let kept = run("", "F\t/home/silo/.config/pulse/client.conf\t5\t1\tabc\n");
    assert_eq!(kept.settings.files.len(), 1);
}

#[test]
fn quiet_parks_parse_from_the_manifest() {
    let parsed = m("QPARK\t/var/spool/cron/crontabs/root\th0\nQCRON\troot\th0\ta\\nb\\n\nCRONT\troot\tx\\n\nDBU\tpostgres\twhy not\n");
    assert_eq!(parsed.qpark["/var/spool/cron/crontabs/root"], "h0");
    assert_eq!(parsed.qcron["root"], ("h0".to_owned(), "a\nb\n".to_owned()));
    assert_eq!(parsed.cront["root"], "x\n");
    assert_eq!(parsed.db.pg_unreadable.as_deref(), Some("why not"));
}

// ---------------------------------------------------------------- macOS churn and rendering

/// An idle macOS fork's churn around one real LaunchAgent change.
fn mac_churn() -> (String, String, String) {
    let home = "/Users/silo";
    let head = format!("META\tformat\t2\nHOME\t{home}\n");
    let (mut base, mut idle, mut task) = (head.clone(), head.clone(), head);
    for i in 0..17 {
        let p = format!("{home}/Library/Preferences/org.vendor.app{i}.plist");
        base += &format!("F\t{p}\t1\t1\th0\n");
        idle += &format!("F\t{p}\t1\t2\th1\n");
        task += &format!("F\t{p}\t1\t3\th2\n");
    }
    for p in [
        "Library/Preferences/ByHost/x.plist",
        "Library/Application Support/AddressBook/AddressBook-v22.abcddb-wal",
    ] {
        task += &format!("F\t{home}/{p}\t1\t3\th9\n");
    }
    let la = format!("{home}/Library/LaunchAgents/org.silo.p2m.web.plist");
    base += &format!("F\t{la}\t1\t1\ta\nLAGENT\tuser\torg.silo.p2m.web\ta\t{la}\n");
    task += &format!("F\t{la}\t1\t3\tb\nLAGENT\tuser\torg.silo.p2m.web\tb\t{la}\n");
    (base, idle, task)
}

#[test]
fn macos_idle_preference_churn_does_not_hide_a_launch_agent() {
    let (base, idle, task) = mac_churn();
    let d = with_baseline(&base, &idle, &task);
    let text = d.render(15);
    assert!(
        text.contains("launch agent org.silo.p2m.web (user): changed"),
        "{text}"
    );
    assert_eq!(d.idle_noise.len(), 17);
    assert!(section(&d, "Settings & files")
        .iter()
        .all(|l| l.contains("launch agent")));
    assert!(d.ignored.count() >= 2, "ByHost and AddressBook are ignored");
}

#[test]
fn a_line_limit_cuts_directory_counts_before_named_changes() {
    let (base, _, task) = mac_churn();
    let d = run(&base, &task);
    let full = d.render_lines(100);
    let at = |needle: &str| full.iter().position(|l| l.contains(needle)).unwrap();
    assert!(at("launch agent") < at("Library/Preferences/ ("));
    let cut = d.render_lines(4);
    assert!(cut.iter().any(|l| l.contains("launch agent")), "{cut:?}");
}

#[test]
fn macos_background_paths_are_ignored_and_permissions_are_not_kept() {
    let ig = IgnoreList::builtin();
    for p in [
        "/Users/a/Library/Preferences/ByHost/com.apple.x.plist",
        "/Users/a/Library/Preferences/com.apple.dock.plist",
        "/Users/a/Library/Application Support/Knowledge/knowledgeC.db-shm",
        "/Users/a/Library/Biome/streams/x",
        "/Users/a/Library/Daemon Containers/x",
    ] {
        assert!(ig.matches(p), "{p}");
    }
    assert!(!ig.matches("/Users/a/Library/Preferences/org.vendor.app.plist"));
    let base = "META\tformat\t2\nHOME\t/Users/a\n";
    let tcc = "/Users/a/Library/Application Support/com.apple.TCC/TCC.db";
    let task = format!("{base}F\t{tcc}\t1\t3\th\nDB\tsqlite\t{tcc}\tdig\n");
    let d = run(base, &task);
    assert!(d.database.is_empty());
    assert!(d
        .not_kept
        .credentials
        .contains_key("/Users/a/Library/Application Support/com.apple.TCC/"));
}

#[test]
fn the_review_digest_ignores_timestamps_and_idle_noise() {
    use crate::changes::keep::review_digest;
    let a = "F\t/srv/big.bin\t10\t100\t-\n";
    let t1 = "F\t/srv/big.bin\t11\t200\t-\nF\t/etc/a\t1\t5\th1\n";
    let t2 = "F\t/srv/big.bin\t11\t999\t-\nF\t/etc/a\t1\t6\th1\n";
    assert_eq!(review_digest(&run(a, t1)), review_digest(&run(a, t2)));
    let t3 = "F\t/srv/big.bin\t11\t200\t-\nF\t/etc/a\t1\t5\th2\n";
    assert_ne!(review_digest(&run(a, t1)), review_digest(&run(a, t3)));
    let idle = "F\t/etc/n\t1\t2\th1\n";
    let n1 = format!("{t1}F\t/etc/n\t1\t3\th2\n");
    let n2 = format!("{t1}F\t/etc/n\t1\t4\th3\n");
    assert_eq!(
        review_digest(&with_baseline(a, idle, &n1)),
        review_digest(&with_baseline(a, idle, &n2))
    );
}

// ---------------------------------------------------------------- item ids

const ID_BASE: &str = "F\t/etc/a.conf\t1\t1\tx\nGIT\t/root/app\taaa\tmain\t0\t0\nPKG\tdpkg\told\t1\nPKGM\tdpkg\told\nCRON\troot\t01\nCRONF\troot\t/var/spool/cron/crontabs/root\nDB\tpostgres\tapp\tschema\ts1\n";
const ID_FORK: &str = "F\t/etc/a.conf\t2\t2\ty\nF\t/opt/new\t1\t1\tz\nGIT\t/root/app\tbbb\tmain\t0\t0\nGITF\t/root/app\t_M\ta.txt\nPKG\tdpkg\tjq\t1.7\nPKGM\tdpkg\tjq\nCRON\troot\t02\nCRONF\troot\t/var/spool/cron/crontabs/root\nDB\tpostgres\tapp\tschema\ts2\nLAGENT\tuser\tdev.x\td1\t/Users/a/Library/LaunchAgents/dev.x.plist\n";

#[test]
fn every_entry_a_keep_acts_on_has_the_id_it_is_answered_by() {
    let d = run(ID_BASE, ID_FORK);
    let ids: Vec<String> = d.items().into_keys().collect();
    assert_eq!(
        ids,
        [
            "cron:root",
            "database:postgres:app",
            "file:/etc/a.conf",
            "file:/opt/new",
            "file:/root/app/a.txt",
            "launch-agent:user:dev.x",
            "package:dpkg:jq",
            "package:dpkg:old",
            "repo:/root/app",
        ]
    );
    assert_eq!(d.settings.files[0].id, "file:/etc/a.conf");
    assert_eq!(d.other[0].id, "file:/opt/new");
    assert_eq!(d.code[0].id, "repo:/root/app");
    assert_eq!(d.code[0].dirty_ids["a.txt"], "file:/root/app/a.txt");
    assert_eq!(d.packages[0].ids, ["package:dpkg:jq", "package:dpkg:old"]);
    assert_eq!(d.settings.cron[0].id, "cron:root");
    assert_eq!(d.settings.launch_agents[0].id, "launch-agent:user:dev.x");
    assert_eq!(d.database[0].id, "database:postgres:app");
    // The ids are in the JSON, and a diff without them still reads.
    let json = serde_json::to_value(&d).unwrap();
    assert_eq!(json["code"][0]["id"], "repo:/root/app");
    let back: Diff = serde_json::from_value(json).unwrap();
    assert_eq!(back, d);
    let bare: Diff =
        serde_json::from_value(serde_json::to_value(d.without_ids()).unwrap()).unwrap();
    assert!(bare.code[0].id.is_empty() && bare.items().len() == d.items().len());
}

#[test]
fn ids_do_not_depend_on_the_order_of_the_manifest() {
    let reversed: String = ID_FORK.lines().rev().map(|l| format!("{l}\n")).collect();
    let (a, b) = (run(ID_BASE, ID_FORK), run(ID_BASE, &reversed));
    assert_eq!(a.items(), b.items());
}

#[test]
fn ids_do_not_change_the_review_digest() {
    use crate::changes::keep::review_digest;
    fn strip(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                for key in ["id", "ids", "dirty_ids"] {
                    map.remove(key);
                }
                map.values_mut().for_each(strip);
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let d = run(ID_BASE, ID_FORK);
    let mut view = serde_json::json!({
        "code": d.code, "packages": d.packages, "settings": d.settings,
        "database": d.database, "other": d.other,
    });
    strip(&mut view);
    let expected = <sha2::Sha256 as sha2::Digest>::digest(serde_json::to_vec(&view).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert_eq!(review_digest(&d), expected);
}
