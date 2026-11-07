//! Whether another account can change a file or directory, and creating directories with
//! fixed modes, for the Linux checks that refuse a path others could swap (the credential
//! store's ancestors in [`crate::credential_file_store`], the engine directory in silod's
//! `install_trust`).
//!
//! Write access for others (`o+w`) is always refused, and so is a sticky directory anyone
//! but its owner can write to. Write access for the group (`g+w`) is refused unless the group
//! is a *user-private group* of the owner, the rule Debian's OpenSSH applies for `StrictModes`
//! (and the layout Debian and Ubuntu create with `USERGROUPS_ENAB` and umask 002): the file's
//! group is the owner's primary group (`getpwuid_r`), and that group lists no member but the
//! owner (`getgrgid_r`: no members, or only the owner's name). Beyond Debian's rule, no other
//! account in `/etc/passwd` may have that group as its primary group either. Only
//! `/etc/passwd` is read for that, because walking the whole user database (`getpwent`)
//! shares one cursor across the process and is not thread-safe: an account that only a
//! network directory (LDAP, SSSD) gives the same primary group is not seen.
//!
//! The lookups go through [`AccountDatabase`], so tests use their own accounts.

use std::{
    ffi::CStr,
    fs::{self, DirBuilder},
    io,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::Path,
};

/// The largest buffer a `getpwuid_r` or `getgrgid_r` lookup grows to.
const MAX_LOOKUP_BUFFER: usize = 1 << 20;
/// The largest `/etc/passwd` read for the primary-group check.
const MAX_PASSWD_BYTES: u64 = 16 * 1024 * 1024;

/// An account from the user database.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub name: String,
    pub primary_gid: u32,
}

/// A group from the group database.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    pub members: Vec<String>,
}

/// The user and group lookups the private-group rule needs.
pub trait AccountDatabase {
    /// `uid`'s account, or `None` when the user database does not know it.
    fn account(&self, uid: u32) -> Option<Account>;
    /// `gid`'s group, or `None` when the group database does not know it.
    fn group(&self, gid: u32) -> Option<Group>;
    /// Whether an account other than `uid` has `gid` as its primary group. An unreadable
    /// database answers true, so the group is not trusted.
    fn other_account_has_primary_group(&self, gid: u32, uid: u32) -> bool;
}

/// This machine's user and group databases.
pub struct SystemAccounts;

impl AccountDatabase for SystemAccounts {
    fn account(&self, uid: u32) -> Option<Account> {
        system_account(uid)
    }

    fn group(&self, gid: u32) -> Option<Group> {
        system_group(gid)
    }

    fn other_account_has_primary_group(&self, gid: u32, uid: u32) -> bool {
        use std::io::Read;
        let mut contents = Vec::new();
        let read = fs::File::open("/etc/passwd")
            .and_then(|file| file.take(MAX_PASSWD_BYTES).read_to_end(&mut contents));
        if read.is_err() {
            return true;
        }
        passwd_has_other_primary(&String::from_utf8_lossy(&contents), gid, uid)
    }
}

/// Whether a `passwd(5)` text lists an account other than `uid` whose primary group is
/// `gid`. A line that does not parse is ignored, as the C library ignores it.
fn passwd_has_other_primary(contents: &str, gid: u32, uid: u32) -> bool {
    contents.lines().any(|line| {
        let fields = line.split(':').collect::<Vec<_>>();
        if fields.len() < 4 {
            return false;
        }
        match (fields[2].parse::<u32>(), fields[3].parse::<u32>()) {
            (Ok(line_uid), Ok(line_gid)) => line_gid == gid && line_uid != uid,
            _ => false,
        }
    })
}

