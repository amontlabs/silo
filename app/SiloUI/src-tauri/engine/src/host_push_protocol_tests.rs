//! Real Git/LFS publication regressions. All repositories and destinations are disposable.
use super::*;
use sha2::{Digest, Sha256};
use std::os::unix::fs::{symlink, PermissionsExt};

struct Fixture {
    _root: tempfile::TempDir,
    tools: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let tools = root.path().join("tools");
        fs::create_dir(&tools).unwrap();
        let runtime = crate::test_support::manifest_root().join("runtime/git");
        assert!(
            runtime.join("bin/git").is_file(),
            "prepare the bundled Git runtime before testing"
        );
        symlink(runtime.join("bin/git"), tools.join("git")).unwrap();
        for helper in ["git-lfs", "git-remote-http"] {
            symlink(
                runtime.join("libexec/git-core").join(helper),
                tools.join(helper),
            )
            .unwrap();
        }
        Self { _root: root, tools }
    }
    fn repo(&self, name: &str, initialize: bool, bare: bool) -> HostGit {
        let directory = self._root.path().join(name);
        let home = self._root.path().join(format!("{name}-home"));
        fs::create_dir_all(&directory).unwrap();
        fs::create_dir_all(home.join("empty-templates")).unwrap();
        let git = HostGit {
            executable: self.tools.join("git"),
            directory,
            home,
            support: self._root.path().into(),
            ssh_command: None,
            cache_lock_fd: None,
            deadline: None,
        };
        if initialize {
            if bare {
                run(&git, &["init", "--bare", "--quiet"]);
            } else {
                run(&git, &["init", "--initial-branch=main", "--quiet"]);
            }
        }
        git
    }
    fn publish(
        &self,
        source: &HostGit,
        remote: &HostGit,
        name: &str,
        oid: &str,
        source_lfs: Option<&Path>,
    ) -> Result<u64, String> {
        let host = self.repo(name, false, true);
        let lfs = format!(
            "file://{}",
            source_lfs
                .unwrap_or(&source.directory.join(".git"))
                .display()
        );
        publish_committed(
            &host,
            source.directory.to_str().unwrap(),
            &lfs,
            "refs/silo/captured",
            oid,
            "main",
            remote.directory.to_str().unwrap(),
            None,
        )
    }
}
fn run(git: &HostGit, args: &[&str]) -> String {
    git.run(args, None, "")
        .unwrap_or_else(|error| panic!("{args:?}: {error}"))
}
fn commit(git: &HostGit) -> String {
    run(git, &["add", "--all"]);
    run(
        git,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
    );
    run(git, &["rev-parse", "HEAD"]).trim().into()
}
fn capture(git: &HostGit, oid: &str) {
    run(git, &["update-ref", "refs/silo/captured", oid]);
}
fn object_path(media: &Path, oid: &str) -> PathBuf {
    media.join(&oid[..2]).join(&oid[2..4]).join(oid)
}
fn pointer(git: &HostGit, filename: &str, payload: &[u8], media: &Path, retain: bool) -> String {
    let oid = format!("{:x}", Sha256::digest(payload));
    fs::write(
        git.directory.join(filename),
        format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {}\n",
            payload.len()
        ),
    )
    .unwrap();
    if retain {
        let path = object_path(media, &oid);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, payload).unwrap();
    }
    oid
}

#[test]
fn empty_and_historical_lfs_objects_publish_without_empty_cache_file() {
    let fixture = Fixture::new();
    let source = fixture.repo("source with space and 'quote", true, false);
    let remote = fixture.repo("remote.git", true, true);
    let media = source.directory.join(".git/lfs/objects");
    pointer(&source, "empty.bin", b"", &media, false);
    let historic = pointer(
        &source,
        "historical.bin",
        b"historical payload",
        &media,
        true,
    );
    commit(&source);
    fs::remove_file(source.directory.join("historical.bin")).unwrap();
    let current = pointer(&source, "current.bin", b"current payload", &media, true);
    let oid = commit(&source);
    capture(&source, &oid);
    assert_eq!(
        fixture
            .publish(&source, &remote, "publisher", &oid, None)
            .unwrap(),
        2
    );
    assert_eq!(run(&remote, &["rev-parse", "refs/heads/main"]).trim(), oid);
    for (oid, payload) in [
        (historic, b"historical payload".as_slice()),
        (current, b"current payload".as_slice()),
    ] {
        assert_eq!(
            fs::read(object_path(&remote.directory.join("lfs/objects"), &oid)).unwrap(),
            payload
        );
    }
}

