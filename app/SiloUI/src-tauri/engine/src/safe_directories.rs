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
//! owner (`getgrgid_r`: no members, or only the owner's name). Silo adds three conditions:
//!
//! - Root gets no exemption: a path owned by uid 0, or in gid 0, is never in a private group.
//! - Only local accounts count. The owner's line in `/etc/passwd` must give the same name, uid
//!   and primary group as `getpwuid_r`, and `/etc/group` must have exactly one line for the
//!   group, with the same name and members as `getgrgid_r`. An account or group that comes
//!   from a network directory (LDAP, SSSD, NIS) is refused rather than trusted.
//! - No other account in `/etc/passwd` may have the group as its primary group.
//!
//! The files are parsed directly because walking the whole user database (`getpwent`) shares
//! one cursor across the process and is not thread-safe. A file that cannot be read, or is
//! larger than [`MAX_LOCAL_FILE_BYTES`], counts as unreadable and the group is refused. What
//! the rule cannot see: memberships a network directory adds to a local group, groups that
//! `pam_group` grants at login, `newgrp` with a group password in `/etc/gshadow`, and setgid
//! programs of the owner; each of those gives another account the group too.
//!
//! The lookups go through [`AccountDatabase`], so tests use their own accounts
//! ([`StaticAccounts`]).

use std::{
    collections::HashMap,
    ffi::CStr,
    fs,
    io::{self, Read},
    path::Path,
};

use rustix::{
    fd::{AsFd, OwnedFd},
    fs::{fchmod, fstat, mkdirat, openat, statat, AtFlags, Mode, OFlags, CWD},
    io::Errno,
};

/// The largest buffer a `getpwuid_r` or `getgrgid_r` lookup grows to.
const MAX_LOOKUP_BUFFER: usize = 1 << 20;
/// The largest `/etc/passwd` or `/etc/group` read; a larger one counts as unreadable.
pub const MAX_LOCAL_FILE_BYTES: u64 = 16 * 1024 * 1024;

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

/// The local account files, `/etc/passwd` and `/etc/group`, as text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalFiles {
    pub passwd: String,
    pub group: String,
}

/// The user and group lookups the private-group rule needs.
pub trait AccountDatabase {
    /// `uid`'s account, or `None` when the user database does not know it.
    fn account(&self, uid: u32) -> Option<Account>;
    /// `gid`'s group, or `None` when the group database does not know it.
    fn group(&self, gid: u32) -> Option<Group>;
    /// The local account files, or `None` when either cannot be read whole.
    fn local_files(&self) -> Option<LocalFiles>;
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

    fn local_files(&self) -> Option<LocalFiles> {
        Some(LocalFiles {
            passwd: read_capped(Path::new("/etc/passwd"), MAX_LOCAL_FILE_BYTES)?,
            group: read_capped(Path::new("/etc/group"), MAX_LOCAL_FILE_BYTES)?,
        })
    }
}

/// `path`'s text, or `None` when it cannot be read or is larger than `cap` bytes.
fn read_capped(path: &Path, cap: u64) -> Option<String> {
    let mut contents = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(cap + 1)
        .read_to_end(&mut contents)
        .ok()?;
    if contents.len() as u64 > cap {
        return None;
    }
    String::from_utf8(contents).ok()
}

/// Accounts and groups given in code, for tests here and in other crates. The local files
/// are made from them unless `passwd` or `group` replaces one.
#[derive(Clone, Debug, Default)]
pub struct StaticAccounts {
    pub accounts: HashMap<u32, Account>,
    pub groups: HashMap<u32, Group>,
    /// `/etc/passwd` as given, in place of one made from `accounts`.
    pub passwd: Option<String>,
    /// `/etc/group` as given, in place of one made from `groups`.
    pub group: Option<String>,
    /// The local files cannot be read.
    pub unreadable: bool,
}

impl StaticAccounts {
    /// `uid` named `name` with the private group `gid` (named `name`, no members).
    pub fn private(uid: u32, name: &str, gid: u32) -> Self {
        let mut accounts = Self::default();
        accounts.add_account(uid, name, gid);
        accounts.add_group(gid, name, &[]);
        accounts
    }

