//! Offline account setup for an installed macOS guest: Setup Assistant is skipped,
//! the account `silo` exists with automatic login, Remote Login and passwordless
//! `sudo`, and the guest never sleeps or locks. The stopped computer's disk is
//! attached on the host and its Data volume edited in place.
//!
//! This is a Rust port of the offline setup in Lume's `MacOSOfflineSetupPatcher`
//! and `UnattendedInstaller` (<https://github.com/trycua/cua/tree/main/libs/lume>,
//! MIT license, commit `ba4c6369660ab4a9c4d3d8af942bc53ad376615f`), changed to
//! create the account `silo`, authorise a per-computer SSH key and allow
//! `sudo -n`. Some system plists are rewritten in place because launchd and
//! loginwindow can ignore atomically replaced files on the next boot.
use super::guest_access::{GuestAccount, USER};
use plist::{Dictionary, Value};
use sha2::Sha512;
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::Duration,
};

/// The exit status of the finalization script when the Preboot volume could not be updated.
pub(super) const PREBOOT_FAILED: i32 = 70;
pub(super) const UID: &str = "501";
pub(super) const GID: &str = "20";
const PBKDF2_ITERATIONS: u32 = 50_000;
const ENTROPY_BYTES: usize = 128;
const SALT_BYTES: usize = 32;
/// The XOR key loginwindow applies to `/etc/kcpassword`.
const KCPASSWORD_KEY: [u8; 11] = [
    0x7d, 0x89, 0x52, 0x23, 0xd2, 0xbc, 0xdd, 0xea, 0xa3, 0xb9, 0x1f,
];

/// The macOS release the guest was installed from, recorded so Setup Assistant
/// considers its own screens already seen.
pub(super) struct Release<'a> {
    pub version: &'a str,
    pub build: &'a str,
}

/// Creates the account and its settings on the disk of a stopped computer.
pub(super) fn run(
    disk: &Path,
    account: &GuestAccount,
    release: Option<Release<'_>>,
) -> Result<(), String> {
    let mut attached = Attached::attach(disk)?;
    let patched = attached
        .mount_data_volume()
        .and_then(|mount| patch(&mount, account, release.as_ref()));
    // The guest must never boot while the host still holds its disk.
    let closed = attached.close();
    patched.and(closed)
}

/// Replaces the credentials of the existing managed account on an imported disk.
/// This deliberately does not create an account or edit its home directory: an
/// import keeps the source computer's account and data identity until its first
/// password-authenticated setup boot.
pub(super) fn run_import_rekey(disk: &Path, account: &GuestAccount) -> Result<(), String> {
    let mut attached = Attached::attach(disk)?;
    let patched = attached
        .mount_data_volume()
        .and_then(|mount| rekey_import_data_volume(&mount, account));
    // Do not boot a computer while its disk is still mounted by the host.
    let closed = attached.close();
    patched.and(closed)
}

// MARK: Disk

/// A disk image attached to the host without mounting its volumes. `close`
/// unmounts whatever was mounted and detaches the image, reporting a failure;
/// dropping it does the same without reporting.
struct Attached {
    whole_disk: String,
    closed: bool,
}

impl Attached {
    fn attach(disk: &Path) -> Result<Self, String> {
        let output = tool(
            "/usr/bin/hdiutil",
            &["attach", "-readwrite", "-nomount", &disk.to_string_lossy()],
        )?;
        match parse_whole_disk(&output) {
            Some(whole_disk) => Ok(Self {
                whole_disk,
                closed: false,
            }),
            None => {
                // The image is attached but its device is unknown: detach by image.
                match ensure_detached(disk) {
                    Ok(()) => Err("Silo could not read the computer's disk.".into()),
                    Err(message) => Err(message),
                }
            }
        }
    }

    fn mount_data_volume(&self) -> Result<PathBuf, String> {
        let partition = format!("{}s2", self.whole_disk);
        let info = tool_plist("/usr/sbin/diskutil", &["info", "-plist", &partition])?;
        let container = info
            .get("APFSContainerReference")
            .and_then(Value::as_string)
            .ok_or("Silo could not find the computer's macOS volume.")?;
        let list = tool_plist("/usr/sbin/diskutil", &["apfs", "list", "-plist", container])?;
        let device = data_volume_device(&list)
            .ok_or("Silo could not find the Data volume of the computer.")?;
        tool("/usr/sbin/diskutil", &["mount", &device])?;
        let info = tool_plist("/usr/sbin/diskutil", &["info", "-plist", &device])?;
        info.get("MountPoint")
            .and_then(Value::as_string)
            .filter(|mount| !mount.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| "Silo could not mount the Data volume of the computer.".into())
    }
}

impl Attached {
    /// Unmounts every volume and detaches the image. Fails if the image is still attached.
    fn close(&mut self) -> Result<(), String> {
        if self.closed {
            return Ok(());
        }
        let device = format!("/dev/{}", self.whole_disk);
        if tool("/usr/sbin/diskutil", &["unmountDisk", &device]).is_err() {
            let _ = tool("/usr/sbin/diskutil", &["unmountDisk", "force", &device]);
        }
        for _ in 0..3 {
            if tool("/usr/bin/hdiutil", &["detach", &device]).is_ok() {
                self.closed = true;
                return Ok(());
            }
            thread::sleep(Duration::from_millis(500));
        }
        tool("/usr/bin/hdiutil", &["detach", "-force", &device])
            .map(|_| self.closed = true)
            .map_err(|error| {
                format!("Silo could not release the computer's disk from this Mac. {error}")
            })
    }
}

impl Drop for Attached {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

/// Detaches every attachment of `disk` on this Mac and fails if one remains.
/// The host's own list of attached images is the record, so this also finds an
/// attachment an earlier run or launch left behind.
pub(super) fn ensure_detached(disk: &Path) -> Result<(), String> {
    detach_matching(
        disk,
        &|| tool_plist("/usr/bin/hdiutil", &["info", "-plist"]),
        &|device| tool("/usr/bin/hdiutil", &["detach", "-force", device]).map(|_| ()),
    )
}

fn detach_matching(
    disk: &Path,
    info: &dyn Fn() -> Result<Dictionary, String>,
    detach: &dyn Fn(&str) -> Result<(), String>,
) -> Result<(), String> {
    let devices = attached_devices(&info()?, disk);
    if devices.is_empty() {
        return Ok(());
    }
    for device in &devices {
        let _ = detach(device);
    }
    if attached_devices(&info()?, disk).is_empty() {
        Ok(())
    } else {
        Err("The computer's disk is still attached to this Mac. Eject it in Disk Utility, then try again.".into())
    }
}

/// The whole-disk devices that `hdiutil info` lists for the image at `disk`.
fn attached_devices(info: &Dictionary, disk: &Path) -> Vec<String> {
    let target = fs::canonicalize(disk).unwrap_or_else(|_| disk.to_path_buf());
    let images = info.get("images").and_then(Value::as_array);
    let mut devices = Vec::new();
    for image in images
        .into_iter()
        .flatten()
        .filter_map(Value::as_dictionary)
    {
        let path = image.get("image-path").and_then(Value::as_string);
        if path.map(|path| fs::canonicalize(path).unwrap_or_else(|_| path.into()))
            != Some(target.clone())
        {
            continue;
        }
        let entities = image.get("system-entities").and_then(Value::as_array);
        devices.extend(
            entities
                .into_iter()
                .flatten()
                .filter_map(Value::as_dictionary)
                .filter_map(|entity| entity.get("dev-entry").and_then(Value::as_string))
                .filter(|device| is_whole_disk(device))
                .map(String::from),
        );
    }
    devices
}

/// Whether `device` (`/dev/disk4`) names a whole disk rather than a partition (`disk4s2`).
fn is_whole_disk(device: &str) -> bool {
    device
        .trim_start_matches("/dev/")
        .strip_prefix("disk")
        .is_some_and(|number| {
            !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
        })
}

/// The whole-disk node (`disk4`) from `hdiutil attach` output: the line whose
/// second field is the GUID partition scheme.
fn parse_whole_disk(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let device = fields.next()?;
        (fields.next()? == "GUID_partition_scheme")
            .then(|| device.trim_start_matches("/dev/").into())
    })
}

fn data_volume_device(list: &Dictionary) -> Option<String> {
    let containers = list.get("Containers")?.as_array()?;
    containers
        .iter()
        .filter_map(Value::as_dictionary)
        .filter_map(|container| container.get("Volumes")?.as_array())
        .flatten()
        .filter_map(Value::as_dictionary)
        .find(|volume| {
            volume
                .get("Roles")
                .and_then(Value::as_array)
                .is_some_and(|roles| roles.iter().any(|role| role.as_string() == Some("Data")))
        })
        .and_then(|volume| {
            volume
                .get("DeviceIdentifier")?
                .as_string()
                .map(String::from)
        })
}

