//! The id mapping of a container computer: the engine's own user and group are the working account
//! inside the container (rootless Podman's `--userns=keep-id`).
//!
//! The holder's user namespace maps id 0 to the engine's user and ids 1..=65535 to its sub-id range
//! (`silo-ctr run`). The container's own user namespace, made by crun inside it, maps container ids
//! onto those:
//!
//! | container ids | holder ids | on the host |
//! |---|---|---|
//! | 0 ..= A-1 | 1 ..= A | sub-ids (root is a sub-id) |
//! | A | 0 | the engine's own user and group |
//! | A+1 ..= 65535 | A+1 ..= 65535 | sub-ids |
//!
//! `A` is the working account, 1001. A file the account writes is the engine's on disk, so the
//! engine reads the account's `0600` request files and the account reads the engine's `0600`
//! answers (the in-guest client's spool). Everything else, root included, is a sub-id.
//!
//! The same layout is what the image, the boot layer and the upper directory are owned by:
//! `silo-ctr userns --keep-id A` makes the namespace those are unpacked in.

/// The working account's user and group id in the container (`working-account.py`).
pub const AGENT_ID: u32 = 1001;

/// The size of a container's id range (0..=65535).
pub const RANGE: u32 = 65536;

/// One extent of an `/proc/<pid>/uid_map`: `size` ids from `container` map to `holder`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Extent {
    pub container: u32,
    pub holder: u32,
    pub size: u32,
}

/// The extents of the container's user namespace, as holder ids.
pub fn extents() -> [Extent; 3] {
    [
        Extent {
            container: 0,
            holder: 1,
            size: AGENT_ID,
        },
        Extent {
            container: AGENT_ID,
            holder: 0,
            size: 1,
        },
        Extent {
            container: AGENT_ID + 1,
            holder: AGENT_ID + 1,
            size: RANGE - (AGENT_ID + 1),
        },
    ]
}

/// The engine's identity on the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeepId {
    pub uid: u32,
    pub gid: u32,
}

impl KeepId {
    /// This process's own user and group.
    pub fn current() -> Self {
        // SAFETY: getuid and getgid have no preconditions and cannot fail.
        unsafe {
            Self {
                uid: libc::getuid(),
                gid: libc::getgid(),
            }
        }
    }

    /// The container's working account, from a file's owner on the host: `None` for any other owner
    /// (a sub-id, or a user of the host), whose files the engine does not treat as the account's.
    pub fn guest_owner(self, uid: u32, gid: u32) -> Option<(u32, u32)> {
        (uid == self.uid && gid == self.gid).then_some((AGENT_ID, AGENT_ID))
    }

    /// Whether `owner`, the user that owns an upper directory, shows it was made by this mapping: its
    /// root is a sub-id. An earlier build left it owned by the engine's own user.
    pub fn upper_owner_is_current(self, owner: u32) -> bool {
        owner != self.uid
    }
}

/// What to tell a person whose container computer was made before the id mapping.
pub fn recreate_message(name: &str) -> String {
    format!(
        "The container computer {name} was created by an earlier version of Silo whose file ownership is not compatible with this one. Recreate it (silo drop {name}, then create it again)."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extents_cover_every_container_id_once_and_hold_the_engine_at_the_account() {
        let extents = extents();
        let total: u32 = extents.iter().map(|e| e.size).sum();
        assert_eq!(total, RANGE);
        let mut next = 0;
        for extent in extents {
            assert_eq!(extent.container, next);
            next += extent.size;
        }
        // The account is the holder's id 0, which is the engine's own user.
        let account = extents.iter().find(|e| e.container == AGENT_ID).unwrap();
        assert_eq!((account.holder, account.size), (0, 1));
        // Holder ids are used once: root is holder 1, and 0 is used by the account only.
        let mut holders: Vec<u32> = extents
            .iter()
            .flat_map(|e| e.holder..e.holder + e.size)
            .collect();
        holders.sort_unstable();
        assert_eq!(holders, (0..RANGE).collect::<Vec<_>>());
    }

    #[test]
    fn only_the_engines_own_user_and_group_is_the_account() {
        let map = KeepId { uid: 1000, gid: 1000 };
        assert_eq!(map.guest_owner(1000, 1000), Some((AGENT_ID, AGENT_ID)));
        // A sub-id owner (a root file, another guest user) and a half match are not the account.
        assert_eq!(map.guest_owner(165536, 165536), None);
        assert_eq!(map.guest_owner(1000, 165536), None);
        assert_eq!(map.guest_owner(165536, 1000), None);
        assert_eq!(map.guest_owner(0, 0), None);
    }

    #[test]
    fn an_upper_owned_by_the_engine_is_from_an_earlier_build() {
        let map = KeepId { uid: 1000, gid: 1000 };
        assert!(!map.upper_owner_is_current(1000));
        assert!(map.upper_owner_is_current(165536));
        assert!(recreate_message("box").contains("Recreate it"));
    }
}