    pub fn add_account(&mut self, uid: u32, name: &str, gid: u32) {
        self.accounts.insert(
            uid,
            Account {
                name: name.to_owned(),
                primary_gid: gid,
            },
        );
    }

    pub fn add_group(&mut self, gid: u32, name: &str, members: &[&str]) {
        self.groups.insert(
            gid,
            Group {
                name: name.to_owned(),
                members: members.iter().map(|member| (*member).to_owned()).collect(),
            },
        );
    }
}

impl AccountDatabase for StaticAccounts {
    fn account(&self, uid: u32) -> Option<Account> {
        self.accounts.get(&uid).cloned()
    }

    fn group(&self, gid: u32) -> Option<Group> {
        self.groups.get(&gid).cloned()
    }

    fn local_files(&self) -> Option<LocalFiles> {
        if self.unreadable {
            return None;
        }
        let passwd = self.passwd.clone().unwrap_or_else(|| {
            self.accounts
                .iter()
                .map(|(uid, account)| {
                    format!(
                        "{}:x:{uid}:{}::/:/bin/sh\n",
                        account.name, account.primary_gid
                    )
                })
                .collect()
        });
        let group = self.group.clone().unwrap_or_else(|| {
            self.groups
                .iter()
                .map(|(gid, group)| format!("{}:x:{gid}:{}\n", group.name, group.members.join(",")))
                .collect()
        });
        Some(LocalFiles { passwd, group })
    }
}

/// The `name:password:id:...` lines of a `passwd(5)` or `group(5)` text with at least
/// `fields` fields and a numeric third one. Other lines are ignored, as the C library
/// ignores them.
fn entries(contents: &str, fields: usize) -> impl Iterator<Item = (Vec<&str>, u32)> {
    contents.lines().filter_map(move |line| {
        let parts = line.split(':').collect::<Vec<_>>();
        if parts.len() < fields {
            return None;
        }
        let id = parts[2].parse().ok()?;
        Some((parts, id))
    })
}

