/// Install a fake executable only inside a caller-owned temporary directory.
#[cfg(target_os = "linux")]
pub(crate) fn write_shell_script(path: &std::path::Path, script: impl AsRef<str>) {
    use std::os::unix::fs::PermissionsExt;
    let script = format!("#!/bin/sh\n{}", script.as_ref());
    std::fs::write(path, script).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}