fn tool(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("Silo could not run {program}: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(format!(
            "{program} {} failed: {}",
            args.first().copied().unwrap_or_default(),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn tool_plist(program: &str, args: &[&str]) -> Result<Dictionary, String> {
    let output = tool(program, args)?;
    Value::from_reader(std::io::Cursor::new(output.as_bytes()))
        .ok()
        .and_then(Value::into_dictionary)
        .ok_or_else(|| format!("{program} did not return a property list."))
}

// MARK: Patching

/// What `run` returns when the guest was stopped before macOS wrote its launchd
/// state. Remote Login is switched on in that state, and a guest patched without it
/// boots with sshd off, so nothing is written until a longer first boot has made it.
pub(super) const FIRST_BOOT_INCOMPLETE: &str = "macOS had not finished its first start.";

/// Where launchd keeps which services are disabled; written during the first boot.
const LAUNCHD_DISABLED: &str = "private/var/db/com.apple.xpc.launchd/disabled.plist";

fn patch(root: &Path, account: &GuestAccount, release: Option<&Release<'_>>) -> Result<(), String> {
    if !root.join(LAUNCHD_DISABLED).exists() {
        return Err(FIRST_BOOT_INCOMPLETE.into());
    }
    let users = root.join("private/var/db/dslocal/nodes/Default/users");
    for dir in [users.parent().unwrap_or(&users), &users] {
        make_executable(dir)?;
    }
    let uuid = create_user(&users, account)?;
    add_to_groups(
        &root.join("private/var/db/dslocal/nodes/Default/groups"),
        &uuid,
    )?;
    create_home(root)?;
    mark_setup_complete(root, release)?;
    configure_autologin(root, &account.password)?;
    enable_ssh(root)?;
    configure_power_and_lock(root)
}

/// Rekeys only the existing managed account, its automatic login, and sshd.
/// Validate the complete identity before making any change so an unrelated or
/// damaged account cannot be silently adopted as Silo's imported account.
fn rekey_import_data_volume(root: &Path, account: &GuestAccount) -> Result<(), String> {
    if account.user != USER || account.password.is_empty() {
        return Err("Silo could not identify the imported computer's managed account.".into());
    }

    for (relative, allow_missing) in [
        (
            "private/var/db/dslocal/nodes/Default/users/silo.plist",
            false,
        ),
        ("Library/Preferences/com.apple.loginwindow.plist", true),
        ("private/etc/kcpassword", true),
        (LAUNCHD_DISABLED, true),
        (
            "private/var/db/com.apple.xpc.launchd/disabled.migrated",
            true,
        ),
        ("Users/silo/.ssh/authorized_keys", false),
        (
            "Users/silo/Library/LaunchAgents/org.silo.computer-use-reconcile.plist",
            true,
        ),
        (
            "Users/silo/Library/LaunchAgents/org.silo.guest-recorder.plist",
            true,
        ),
        (
            "Users/silo/Library/Application Support/Silo/computer-use-receipt.json",
            true,
        ),
        (
            "Library/Application Support/Silo/developer-tools-receipt.json",
            true,
        ),
    ] {
        validate_import_data_path(root, Path::new(relative), allow_missing)?;
    }

    let path = root.join("private/var/db/dslocal/nodes/Default/users/silo.plist");
    let mut user = read_plist(&path)?;
    validate_import_account_identity(&user)?;

    let salt = random_bytes(SALT_BYTES)?;
    user.insert(
        "ShadowHashData".into(),
        Value::Array(vec![Value::Data(shadow_hash_data(
            &account.password,
            &salt,
        )?)]),
    );
    normalize_import_shadow_hash_authority(&mut user);
    write_plist(&user, &path, 0o600, true, Format::Binary)?;
    configure_autologin(root, &account.password)?;
    remove_import_managed_file(root, "Users/silo/.ssh/authorized_keys")?;
    for relative in [
        "Users/silo/Library/LaunchAgents/org.silo.computer-use-reconcile.plist",
        "Users/silo/Library/LaunchAgents/org.silo.guest-recorder.plist",
        "Users/silo/Library/Application Support/Silo/computer-use-receipt.json",
        "Library/Application Support/Silo/developer-tools-receipt.json",
    ] {
        remove_import_managed_file(root, relative)?;
    }
    disable_import_screen_sharing(root)?;
    enable_ssh(root)
}

/// Prevent the copied guest's Silo automation and key from running before the new
/// computer has completed Retry setup. The setup scripts install fresh copies.
fn remove_import_managed_file(root: &Path, relative: &str) -> Result<(), String> {
    let path = validate_import_data_path(root, Path::new(relative), true)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(fs_error("clear inherited Silo setup state", &error)),
    }
}

/// Screen Sharing is a guest service enabled by Silo's Computer Use setup. Keep
/// the cloned service disabled on the first boot; setup will grant and enable it
/// again after the imported identity has been finalized.
fn disable_import_screen_sharing(root: &Path) -> Result<(), String> {
    let path = root.join(LAUNCHD_DISABLED);
    let mut disabled = read_plist_if_present(&path)?;
    disabled.insert("com.apple.screensharing".into(), Value::Boolean(true));
    write_plist(&disabled, &path, 0o644, path.exists(), Format::Xml)
}

/// Reject links in the mounted, archive-derived Data volume before privileged
/// reads or writes. The import disk is stopped and exclusively attached here, so
/// checking each component also prevents a planted parent symlink from redirecting
/// the account or login files into the host filesystem.
fn validate_import_data_path(
    root: &Path,
    relative: &Path,
    allow_missing: bool,
) -> Result<PathBuf, String> {
    use std::path::Component;

    let root_metadata = fs::symlink_metadata(root)
        .map_err(|error| fs_error("inspect the imported computer's Data volume", &error))?;
    if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
        return Err("Silo could not safely access the imported computer's Data volume.".into());
    }

    let components: Vec<_> = relative.components().collect();
    if components.is_empty()
        || components
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("Silo could not safely access the imported computer's account files.".into());
    }

    let mut path = root.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            unreachable!("components were validated above")
        };
        path.push(name);
        let leaf = index + 1 == components.len();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if allow_missing && error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(root.join(relative));
            }
            Err(error) => {
                return Err(fs_error(
                    "inspect the imported computer's account files",
                    &error,
                ));
            }
        };
        if metadata.file_type().is_symlink()
            || (leaf && !metadata.is_file())
            || (!leaf && !metadata.is_dir())
        {
            return Err(
                "Silo could not safely access the imported computer's account files.".into(),
            );
        }
    }
    Ok(path)
}

fn validate_import_account_identity(user: &Dictionary) -> Result<(), String> {
    let generated_uid = required_import_account_field(user, "generateduid")?;
    if uuid::Uuid::parse_str(generated_uid).is_err()
        || required_import_account_field(user, "name")? != USER
        || required_import_account_field(user, "uid")? != UID
        || required_import_account_field(user, "gid")? != GID
        || required_import_account_field(user, "home")? != "/Users/silo"
    {
        return Err(
            "Silo could not verify the imported computer's managed account identity.".into(),
        );
    }
    validate_import_authentication_authorities(user)?;
    Ok(())
}

/// macOS augments Silo's initial ShadowHash authority after first boot. Accept
/// the two observed password-hash variants and the matching local Kerberos and
/// SecureToken entries, in any order, while rejecting any authority Silo did
/// not create or observe. Import keeps the account identity but replaces these
/// source-machine authentication methods with the one verifier it writes.
fn validate_import_authentication_authorities(user: &Dictionary) -> Result<(), String> {
    const SINGLE_HASH: &str = ";ShadowHash;HASHLIST:<SALTED-SHA512-PBKDF2>";
    const MACOS_HASH: &str =
        ";ShadowHash;HASHLIST:<SALTED-SHA512-PBKDF2,SRP-RFC5054-4096-SHA512-PBKDF2>";
    const SECURE_TOKEN: &str = ";SecureToken;";

    let invalid =
        || Err("Silo could not verify the imported computer's managed account identity.".into());
    let authorities = user
        .get("authentication_authority")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            "Silo could not verify the imported computer's managed account identity.".to_string()
        })?;
    let mut saw_shadow_hash = false;
    let mut saw_kerberos = false;
    let mut saw_secure_token = false;
    for authority in authorities {
        let Some(authority) = authority.as_string() else {
            return invalid();
        };
        match authority {
            SINGLE_HASH | MACOS_HASH if !saw_shadow_hash => saw_shadow_hash = true,
            SINGLE_HASH | MACOS_HASH => return invalid(),
            SECURE_TOKEN if !saw_secure_token => saw_secure_token = true,
            SECURE_TOKEN => return invalid(),
            _ if !saw_kerberos && is_silo_local_kerberos_authority(authority) => {
                saw_kerberos = true
            }
            _ => return invalid(),
        }
    }
    if saw_shadow_hash {
        Ok(())
    } else {
        invalid()
    }
}

/// ShadowHashData contains only the salted verifier after import rekeying. Do
/// not retain the source machine's LKDC authority or password-wrapped
/// SecureToken state, or advertise macOS's SRP method without its verifier.
fn normalize_import_shadow_hash_authority(user: &mut Dictionary) {
    const SINGLE_HASH: &str = ";ShadowHash;HASHLIST:<SALTED-SHA512-PBKDF2>";
    user.insert("authentication_authority".into(), string_list(SINGLE_HASH));
}

fn is_silo_local_kerberos_authority(authority: &str) -> bool {
    let Some(body) = authority.strip_prefix(";Kerberosv5;;silo@LKDC:SHA1.") else {
        return false;
    };
    let Some((principal_hash, realm)) = body.split_once(';') else {
        return false;
    };
    let Some(realm_hash) = realm
        .strip_prefix("LKDC:SHA1.")
        .and_then(|value| value.strip_suffix(';'))
    else {
        return false;
    };
    principal_hash.len() == 40
        && principal_hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        && realm_hash == principal_hash
}

fn required_import_account_field<'a>(user: &'a Dictionary, name: &str) -> Result<&'a str, String> {
    let values = user
        .get(name)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("Silo could not verify the imported account's {name}."))?;
    if values.len() != 1 {
        return Err(format!(
            "Silo could not verify the imported account's {name}."
        ));
    }
    values[0]
        .as_string()
        .ok_or_else(|| format!("Silo could not verify the imported account's {name}."))
}

fn string_list(value: &str) -> Value {
    Value::Array(vec![Value::String(value.into())])
}

