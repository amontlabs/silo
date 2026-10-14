//! Resolve only Silo-owned tools. Debian's global /usr/bin belongs to the package
//! manager and must never contain, or substitute for, Silo's private Git/runtime.
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BundleKind {
    App,
    AppImage,
    Deb,
    Rpm,
    Other,
}

pub(crate) fn is_packaged_linux(bundle: Option<BundleKind>) -> bool {
    matches!(
        bundle,
        Some(BundleKind::AppImage | BundleKind::Deb | BundleKind::Rpm)
    )
}

pub fn resolve(
    executable: &Path,
    _resources: &Path,
    bundle: Option<BundleKind>,
    appimage_root: Option<&Path>,
) -> Result<PathBuf, String> {
    match bundle {
        Some(BundleKind::AppImage) => appimage_root
            .map(|root| root.join("usr/libexec/silo/tools"))
            .ok_or_else(|| "Silo could not locate its AppImage tools directory.".into()),
        Some(BundleKind::Deb | BundleKind::Rpm) => Ok(PathBuf::from("/usr/libexec/silo/tools")),
        _ => executable
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| "Silo could not locate its bundled tools directory.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packaged_linux_resolves_managed_tools_outside_linuxdeploy_scan_roots() {
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("usr/bin/silo-ui");
        let resources = root.path().join("usr/lib/Silo");
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(executable.with_file_name("git"), "unrelated system Git").unwrap();
        for bundle in [BundleKind::AppImage, BundleKind::Deb, BundleKind::Rpm] {
            let appimage_root = root.path().join("Silo.AppDir");
            let directory =
                resolve(&executable, &resources, Some(bundle), Some(&appimage_root)).unwrap();
            let expected = if bundle == BundleKind::AppImage {
                appimage_root.join("usr/libexec/silo/tools")
            } else {
                PathBuf::from("/usr/libexec/silo/tools")
            };
            assert_eq!(directory, expected);
            // Never the executable's own directory, whose tool is unrelated. The fixed
            // package path is not inspected: a computer with Silo installed has files there.
            assert_ne!(directory, executable.parent().unwrap());
            if bundle == BundleKind::AppImage {
                assert!(!directory.join("git").exists());
            }
        }
        assert_eq!(
            std::fs::read(executable.with_file_name("git")).unwrap(),
            b"unrelated system Git"
        );
    }
    #[test]
    fn appimage_macos_and_development_keep_their_existing_sibling_tools() {
        for bundle in [Some(BundleKind::App), None] {
            assert_eq!(
                resolve(
                    Path::new("/bundle/bin/silo-ui"),
                    Path::new("/bundle/resources"),
                    bundle,
                    None,
                )
                .unwrap(),
                Path::new("/bundle/bin")
            );
        }
    }

    #[test]
    fn appimage_requires_its_runtime_root_instead_of_host_lookup() {
        let error = resolve(
            Path::new("/tmp/AppDir/usr/bin/silo-ui"),
            Path::new("/tmp/AppDir/usr/lib/Silo"),
            Some(BundleKind::AppImage),
            None,
        )
        .unwrap_err();
        assert!(error.contains("AppImage tools directory"));
    }
}