/// Runs a `get*_r` lookup with a buffer that grows on `ERANGE` up to [`MAX_LOOKUP_BUFFER`].
fn with_buffer<T>(
    mut lookup: impl FnMut(&mut [libc::c_char]) -> Result<Option<T>, i32>,
) -> Option<T> {
    let mut buffer = vec![0 as libc::c_char; 4096];
    loop {
        match lookup(&mut buffer) {
            Err(libc::ERANGE) if buffer.len() < MAX_LOOKUP_BUFFER => {
                let larger = buffer.len() * 2;
                buffer.resize(larger, 0);
            }
            Err(_) => return None,
            Ok(found) => return found,
        }
    }
}

fn text(pointer: *const libc::c_char) -> String {
    // SAFETY: callers pass a NUL-terminated string from a lookup's buffer, still alive.
    unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned()
}

fn system_account(uid: u32) -> Option<Account> {
    with_buffer(|buffer| {
        // SAFETY: an all-zero passwd is a valid out-parameter.
        let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        // SAFETY: every pointer is valid for the call, and the buffer's length is passed.
        let code = unsafe {
            libc::getpwuid_r(
                uid,
                &mut entry,
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut result,
            )
        };
        if code != 0 {
            return Err(code);
        }
        if result.is_null() || entry.pw_name.is_null() {
            return Ok(None);
        }
        Ok(Some(Account {
            name: text(entry.pw_name),
            primary_gid: entry.pw_gid,
        }))
    })
}

fn system_group(gid: u32) -> Option<Group> {
    with_buffer(|buffer| {
        // SAFETY: an all-zero group is a valid out-parameter.
        let mut entry: libc::group = unsafe { std::mem::zeroed() };
        let mut result: *mut libc::group = std::ptr::null_mut();
        // SAFETY: every pointer is valid for the call, and the buffer's length is passed.
        let code = unsafe {
            libc::getgrgid_r(
                gid,
                &mut entry,
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut result,
            )
        };
        if code != 0 {
            return Err(code);
        }
        if result.is_null() || entry.gr_name.is_null() {
            return Ok(None);
        }
        let mut members = Vec::new();
        let mut cursor = entry.gr_mem;
        // SAFETY: `gr_mem` is a NULL-terminated array of strings in the buffer.
        while !cursor.is_null() && !unsafe { *cursor }.is_null() {
            members.push(text(unsafe { *cursor }));
            cursor = unsafe { cursor.add(1) };
        }
        Ok(Some(Group {
            name: text(entry.gr_name),
            members,
        }))
    })
}

/// Whether `gid` is a user-private group of `uid` (see the module documentation).
pub fn is_private_group(uid: u32, gid: u32, accounts: &dyn AccountDatabase) -> bool {
    let Some(account) = accounts.account(uid) else {
        return false;
    };
    if account.primary_gid != gid {
        return false;
    }
    let Some(group) = accounts.group(gid) else {
        return false;
    };
    let only_owner = match group.members.as_slice() {
        [] => true,
        [member] => *member == account.name,
        _ => false,
    };
    only_owner && !accounts.other_account_has_primary_group(gid, uid)
}

/// Why accounts other than its owner (`uid`) can change a file or directory with `mode` and
/// group `gid`, or `None` when they cannot. The reason starts "can be changed by other
/// accounts" and names the mode and the group; `path` names the path in the advice.
pub fn others_can_change(
    path: &Path,
    mode: u32,
    uid: u32,
    gid: u32,
    accounts: &dyn AccountDatabase,
) -> Option<String> {
    if mode & 0o022 == 0 {
        return None;
    }
    let group = accounts
        .group(gid)
        .map_or_else(|| gid.to_string(), |group| group.name);
    let shown = mode & 0o7777;
    if mode & 0o002 != 0 {
        return Some(format!(
            "can be changed by other accounts (mode {shown:o}, writable by everyone): run \
             chmod go-w {}",
            path.display()
        ));
    }
    if mode & 0o1000 != 0 {
        return Some(format!(
            "can be changed by other accounts (mode {shown:o}, sticky and writable by group \
             {group}): run chmod g-w {}",
            path.display()
        ));
    }
    if is_private_group(uid, gid, accounts) {
        return None;
    }
    Some(format!(
        "can be changed by other accounts (mode {shown:o}, writable by group {group}, which is \
         not a private group of its owner): run chmod g-w {}",
        path.display()
    ))
}