/// Writes (or updates) the account's directory-services record and returns its UUID.
fn create_user(users: &Path, account: &GuestAccount) -> Result<String, String> {
    let path = users.join(format!("{USER}.plist"));
    let existing = read_plist_if_present(&path)?;
    let known = path.exists();
    if !known {
        ensure_uid_available(users)?;
    }
    let uuid = existing
        .get("generateduid")
        .and_then(Value::as_array)
        .and_then(|list| list.first())
        .and_then(Value::as_string)
        .map_or_else(
            || uuid::Uuid::new_v4().to_string().to_uppercase(),
            String::from,
        );
    let salt = random_bytes(SALT_BYTES)?;
    let record = user_record(existing, &account.password, &uuid, &salt)?;
    write_plist(&record, &path, 0o600, known, Format::Binary)?;
    Ok(uuid)
}

/// The account's record: the existing one with its credentials replaced, or a new one.
fn user_record(
    existing: Dictionary,
    password: &str,
    uuid: &str,
    salt: &[u8],
) -> Result<Dictionary, String> {
    let mut record = existing;
    if record.is_empty() {
        for key in [
            "_writers_UserCertificate",
            "_writers_hint",
            "_writers_jpegphoto",
            "_writers_passwd",
            "_writers_picture",
            "_writers_realname",
        ] {
            record.insert(key.into(), string_list(USER));
        }
        record.insert("record_daemon_version".into(), string_list("9040000"));
        record.insert("unlockOptions".into(), string_list("0"));
    }
    record.insert(
        "ShadowHashData".into(),
        Value::Array(vec![Value::Data(shadow_hash_data(password, salt)?)]),
    );
    record.insert(
        "authentication_authority".into(),
        string_list(";ShadowHash;HASHLIST:<SALTED-SHA512-PBKDF2>"),
    );
    record.insert("generateduid".into(), string_list(uuid));
    record.insert("gid".into(), string_list(GID));
    record.insert("home".into(), string_list(&format!("/Users/{USER}")));
    record.insert("name".into(), string_list(USER));
    record.insert("passwd".into(), string_list("********"));
    record.insert("realname".into(), string_list(USER));
    record.insert("shell".into(), string_list("/bin/zsh"));
    record.insert("uid".into(), string_list(UID));
    Ok(record)
}

fn ensure_uid_available(users: &Path) -> Result<(), String> {
    let entries = fs::read_dir(users).map_err(|error| fs_error("read the accounts", &error))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .extension()
            .is_none_or(|extension| extension != "plist")
        {
            continue;
        }
        let uid = read_plist_value(&path).ok().and_then(|record| {
            let record = record.into_dictionary()?;
            let list = record.get("uid")?.as_array()?;
            list.first()?.as_string().map(String::from)
        });
        if uid.as_deref() == Some(UID) {
            return Err(format!(
                "The computer already has an account with user ID {UID}."
            ));
        }
    }
    Ok(())
}

/// The `ShadowHashData` value: a binary property list holding the salted PBKDF2 hash.
fn shadow_hash_data(password: &str, salt: &[u8]) -> Result<Vec<u8>, String> {
    let entropy = pbkdf2_sha512(password.as_bytes(), salt, PBKDF2_ITERATIONS, ENTROPY_BYTES);
    let mut hash = Dictionary::new();
    hash.insert("entropy".into(), Value::Data(entropy));
    hash.insert(
        "iterations".into(),
        Value::Integer(u64::from(PBKDF2_ITERATIONS).into()),
    );
    hash.insert("salt".into(), Value::Data(salt.to_vec()));
    let mut shadow = Dictionary::new();
    shadow.insert("SALTED-SHA512-PBKDF2".into(), Value::Dictionary(hash));
    let mut bytes = Vec::new();
    Value::Dictionary(shadow)
        .to_writer_binary(&mut bytes)
        .map_err(|error| format!("Silo could not encode the account's password: {error}"))?;
    Ok(bytes)
}

fn pbkdf2_sha512(password: &[u8], salt: &[u8], iterations: u32, length: usize) -> Vec<u8> {
    use hmac::{Hmac, Mac};
    let prf = Hmac::<Sha512>::new_from_slice(password).expect("HMAC accepts keys of any length");
    let mut derived = Vec::with_capacity(length);
    let mut block_index = 1u32;
    while derived.len() < length {
        let mut mac = prf.clone();
        mac.update(salt);
        mac.update(&block_index.to_be_bytes());
        let mut u = mac.finalize().into_bytes();
        let mut t = u;
        for _ in 1..iterations {
            let mut mac = prf.clone();
            mac.update(&u);
            u = mac.finalize().into_bytes();
            t.iter_mut().zip(u.iter()).for_each(|(t, u)| *t ^= u);
        }
        derived.extend_from_slice(&t);
        block_index += 1;
    }
    derived.truncate(length);
    derived
}

/// The contents of `/etc/kcpassword`: the password followed by at least one NUL,
/// padded to a multiple of 12 bytes before it is XORed with the key. Padding after
/// the XOR leaves raw NULs that loginwindow decodes into key material, and the
/// login keychain then cannot be unlocked.
pub(super) fn kcpassword(password: &str) -> Vec<u8> {
    let mut bytes = password.as_bytes().to_vec();
    bytes.resize(bytes.len() + 12 - bytes.len() % 12, 0);
    bytes
        .iter()
        .enumerate()
        .map(|(index, byte)| byte ^ KCPASSWORD_KEY[index % KCPASSWORD_KEY.len()])
        .collect()
}

fn add_to_groups(groups: &Path, uuid: &str) -> Result<(), String> {
    for name in ["admin", "staff"] {
        let path = groups.join(format!("{name}.plist"));
        if !path.exists() {
            continue;
        }
        let mut group = read_plist(&path)?;
        add_member(&mut group, "users", USER);
        add_member(&mut group, "groupmembers", uuid);
        let mode = mode_of(&path).unwrap_or(0o644);
        write_plist(&group, &path, mode, true, Format::Binary)?;
    }
    Ok(())
}

fn add_member(group: &mut Dictionary, key: &str, member: &str) {
    let mut members: Vec<Value> = group
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if !members
        .iter()
        .any(|value| value.as_string() == Some(member))
    {
        members.push(Value::String(member.into()));
    }
    group.insert(key.into(), Value::Array(members));
}

fn home(root: &Path) -> PathBuf {
    root.join("Users").join(USER)
}

fn create_home(root: &Path) -> Result<(), String> {
    let home = home(root);
    for relative in ["Library/Preferences", "Library/Preferences/ByHost"] {
        create_dir(&home.join(relative), 0o755)?;
    }
    set_mode(&home, 0o755)?;
    write_file(
        &home.join(".CFUserTextEncoding"),
        format!("0x{:X}:0x0:0x0\n", UID.parse::<u32>().unwrap_or(501)).as_bytes(),
        0o644,
        false,
    )?;
    update_plist(
        &home.join("Library/Preferences/.GlobalPreferences.plist"),
        0o644,
        |preferences| {
            preferences.insert("AppleKeyboardUIMode".into(), Value::Integer(3.into()));
        },
    )?;
    update_plist(
        &home.join("Library/Preferences/com.apple.screensaver.plist"),
        0o644,
        |screensaver| {
            screensaver.insert("askForPassword".into(), Value::Integer(0.into()));
            screensaver.insert("askForPasswordDelay".into(), Value::Integer(0.into()));
            screensaver.insert("idleTime".into(), Value::Integer(0.into()));
        },
    )?;
    let by_host = home.join("Library/Preferences/ByHost");
    for uuid in by_host_uuids(&by_host) {
        let mut idle = Dictionary::new();
        idle.insert("idleTime".into(), Value::Integer(0.into()));
        write_plist(
            &idle,
            &by_host.join(format!("com.apple.screensaver.{uuid}.plist")),
            0o644,
            false,
            Format::Binary,
        )?;
    }
    Ok(())
}

fn mark_setup_complete(root: &Path, release: Option<&Release<'_>>) -> Result<(), String> {
    let marker = root.join("private/var/db/.AppleSetupDone");
    write_file(&marker, b"", 0o644, marker.exists())?;

    let mut seen = Dictionary::new();
    for key in [
        "DidSeeCloudSetup",
        "DidSeePrivacy",
        "DidSeeSiriSetup",
        "DidSeeTouchIDSetup",
        "DidSeeTrueToneSetup",
    ] {
        seen.insert(key.into(), Value::Boolean(true));
    }
    seen.insert("GestureMovieSeen".into(), Value::String("none".into()));
    if let Some(release) = release {
        seen.insert(
            "LastSeenBuddyBuildVersion".into(),
            Value::String(release.build.into()),
        );
        seen.insert(
            "LastSeenCloudProductVersion".into(),
            Value::String(release.version.into()),
        );
    }
    let user_path = home(root).join("Library/Preferences/com.apple.SetupAssistant.plist");
    write_plist(&seen, &user_path, 0o644, user_path.exists(), Format::Binary)?;
    let system_path = root.join("Library/Preferences/com.apple.SetupAssistant.plist");
    write_plist(
        &seen,
        &system_path,
        0o644,
        system_path.exists(),
        Format::Binary,
    )
}

fn configure_autologin(root: &Path, password: &str) -> Result<(), String> {
    update_plist(
        &root.join("Library/Preferences/com.apple.loginwindow.plist"),
        0o644,
        |loginwindow| {
            let mut info = loginwindow
                .get("AccountInfo")
                .and_then(Value::as_dictionary)
                .cloned()
                .unwrap_or_default();
            // A FirstLogins entry makes macOS start Setup Assistant at the next login.
            info.remove("FirstLogins");
            let users = info
                .get("MaximumUsers")
                .and_then(Value::as_signed_integer)
                .unwrap_or(1)
                .max(1);
            info.insert("MaximumUsers".into(), Value::Integer(users.into()));
            if !matches!(info.get("OnConsole"), Some(Value::Dictionary(_))) {
                info.insert("OnConsole".into(), Value::Dictionary(Dictionary::new()));
            }
            loginwindow.insert("AccountInfo".into(), Value::Dictionary(info));
            loginwindow.insert("autoLoginUser".into(), Value::String(USER.into()));
            loginwindow.insert("GuestEnabled".into(), Value::Boolean(false));
            loginwindow.insert("lastUser".into(), Value::String("loggedIn".into()));
            loginwindow.insert("lastUserName".into(), Value::String(USER.into()));
            loginwindow.insert("RecentUsers".into(), string_list(USER));
            loginwindow.insert("SHOWFULLNAME".into(), Value::Boolean(false));
        },
    )?;
    let path = root.join("private/etc/kcpassword");
    write_file(&path, &kcpassword(password), 0o600, path.exists())
}

