//! The names a stopped macOS computer calls itself, read and written on its mounted Data
//! volume (`scutil --set ComputerName/LocalHostName/HostName` keeps them in the system
//! configuration preferences). A promote gives the new main the hostname of the computer it
//! replaced, so what knows main by its hostname keeps finding it.

use plist::{Dictionary, Value};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

const PREFERENCES: &str = "Library/Preferences/SystemConfiguration/preferences.plist";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Hostnames {
    pub computer: Option<String>,
    pub local: Option<String>,
    pub host: Option<String>,
}

fn load(root: &Path) -> Result<Dictionary, String> {
    match Value::from_file(root.join(PREFERENCES)) {
        Ok(Value::Dictionary(dictionary)) => Ok(dictionary),
        Ok(_) => Err("The computer's network settings are not readable.".into()),
        Err(error) if is_missing(&error) => Ok(Dictionary::new()),
        Err(_) => Err("The computer's network settings are not readable.".into()),
    }
}

fn is_missing(error: &plist::Error) -> bool {
    error
        .as_io()
        .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound)
}

fn string(dictionary: &Dictionary, path: &[&str]) -> Option<String> {
    let (last, parents) = path.split_last()?;
    let mut current = dictionary;
    for key in parents {
        current = current.get(key)?.as_dictionary()?;
    }
    current.get(last)?.as_string().map(str::to_owned)
}

fn set(dictionary: &mut Dictionary, path: &[&str], value: Option<&str>) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let mut current = dictionary;
    for key in parents {
        if !matches!(current.get(key), Some(Value::Dictionary(_))) {
            if value.is_none() {
                return;
            }
            current.insert((*key).to_owned(), Value::Dictionary(Dictionary::new()));
        }
        let Some(Value::Dictionary(next)) = current.get_mut(key) else {
            return;
        };
        current = next;
    }
    match value {
        Some(value) => {
            current.insert((*last).to_owned(), Value::String(value.to_owned()));
        }
        None => {
            current.remove(last);
        }
    }
}

const COMPUTER: [&str; 3] = ["System", "System", "ComputerName"];
const HOST: [&str; 3] = ["System", "System", "HostName"];
const LOCAL: [&str; 4] = ["System", "Network", "HostNames", "LocalHostName"];

/// The names the computer has now. A computer that never set one reports none.
pub(super) fn read(root: &Path) -> Result<Hostnames, String> {
    let dictionary = load(root)?;
    Ok(Hostnames {
        computer: string(&dictionary, &COMPUTER),
        local: string(&dictionary, &LOCAL),
        host: string(&dictionary, &HOST),
    })
}

/// Makes these the computer's names; a name that is none is removed. Repeating it changes
/// nothing.
pub(super) fn write(root: &Path, names: &Hostnames) -> Result<(), String> {
    let mut dictionary = load(root)?;
    set(&mut dictionary, &COMPUTER, names.computer.as_deref());
    set(&mut dictionary, &LOCAL, names.local.as_deref());
    set(&mut dictionary, &HOST, names.host.as_deref());
    let path = root.join(PREFERENCES);
    if fs::symlink_metadata(&path).is_err() {
        if *names == Hostnames::default() {
            return Ok(());
        }
        return Err("The computer has no network settings file to change.".into());
    }
    Value::Dictionary(dictionary)
        .to_file_xml(&path)
        .map_err(|_| "The computer's network settings could not be saved.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume(names: &Hostnames) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(PREFERENCES);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut dictionary = Dictionary::new();
        dictionary.insert("Model".into(), Value::String("VirtualMac2,1".into()));
        Value::Dictionary(dictionary).to_file_xml(&path).unwrap();
        write(root.path(), names).unwrap();
        root
    }

    fn names(computer: &str) -> Hostnames {
        Hostnames {
            computer: Some(computer.into()),
            local: Some(computer.into()),
            host: Some(computer.into()),
        }
    }

    #[test]
    fn names_round_trip_and_leave_the_rest_of_the_file_alone() {
        let root = volume(&names("main"));
        assert_eq!(read(root.path()).unwrap(), names("main"));
        write(root.path(), &names("try-it")).unwrap();
        write(root.path(), &names("try-it")).unwrap();
        assert_eq!(read(root.path()).unwrap(), names("try-it"));
        let kept = Value::from_file(root.path().join(PREFERENCES)).unwrap();
        assert_eq!(
            kept.as_dictionary()
                .unwrap()
                .get("Model")
                .unwrap()
                .as_string(),
            Some("VirtualMac2,1")
        );
    }

    #[test]
    fn a_name_that_was_never_set_stays_unset() {
        let root = volume(&Hostnames::default());
        assert_eq!(read(root.path()).unwrap(), Hostnames::default());
        write(root.path(), &names("main")).unwrap();
        write(root.path(), &Hostnames::default()).unwrap();
        assert_eq!(read(root.path()).unwrap(), Hostnames::default());
    }

    #[test]
    fn a_volume_without_network_settings_reads_as_unset_and_refuses_a_write() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(read(root.path()).unwrap(), Hostnames::default());
        assert!(write(root.path(), &names("main")).is_err());
    }
}