/// The local-file half of [`is_private_group`]: the owner's own `/etc/passwd` line agrees with
/// `account`, no other account there has `gid` as its primary group, and `/etc/group` has
/// exactly one line for `gid`, agreeing with `group`.
fn local_files_agree(
    uid: u32,
    account: &Account,
    gid: u32,
    group: &Group,
    files: &LocalFiles,
) -> bool {
    let mut own = false;
    for (fields, line_uid) in entries(&files.passwd, 4) {
        let Ok(line_gid) = fields[3].parse::<u32>() else {
            continue;
        };
        if line_uid == uid && fields[0] == account.name && line_gid == account.primary_gid {
            own = true;
        } else if line_gid == gid && line_uid != uid {
            return false;
        }
    }
    let lines = entries(&files.group, 4)
        .filter(|(_, line_gid)| *line_gid == gid)
        .collect::<Vec<_>>();
    let [(fields, _)] = lines.as_slice() else {
        return false;
    };
    let mut listed = fields[3]
        .split(',')
        .filter(|member| !member.is_empty())
        .collect::<Vec<_>>();
    let mut members = group.members.iter().map(String::as_str).collect::<Vec<_>>();
    listed.sort_unstable();
    members.sort_unstable();
    own && fields[0] == group.name && listed == members
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

/// Whether `gid` is a user-private group of `uid` (see the module documentation). Never for
/// root's account or group.
pub fn is_private_group(uid: u32, gid: u32, accounts: &dyn AccountDatabase) -> bool {
    if uid == 0 || gid == 0 {
        return false;
    }
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
    only_owner
        && accounts
            .local_files()
            .is_some_and(|files| local_files_agree(uid, &account, gid, &group, &files))
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

/// Creates `path` and its missing parents, never with umask-dependent modes and never
/// through a link: every missing directory down to `shared` (an XDG base directory such as
/// `~/.local/share`, or `path` itself) is created with mode 0755, every one below it with 0700.
/// Existing directories are left as they are.
///
/// It works through descriptors. The deepest existing directory is opened without following
/// a link and must belong to `euid` or root and pass [`others_can_change`], since another
/// account that could write there could swap what is created in it; each missing level is
/// then made with `mkdirat`, opened with `O_NOFOLLOW`, checked to be a directory of `euid`,
/// and given its mode with `fchmod`. A level another process made first must pass the same
/// check as the existing directory.
pub fn create_directories(
    path: &Path,
    shared: &Path,
    euid: u32,
    accounts: &dyn AccountDatabase,
) -> io::Result<()> {
    let mut missing = Vec::new();
    let mut existing = path;
    loop {
        match statat(CWD, existing, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(_) => break,
            Err(Errno::NOENT) => {
                missing.push(existing);
                existing = existing
                    .parent()
                    .ok_or_else(|| refused(path, "has no existing parent directory"))?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    let mut directory = open_directory(CWD, existing, existing)?;
    check_writable_by_this_account_only(existing, &directory, euid, accounts)?;
    for level in missing.into_iter().rev() {
        let name = level
            .file_name()
            .ok_or_else(|| refused(level, "has no name"))?;
        let mode = if shared.starts_with(level) {
            0o755
        } else {
            0o700
        };
        let created = match mkdirat(&directory, name, Mode::from_bits_truncate(mode)) {
            Ok(()) => true,
            Err(Errno::EXIST) => false,
            Err(error) => return Err(error.into()),
        };
        let next = open_directory(&directory, name, level)?;
        if created {
            if fstat(&next)?.st_uid != euid {
                return Err(refused(level, "is not a directory owned by this account"));
            }
            // The umask only narrows the mode; the explicit mode is set whatever it is.
            fchmod(&next, Mode::from_bits_truncate(mode))?;
        } else {
            check_writable_by_this_account_only(level, &next, euid, accounts)?;
        }
        directory = next;
    }
    Ok(())
}

/// Opens `name` below `parent` as a directory without following a link.
fn open_directory<P: rustix::path::Arg>(
    parent: impl AsFd,
    name: P,
    path: &Path,
) -> io::Result<OwnedFd> {
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    match openat(parent, name, flags, Mode::empty()) {
        Ok(directory) => Ok(directory),
        Err(error @ (Errno::LOOP | Errno::NOTDIR)) => {
            Err(refused(path, &format!("is not a directory ({error})")))
        }
        Err(error) => Err(error.into()),
    }
}

/// The opened `directory` belongs to `euid` or root and no other account can change it.
fn check_writable_by_this_account_only(
    path: &Path,
    directory: &OwnedFd,
    euid: u32,
    accounts: &dyn AccountDatabase,
) -> io::Result<()> {
    let stat = fstat(directory)?;
    if stat.st_uid != euid && stat.st_uid != 0 {
        return Err(refused(
            path,
            "belongs to another account, not this one or root",
        ));
    }
    match others_can_change(path, stat.st_mode, stat.st_uid, stat.st_gid, accounts) {
        Some(reason) => Err(refused(path, &reason)),
        None => Ok(()),
    }
}

fn refused(path: &Path, reason: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("{} {reason}", path.display()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    const PATH: &str = "/home/ana/.local/share";

    fn check(mode: u32, uid: u32, gid: u32, accounts: &StaticAccounts) -> Option<String> {
        others_can_change(Path::new(PATH), mode, uid, gid, accounts)
    }

    fn euid() -> u32 {
        // SAFETY: geteuid has no preconditions.
        unsafe { libc::geteuid() }
    }

    #[test]
    fn the_owners_private_group_may_write() {
        let accounts = StaticAccounts::private(1000, "ana", 1000);
        assert!(is_private_group(1000, 1000, &accounts));
        assert_eq!(check(0o40775, 1000, 1000, &accounts), None);
        assert_eq!(check(0o100664, 1000, 1000, &accounts), None);
        assert_eq!(check(0o40755, 1000, 1000, &accounts), None);

        // The owner listed as the group's only member is still private.
        let mut accounts = StaticAccounts::private(1000, "ana", 1000);
        accounts.add_group(1000, "ana", &["ana"]);
        assert_eq!(check(0o40775, 1000, 1000, &accounts), None);
    }

    #[test]
    fn a_shared_group_with_other_members_is_refused() {
        let mut accounts = StaticAccounts::private(1000, "ana", 1000);
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
        let mut accounts = StaticAccounts::private(1000, "ana", 1000);
        accounts.add_account(1001, "ben", 1000);
        assert!(!is_private_group(1000, 1000, &accounts));
        assert!(check(0o40775, 1000, 1000, &accounts).is_some());
    }

    #[test]
    fn a_group_that_is_not_the_owners_primary_one_is_refused() {
        let mut accounts = StaticAccounts::private(1000, "ana", 1000);
        accounts.add_group(100, "users", &[]);
        let reason = check(0o40775, 1000, 100, &accounts).unwrap();
        assert!(reason.contains("writable by group users"), "{reason}");
        // Unknown accounts and groups are never private; the gid stands in for the name.
        let reason = check(0o40775, 4242, 4242, &StaticAccounts::default()).unwrap();
        assert!(reason.contains("writable by group 4242"), "{reason}");
        let mut accounts = StaticAccounts::default();
        accounts.add_account(1000, "ana", 1000);
        assert!(!is_private_group(1000, 1000, &accounts));
    }

    #[test]
    fn world_writable_and_sticky_stay_refused_in_a_private_group() {
        let accounts = StaticAccounts::private(1000, "ana", 1000);
        for mode in [0o40777, 0o40757, 0o41777, 0o100666] {
            let reason = check(mode, 1000, 1000, &accounts).unwrap();
            assert!(reason.contains("writable by everyone"), "{reason}");
            assert!(reason.ends_with(&format!("chmod go-w {PATH}")), "{reason}");
        }
        let reason = check(0o41775, 1000, 1000, &accounts).unwrap();
        assert!(reason.contains("sticky"), "{reason}");
    }

    /// Root's account and root's group get no exemption, even when they look private.
    #[test]
    fn root_and_its_group_are_never_a_private_group() {
        let accounts = StaticAccounts::private(0, "root", 0);
        assert!(!is_private_group(0, 0, &accounts));
        let reason = check(0o40775, 0, 0, &accounts).unwrap();
        assert!(reason.contains("writable by group root"), "{reason}");
        let mut accounts = StaticAccounts::private(1000, "ana", 0);
        accounts.add_group(0, "root", &[]);
        assert!(check(0o40775, 1000, 0, &accounts).is_some());
    }

    /// Only an account and a group the local files list, exactly as the lookups give them,
    /// are trusted; a network account, a disagreeing line or unreadable files are refused.
    #[test]
    fn only_local_accounts_and_groups_are_trusted() {
        let private = || StaticAccounts::private(1000, "ana", 1000);
        let passwd = "root:x:0:0:root:/root:/bin/bash\n\
                      # a comment\n\
                      broken line\n\
                      ana:x:1000:1000::/home/ana:/bin/sh\n";
        let group = "root:x:0:\nana:x:1000:\n";
        let with = |passwd: &str, group: &str| {
            let mut accounts = private();
            accounts.passwd = Some(passwd.to_owned());
            accounts.group = Some(group.to_owned());
            is_private_group(1000, 1000, &accounts)
        };
        assert!(with(passwd, group));
        let mut listed = private();
        listed.add_group(1000, "ana", &["ana"]);
        listed.passwd = Some(passwd.to_owned());
        listed.group = Some("ana:x:1000:ana\n".to_owned());
        assert!(is_private_group(1000, 1000, &listed));
        // The file lists a member the lookup does not give, or the other way round.
        assert!(!with(passwd, "ana:x:1000:ana\n"));
        listed.group = Some(group.to_owned());
        assert!(!is_private_group(1000, 1000, &listed));

        // The owner is not in /etc/passwd (LDAP, SSSD), or is there with another gid or name.
        assert!(!with("root:x:0:0:root:/root:/bin/bash\n", group));
        assert!(!with("ana:x:1000:100::/home/ana:/bin/sh\n", group));
        assert!(!with("anna:x:1000:1000::/home/ana:/bin/sh\n", group));
        // Another local account has the group as its primary group.
        assert!(!with(
            &format!("{passwd}ben:x:1001:1000::/home/ben:/bin/sh\n"),
            group
        ));
        // The group is not in /etc/group, lists other members there, has another name, or
        // has two lines.
        assert!(!with(passwd, "root:x:0:\n"));
        assert!(!with(passwd, "ana:x:1000:ben\n"));
        assert!(!with(passwd, "anas:x:1000:\n"));
        assert!(!with(passwd, "ana:x:1000:\nana2:x:1000:\n"));

        let mut accounts = private();
        accounts.unreadable = true;
        assert!(!is_private_group(1000, 1000, &accounts));
    }

    #[test]
    fn a_local_file_over_the_cap_is_unreadable() {
        let top = tempfile::tempdir().unwrap();
        let file = top.path().join("passwd");
        fs::write(&file, "root:x:0:0\n").unwrap();
        assert_eq!(read_capped(&file, 11).as_deref(), Some("root:x:0:0\n"));
        assert_eq!(read_capped(&file, 10), None);
        assert_eq!(read_capped(&top.path().join("missing"), 10), None);
    }

    #[test]
    fn missing_directories_get_fixed_modes() {
        // The process umask is shared by every test thread, so it is left alone here; the
        // explicit `fchmod` is what makes the modes independent of it.
        let top = tempfile::tempdir().unwrap();
        let share = top.path().join("home/.local/share");
        let store = share.join("org.silo.dev/credentials");
        let accounts = StaticAccounts::default();
        create_directories(&store, &share, euid(), &accounts).unwrap();
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode(&top.path().join("home")), 0o755);
        assert_eq!(mode(&top.path().join("home/.local")), 0o755);
        assert_eq!(mode(&share), 0o755);
        assert_eq!(mode(&share.join("org.silo.dev")), 0o700);
        assert_eq!(mode(&store), 0o700);
        // Existing directories keep their modes.
        fs::set_permissions(&share, fs::Permissions::from_mode(0o750)).unwrap();
        create_directories(&store, &share, euid(), &accounts).unwrap();
        assert_eq!(mode(&share), 0o750);
    }

    /// Nothing is created in a directory others can change, through a link, or in another
    /// account's directory.
    #[test]
    fn directories_are_only_created_where_only_this_account_can_write() {
        let accounts = StaticAccounts::default();
        let top = tempfile::tempdir().unwrap();
        let open = top.path().join("open");
        fs::create_dir(&open).unwrap();
        for wide in [0o777, 0o1777, 0o775] {
            fs::set_permissions(&open, fs::Permissions::from_mode(wide)).unwrap();
            let error = create_directories(&open.join("a/b"), &open, euid(), &accounts)
                .unwrap_err()
                .to_string();
            assert!(
                error.contains(&format!(
                    "{} can be changed by other accounts",
                    open.display()
                )),
                "{error}"
            );
            assert!(!open.join("a").exists());
        }

        let elsewhere = top.path().join("elsewhere");
        fs::create_dir(&elsewhere).unwrap();
        let link = top.path().join("link");
        std::os::unix::fs::symlink(&elsewhere, &link).unwrap();
        let error = create_directories(&link.join("a"), &link, euid(), &accounts)
            .unwrap_err()
            .to_string();
        assert!(error.contains("is not a directory"), "{error}");
        assert_eq!(fs::read_dir(&elsewhere).unwrap().count(), 0);

        // Another account's directory: changing an owner needs root.
        let foreign = top.path().join("foreign");
        fs::create_dir(&foreign).unwrap();
        let path = std::ffi::CString::new(foreign.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: `path` is a valid NUL-terminated string.
        if unsafe { libc::lchown(path.as_ptr(), 4242, 4242) } == 0 {
            assert_eq!(fs::metadata(&foreign).unwrap().uid(), 4242);
            let error = create_directories(&foreign.join("a"), &foreign, euid(), &accounts)
                .unwrap_err()
                .to_string();
            assert!(error.contains("belongs to another account"), "{error}");
        }
    }
}