fn enable_ssh(root: &Path) -> Result<(), String> {
    let path = root.join("private/var/db/com.apple.xpc.launchd/disabled.plist");
    let state = path.parent().unwrap_or(&path);
    create_dir(state, 0o755)?;
    let migrated = state.join("disabled.migrated");
    write_file(&migrated, b"", 0o644, migrated.exists())?;
    let mut disabled = read_plist_if_present(&path)?;
    disabled.insert("com.openssh.sshd".into(), Value::Boolean(false));
    write_plist(&disabled, &path, 0o644, path.exists(), Format::Xml)
}

fn configure_power_and_lock(root: &Path) -> Result<(), String> {
    update_plist(
        &root.join("Library/Preferences/.GlobalPreferences.plist"),
        0o644,
        |global| {
            global.insert(
                "com.apple.autologout.AutoLogOutDelay".into(),
                Value::Integer(0.into()),
            );
        },
    )?;
    let zero = || Value::Integer(0.into());
    let mut ac = Dictionary::new();
    ac.insert(
        "Automatic Restart On Power Loss".into(),
        Value::Boolean(true),
    );
    ac.insert("DarkWakeBackgroundTasks".into(), zero());
    ac.insert("Disk Sleep Timer".into(), zero());
    ac.insert("Display Sleep Timer".into(), zero());
    ac.insert("System Sleep Timer".into(), zero());
    ac.insert("Wake On LAN".into(), Value::Integer(1.into()));
    let mut system = Dictionary::new();
    system.insert("Update DarkWakeBG Setting".into(), Value::Boolean(true));
    let mut power = Dictionary::new();
    power.insert("AC Power".into(), Value::Dictionary(ac));
    power.insert("SystemPowerSettings".into(), Value::Dictionary(system));
    let path = root.join("Library/Preferences/com.apple.PowerManagement.plist");
    write_plist(&power, &path, 0o644, path.exists(), Format::Binary)?;

    for uuid in by_host_uuids(&home(root).join("Library/Preferences/ByHost")) {
        let mut ac = Dictionary::new();
        ac.insert("PrioritizeNetworkReachabilityOverSleep".into(), zero());
        ac.insert("Sleep On Power Button".into(), Value::Boolean(false));
        ac.insert("SleepServices".into(), zero());
        ac.insert("Standby Delay".into(), zero());
        ac.insert("Standby Enabled".into(), zero());
        ac.insert("TCPKeepAlivePref".into(), Value::Integer(1.into()));
        ac.insert("TTYSPreventSleep".into(), Value::Integer(1.into()));
        let mut per_host = Dictionary::new();
        per_host.insert("AC Power".into(), Value::Dictionary(ac));
        let path = root.join(format!(
            "Library/Preferences/com.apple.PowerManagement.{uuid}.plist"
        ));
        write_plist(&per_host, &path, 0o644, path.exists(), Format::Binary)?;
    }
    Ok(())
}

/// Bump when the account setup changes in a way its text does not show.
const SETUP_REVISION: u32 = 1;

/// What the account setup consists of, for the version of a template.
pub(super) fn setup_inputs() -> String {
    format!(
        "{SETUP_REVISION}\n{}\n{}\n{}",
        finalization_script(""),
        import_finalization_script("", ""),
        sudoers()
    )
}

/// The sudoers entry that lets the account run root commands with `sudo -n`. It is
/// installed from inside the guest: sudo refuses a file the offline edit left owned
/// by an unknown user.
pub(super) fn sudoers() -> String {
    format!("{USER} ALL=(ALL) NOPASSWD: ALL\n")
}

/// The commands run as root in the guest once it is up. Lume's finalization,
/// plus the ownership the host cannot set while it edits the volume offline and
/// the installation of the sudoers entry.
pub(super) fn finalization_script(public_key: &str) -> String {
    finalization_script_with_host_key_rotation(public_key, None, false)
}

/// Root finalization for an imported disk. It installs this computer's key and
/// replaces the source disk's SSH host keys without needing source credentials.
pub(super) fn import_finalization_script(public_key: &str, computer_name: &str) -> String {
    finalization_script_with_host_key_rotation(public_key, Some(computer_name), true)
}