/// Creates `path` and its missing parents, never with umask-dependent modes: every missing
/// directory down to `shared` (an XDG base directory such as `~/.local/share`, or `path`
/// itself) is created with mode 0755, every one below it with 0700. Existing directories are
/// left as they are.
pub fn create_directories(path: &Path, shared: &Path) -> io::Result<()> {
    let mut missing = Vec::new();
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::NotFound => missing.push(ancestor),
            Err(error) => return Err(error),
        }
    }
    for directory in missing.into_iter().rev() {
        let mode = if shared.starts_with(directory) {
            0o755
        } else {
            0o700
        };
        create_directory(directory, mode)?;
    }
    Ok(())
}

/// Creates the directory `path` with exactly `mode` when it is missing; an existing one is
/// left as it is.
pub fn create_directory(path: &Path, mode: u32) -> io::Result<()> {
    match DirBuilder::new().mode(mode).create(path) {
        // The umask only narrows the mode; the explicit mode is set whatever it is.
        Ok(()) => fs::set_permissions(path, fs::Permissions::from_mode(mode)),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Accounts and groups a test names.
    #[derive(Default)]
    pub(crate) struct FakeAccounts {
        pub accounts: HashMap<u32, Account>,
        pub groups: HashMap<u32, Group>,
    }

    impl FakeAccounts {
        /// `uid` named `name` with the private group `gid` (named `name`, no members).
        pub(crate) fn private(uid: u32, name: &str, gid: u32) -> Self {
            let mut accounts = Self::default();
            accounts.add_account(uid, name, gid);
            accounts.add_group(gid, name, &[]);
            accounts
        }

        pub(crate) fn add_account(&mut self, uid: u32, name: &str, gid: u32) {
            self.accounts.insert(
                uid,
                Account {
                    name: name.to_owned(),
                    primary_gid: gid,
                },
            );
        }

        pub(crate) fn add_group(&mut self, gid: u32, name: &str, members: &[&str]) {
            self.groups.insert(
                gid,
                Group {
                    name: name.to_owned(),
                    members: members.iter().map(|member| (*member).to_owned()).collect(),
                },
            );
        }
    }

    impl AccountDatabase for FakeAccounts {
        fn account(&self, uid: u32) -> Option<Account> {
            self.accounts.get(&uid).cloned()
        }

        fn group(&self, gid: u32) -> Option<Group> {
            self.groups.get(&gid).cloned()
        }

        fn other_account_has_primary_group(&self, gid: u32, uid: u32) -> bool {
            self.accounts
                .iter()
                .any(|(other, account)| *other != uid && account.primary_gid == gid)
        }
    }

    const PATH: &str = "/home/ana/.local/share";

    fn check(mode: u32, uid: u32, gid: u32, accounts: &FakeAccounts) -> Option<String> {
        others_can_change(Path::new(PATH), mode, uid, gid, accounts)
    }

    #[test]
    fn the_owners_private_group_may_write() {
        let accounts = FakeAccounts::private(1000, "ana", 1000);
        assert!(is_private_group(1000, 1000, &accounts));
        assert_eq!(check(0o40775, 1000, 1000, &accounts), None);
        assert_eq!(check(0o100664, 1000, 1000, &accounts), None);
        assert_eq!(check(0o40755, 1000, 1000, &accounts), None);

        // The owner listed as the group's only member is still private.
        let mut accounts = FakeAccounts::private(1000, "ana", 1000);
        accounts.add_group(1000, "ana", &["ana"]);
        assert_eq!(check(0o40775, 1000, 1000, &accounts), None);
    }

    #[test]
    fn a_shared_group_with_other_members_is_refused() {
        let mut accounts = FakeAccounts::private(1000, "ana", 1000);
        accounts.add_group(1000, "ana", &["ana", "ben"]);
        let reason = check(0o40775, 1000, 1000, &accounts).unwrap();
        assert!(
            reason.starts_with("can be changed by other accounts (mode 775, writable by group ana"),
            "{reason}"
        );
        assert!(
            reason.ends_with(&format!("run chmod g-w {PATH}")),
            "{reason}"
        );

        accounts.add_group(1000, "ana", &["ben"]);
        assert!(check(0o40775, 1000, 1000, &accounts).is_some());

        // Another account whose primary group it is shares it too.
        let mut accounts = FakeAccounts::private(1000, "ana", 1000);
        accounts.add_account(1001, "ben", 1000);
        assert!(!is_private_group(1000, 1000, &accounts));
        assert!(check(0o40775, 1000, 1000, &accounts).is_some());
    }

    #[test]
    fn a_group_that_is_not_the_owners_primary_one_is_refused() {
        let mut accounts = FakeAccounts::private(1000, "ana", 1000);
        accounts.add_group(100, "users", &[]);
        let reason = check(0o40775, 1000, 100, &accounts).unwrap();
        assert!(reason.contains("writable by group users"), "{reason}");
        // Unknown accounts and groups are never private; the gid stands in for the name.
        let reason = check(0o40775, 4242, 4242, &FakeAccounts::default()).unwrap();
        assert!(reason.contains("writable by group 4242"), "{reason}");
        let mut accounts = FakeAccounts::default();
        accounts.add_account(1000, "ana", 1000);
        assert!(!is_private_group(1000, 1000, &accounts));
    }

    #[test]
    fn world_writable_and_sticky_stay_refused_in_a_private_group() {
        let accounts = FakeAccounts::private(1000, "ana", 1000);
        for mode in [0o40777, 0o40757, 0o41777, 0o100666] {
            let reason = check(mode, 1000, 1000, &accounts).unwrap();
            assert!(reason.contains("writable by everyone"), "{reason}");
            assert!(reason.ends_with(&format!("chmod go-w {PATH}")), "{reason}");
        }
        let reason = check(0o41775, 1000, 1000, &accounts).unwrap();
        assert!(reason.contains("sticky"), "{reason}");
    }

    #[test]
    fn other_primary_groups_are_found_in_a_passwd_text() {
        let passwd = "root:x:0:0:root:/root:/bin/bash\n\
                      # a comment\n\
                      ana:x:1000:1000::/home/ana:/bin/sh\n\
                      broken line\n\
                      ben:x:1001:1001::/home/ben:/bin/sh\n";
        assert!(!passwd_has_other_primary(passwd, 1000, 1000));
        assert!(passwd_has_other_primary(passwd, 1001, 1000));
        assert!(!passwd_has_other_primary(passwd, 4242, 1000));
    }

    #[test]
    fn missing_directories_get_fixed_modes() {
        // The process umask is shared by every test thread, so it is left alone here; the
        // explicit `set_permissions` is what makes the modes independent of it.
        let top = tempfile::tempdir().unwrap();
        let share = top.path().join("home/.local/share");
        let store = share.join("org.silo.dev/credentials");
        create_directories(&store, &share).unwrap();
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode(&top.path().join("home")), 0o755);
        assert_eq!(mode(&top.path().join("home/.local")), 0o755);
        assert_eq!(mode(&share), 0o755);
        assert_eq!(mode(&share.join("org.silo.dev")), 0o700);
        assert_eq!(mode(&store), 0o700);
        // Existing directories keep their modes.
        fs::set_permissions(&share, fs::Permissions::from_mode(0o750)).unwrap();
        create_directories(&store, &share).unwrap();
        assert_eq!(mode(&share), 0o750);
    }
}
