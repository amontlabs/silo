//! The shell scripts and commands a reconcile runs in a computer. Every value in them is Silo's
//! own constant or a positional argument; nothing the computer or an agent wrote is put into
//! script text.

use super::MCP_SERVER_NAME;

/// Where the program goes in the computer.
pub const PROGRAM_PATH: &str = "/usr/local/bin/silo";
/// Where the registration helper is in a Linux computer (shared with computer use).
pub const LINUX_HELPER: &str = "/usr/local/libexec/silo-computer-use";
/// Where the macOS register script and receipt are, relative to the working account's home.
pub const MACOS_SUPPORT: &str = "Library/Application Support/Silo";
pub const MACOS_REGISTER_NAME: &str = "silo-cua-register.py";

/// A shell expression for a path in a computer.
#[derive(Clone, Copy, Debug)]
pub struct Place(pub &'static str);

pub const LINUX_HELPER_PLACE: Place = Place("'/usr/local/libexec/silo-computer-use'");
pub const LINUX_RECEIPT_PLACE: Place = Place("'/var/lib/silo/components.json'");
pub const MACOS_REGISTER_PLACE: Place =
    Place("\"$HOME/Library/Application Support/Silo/silo-cua-register.py\"");
pub const MACOS_RECEIPT_PLACE: Place =
    Place("\"$HOME/Library/Application Support/Silo/components.json\"");

const HASH_FUNCTION: &str = "hash() { if command -v sha256sum >/dev/null 2>&1; then sha256sum \"$1\" | cut -d' ' -f1; else shasum -a 256 \"$1\" | cut -d' ' -f1; fi; }";

/// Prints what the computer has: `program=<sha256>`, `helper=<sha256>`, then `receipt:` and the
/// receipt's first 64 KiB plus one byte (so a larger one is seen as such). A path that is not a
/// regular file, or is a link, counts as missing.
pub fn observe(helper: Place, receipt: Place) -> String {
    format!(
        "{HASH_FUNCTION}\n\
         regular() {{ [ -f \"$1\" ] && [ ! -L \"$1\" ]; }}\n\
         helper={}; receipt={}\n\
         program=; if regular '{PROGRAM_PATH}'; then program=$(hash '{PROGRAM_PATH}' 2>/dev/null) || program=; fi\n\
         helper_sha=; if regular \"$helper\"; then helper_sha=$(hash \"$helper\" 2>/dev/null) || helper_sha=; fi\n\
         printf 'program=%s\\nhelper=%s\\nreceipt:\\n' \"$program\" \"$helper_sha\"\n\
         if regular \"$receipt\"; then head -c {} \"$receipt\" 2>/dev/null || true; fi\n",
        helper.0,
        receipt.0,
        super::receipt::MAX_RECEIPT_BYTES + 1
    )
}

/// Installs the program that arrives on standard input. `$1` is the SHA-256 it must have, `$2`
/// where it goes, `$3` the owner to give it (empty for the current user). The bytes land in a
/// folder only root can enter, are checked there, and replace the old file by a rename in the
/// destination's folder, so a program is never half written. A mismatch replaces nothing.
pub const INSTALL_SCRIPT: &str = "set -eu\numask 077\n\
want=\"$1\"; dest=\"$2\"; owner=\"$3\"; staged=\n\
hash() { if command -v sha256sum >/dev/null 2>&1; then sha256sum \"$1\" | cut -d' ' -f1; else shasum -a 256 \"$1\" | cut -d' ' -f1; fi; }\n\
tmp=$(mktemp -d)\n\
trap 'rm -rf \"$tmp\"; [ -z \"$staged\" ] || rm -f \"$staged\"' EXIT\n\
cat > \"$tmp/program\"\n\
got=$(hash \"$tmp/program\")\n\
if [ \"$got\" != \"$want\" ]; then echo 'The program that arrived does not match its checksum; nothing was replaced.' >&2; exit 1; fi\n\
if [ -d \"$dest\" ] && [ ! -L \"$dest\" ]; then echo \"$dest is a folder; nothing was replaced.\" >&2; exit 1; fi\n\
dir=$(dirname \"$dest\"); mkdir -p \"$dir\"\n\
staged=$(mktemp \"$dir/.silo-install.XXXXXX\")\n\
cp \"$tmp/program\" \"$staged\"\n\
if [ -n \"$owner\" ]; then chown \"$owner\" \"$staged\"; fi\n\
chmod 0755 \"$staged\"\n\
[ -L \"$dest\" ] && rm -f \"$dest\"\n\
mv -f \"$staged\" \"$dest\"\n\
staged=\n";

/// Writes standard input to a file atomically: `$1` is its mode. Its folder is made when needed.
pub fn write_file(place: Place) -> String {
    format!(
        "set -eu\numask 022\npath={}; dir=$(dirname \"$path\"); mkdir -p \"$dir\"\n\
         tmp=$(mktemp \"$dir/.silo-write.XXXXXX\"); trap 'rm -f \"$tmp\"' EXIT\n\
         cat > \"$tmp\"; chmod \"$1\" \"$tmp\"\n\
         [ -L \"$path\" ] && rm -f \"$path\"\n\
         mv -f \"$tmp\" \"$path\"\n",
        place.0
    )
}

/// The command that registers (`server-add`) or removes (`server-remove`) Silo's MCP server with
/// the agents; an agent's own server of that name is left alone and reported. Linux runs the helper as root; macOS runs the register script as the working
/// account.
pub fn registration(macos: bool, add: bool) -> Vec<String> {
    let verb = if add { "server-add" } else { "server-remove" };
    // Removal names the command too, so an entry an earlier Silo wrote without its mark is
    // recognised as Silo's.
    let tail = vec![
        verb.to_owned(),
        "--name".into(),
        MCP_SERVER_NAME.into(),
        "--command".into(),
        PROGRAM_PATH.into(),
        "--arg".into(),
        "mcp".into(),
    ];
    let mut argv: Vec<String> = if macos {
        vec![
            "sh".into(),
            "-c".into(),
            format!("exec /usr/bin/python3 \"$HOME/{MACOS_SUPPORT}/{MACOS_REGISTER_NAME}\" \"$@\""),
            "silo-register".into(),
        ]
    } else {
        vec![LINUX_HELPER.into()]
    };
    argv.extend(tail);
    argv
}

/// The part of an agent's install script that registers Silo's MCP server with agents installed
/// since the tools were set up. It does nothing in a computer without the program, the endpoint
/// file or the helper, and a failure never matters: the next start registers again.
pub fn linux_register_after_install() -> String {
    format!(
        "if [ -x {PROGRAM_PATH} ] && [ -f {} ] && [ -x {LINUX_HELPER} ]; then {LINUX_HELPER} server-add --name {MCP_SERVER_NAME} --command {PROGRAM_PATH} --arg mcp >/dev/null 2>&1 || true; fi",
        crate::guest_components::ENDPOINT_PATH
    )
}

/// The macOS counterpart, run as the working account.
pub fn macos_register_after_install() -> String {
    format!(
        "REGISTER=\"$HOME/{MACOS_SUPPORT}/{MACOS_REGISTER_NAME}\"; if [ -x {PROGRAM_PATH} ] && [ -f {} ] && [ -f \"$REGISTER\" ]; then /usr/bin/python3 \"$REGISTER\" server-add --name {MCP_SERVER_NAME} --command {PROGRAM_PATH} --arg mcp >/dev/null 2>&1 || true; fi",
        crate::guest_components::ENDPOINT_PATH
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    fn run_script(script: &str, args: &[&str], stdin: &[u8]) -> std::process::Output {
        use std::io::Write as _;
        let mut child = Command::new("sh")
            .args(["-c", script, "silo-test"])
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(stdin).unwrap();
        child.wait_with_output().unwrap()
    }

    fn sha(bytes: &[u8]) -> String {
        super::super::artifact::sha256_hex(bytes)
    }

    #[test]
    fn the_install_script_checks_the_hash_in_the_computer_before_it_replaces_anything() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("bin/silo");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(&dest, b"old program").unwrap();
        let wrong = run_script(
            INSTALL_SCRIPT,
            &[&sha(b"something else"), dest.to_str().unwrap(), ""],
            b"new program",
        );
        assert!(!wrong.status.success());
        assert!(String::from_utf8_lossy(&wrong.stderr).contains("does not match its checksum"));
        assert_eq!(
            std::fs::read(&dest).unwrap(),
            b"old program",
            "a mismatch replaces nothing"
        );
        assert_eq!(
            std::fs::read_dir(dest.parent().unwrap()).unwrap().count(),
            1,
            "nothing is left behind"
        );
        let right = run_script(
            INSTALL_SCRIPT,
            &[&sha(b"new program"), dest.to_str().unwrap(), ""],
            b"new program",
        );
        assert!(
            right.status.success(),
            "{}",
            String::from_utf8_lossy(&right.stderr)
        );
        assert_eq!(std::fs::read(&dest).unwrap(), b"new program");
        let mode = std::os::unix::fs::PermissionsExt::mode(
            &std::fs::metadata(&dest).unwrap().permissions(),
        ) & 0o777;
        assert_eq!(mode, 0o755);
        assert_eq!(
            std::fs::read_dir(dest.parent().unwrap()).unwrap().count(),
            1
        );
    }

    #[test]
    fn the_install_script_makes_the_folder_replaces_a_link_and_refuses_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        let fresh = dir.path().join("a/b/silo");
        let ok = run_script(
            INSTALL_SCRIPT,
            &[&sha(b"x"), fresh.to_str().unwrap(), ""],
            b"x",
        );
        assert!(ok.status.success());
        assert_eq!(std::fs::read(&fresh).unwrap(), b"x");
        // A link planted at the destination is replaced, never written through.
        let target = dir.path().join("victim");
        std::fs::write(&target, b"victim").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let ok = run_script(
            INSTALL_SCRIPT,
            &[&sha(b"y"), link.to_str().unwrap(), ""],
            b"y",
        );
        assert!(ok.status.success());
        assert_eq!(std::fs::read(&target).unwrap(), b"victim");
        assert_eq!(std::fs::read(&link).unwrap(), b"y");
        assert!(!std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        let folder = dir.path().join("folder");
        std::fs::create_dir(&folder).unwrap();
        let refused = run_script(
            INSTALL_SCRIPT,
            &[&sha(b"z"), folder.to_str().unwrap(), ""],
            b"z",
        );
        assert!(!refused.status.success());
        assert!(folder.is_dir() && std::fs::read_dir(&folder).unwrap().count() == 0);
    }

    #[test]
    fn an_unwritable_destination_fails_without_leaving_the_upload() {
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::os::unix::fs::PermissionsExt::from_mode(0o500))
            .unwrap();
        let denied = run_script(
            INSTALL_SCRIPT,
            &[&sha(b"x"), locked.join("silo").to_str().unwrap(), ""],
            b"x",
        );
        std::fs::set_permissions(&locked, std::os::unix::fs::PermissionsExt::from_mode(0o700))
            .unwrap();
        if unsafe { libc::geteuid() } != 0 {
            assert!(!denied.status.success());
        }
        assert!(!locked.join("silo").exists() || unsafe { libc::geteuid() } == 0);
    }

    #[test]
    fn the_write_script_makes_the_folder_and_replaces_the_file_whole() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("var/lib/silo/components.json");
        let script = write_file(Place(Box::leak(
            format!("'{}'", path.display()).into_boxed_str(),
        )));
        let first = run_script(&script, &["644"], b"one");
        assert!(first.status.success());
        let second = run_script(&script, &["644"], b"two");
        assert!(second.status.success());
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        let mode = std::os::unix::fs::PermissionsExt::mode(
            &std::fs::metadata(&path).unwrap().permissions(),
        ) & 0o777;
        assert_eq!(mode, 0o644);
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );

        // A symlink at path is unlinked and replaced, never followed
        let victim = dir.path().join("victim.txt");
        std::fs::write(&victim, b"untouched").unwrap();
        let symlink_path = dir.path().join("var/lib/silo/link.json");
        std::os::unix::fs::symlink(&victim, &symlink_path).unwrap();
        let link_script = write_file(Place(Box::leak(
            format!("'{}'", symlink_path.display()).into_boxed_str(),
        )));
        let third = run_script(&link_script, &["644"], b"new content");
        assert!(third.status.success());
        assert_eq!(std::fs::read(&victim).unwrap(), b"untouched");
        assert_eq!(std::fs::read(&symlink_path).unwrap(), b"new content");
        assert!(!std::fs::symlink_metadata(&symlink_path)
            .unwrap()
            .file_type()
            .is_symlink());

        // A link to a folder is replaced too; nothing lands inside the folder it names.
        let folder = dir.path().join("elsewhere");
        std::fs::create_dir(&folder).unwrap();
        let folder_link = dir.path().join("var/lib/silo/folder.json");
        std::os::unix::fs::symlink(&folder, &folder_link).unwrap();
        let fourth = run_script(
            &write_file(Place(Box::leak(
                format!("'{}'", folder_link.display()).into_boxed_str(),
            ))),
            &["644"],
            b"data",
        );
        assert!(fourth.status.success());
        assert_eq!(std::fs::read(&folder_link).unwrap(), b"data");
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
    }

    #[test]
    fn the_observe_script_reports_hashes_the_receipt_and_treats_links_as_missing() {
        let dir = tempfile::tempdir().unwrap();
        let helper = dir.path().join("helper");
        let receipt = dir.path().join("receipt.json");
        std::fs::write(&helper, b"helper text").unwrap();
        std::fs::write(&receipt, b"{\"a\":1}").unwrap();
        let quoted = |path: &std::path::Path| -> Place {
            Place(Box::leak(format!("'{}'", path.display()).into_boxed_str()))
        };
        let script = observe(quoted(&helper), quoted(&receipt));
        // PROGRAM_PATH is the real /usr/local/bin/silo; only the helper and receipt are asserted.
        let out = run_script(&script, &[], b"");
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(
            text.contains(&format!("helper={}\n", sha(b"helper text"))),
            "{text}"
        );
        assert!(text.ends_with("receipt:\n{\"a\":1}"), "{text}");
        std::fs::remove_file(&receipt).unwrap();
        std::os::unix::fs::symlink(&helper, &receipt).unwrap();
        std::fs::remove_file(&helper).unwrap();
        std::fs::write(dir.path().join("elsewhere"), b"x").unwrap();
        std::os::unix::fs::symlink(dir.path().join("elsewhere"), &helper).unwrap();
        let out = run_script(&script, &[], b"");
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(text.contains("helper=\n"), "a link is missing: {text}");
        assert!(text.ends_with("receipt:\n"), "{text}");
        std::fs::remove_file(&receipt).unwrap();
        std::fs::write(&receipt, vec![b'x'; 200_000]).unwrap();
        let text = run_script(&script, &[], b"").stdout;
        let body = &text[text.windows(9).position(|w| w == b"receipt:\n").unwrap() + 9..];
        assert_eq!(
            body.len(),
            super::super::receipt::MAX_RECEIPT_BYTES + 1,
            "the read is bounded"
        );
    }

    #[test]
    fn registration_commands_are_fixed_argument_lists() {
        assert_eq!(
            registration(false, true),
            vec![
                LINUX_HELPER,
                "server-add",
                "--name",
                "silo",
                "--command",
                "/usr/local/bin/silo",
                "--arg",
                "mcp"
            ]
        );
        assert_eq!(
            registration(false, false),
            vec![
                LINUX_HELPER,
                "server-remove",
                "--name",
                "silo",
                "--command",
                "/usr/local/bin/silo",
                "--arg",
                "mcp"
            ]
        );
        let mac = registration(true, true);
        assert_eq!(&mac[..2], ["sh", "-c"]);
        assert!(mac[2].contains("$HOME/Library/Application Support/Silo/silo-cua-register.py"));
        assert_eq!(
            &mac[4..],
            [
                "server-add",
                "--name",
                "silo",
                "--command",
                "/usr/local/bin/silo",
                "--arg",
                "mcp"
            ]
        );
    }

    #[test]
    fn the_after_install_commands_do_nothing_without_the_program_the_file_or_the_helper() {
        let linux = linux_register_after_install();
        assert!(linux.starts_with("if [ -x /usr/local/bin/silo ] && [ -f /etc/silo/endpoint.json ] && [ -x /usr/local/libexec/silo-computer-use ]"));
        assert!(linux.contains("server-add --name silo --command /usr/local/bin/silo --arg mcp"));
        assert!(linux.ends_with("|| true; fi"));
        let mac = macos_register_after_install();
        assert!(mac.contains("[ -f /etc/silo/endpoint.json ] && [ -f \"$REGISTER\" ]"));
        assert!(mac.contains("/usr/bin/python3 \"$REGISTER\" server-add --name silo"));
        // Runs under `sh -n` as written.
        for command in [linux, mac] {
            let parsed = Command::new("sh")
                .args(["-n", "-c", &command])
                .status()
                .unwrap();
            assert!(parsed.success());
        }
    }
}