fn finalization_script_with_host_key_rotation(
    public_key: &str,
    computer_name: Option<&str>,
    rotate_host_keys: bool,
) -> String {
    let authorized_key = super::guest_access::shell_quote(public_key.trim());
    let identity_reset = computer_name.map_or_else(String::new, |name| {
        let name = super::guest_access::shell_quote(name);
        format!(
            "/usr/sbin/scutil --set ComputerName {name}\n/usr/sbin/scutil --set LocalHostName {name}\n/usr/sbin/scutil --set HostName {name}\n/usr/bin/find /Users/{USER}/Library/Keychains -mindepth 1 -maxdepth 1 -exec /bin/rm -rf {{}} + 2>/dev/null || true\n"
        )
    });
    let sudoers_entry = sudoers();
    let sudoers_entry = sudoers_entry.trim_end();
    let rotate_host_keys = if rotate_host_keys {
        "/bin/rm -f /etc/ssh/ssh_host_*\n/usr/bin/ssh-keygen -A\n"
    } else {
        ""
    };
    format!(
        "set -e
/usr/bin/touch /var/db/.AppleSetupDone
/usr/sbin/chown root:wheel /var/db/.AppleSetupDone
/bin/chmod 644 /var/db/.AppleSetupDone
/usr/bin/defaults write /Library/Preferences/com.apple.loginwindow autoLoginUser -string {USER}
/usr/bin/defaults write /Library/Preferences/com.apple.loginwindow GuestEnabled -bool false
/usr/bin/defaults write /Library/Preferences/com.apple.loginwindow lastUser -string loggedIn
/usr/bin/defaults write /Library/Preferences/com.apple.loginwindow lastUserName -string {USER}
/usr/bin/defaults write /Library/Preferences/com.apple.SetupAssistant DidSeeCloudSetup -bool true
/usr/bin/defaults write /Library/Preferences/com.apple.SetupAssistant DidSeePrivacy -bool true
/usr/bin/defaults write /Library/Preferences/com.apple.SetupAssistant DidSeeSiriSetup -bool true
/usr/bin/defaults write /Library/Preferences/com.apple.SetupAssistant DidSeeTouchIDSetup -bool true
/usr/bin/defaults write /Library/Preferences/com.apple.SetupAssistant DidSeeTrueToneSetup -bool true
{identity_reset}
/usr/sbin/chown root:wheel /etc/kcpassword
/bin/chmod 600 /etc/kcpassword
/usr/sbin/chown root:wheel /var/db/dslocal/nodes/Default/users/{USER}.plist 2>/dev/null || true
/usr/sbin/chown root:wheel /Library/Preferences/com.apple.loginwindow.plist /Library/Preferences/com.apple.SetupAssistant.plist /Library/Preferences/.GlobalPreferences.plist /Library/Preferences/com.apple.PowerManagement.plist 2>/dev/null || true
/usr/sbin/chown root:wheel /var/db/com.apple.xpc.launchd/disabled.plist /var/db/com.apple.xpc.launchd/disabled.migrated 2>/dev/null || true
/bin/mkdir -p /Users/{USER}/.ssh
/usr/bin/printf '%s\\n' {authorized_key} > /Users/{USER}/.ssh/authorized_keys
/usr/sbin/chown {UID}:{GID} /Users/{USER}
/usr/sbin/chown {UID}:{GID} /Users/{USER}/.CFUserTextEncoding /Users/{USER}/Library /Users/{USER}/Library/Preferences /Users/{USER}/Library/Preferences/ByHost /Users/{USER}/Library/Preferences/*.plist 2>/dev/null || true
/usr/sbin/chown -R {UID}:{GID} /Users/{USER}/.ssh
/bin/chmod 700 /Users/{USER}/.ssh
/bin/chmod 600 /Users/{USER}/.ssh/authorized_keys
/usr/bin/printf '%s\\n' '{sudoers_entry}' > /etc/sudoers.d/{USER}.new
/usr/sbin/visudo -cf /etc/sudoers.d/{USER}.new >/dev/null
/usr/sbin/chown root:wheel /etc/sudoers.d/{USER}.new
/bin/chmod 440 /etc/sudoers.d/{USER}.new
/bin/mv /etc/sudoers.d/{USER}.new /etc/sudoers.d/{USER}
{rotate_host_keys}
attempt=0
until err=$(/usr/sbin/diskutil apfs updatePreboot / 2>&1) || err=$(/usr/sbin/diskutil apfs updatePreboot /System/Volumes/Data 2>&1); do
attempt=$((attempt + 1))
if [ \"$attempt\" -ge 5 ]; then echo \"$err\" >&2; exit {PREBOOT_FAILED}; fi
/bin/sleep 5
done
/bin/launchctl enable system/com.openssh.sshd
/usr/bin/stat -f 'MARKER_OWNER=%u:%g' /var/db/.AppleSetupDone
"
    )
}

// MARK: Files

#[derive(Clone, Copy)]
enum Format {
    Binary,
    Xml,
}

fn fs_error(action: &str, error: &std::io::Error) -> String {
    super::store::io_error(action, error)
}

/// The largest property list Silo decodes from a guest disk. The account, group, and setup
/// lists it edits are a few kilobytes; an imported disk is archive-controlled, so a larger file
/// is refused before it is decoded.
const MAX_PLIST_BYTES: u64 = 1024 * 1024;

fn read_plist_value(path: &Path) -> Result<Value, String> {
    read_plist_value_limited(path, MAX_PLIST_BYTES)
}

fn read_plist_value_limited(path: &Path, limit: u64) -> Result<Value, String> {
    use std::io::Read as _;

    let unreadable = || format!("Silo could not read {}.", path.display());
    let file = fs::File::open(path).map_err(|_| unreadable())?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| unreadable())?;
    if bytes.len() as u64 > limit {
        return Err(format!(
            "{} is larger than Silo reads from a computer's disk.",
            path.display()
        ));
    }
    Value::from_reader(std::io::Cursor::new(bytes)).map_err(|_| unreadable())
}

fn read_plist(path: &Path) -> Result<Dictionary, String> {
    read_plist_value(path)?
        .into_dictionary()
        .ok_or_else(|| format!("Silo could not read {}.", path.display()))
}

fn read_plist_if_present(path: &Path) -> Result<Dictionary, String> {
    if path.exists() {
        read_plist(path)
    } else {
        Ok(Dictionary::new())
    }
}

fn update_plist(
    path: &Path,
    mode: u32,
    change: impl FnOnce(&mut Dictionary),
) -> Result<(), String> {
    let mut plist = read_plist_if_present(path)?;
    change(&mut plist);
    write_plist(&plist, path, mode, path.exists(), Format::Binary)
}

fn write_plist(
    plist: &Dictionary,
    path: &Path,
    mode: u32,
    preserve_existing: bool,
    format: Format,
) -> Result<(), String> {
    let mut bytes = Vec::new();
    let value = Value::Dictionary(plist.clone());
    match format {
        Format::Binary => value.to_writer_binary(&mut bytes),
        Format::Xml => value.to_writer_xml(&mut bytes),
    }
    .map_err(|error| format!("Silo could not encode {}: {error}", path.display()))?;
    write_file(path, &bytes, mode, preserve_existing)
}

/// Writes `bytes` to `path`. An existing file is rewritten in place when
/// `preserve_existing`, so its inode survives.
fn write_file(path: &Path, bytes: &[u8], mode: u32, preserve_existing: bool) -> Result<(), String> {
    let action = "set up the computer's account";
    if let Some(parent) = path.parent() {
        create_dir(parent, 0o755)?;
    }
    let exists = path.exists();
    if exists {
        set_mode(path, mode | 0o200)?;
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).truncate(true);
    if !(preserve_existing && exists) {
        // A new file is created with its final mode, never readable by others first.
        options.create(true).mode(mode);
    }
    options
        .open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|error| fs_error(action, &error))?;
    set_mode(path, mode)
}

/// Creates `path` and any missing parents; only a directory this call creates gets `mode`.
fn create_dir(path: &Path, mode: u32) -> Result<(), String> {
    if path.is_dir() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        create_dir(parent, 0o755)?;
    }
    fs::DirBuilder::new()
        .mode(mode)
        .create(path)
        .map_err(|error| fs_error("set up the computer's account", &error))?;
    set_mode(path, mode)
}

fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|error| fs_error("set up the computer's account", &error))
}

fn mode_of(path: &Path) -> Option<u32> {
    fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions().mode() & 0o7777)
}

fn make_executable(dir: &Path) -> Result<(), String> {
    if !dir.is_dir() {
        return Ok(());
    }
    set_mode(dir, mode_of(dir).unwrap_or(0o700) | 0o100)
}

/// The host UUIDs in the names of `ByHost` preference files.
fn by_host_uuids(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut uuids: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let stem = name.strip_suffix(".plist")?;
            let candidate = stem.get(stem.len().checked_sub(36)?..)?;
            uuid::Uuid::parse_str(candidate)
                .ok()
                .map(|uuid| uuid.hyphenated().to_string().to_uppercase())
        })
        .collect();
    uuids.sort();
    uuids.dedup();
    uuids
}

fn random_bytes(count: usize) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut bytes = vec![0; count];
    fs::File::open("/dev/urandom")
        .and_then(|mut random| random.read_exact(&mut bytes))
        .map_err(|error| fs_error("create a random password", &error))?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn account() -> GuestAccount {
        GuestAccount {
            user: USER.into(),
            password: "correct-horse".into(),
            key: PathBuf::from("/nonexistent/id_ed25519"),
        }
    }

    #[test]
    fn pbkdf2_matches_the_published_sha512_vectors() {
        assert_eq!(
            hex(&pbkdf2_sha512(b"password", b"salt", 1, 64)),
            "867f70cf1ade02cff3752599a3a53dc4af34c7a669815ae5d513554e1c8cf252c02d470a285a0501bad999bfe943c08f050235d7d68b1da55e63f73b60a57fce"
        );
        assert_eq!(
            hex(&pbkdf2_sha512(b"password", b"salt", 2, 64)),
            "e1d9c16aa681708a45f5c7c4e215ceb66e011a2e9f0040713f18aefdb866d53cf76cab2868a39b9f7840edce4fef5a82be67335c77a6068e04112754f27ccf4e"
        );
        // Two blocks, as the 128-byte account hash needs.
        assert_eq!(
            hex(&pbkdf2_sha512(b"pass", b"saltsalt", 50, 128)),
            "0c6fb85c9fd3ed9ad394d8a11678d2d09ec779f354492a175d9821287d73d0ba81a2e8ce91aefa604d359f7a3590323ebc9f467e2735d7e0ce48e167994b8245fe8c3f938b969740dcae73383311fbf027b42fdad8f788b32165b15c6768dd2605cca90eac785f4cf4987b93fdfcb6615133c8f8b684c150be523c99cd49ec51"
        );
    }

    #[test]
    fn kcpassword_pads_before_encoding_like_loginwindow_expects() {
        // Lume's documented bytes for the password "lume".
        assert_eq!(hex(&kcpassword("lume")), "11fc3f46d2bcddeaa3b91f7d");
        // A password that already fills a block still gets a terminator.
        let twelve = kcpassword("abcdefghijkl");
        assert_eq!(twelve.len(), 24);
        assert_eq!(twelve[12] ^ KCPASSWORD_KEY[12 % 11], 0);
        // The decoded form is the password then NULs.
        let decoded: Vec<u8> = kcpassword("lume")
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ KCPASSWORD_KEY[index % 11])
            .collect();
        assert_eq!(decoded, b"lume\0\0\0\0\0\0\0\0");
    }

    #[test]
    fn plist_reads_are_capped_before_decoding() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("user.plist");
        let mut small = Dictionary::new();
        small.insert("name".into(), Value::String("silo".into()));
        write_plist(&small, &path, 0o600, false, Format::Xml).unwrap();
        let size = fs::metadata(&path).unwrap().len();
        // A file at the limit decodes; one byte over is refused without being parsed.
        assert_eq!(
            read_plist_value_limited(&path, size)
                .unwrap()
                .into_dictionary()
                .unwrap(),
            small
        );
        let error = read_plist_value_limited(&path, size - 1).unwrap_err();
        assert!(error.contains("larger than Silo reads"), "{error}");

        // The real cap applies to every account and group read of an imported disk.
        let oversized = directory.path().join("oversized.plist");
        let file = fs::File::create(&oversized).unwrap();
        file.set_len(MAX_PLIST_BYTES + 1).unwrap();
        drop(file);
        assert!(read_plist(&oversized)
            .unwrap_err()
            .contains("larger than Silo reads"));
        assert!(read_plist_value(&oversized).is_err());
        // An oversized neighbour never counts as a match when checking user IDs.
        fs::copy(&oversized, directory.path().join("neighbour.plist")).unwrap();
        ensure_uid_available(directory.path()).unwrap();
    }

    #[test]
    fn shadow_hash_data_holds_the_salted_hash() {
        let salt = [7u8; SALT_BYTES];
        let bytes = shadow_hash_data("correct-horse", &salt).unwrap();
        assert!(bytes.starts_with(b"bplist00"));
        let parsed = Value::from_reader(std::io::Cursor::new(&bytes))
            .unwrap()
            .into_dictionary()
            .unwrap();
        let hash = parsed
            .get("SALTED-SHA512-PBKDF2")
            .and_then(Value::as_dictionary)
            .unwrap();
        assert_eq!(
            hash.get("iterations").and_then(Value::as_unsigned_integer),
            Some(50_000)
        );
        assert_eq!(hash.get("salt").and_then(Value::as_data), Some(&salt[..]));
        let entropy = hash.get("entropy").and_then(Value::as_data).unwrap();
        assert_eq!(entropy.len(), 128);
        assert_eq!(
            entropy,
            &pbkdf2_sha512(b"correct-horse", &salt, 50_000, 128)[..]
        );
    }

    #[test]
    fn a_new_user_record_carries_the_account_fields() {
        let record = user_record(Dictionary::new(), "pw", "UUID-1", &[1; 32]).unwrap();
        let first = |key: &str| {
            record
                .get(key)
                .and_then(Value::as_array)
                .and_then(|list| list.first())
                .and_then(Value::as_string)
                .map(String::from)
        };
        assert_eq!(first("name").as_deref(), Some("silo"));
        assert_eq!(first("uid").as_deref(), Some("501"));
        assert_eq!(first("gid").as_deref(), Some("20"));
        assert_eq!(first("home").as_deref(), Some("/Users/silo"));
        assert_eq!(first("shell").as_deref(), Some("/bin/zsh"));
        assert_eq!(first("generateduid").as_deref(), Some("UUID-1"));
        assert_eq!(
            first("authentication_authority").as_deref(),
            Some(";ShadowHash;HASHLIST:<SALTED-SHA512-PBKDF2>")
        );
        assert_eq!(first("_writers_passwd").as_deref(), Some("silo"));
        assert!(record.get("ShadowHashData").is_some_and(|data| matches!(
            data.as_array().and_then(|list| list.first()),
            Some(Value::Data(_))
        )));
    }

    #[test]
    fn an_existing_user_record_keeps_its_other_fields() {
        let mut existing = Dictionary::new();
        existing.insert("picture".into(), string_list("/keep/me"));
        let record = user_record(existing, "pw", "UUID-2", &[1; 32]).unwrap();
        assert!(record.get("picture").is_some());
        assert!(record.get("_writers_passwd").is_none());
        assert!(record.get("ShadowHashData").is_some());
    }

    #[test]
    fn the_sudoers_entry_allows_passwordless_root() {
        assert_eq!(sudoers(), "silo ALL=(ALL) NOPASSWD: ALL\n");
    }

    #[test]
    fn finalization_installs_sudoers_ownership_and_prebooted_recovery_metadata() {
        let script = finalization_script("ssh-ed25519 AAAA test");
        assert!(script.starts_with("set -e\n"));
        assert!(script.contains("autoLoginUser -string silo"));
        assert!(script.contains("diskutil apfs updatePreboot / 2>&1"));
        assert!(script.contains("updatePreboot /System/Volumes/Data"));
        assert!(script.contains("exit 70;"));
        // Tolerated failures stay out of the script's stderr, which becomes the error.
        for line in script.lines().filter(|line| line.contains("|| true")) {
            assert!(line.contains("2>/dev/null"), "{line}");
        }
        assert!(script.contains("visudo -cf /etc/sudoers.d/silo.new >/dev/null"));
        assert!(script.contains("launchctl enable system/com.openssh.sshd"));
        assert!(script.contains("visudo -cf /etc/sudoers.d/silo.new"));
        assert!(script.contains("chmod 440 /etc/sudoers.d/silo.new"));
        assert!(script.contains("chown 501:20 /Users/silo\n"));
        assert!(!script.contains("chown -R 501:20 /Users/silo\n"));
        assert!(script
            .contains("printf '%s\\n' 'ssh-ed25519 AAAA test' > /Users/silo/.ssh/authorized_keys"));
        assert!(script.contains("MARKER_OWNER=%u:%g"));
        assert!(!script.contains("-string lume"));
    }

    #[test]
    fn import_finalization_installs_its_key_and_rotates_guest_host_keys() {
        let script = import_finalization_script("ssh-ed25519 AAAA imported-device", "new-computer");
        assert!(script.contains(
            "printf '%s\\n' 'ssh-ed25519 AAAA imported-device' > /Users/silo/.ssh/authorized_keys"
        ));
        assert!(script.contains("/bin/rm -f /etc/ssh/ssh_host_*\n/usr/bin/ssh-keygen -A"));
        assert!(script.contains("launchctl enable system/com.openssh.sshd"));
        assert!(script.contains("scutil --set ComputerName 'new-computer'"));
        assert!(script.contains("scutil --set LocalHostName 'new-computer'"));
        assert!(script.contains("scutil --set HostName 'new-computer'"));
        assert!(script.contains("find /Users/silo/Library/Keychains -mindepth 1 -maxdepth 1"));
        assert!(!script.contains("source-password"));
        assert!(!script.contains("source-key"));
    }

    #[test]
    fn hdiutil_output_names_the_whole_disk() {
        let output = "/dev/disk7          \tGUID_partition_scheme          \t\n/dev/disk7s1        \tApple_APFS_ISC                 \t\n/dev/disk7s2        \tApple_APFS                     \t\n";
        assert_eq!(parse_whole_disk(output).as_deref(), Some("disk7"));
        assert_eq!(parse_whole_disk("nothing useful"), None);
    }

    #[test]
    fn the_data_volume_is_found_by_role() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>Containers</key><array><dict><key>Volumes</key><array>
<dict><key>DeviceIdentifier</key><string>disk8s1</string><key>Roles</key><array><string>System</string></array></dict>
<dict><key>DeviceIdentifier</key><string>disk8s2</string><key>Roles</key><array><string>Data</string></array></dict>
</array></dict></array></dict></plist>"#;
        let list = Value::from_reader_xml(xml.as_bytes())
            .unwrap()
            .into_dictionary()
            .unwrap();
        assert_eq!(data_volume_device(&list).as_deref(), Some("disk8s2"));
    }

    fn info(images: &[(&str, &[&str])]) -> Dictionary {
        let images = images
            .iter()
            .map(|(path, devices)| {
                let entities = devices
                    .iter()
                    .map(|device| {
                        let mut entity = Dictionary::new();
                        entity.insert("dev-entry".into(), Value::String((*device).into()));
                        Value::Dictionary(entity)
                    })
                    .collect();
                let mut image = Dictionary::new();
                image.insert("image-path".into(), Value::String((*path).into()));
                image.insert("system-entities".into(), Value::Array(entities));
                Value::Dictionary(image)
            })
            .collect();
        let mut info = Dictionary::new();
        info.insert("images".into(), Value::Array(images));
        info
    }

    #[test]
    fn only_the_whole_disks_of_the_named_image_are_found() {
        let list = info(&[
            ("/other.dmg", &["/dev/disk3", "/dev/disk3s1"]),
            (
                "/vm/disk.img",
                &["/dev/disk7", "/dev/disk7s1", "/dev/disk8"],
            ),
        ]);
        assert_eq!(
            attached_devices(&list, Path::new("/vm/disk.img")),
            ["/dev/disk7", "/dev/disk8"]
        );
        assert!(attached_devices(&list, Path::new("/vm/none.img")).is_empty());
    }

    #[test]
    fn an_attachment_that_cannot_be_detached_is_reported() {
        use std::cell::{Cell, RefCell};
        let attached = RefCell::new(info(&[("/vm/disk.img", &["/dev/disk7"])]));
        let detached = Cell::new(Vec::new());
        // A detach that works leaves nothing attached.
        let result = detach_matching(
            Path::new("/vm/disk.img"),
            &|| Ok(attached.borrow().clone()),
            &|device| {
                let mut seen = detached.take();
                seen.push(device.to_string());
                detached.set(seen);
                *attached.borrow_mut() = info(&[]);
                Ok(())
            },
        );
        assert_eq!(result, Ok(()));
        assert_eq!(detached.take(), ["/dev/disk7"]);
        // A detach that fails leaves it attached, and that is an error.
        let stuck = info(&[("/vm/disk.img", &["/dev/disk7"])]);
        let result = detach_matching(Path::new("/vm/disk.img"), &|| Ok(stuck.clone()), &|_| {
            Err("busy".into())
        });
        assert!(result.unwrap_err().contains("still attached"));
        // Nothing attached needs no detach at all.
        let result = detach_matching(Path::new("/vm/disk.img"), &|| Ok(info(&[])), &|_| {
            panic!("nothing to detach")
        });
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn whole_disks_are_told_from_partitions() {
        for device in ["/dev/disk7", "disk10", "/dev/disk0"] {
            assert!(is_whole_disk(device), "{device}");
        }
        for device in [
            "/dev/disk7s2",
            "disk10s1",
            "/dev/disk",
            "/dev/rdisk7",
            "disk7x",
        ] {
            assert!(!is_whole_disk(device), "{device}");
        }
    }

    #[test]
    fn secret_files_are_created_private() {
        let dir = tempfile::tempdir().unwrap();
        for (name, mode) in [("kcpassword", 0o600), ("sudoers", 0o440), ("plain", 0o644)] {
            let path = dir.path().join(name);
            write_file(&path, b"secret", mode, false).unwrap();
            assert_eq!(mode_of(&path), Some(mode), "{name}");
            // Rewriting in place keeps the mode.
            write_file(&path, b"other", mode, true).unwrap();
            assert_eq!(mode_of(&path), Some(mode), "{name}");
            assert_eq!(fs::read(&path).unwrap(), b"other");
        }
        // The file exists with its final mode as soon as it is created, whatever the umask allows.
        let mut options = fs::OpenOptions::new();
        let path = dir.path().join("direct");
        options.write(true).create_new(true).mode(0o600);
        options.open(&path).unwrap();
        assert_eq!(mode_of(&path), Some(0o600));
        create_dir(&dir.path().join("a/b"), 0o700).unwrap();
        assert_eq!(mode_of(&dir.path().join("a/b")), Some(0o700));
    }

    #[test]
    fn by_host_uuids_come_from_file_names() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "com.apple.screensaver.0a1b2c3d-1111-2222-3333-444455556666.plist",
            "com.apple.loginwindow.0A1B2C3D-1111-2222-3333-444455556666.plist",
            "com.apple.other.plist",
        ] {
            fs::write(dir.path().join(name), b"").unwrap();
        }
        assert_eq!(
            by_host_uuids(dir.path()),
            vec!["0A1B2C3D-1111-2222-3333-444455556666".to_string()]
        );
    }

    fn volume() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        // macOS writes its launchd state during the first boot.
        let launchd = root.path().join(LAUNCHD_DISABLED);
        fs::create_dir_all(launchd.parent().unwrap()).unwrap();
        write_plist(&Dictionary::new(), &launchd, 0o644, false, Format::Xml).unwrap();
        let nodes = root.path().join("private/var/db/dslocal/nodes/Default");
        fs::create_dir_all(nodes.join("users")).unwrap();
        fs::create_dir_all(nodes.join("groups")).unwrap();
        let mut admin = Dictionary::new();
        admin.insert("users".into(), string_list("root"));
        admin.insert("name".into(), string_list("admin"));
        write_plist(
            &admin,
            &nodes.join("groups/admin.plist"),
            0o644,
            false,
            Format::Binary,
        )
        .unwrap();
        root
    }

    fn imported_volume() -> (tempfile::TempDir, String, Value) {
        let root = volume();
        let generated_uid = "0A1B2C3D-1111-2222-3333-444455556666".to_string();
        let old_hash = Value::Array(vec![Value::Data(b"source password hash".to_vec())]);
        let users = root
            .path()
            .join("private/var/db/dslocal/nodes/Default/users");
        let mut user = Dictionary::new();
        user.insert("name".into(), string_list(USER));
        user.insert("uid".into(), string_list(UID));
        user.insert("gid".into(), string_list(GID));
        user.insert("home".into(), string_list("/Users/silo"));
        user.insert("generateduid".into(), string_list(&generated_uid));
        user.insert(
            "authentication_authority".into(),
            string_list(";ShadowHash;HASHLIST:<SALTED-SHA512-PBKDF2>"),
        );
        user.insert("ShadowHashData".into(), old_hash.clone());
        user.insert("retainedField".into(), Value::String("keep me".into()));
        write_plist(
            &user,
            &users.join("silo.plist"),
            0o600,
            false,
            Format::Binary,
        )
        .unwrap();

        let home = root.path().join("Users/silo");
        fs::create_dir_all(home.join("Documents")).unwrap();
        fs::write(home.join("Documents/original.txt"), b"archive home marker").unwrap();
        fs::create_dir_all(home.join(".ssh")).unwrap();
        fs::write(
            home.join(".ssh/authorized_keys"),
            b"ssh-ed25519 AAAA source-device\n",
        )
        .unwrap();
        fs::create_dir_all(home.join("Library/LaunchAgents")).unwrap();
        fs::write(
            home.join("Library/LaunchAgents/org.silo.computer-use-reconcile.plist"),
            b"source reconcile agent",
        )
        .unwrap();
        fs::write(
            home.join("Library/LaunchAgents/org.silo.guest-recorder.plist"),
            b"source recorder agent",
        )
        .unwrap();
        fs::create_dir_all(home.join("Library/Application Support/Silo")).unwrap();
        fs::write(
            home.join("Library/Application Support/Silo/computer-use-receipt.json"),
            b"source computer-use receipt",
        )
        .unwrap();
        let shared_support = root.path().join("Library/Application Support/Silo");
        fs::create_dir_all(&shared_support).unwrap();
        fs::write(
            shared_support.join("developer-tools-receipt.json"),
            b"source developer-tools receipt",
        )
        .unwrap();

        let preferences = root.path().join("Library/Preferences");
        fs::create_dir_all(&preferences).unwrap();
        write_plist(
            &Dictionary::new(),
            &preferences.join("com.apple.loginwindow.plist"),
            0o644,
            false,
            Format::Binary,
        )
        .unwrap();
        let etc = root.path().join("private/etc");
        fs::create_dir_all(&etc).unwrap();
        write_file(
            &etc.join("kcpassword"),
            &kcpassword("source-password"),
            0o600,
            false,
        )
        .unwrap();
        (root, generated_uid, old_hash)
    }

    #[test]
    fn patching_a_volume_writes_the_account_and_its_settings() {
        let root = volume();
        let mount = root.path();
        patch(
            mount,
            &account(),
            Some(&Release {
                version: "26.6.2",
                build: "25G83",
            }),
        )
        .unwrap();

        let user = read_plist(&mount.join("private/var/db/dslocal/nodes/Default/users/silo.plist"))
            .unwrap();
        assert!(user.get("ShadowHashData").is_some());
        let uuid = user["generateduid"].as_array().unwrap()[0]
            .as_string()
            .unwrap()
            .to_string();

        let admin =
            read_plist(&mount.join("private/var/db/dslocal/nodes/Default/groups/admin.plist"))
                .unwrap();
        let users: Vec<&str> = admin["users"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_string)
            .collect();
        assert_eq!(users, ["root", "silo"]);
        assert!(admin["groupmembers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|member| member.as_string() == Some(uuid.as_str())));

        assert!(mount.join("private/var/db/.AppleSetupDone").exists());
        let login =
            read_plist(&mount.join("Library/Preferences/com.apple.loginwindow.plist")).unwrap();
        assert_eq!(login["autoLoginUser"].as_string(), Some("silo"));
        assert!(login["AccountInfo"].as_dictionary().is_some());
        let setup = read_plist(
            &mount.join("Users/silo/Library/Preferences/com.apple.SetupAssistant.plist"),
        )
        .unwrap();
        assert_eq!(setup["DidSeePrivacy"].as_boolean(), Some(true));
        assert_eq!(
            setup["LastSeenBuddyBuildVersion"].as_string(),
            Some("25G83")
        );
        assert_eq!(
            setup["LastSeenCloudProductVersion"].as_string(),
            Some("26.6.2")
        );

        assert_eq!(
            fs::read(mount.join("private/etc/kcpassword")).unwrap(),
            kcpassword("correct-horse")
        );
        assert_eq!(mode_of(&mount.join("private/etc/kcpassword")), Some(0o600));

        let disabled =
            read_plist(&mount.join("private/var/db/com.apple.xpc.launchd/disabled.plist")).unwrap();
        assert_eq!(disabled["com.openssh.sshd"].as_boolean(), Some(false));

        // Sudo refuses a sudoers file with an unknown owner, and sshd an unknown owner of
        // authorized_keys, so both are installed from inside the guest.
        assert!(!mount.join("private/etc/sudoers.d/silo").exists());
        assert!(!mount.join("Users/silo/.ssh").exists());

        let power =
            read_plist(&mount.join("Library/Preferences/com.apple.PowerManagement.plist")).unwrap();
        assert_eq!(
            power["AC Power"].as_dictionary().unwrap()["System Sleep Timer"].as_signed_integer(),
            Some(0)
        );
    }

    #[test]
    fn import_rekey_preserves_account_identity_and_home_data_and_changes_auth_state() {
        let (root, generated_uid, old_hash) = imported_volume();
        let user_path = root
            .path()
            .join("private/var/db/dslocal/nodes/Default/users/silo.plist");
        let home_marker = root.path().join("Users/silo/Documents/original.txt");
        let mut new_account = account();
        new_account.password = "new-import-password".into();

        rekey_import_data_volume(root.path(), &new_account).unwrap();

        let user = read_plist(&user_path).unwrap();
        assert_eq!(user["name"], string_list(USER));
        assert_eq!(user["uid"], string_list(UID));
        assert_eq!(user["gid"], string_list(GID));
        assert_eq!(user["home"], string_list("/Users/silo"));
        assert_eq!(user["generateduid"], string_list(&generated_uid));
        assert_eq!(user["retainedField"].as_string(), Some("keep me"));
        assert_ne!(user["ShadowHashData"], old_hash);
        let hash_data = user["ShadowHashData"].as_array().unwrap()[0]
            .as_data()
            .unwrap();
        let decoded = Value::from_reader(std::io::Cursor::new(hash_data))
            .unwrap()
            .into_dictionary()
            .unwrap();
        let salted = decoded
            .get("SALTED-SHA512-PBKDF2")
            .and_then(Value::as_dictionary)
            .unwrap();
        let salt = salted.get("salt").and_then(Value::as_data).unwrap();
        assert_eq!(
            hash_data,
            shadow_hash_data(&new_account.password, salt).unwrap()
        );
        assert_eq!(fs::read(home_marker).unwrap(), b"archive home marker");

        let login = read_plist(
            &root
                .path()
                .join("Library/Preferences/com.apple.loginwindow.plist"),
        )
        .unwrap();
        assert_eq!(login["autoLoginUser"].as_string(), Some(USER));
        assert_eq!(
            fs::read(root.path().join("private/etc/kcpassword")).unwrap(),
            kcpassword(&new_account.password)
        );
        let disabled = read_plist(&root.path().join(LAUNCHD_DISABLED)).unwrap();
        assert_eq!(disabled["com.openssh.sshd"].as_boolean(), Some(false));
        assert_eq!(disabled["com.apple.screensharing"].as_boolean(), Some(true));
        assert!(root
            .path()
            .join("private/var/db/com.apple.xpc.launchd/disabled.migrated")
            .is_file());
        for relative in [
            "Users/silo/.ssh/authorized_keys",
            "Users/silo/Library/LaunchAgents/org.silo.computer-use-reconcile.plist",
            "Users/silo/Library/LaunchAgents/org.silo.guest-recorder.plist",
            "Users/silo/Library/Application Support/Silo/computer-use-receipt.json",
            "Library/Application Support/Silo/developer-tools-receipt.json",
        ] {
            assert!(!root.path().join(relative).exists(), "{relative}");
        }
    }

    #[test]
    fn import_rekey_refuses_an_oversized_account_plist_and_leaves_it_untouched() {
        let (root, _, _) = imported_volume();
        let user_path = root
            .path()
            .join("private/var/db/dslocal/nodes/Default/users/silo.plist");
        let file = fs::OpenOptions::new().write(true).open(&user_path).unwrap();
        file.set_len(MAX_PLIST_BYTES + 1).unwrap();
        drop(file);

        let error = rekey_import_data_volume(root.path(), &account()).unwrap_err();
        assert!(error.contains("larger than Silo reads"), "{error}");
        assert_eq!(fs::metadata(&user_path).unwrap().len(), MAX_PLIST_BYTES + 1);
    }

    #[test]
    fn import_rekey_accepts_macos_authorities_in_any_order_and_clears_source_auth_state() {
        let (root, _, _) = imported_volume();
        let user_path = root
            .path()
            .join("private/var/db/dslocal/nodes/Default/users/silo.plist");
        let mut user = read_plist(&user_path).unwrap();
        let kerberos = Value::String(
            ";Kerberosv5;;silo@LKDC:SHA1.00112233445566778899AABBCCDDEEFF00112233;LKDC:SHA1.00112233445566778899AABBCCDDEEFF00112233;"
                .into(),
        );
        let authorities = Value::Array(vec![
            Value::String(";SecureToken;".into()),
            Value::String(
                ";ShadowHash;HASHLIST:<SALTED-SHA512-PBKDF2,SRP-RFC5054-4096-SHA512-PBKDF2>".into(),
            ),
            kerberos.clone(),
        ]);
        user.insert("authentication_authority".into(), authorities.clone());
        write_plist(&user, &user_path, 0o600, true, Format::Binary).unwrap();

        let mut new_account = account();
        new_account.password = "new-import-password".into();
        rekey_import_data_volume(root.path(), &new_account).unwrap();

        assert_eq!(
            read_plist(&user_path).unwrap()["authentication_authority"],
            string_list(";ShadowHash;HASHLIST:<SALTED-SHA512-PBKDF2>")
        );
        assert_eq!(
            authorities,
            Value::Array(vec![
                Value::String(";SecureToken;".into()),
                Value::String(
                    ";ShadowHash;HASHLIST:<SALTED-SHA512-PBKDF2,SRP-RFC5054-4096-SHA512-PBKDF2>"
                        .into(),
                ),
                kerberos,
            ])
        );
    }

    #[test]
    fn import_rekey_rejects_a_symlinked_authorized_keys_path_before_writing() {
        let (root, _, old_hash) = imported_volume();
        let external = tempfile::tempdir().unwrap();
        let external_keys = external.path().join("authorized_keys");
        fs::write(&external_keys, b"keep outside data").unwrap();
        let ssh_dir = root.path().join("Users/silo/.ssh");
        fs::remove_dir_all(&ssh_dir).unwrap();
        std::os::unix::fs::symlink(external.path(), &ssh_dir).unwrap();
        let user_path = root
            .path()
            .join("private/var/db/dslocal/nodes/Default/users/silo.plist");

        let error = rekey_import_data_volume(root.path(), &account()).unwrap_err();

        assert!(error.contains("safely access"), "{error}");
        assert_eq!(read_plist(&user_path).unwrap()["ShadowHashData"], old_hash);
        assert_eq!(fs::read(external_keys).unwrap(), b"keep outside data");
    }

    #[test]
    fn import_rekey_refuses_a_missing_silo_account_without_creating_one() {
        let root = volume();
        fs::create_dir_all(root.path().join("Library/Preferences")).unwrap();
        write_plist(
            &Dictionary::new(),
            &root
                .path()
                .join("Library/Preferences/com.apple.loginwindow.plist"),
            0o644,
            false,
            Format::Binary,
        )
        .unwrap();
        fs::create_dir_all(root.path().join("private/etc")).unwrap();
        write_file(
            &root.path().join("private/etc/kcpassword"),
            &kcpassword("source-password"),
            0o600,
            false,
        )
        .unwrap();
        let user_path = root
            .path()
            .join("private/var/db/dslocal/nodes/Default/users/silo.plist");

        let error = rekey_import_data_volume(root.path(), &account()).unwrap_err();

        assert!(error.contains("could not inspect"), "{error}");
        assert!(!user_path.exists());
        assert_eq!(
            fs::read(root.path().join("private/etc/kcpassword")).unwrap(),
            kcpassword("source-password")
        );
    }

    #[test]
    fn import_rekey_refuses_a_malformed_account_identity_before_writing() {
        let (root, _, old_hash) = imported_volume();
        let user_path = root
            .path()
            .join("private/var/db/dslocal/nodes/Default/users/silo.plist");
        let mut user = read_plist(&user_path).unwrap();
        user.insert("uid".into(), string_list("502"));
        write_plist(&user, &user_path, 0o600, true, Format::Binary).unwrap();
        let old_login = fs::read(
            root.path()
                .join("Library/Preferences/com.apple.loginwindow.plist"),
        )
        .unwrap();

        let error = rekey_import_data_volume(root.path(), &account()).unwrap_err();

        assert!(error.contains("managed account identity"), "{error}");
        let user = read_plist(&user_path).unwrap();
        assert_eq!(user["ShadowHashData"], old_hash);
        assert_eq!(
            fs::read(
                root.path()
                    .join("Library/Preferences/com.apple.loginwindow.plist")
            )
            .unwrap(),
            old_login
        );
    }

    #[test]
    fn import_rekey_refuses_an_unmanaged_authentication_authority_before_writing() {
        let (root, _, old_hash) = imported_volume();
        let user_path = root
            .path()
            .join("private/var/db/dslocal/nodes/Default/users/silo.plist");
        let mut user = read_plist(&user_path).unwrap();
        user.insert(
            "authentication_authority".into(),
            string_list(";LocalCachedUser;"),
        );
        write_plist(&user, &user_path, 0o600, true, Format::Binary).unwrap();

        let error = rekey_import_data_volume(root.path(), &account()).unwrap_err();

        assert!(error.contains("managed account identity"), "{error}");
        assert_eq!(read_plist(&user_path).unwrap()["ShadowHashData"], old_hash);
    }

    #[test]
    fn import_rekey_refuses_an_unrecognized_additional_authority_before_writing() {
        let (root, _, old_hash) = imported_volume();
        let user_path = root
            .path()
            .join("private/var/db/dslocal/nodes/Default/users/silo.plist");
        let mut user = read_plist(&user_path).unwrap();
        user.insert(
            "authentication_authority".into(),
            Value::Array(vec![
                Value::String(";ShadowHash;HASHLIST:<SALTED-SHA512-PBKDF2>".into()),
                Value::String(";LocalCachedUser;".into()),
            ]),
        );
        write_plist(&user, &user_path, 0o600, true, Format::Binary).unwrap();

        let error = rekey_import_data_volume(root.path(), &account()).unwrap_err();

        assert!(error.contains("managed account identity"), "{error}");
        assert_eq!(read_plist(&user_path).unwrap()["ShadowHashData"], old_hash);
    }

    #[test]
    fn a_guest_stopped_before_its_first_boot_wrote_launchd_state_is_left_untouched() {
        let root = volume();
        fs::remove_file(root.path().join(LAUNCHD_DISABLED)).unwrap();
        let error = patch(root.path(), &account(), None).unwrap_err();
        assert_eq!(error, FIRST_BOOT_INCOMPLETE);
        // Nothing was written, so the same disk can be patched after a longer first boot.
        assert!(!root.path().join("private/var/db/.AppleSetupDone").exists());
        assert!(!root
            .path()
            .join("private/var/db/dslocal/nodes/Default/users/silo.plist")
            .exists());
        let launchd = root.path().join(LAUNCHD_DISABLED);
        write_plist(&Dictionary::new(), &launchd, 0o644, false, Format::Xml).unwrap();
        patch(root.path(), &account(), None).unwrap();
    }

    #[test]
    fn patching_twice_keeps_one_account_and_one_group_entry() {
        let root = volume();
        patch(root.path(), &account(), None).unwrap();
        let path = root
            .path()
            .join("private/var/db/dslocal/nodes/Default/users/silo.plist");
        let first = read_plist(&path).unwrap()["generateduid"].clone();
        patch(root.path(), &account(), None).unwrap();
        assert_eq!(read_plist(&path).unwrap()["generateduid"], first);
        let admin = read_plist(
            &root
                .path()
                .join("private/var/db/dslocal/nodes/Default/groups/admin.plist"),
        )
        .unwrap();
        assert_eq!(admin["users"].as_array().unwrap().len(), 2);
        assert_eq!(admin["groupmembers"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn another_account_with_the_same_user_id_is_refused() {
        let root = volume();
        let users = root
            .path()
            .join("private/var/db/dslocal/nodes/Default/users");
        let mut other = Dictionary::new();
        other.insert("uid".into(), string_list("501"));
        write_plist(
            &other,
            &users.join("someone.plist"),
            0o600,
            false,
            Format::Binary,
        )
        .unwrap();
        let error = patch(root.path(), &account(), None).unwrap_err();
        assert!(error.contains("501"), "{error}");
    }

    /// Patches a copy of a real installed computer's disk. Run by hand with
    /// `SILO_OFFLINE_SETUP_DISK=<copy of disk.img> cargo test offline_setup_on_a_real_disk -- --ignored`.
    #[test]
    #[ignore = "needs a copy of an installed computer's disk"]
    fn offline_setup_on_a_real_disk() {
        let disk = std::env::var("SILO_OFFLINE_SETUP_DISK").expect("SILO_OFFLINE_SETUP_DISK");
        run(
            Path::new(&disk),
            &account(),
            Some(Release {
                version: "26.6.2",
                build: "25G83",
            }),
        )
        .unwrap();
    }

    /// Rekeys a disposable APFS clone of an installed computer's disk. Run with
    /// `SILO_IMPORT_REKEY_DISK=<copy of disk.img> cargo test import_rekey_on_a_real_disk -- --ignored`.
    #[test]
    #[ignore = "needs a disposable copy of an installed computer's disk"]
    fn import_rekey_on_a_real_disk() {
        let disk = std::env::var("SILO_IMPORT_REKEY_DISK").expect("SILO_IMPORT_REKEY_DISK");
        let mut imported_account = account();
        imported_account.password = "disposable-import-rekey-passphrase".into();
        run_import_rekey(Path::new(&disk), &imported_account).unwrap();

        let mut attached = Attached::attach(Path::new(&disk)).unwrap();
        let root = attached.mount_data_volume().unwrap();
        let user = read_plist(&root.join("private/var/db/dslocal/nodes/Default/users/silo.plist"))
            .unwrap();
        assert_eq!(user["name"], string_list(USER));
        assert_eq!(
            user["authentication_authority"],
            string_list(";ShadowHash;HASHLIST:<SALTED-SHA512-PBKDF2>")
        );
        attached.close().unwrap();
    }
}
