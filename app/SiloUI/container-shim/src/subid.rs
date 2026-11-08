//! The caller's sub-uid and sub-gid ranges (`/etc/subuid`, `/etc/subgid`).

/// Ids a container needs besides its root: 1..=65535.
pub const RANGE: u32 = 65535;

/// The start of the first range in `content` (lines `name:start:count`) that belongs to `user` or to the
/// numeric id `ident` and holds at least `RANGE` ids.
pub fn parse_range(content: &str, user: &str, ident: u32) -> Option<u32> {
    let ident = ident.to_string();
    content.lines().find_map(|line| {
        let mut fields = line.trim().split(':');
        let (name, start, count) = (fields.next()?, fields.next()?, fields.next()?);
        if fields.next().is_some() || (name != user && name != ident) {
            return None;
        }
        let start: u32 = start.parse().ok()?;
        let count: u32 = count.parse().ok()?;
        (count >= RANGE && start.checked_add(RANGE).is_some()).then_some(start)
    })
}

/// The range start from a file, or why there is none.
#[cfg(target_os = "linux")]
pub fn range_from_file(path: &str, user: &str, ident: u32) -> Result<u32, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|error| crate::common::io_err(&format!("read {path}"), &error))?;
    parse_range(&content, user, ident)
        .ok_or_else(|| format!("no sub-id range of {RANGE} for {user} in {path}"))
}

/// The account name of a numeric uid (`getpwuid_r`), or the number when it has no entry.
#[cfg(target_os = "linux")]
pub fn user_name(uid: u32) -> String {
    let mut buf = vec![0 as libc::c_char; 4096];
    // SAFETY: a zeroed passwd is a valid out value for getpwuid_r to fill.
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: `entry`, `buf` and `found` are valid for the sizes passed.
    let rc = unsafe { libc::getpwuid_r(uid, &mut entry, buf.as_mut_ptr(), buf.len(), &mut found) };
    if rc != 0 || found.is_null() {
        return uid.to_string();
    }
    // SAFETY: on success pw_name points at a NUL-terminated string inside `buf`.
    unsafe { std::ffi::CStr::from_ptr(entry.pw_name) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_matching_large_enough_range_wins() {
        let content = "other:100000:65536\nbob:200000:1000\nbob:300000:65536\n1000:400000:65536\n";
        assert_eq!(parse_range(content, "bob", 1000), Some(300000));
        assert_eq!(parse_range(content, "alice", 1000), Some(400000));
        assert_eq!(parse_range(content, "alice", 7), None);
    }

    #[test]
    fn the_exact_minimum_counts_and_bad_lines_are_skipped() {
        let content = "garbage\nbob:x:65535\nbob:1:\nbob:5:6:7\nbob:165536:65535\n";
        assert_eq!(parse_range(content, "bob", 1), Some(165536));
        assert_eq!(parse_range("bob:165536:65534\n", "bob", 1), None);
        assert_eq!(parse_range("bob:4294967295:65535\n", "bob", 1), None);
        assert_eq!(parse_range("", "bob", 1), None);
    }
}