#[test]
fn source_lfs_view_supports_custom_storage_and_linked_worktrees() {
    let fixture = Fixture::new();
    let primary = fixture.repo("primary", true, false);
    fs::write(primary.directory.join("README"), "base").unwrap();
    commit(&primary);
    let worktree = fixture.repo("linked worktree", false, false);
    run(
        &primary,
        &[
            "worktree",
            "add",
            "-b",
            "topic",
            worktree.directory.to_str().unwrap(),
        ],
    );
    let media = fixture._root.path().join("custom storage/objects");
    run(
        &worktree,
        &[
            "config",
            "lfs.storage",
            media.parent().unwrap().to_str().unwrap(),
        ],
    );
    let payload = pointer(&worktree, "custom.bin", b"custom payload", &media, true);
    let oid = commit(&worktree);
    capture(&worktree, &oid);
    let view = fixture._root.path().join("source-view.git");
    // The file adapter requires a bare Git directory; the pure SSH server only
    // needs the LFS layout. Both resolve the same custom media directory.
    let view_git = fixture.repo("source-view.git", true, true);
    assert_eq!(view_git.directory, view);
    fs::create_dir_all(view.join("lfs")).unwrap();
    symlink(&media, view.join("lfs/objects")).unwrap();
    let remote = fixture.repo("remote.git", true, true);
    fixture
        .publish(&worktree, &remote, "publisher", &oid, Some(&view))
        .unwrap();
    assert_eq!(
        fs::read(object_path(&remote.directory.join("lfs/objects"), &payload)).unwrap(),
        b"custom payload"
    );
}

#[test]
fn pruned_source_objects_already_upstream_do_not_prevent_publication() {
    let fixture = Fixture::new();
    let source = fixture.repo("source", true, false);
    let remote = fixture.repo("remote.git", true, true);
    let media = source.directory.join(".git/lfs/objects");
    let payload = pointer(&source, "data.bin", b"existing upstream", &media, true);
    let first = commit(&source);
    capture(&source, &first);
    fixture
        .publish(&source, &remote, "first-publisher", &first, None)
        .unwrap();
    fs::remove_file(object_path(&media, &payload)).unwrap();
    fs::write(source.directory.join("README"), "new commit").unwrap();
    let second = commit(&source);
    capture(&source, &second);
    assert_eq!(
        fixture
            .publish(&source, &remote, "second-publisher", &second, None)
            .unwrap(),
        1
    );
    assert_eq!(
        run(&remote, &["rev-parse", "refs/heads/main"]).trim(),
        second
    );
}

#[test]
fn truly_missing_lfs_data_never_advances_remote_branch() {
    let fixture = Fixture::new();
    let source = fixture.repo("source", true, false);
    let remote = fixture.repo("remote.git", true, true);
    fs::write(source.directory.join("README"), "base").unwrap();
    let first = commit(&source);
    capture(&source, &first);
    fixture
        .publish(&source, &remote, "first-publisher", &first, None)
        .unwrap();
    pointer(
        &source,
        "missing.bin",
        b"never stored",
        &source.directory.join(".git/lfs/objects"),
        false,
    );
    let second = commit(&source);
    capture(&source, &second);
    let error = fixture
        .publish(&source, &remote, "second-publisher", &second, None)
        .unwrap_err();
    assert!(error.contains("Git lfs push failed"), "{error}");
    assert_eq!(
        run(&remote, &["rev-parse", "refs/heads/main"]).trim(),
        first
    );
}

#[test]
fn publication_uses_captured_commit_and_ignores_guest_hooks_dirty_files_and_config() {
    let fixture = Fixture::new();
    let source = fixture.repo("source", true, false);
    let remote = fixture.repo("remote.git", true, true);
    fs::write(source.directory.join("README"), "captured").unwrap();
    let first = commit(&source);
    capture(&source, &first);
    fs::write(source.directory.join("README"), "later commit").unwrap();
    commit(&source);
    fs::write(source.directory.join("README"), "dirty").unwrap();
    fs::write(source.directory.join("private"), "untracked").unwrap();
    let marker = fixture._root.path().join("hook-executed");
    let hook = source.directory.join(".git/hooks/pre-push");
    fs::create_dir_all(hook.parent().unwrap()).unwrap();
    fs::write(
        &hook,
        format!("#!/bin/sh\ntouch '{}'\nexit 42\n", marker.display()),
    )
    .unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o700)).unwrap();
    run(&source, &["config", "credential.helper", "!exit 42"]);
    run(
        &source,
        &["config", "lfs.url", "https://invalid.invalid/lfs"],
    );
    fixture
        .publish(&source, &remote, "publisher", &first, None)
        .unwrap();
    assert_eq!(run(&remote, &["show", "main:README"]), "captured");
    assert!(!marker.exists());
    assert_eq!(
        fs::read_to_string(source.directory.join("README")).unwrap(),
        "dirty"
    );
    assert!(!run(&remote, &["ls-tree", "--name-only", "main"]).contains("private"));
    // Advance the destination independently; a captured ancestor cannot overwrite it.
    let newer = run(&source, &["rev-parse", "HEAD"]);
    run(
        &source,
        &[
            "push",
            remote.directory.to_str().unwrap(),
            "HEAD:refs/heads/main",
        ],
    );
    let error = fixture
        .publish(&source, &remote, "conflict-publisher", &first, None)
        .unwrap_err();
    assert!(
        error.contains("remote branch has commits missing"),
        "{error}"
    );
    assert_eq!(run(&remote, &["rev-parse", "main"]), newer);
}

