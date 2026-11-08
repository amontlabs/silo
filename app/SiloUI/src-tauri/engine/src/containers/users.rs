//! Resolving a user name to the numeric ids `crun exec` wants, from the computer's own
//! `/etc/passwd` and `/etc/group` as the overlay shows them: the writable layer, then the
//! sealed layers, the shim layer and the image.

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub uid: u32,
    pub gid: u32,
    pub home: String,
}

/// Reads `relative` (such as `etc/passwd`) from the first layer that has it.
pub fn read_from_layers(layers: &[PathBuf], relative: &str) -> Option<String> {
    layers
        .iter()
        .find_map(|layer| std::fs::read_to_string(layer.join(relative)).ok())
}

/// `name` as a number (`1001`, `1001:1001`) or looked up in `passwd` and `group`.
pub fn resolve(name: &str, passwd: &str, group: &str) -> Option<Identity> {
    let (user, group_name) = match name.split_once(':') {
        Some((user, group)) => (user, Some(group)),
        None => (name, None),
    };
    let entry = passwd
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| line.split(':').collect::<Vec<_>>())
        .find(|fields| fields.len() >= 6 && (fields[0] == user || fields[2] == user));
    let (uid, mut gid, home) = match entry {
        Some(fields) => (fields[2].parse().ok()?, fields[3].parse().ok()?, fields[5].to_owned()),
        None => {
            let number = user.parse::<u32>().ok()?;
            (number, number, "/".to_owned())
        }
    };
    if let Some(group_name) = group_name {
        gid = group
            .lines()
            .map(|line| line.split(':').collect::<Vec<_>>())
            .find(|fields| fields.len() >= 3 && (fields[0] == group_name || fields[2] == group_name))
            .and_then(|fields| fields[2].parse().ok())
            .or_else(|| group_name.parse().ok())?;
    }
    Some(Identity { uid, gid, home })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWD: &str = "root:x:0:0:root:/root:/bin/bash\nsilo:x:1001:1001::/home/silo:/bin/bash\n";
    const GROUP: &str = "root:x:0:\nsilo:x:1001:\nsudo:x:27:silo\nvideo:x:44:silo,other\n";

    #[test]
    fn names_resolve_to_ids_and_home() {
        let silo = resolve("silo", PASSWD, GROUP).unwrap();
        assert_eq!((silo.uid, silo.gid, silo.home.as_str()), (1001, 1001, "/home/silo"));
        let root = resolve("root", PASSWD, GROUP).unwrap();
        assert_eq!((root.uid, root.gid, root.home.as_str()), (0, 0, "/root"));
    }

    #[test]
    fn numbers_and_explicit_groups_resolve() {
        assert_eq!(resolve("1001", PASSWD, GROUP).unwrap().home, "/home/silo");
        let unknown = resolve("4242", PASSWD, GROUP).unwrap();
        assert_eq!((unknown.uid, unknown.gid, unknown.home.as_str()), (4242, 4242, "/"));
        assert_eq!(resolve("silo:video", PASSWD, GROUP).unwrap().gid, 44);
        assert_eq!(resolve("nobody-here", PASSWD, GROUP), None);
    }

    #[test]
    fn the_first_layer_with_the_file_wins() {
        let upper = tempfile::tempdir().unwrap();
        let lower = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(lower.path().join("etc")).unwrap();
        std::fs::write(lower.path().join("etc/passwd"), "lower").unwrap();
        let layers = [upper.path().to_path_buf(), lower.path().to_path_buf()];
        assert_eq!(read_from_layers(&layers, "etc/passwd").as_deref(), Some("lower"));
        std::fs::create_dir_all(upper.path().join("etc")).unwrap();
        std::fs::write(upper.path().join("etc/passwd"), "upper").unwrap();
        assert_eq!(read_from_layers(&layers, "etc/passwd").as_deref(), Some("upper"));
    }
}