#[test]
fn repeated_publication_reuses_host_objects_and_refreshes_branch_tracking() {
    let fixture = Fixture::new();
    let source = fixture.repo("source", true, false);
    let remote = fixture.repo("remote.git", true, true);
    // Like GitHub, the destination has a default branch.
    run(&remote, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    let host = fixture.repo("publisher", false, true);
    let media = source.directory.join(".git/lfs/objects");
    let lfs_url = format!("file://{}", source.directory.join(".git").display());
    let first_object = pointer(&source, "first.bin", b"first payload", &media, true);
    let first = commit(&source);
    capture(&source, &first);
    let publish = |oid: &str, branch: &str| {
        publish_committed(
            &host,
            source.directory.to_str().unwrap(),
            &lfs_url,
            "refs/silo/captured",
            oid,
            branch,
            remote.directory.to_str().unwrap(),
            None,
        )
        .unwrap()
    };
    assert_eq!(publish(&first, "main"), 1);
    fs::remove_file(object_path(&media, &first_object)).unwrap();
    let second_object = pointer(&source, "second.bin", b"second payload", &media, true);
    let second = commit(&source);
    capture(&source, &second);
    assert_eq!(publish(&second, "main"), 1);
    // A new branch at an already published commit adds no commits.
    assert_eq!(publish(&second, "other"), 0);
    assert_eq!(
        run(&remote, &["rev-parse", "refs/heads/main"]).trim(),
        second
    );
    assert_eq!(
        run(&remote, &["rev-parse", "refs/heads/other"]).trim(),
        second
    );
    for oid in [first_object, second_object] {
        assert!(object_path(&host.directory.join("lfs/objects"), &oid).is_file());
        assert!(object_path(&remote.directory.join("lfs/objects"), &oid).is_file());
    }
}

/// Answers 401 until a request carries credentials, then records them.
fn credential_server() -> (String, std::sync::mpsc::Receiver<String>) {
    use std::io::{BufRead, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut authorization = None;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':') {
                    if name.eq_ignore_ascii_case("authorization") {
                        authorization = Some(value.trim().to_owned());
                    }
                }
            }
            let response = match authorization {
                Some(value) => {
                    let _ = sender.send(value);
                    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                }
                None => "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"silo\"\r\nLFS-Authenticate: Basic realm=\"silo\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            };
            let _ = stream.write_all(response.as_bytes());
        }
    });
    (format!("http://{address}/owner/repo.git"), receiver)
}

#[test]
fn git_and_git_lfs_authenticate_through_the_credential_pipe() {
    let fixture = Fixture::new();
    let (remote, requests) = credential_server();
    let git = fixture.repo("credential", true, true);
    run(&git, &["config", "remote.origin.url", &remote]);
    let expected = format!(
        "Basic {}",
        STANDARD.encode("x-access-token:ghu_fixtureToken")
    );
    let timeout = std::time::Duration::from_secs(20);
    // Git's HTTP transport and Git LFS each ask the credential helper.
    assert!(git
        .run(
            &["ls-remote", "--heads", "origin"],
            Some("ghu_fixtureToken"),
            &remote
        )
        .is_err());
    assert_eq!(requests.recv_timeout(timeout).unwrap(), expected);
    assert!(git
        .run(&["lfs", "locks"], Some("ghu_fixtureToken"), &remote)
        .is_err());
    assert_eq!(requests.recv_timeout(timeout).unwrap(), expected);
    // Other hosts never receive the credential.
    let (other, other_requests) = credential_server();
    assert!(git
        .run(
            &["ls-remote", "--heads", &other],
            Some("ghu_fixtureToken"),
            &remote
        )
        .is_err());
    assert!(other_requests
        .recv_timeout(std::time::Duration::from_secs(2))
        .is_err());
}

#[test]
fn a_new_branch_counts_only_commits_missing_from_the_default_branch() {
    let fixture = Fixture::new();
    let source = fixture.repo("source", true, false);
    let remote = fixture.repo("remote.git", true, true);
    run(&remote, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    fs::write(source.directory.join("first.txt"), "first").unwrap();
    let base = commit(&source);
    capture(&source, &base);
    assert_eq!(
        fixture
            .publish(&source, &remote, "publisher", &base, None)
            .unwrap(),
        1
    );
    for name in ["second.txt", "third.txt"] {
        fs::write(source.directory.join(name), name).unwrap();
        commit(&source);
    }
    let feature = run(&source, &["rev-parse", "HEAD"]).trim().to_owned();
    capture(&source, &feature);
    // A fresh publishing cache has never seen the remote's default branch.
    let host = fixture.repo("feature-publisher", false, true);
    let count = publish_committed(
        &host,
        source.directory.to_str().unwrap(),
        &format!("file://{}", source.directory.join(".git").display()),
        "refs/silo/captured",
        &feature,
        "feature",
        remote.directory.to_str().unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(count, 2);
    assert_eq!(
        run(&remote, &["rev-parse", "refs/heads/feature"]).trim(),
        feature
    );
}
