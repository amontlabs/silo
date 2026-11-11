//! Engine implementation of [`super::RebuildEnv`] backed by the MicroSandbox runtime host.

use std::{any::Any, fs, path::Path, sync::Arc, time::Duration};

use serde_json::Value;

use super::{ComputerInfo, RebuildEnv};
use crate::{
    guest::{self, ExecRequest, GuestUser},
    recipes::scripts,
    runtime::{self, Booted, ProcessRunner, RuntimeHost, RuntimePaths, RuntimeRunner},
};

pub struct EngineRebuildEnv {
    host: Arc<dyn RuntimeHost>,
}

impl EngineRebuildEnv {
    pub fn new(host: Arc<dyn RuntimeHost>) -> Self {
        Self { host }
    }

    fn paths(&self) -> Result<RuntimePaths, String> {
        runtime::runtime_paths(self.host.as_ref()).map_err(|e| e.to_string())
    }

    fn sandboxes_dir(&self) -> Result<std::path::PathBuf, String> {
        Ok(self.paths()?.home.join("sandboxes"))
    }
}

impl RebuildEnv for EngineRebuildEnv {
    fn computer_info(&self, computer: &str) -> Result<ComputerInfo, String> {
        let paths = self.paths()?;
        let runner = ProcessRunner;
        let inspected = runtime::inspect_computer(&runner, &paths, computer)
            .map_err(|e| format!("Cannot inspect computer '{computer}': {e}"))?;

        let id = inspected
            .config
            .pointer("/labels/silo.machine-id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();

        let cpus = inspected
            .config
            .pointer("/resources/cpus")
            .and_then(Value::as_u64)
            .unwrap_or(1) as u32;

        let max_cpus = inspected
            .config
            .pointer("/resources/max_cpus")
            .and_then(Value::as_u64)
            .unwrap_or(cpus as u64) as u32;

        let memory_mib = inspected
            .config
            .pointer("/resources/memory_mib")
            .and_then(Value::as_u64)
            .unwrap_or(1024);
        let memory_gib = (memory_mib / 1024).max(1) as u32;

        let max_memory_mib = inspected
            .config
            .pointer("/resources/max_memory_mib")
            .and_then(Value::as_u64)
            .unwrap_or(memory_mib);
        let max_memory_gib = (max_memory_mib / 1024).max(memory_gib as u64) as u32;

        let root_disk_gib = inspected
            .config
            .pointer("/labels/silo.runtime-storage-gib")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(4);

        let workspace_storage_gib = inspected
            .config
            .pointer("/labels/silo.workspace-storage-gib")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(1);

        let desktop = inspected
            .config
            .pointer("/desktop/startWithComputer")
            .and_then(Value::as_bool)
            .unwrap_or(true);

        let running = inspected.status.eq_ignore_ascii_case("running");

        // Inspect layers in workspace disk.json
        let sandboxes = self.sandboxes_dir()?;
        let owned_disks = sandboxes.join(computer).join("runtime").join("owned-disks");
        let mut has_checkpoint_layers = false;
        if let Ok(entries) = fs::read_dir(&owned_disks) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("workspace_") {
                    let disk_json = entry.path().join("disk.json");
                    if let Ok(content) = fs::read_to_string(&disk_json) {
                        if let Ok(parsed) = serde_json::from_str::<Value>(&content) {
                            if let Some(layers) = parsed.get("layers").and_then(Value::as_array) {
                                if layers.len() > 1 {
                                    has_checkpoint_layers = true;
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(ComputerInfo {
            id,
            name: computer.to_owned(),
            is_linux: true,
            running,
            cpus,
            max_cpus,
            memory_gib,
            max_memory_gib,
            root_disk_gib,
            workspace_storage_gib,
            desktop,
            has_checkpoint_layers,
        })
    }

    fn computer_exists(&self, name: &str) -> bool {
        let Ok(paths) = self.paths() else { return false };
        let runner = ProcessRunner;
        runtime::sandbox_machine_id(&runner, &paths, name)
            .ok()
            .flatten()
            .is_some()
    }

    fn supports_reflink(&self) -> bool {
        let Ok(sandboxes) = self.sandboxes_dir() else { return false };
        let _ = fs::create_dir_all(&sandboxes);
        let src = sandboxes.join(".reflink_probe_src");
        let dst = sandboxes.join(".reflink_probe_dst");
        let _ = fs::write(&src, b"probe");

        let ok = {
            #[cfg(target_os = "macos")]
            {
                use std::ffi::CString;
                use std::os::unix::ffi::OsStrExt;
                let c_src = CString::new(src.as_os_str().as_bytes()).unwrap();
                let c_dst = CString::new(dst.as_os_str().as_bytes()).unwrap();
                unsafe { libc::clonefile(c_src.as_ptr(), c_dst.as_ptr(), 0) == 0 }
            }
            #[cfg(target_os = "linux")]
            {
                std::process::Command::new("cp")
                    .args(["--reflink=always", src.to_str().unwrap(), dst.to_str().unwrap()])
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
            }
            #[cfg(not(any(target_os = "macos", target_os = "linux")))]
            {
                false
            }
        };

        let _ = fs::remove_file(&src);
        let _ = fs::remove_file(&dst);
        ok
    }

    fn measure_home_bytes(&self, computer: &str) -> Result<u64, String> {
        let paths = self.paths()?;
        let info = self.computer_info(computer)?;
        if info.running {
            let id = runtime::resolve_computer_id(&paths, computer).map_err(|e| e.to_string())?;
            let target = guest::Target::Linux {
                paths: paths.clone(),
                id,
                name: computer.to_owned(),
            };
            let out = guest::run_bounded(
                &target,
                &ExecRequest {
                    argv: vec!["sh".into(), "-c".into(), "du -sb /home/silo 2>/dev/null | cut -f1".into()],
                    cwd: None,
                    env: Vec::new(),
                    user: GuestUser::Root,
                    stdin: None,
                    timeout: Duration::from_secs(60),
                    max_stdout: 1024 * 1024,
                },
            )
            .map_err(|e| e.message)?;
            let text = String::from_utf8_lossy(&out.stdout);
            let bytes = text.trim().parse::<u64>().unwrap_or(0);
            return Ok(bytes);
        }

        // When stopped, check upper.ext4 file size
        let sandboxes = self.sandboxes_dir()?;
        let upper = sandboxes.join(computer).join("upper.ext4");
        if let Ok(meta) = fs::metadata(&upper) {
            let file_size = meta.len();
            // If upper.ext4 is small enough, home cannot be larger than it
            if file_size <= 1_887_436_800 {
                return Ok(file_size);
            }
        }

        // Otherwise temporarily boot to get exact home size
        self.start_computer(computer)?;
        let res = (|| {
            let id = runtime::resolve_computer_id(&paths, computer).map_err(|e| e.to_string())?;
            let target = guest::Target::Linux {
                paths: paths.clone(),
                id,
                name: computer.to_owned(),
            };
            let out = guest::run_bounded(
                &target,
                &ExecRequest {
                    argv: vec!["sh".into(), "-c".into(), "du -sb /home/silo 2>/dev/null | cut -f1".into()],
                    cwd: None,
                    env: Vec::new(),
                    user: GuestUser::Root,
                    stdin: None,
                    timeout: Duration::from_secs(60),
                    max_stdout: 1024 * 1024,
                },
            )
            .map_err(|e| e.message)?;
            let text = String::from_utf8_lossy(&out.stdout);
            Ok(text.trim().parse::<u64>().unwrap_or(0))
        })();
        let _ = self.stop_computer(computer);
        res
    }

    fn hold_lane(&self, computer_id: &str) -> Result<Box<dyn Any>, String> {
        let guard = crate::changes::keep::lane::acquire(computer_id, Duration::from_secs(120))?;
        Ok(Box::new(guard))
    }

    fn stop_computer(&self, computer: &str) -> Result<(), String> {
        let paths = self.paths()?;
        runtime::computer_lifecycle(&paths, "stop", computer).map_err(|e| e.to_string())
    }

    fn start_computer(&self, computer: &str) -> Result<(), String> {
        let paths = self.paths()?;
        runtime::computer_lifecycle(&paths, "start", computer).map_err(|e| e.to_string())
    }

    fn checkpoint_integrity(&self, computer: &str, name: &str) -> Result<String, String> {
        let paths = self.paths()?;
        let runner = ProcessRunner;
        let group = format!("g-{computer}");
        let args = vec![
            "snapshot".into(),
            "create".into(),
            name.into(),
            "--from-sandbox".into(),
            computer.into(),
            "--group".into(),
            group,
            "--integrity".into(),
        ];
        runner
            .run(&paths, &args, Duration::from_secs(300))
            .map_err(|e| format!("Integrity checkpoint failed: {e}"))?;
        Ok(name.to_owned())
    }

    fn newest_image(&self) -> Result<String, String> {
        let paths = self.paths()?;
        let runner = ProcessRunner;
        runner
            .prepare_guest_image(&paths)
            .map_err(|e| format!("Cannot prepare newest guest image: {e}"))
    }

    fn create_computer(
        &self,
        name: &str,
        id: &str,
        image: &str,
        info: &ComputerInfo,
    ) -> Result<(), String> {
        let paths = self.paths()?;
        let runner = ProcessRunner;
        let mut args: Vec<String> = vec![
            "create".into(),
            image.to_owned(),
            "--pull".into(),
            "never".into(),
            "--name".into(),
            name.to_owned(),
            "--cpus".into(),
            info.cpus.to_string(),
            "--max-cpus".into(),
            info.max_cpus.to_string(),
            "--memory".into(),
            format!("{}G", info.memory_gib.max(1)),
            "--max-memory".into(),
            format!("{}G", info.max_memory_gib.max(1)),
            "--root-disk".into(),
            format!("{}G", info.root_disk_gib.max(4)),
            "--mount-owned".into(),
            format!("/workspace:kind=disk,size={}G", info.workspace_storage_gib.max(1)),
            "--label".into(),
            "silo.managed=true".into(),
            "--label".into(),
            format!("silo.machine-id={id}"),
            "--label".into(),
            format!("silo.workspace-storage-gib={}", info.workspace_storage_gib),
            "--label".into(),
            format!("silo.runtime-storage-gib={}", info.root_disk_gib),
            "--net-strict=true".into(),
            "--no-start".into(),
            "--quiet".into(),
        ];
        runner
            .run(&paths, &args, Duration::from_secs(300))
            .map_err(|e| format!("Cannot create rebuild sandbox '{name}': {e}"))?;
        Ok(())
    }

    fn setup_account(&self, computer: &str) -> Result<(), String> {
        let paths = self.paths()?;
        crate::working_account::prepare(&Booted, &paths, computer).map_err(|e| e.to_string())
    }

    fn move_workspace_disk(&self, from: &str, to: &str) -> Result<(), String> {
        let sandboxes = self.sandboxes_dir()?;
        let src_sandbox = sandboxes.join(from);
        let dst_sandbox = sandboxes.join(to);

        // Find the owned workspace volume
        let src_owned = src_sandbox.join("owned-volumes");
        let dst_owned = dst_sandbox.join("owned-volumes");

        let mut vol_name = None;
        if let Ok(entries) = fs::read_dir(&src_owned) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("workspace_") {
                    vol_name = Some(name);
                    break;
                }
            }
        }
        let vol = vol_name.ok_or_else(|| "Source workspace volume not found".to_owned())?;

        let src_vol = src_owned.join(&vol);
        let staged = dst_owned.join(format!(".swap-{vol}"));
        let dst_vol = dst_owned.join(&vol);

        // Clone src_vol to staged
        #[cfg(target_os = "macos")]
        {
            let status = std::process::Command::new("cp")
                .args(["-c", "-R", src_vol.to_str().unwrap(), staged.to_str().unwrap()])
                .status()
                .map_err(|e| format!("Failed to clone workspace volume: {e}"))?;
            if !status.success() {
                return Err("Failed to copy workspace disk with cp -c".into());
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let status = std::process::Command::new("cp")
                .args(["--reflink=always", "-R", src_vol.to_str().unwrap(), staged.to_str().unwrap()])
                .status()
                .map_err(|e| format!("Failed to clone workspace volume: {e}"))?;
            if !status.success() {
                return Err("Failed to copy workspace disk with cp --reflink".into());
            }
        }

        let old_backup = dst_sandbox.join(".workspace-before-swap");
        let _ = fs::remove_dir_all(&old_backup);
        fs::rename(&dst_vol, &old_backup).map_err(|e| e.to_string())?;
        fs::rename(&staged, &dst_vol).map_err(|e| e.to_string())?;

        // Update runtime/owned-disks/<vol>/disk.json
        let src_disk_json = src_sandbox
            .join("runtime")
            .join("owned-disks")
            .join(&vol)
            .join("disk.json");
        let dst_disk_json = dst_sandbox
            .join("runtime")
            .join("owned-disks")
            .join(&vol)
            .join("disk.json");

        if src_disk_json.exists() && dst_disk_json.exists() {
            let src_text = fs::read_to_string(&src_disk_json).map_err(|e| e.to_string())?;
            let dst_text = fs::read_to_string(&dst_disk_json).map_err(|e| e.to_string())?;
            let mut src_val: Value = serde_json::from_str(&src_text).map_err(|e| e.to_string())?;
            let dst_val: Value = serde_json::from_str(&dst_text).map_err(|e| e.to_string())?;

            if let Some(target_volume_id) = dst_val.get("volume_id") {
                src_val["volume_id"] = target_volume_id.clone();
            }
            let tmp = dst_disk_json.with_extension("tmp");
            fs::write(&tmp, serde_json::to_string(&src_val).unwrap()).map_err(|e| e.to_string())?;
            fs::rename(&tmp, &dst_disk_json).map_err(|e| e.to_string())?;
        }

        Ok(())
    }

    fn carry_home(&self, from: &str, to: &str) -> Result<(), String> {
        let paths = self.paths()?;
        let runner = ProcessRunner;
        let id_from = runtime::resolve_computer_id(&paths, from).map_err(|e| e.to_string())?;
        let id_to = runtime::resolve_computer_id(&paths, to).map_err(|e| e.to_string())?;

        let target_from = guest::Target::Linux {
            paths: paths.clone(),
            id: id_from,
            name: from.to_owned(),
        };
        let target_to = guest::Target::Linux {
            paths: paths.clone(),
            id: id_to,
            name: to.to_owned(),
        };

        // 1. Tar inside from
        let tar_out = guest::run_bounded(
            &target_from,
            &ExecRequest {
                argv: vec![
                    "sh".into(),
                    "-c".into(),
                    "tar -C /home -cpf /tmp/home.tar --sparse --xattrs --acls --numeric-owner silo 2>&1 | grep -v 'socket ignored'; true".into(),
                ],
                cwd: None,
                env: Vec::new(),
                user: GuestUser::Root,
                stdin: None,
                timeout: Duration::from_secs(300),
                max_stdout: 1024 * 1024,
            },
        )
        .map_err(|e| e.message)?;

        let stage_file = paths.home.join(format!("home-rebuild-{}.tar", uuid::Uuid::new_v4().simple()));
        // 2. msb cp from:/tmp/home.tar stage_file
        runner
            .run(
                &paths,
                &["cp".into(), format!("{from}:/tmp/home.tar"), stage_file.to_str().unwrap().into()],
                Duration::from_secs(300),
            )
            .map_err(|e| format!("Failed to copy home.tar from computer: {e}"))?;

        // Clean up from computer
        let _ = guest::run_bounded(
            &target_from,
            &ExecRequest {
                argv: vec!["rm".into(), "-f".into(), "/tmp/home.tar".into()],
                cwd: None,
                env: Vec::new(),
                user: GuestUser::Root,
                stdin: None,
                timeout: Duration::from_secs(30),
                max_stdout: 1024,
            },
        );

        // 3. msb cp stage_file to:/tmp/home.tar
        runner
            .run(
                &paths,
                &["cp".into(), stage_file.to_str().unwrap().into(), format!("{to}:/tmp/home.tar")],
                Duration::from_secs(300),
            )
            .map_err(|e| format!("Failed to copy home.tar to new computer: {e}"))?;

        let _ = fs::remove_file(&stage_file);

        // 4. Extract inside to
        let extract_out = guest::run_bounded(
            &target_to,
            &ExecRequest {
                argv: vec![
                    "sh".into(),
                    "-c".into(),
                    "tar -C /home -xpf /tmp/home.tar --xattrs --acls --numeric-owner --same-owner && rm -f /tmp/home.tar && sync".into(),
                ],
                cwd: None,
                env: Vec::new(),
                user: GuestUser::Root,
                stdin: None,
                timeout: Duration::from_secs(300),
                max_stdout: 1024 * 1024,
            },
        )
        .map_err(|e| e.message)?;

        if extract_out.exit_code != Some(0) {
            return Err("Failed to extract home directory in rebuilt computer".into());
        }

        Ok(())
    }

    fn copy_ssh_folder(&self, from: &str, to: &str) -> Result<(), String> {
        let sandboxes = self.sandboxes_dir()?;
        let from_ssh = sandboxes.join(from).join("ssh");
        let to_ssh = sandboxes.join(to).join("ssh");
        if from_ssh.exists() {
            let _ = fs::create_dir_all(&to_ssh);
            if let Ok(entries) = fs::read_dir(&from_ssh) {
                for entry in entries.flatten() {
                    let dest = to_ssh.join(entry.file_name());
                    let _ = fs::copy(entry.path(), dest);
                }
            }
        }
        Ok(())
    }

    fn replay_recipe(&self, computer: &str, spec: &RecipeSpec) -> Result<(), String> {
        let paths = self.paths()?;
        let id = runtime::resolve_computer_id(&paths, computer).map_err(|e| e.to_string())?;
        let target = guest::Target::Linux {
            paths: paths.clone(),
            id,
            name: computer.to_owned(),
        };

        for step in scripts::all_steps(spec) {
            let out = guest::run_bounded(
                &target,
                &ExecRequest {
                    argv: vec!["bash".into(), "-c".into(), step.script.clone()],
                    cwd: None,
                    env: step
                        .args
                        .iter()
                        .map(|(k, v)| (format!("A_{k}"), v.clone()))
                        .collect(),
                    user: match step.who {
                        crate::recipes::RunAs::Root => GuestUser::Root,
                        crate::recipes::RunAs::Account => GuestUser::Account,
                    },
                    stdin: None,
                    timeout: Duration::from_secs(1800),
                    max_stdout: 16 * 1024 * 1024,
                },
            )
            .map_err(|e| format!("{}: {}", step.label, e.message))?;

            if out.exit_code != Some(0) {
                return Err(format!("Recipe step '{}' failed", step.label));
            }
        }
        Ok(())
    }

    fn swap_names_and_relabel(
        &self,
        old_name: &str,
        old_id: &str,
        rebuild_name: &str,
        rebuild_id: &str,
        undo_name: &str,
        undo_id: &str,
    ) -> Result<(), String> {
        let paths = self.paths()?;
        let runner = ProcessRunner;

        // 1. Rename old -> undo
        runtime::rename_sandbox(&runner, &paths, old_name, undo_name)
            .map_err(|e| e.to_string())?;

        // 2. Rename rebuild -> old_name
        runtime::rename_sandbox(&runner, &paths, rebuild_name, old_name)
            .map_err(|e| e.to_string())?;

        // 3. Relabel machine-id
        runner
            .run(
                &paths,
                &[
                    "modify".into(),
                    undo_name.into(),
                    "--label".into(),
                    format!("silo.machine-id={undo_id}"),
                ],
                Duration::from_secs(60),
            )
            .map_err(|e| e.to_string())?;

        runner
            .run(
                &paths,
                &[
                    "modify".into(),
                    old_name.into(),
                    "--label".into(),
                    format!("silo.machine-id={old_id}"),
                ],
                Duration::from_secs(60),
            )
            .map_err(|e| e.to_string())?;

        // 4. Update metadata in inventory: old_id keeps old_name, undo_id is added
        if let Ok(mut inventory) = runtime::read_metadata(&paths.metadata) {
            let old_entry = inventory.computers.iter().find(|c| c.id() == old_id).cloned();
            if let Some(mut undo_entry) = old_entry {
                undo_entry.id = undo_id.to_owned();
                undo_entry.name = undo_name.to_owned();
                inventory.computers.push(undo_entry);
                let _ = runtime::write_metadata(&paths.metadata, &inventory);
            }
        }

        // 5. Invalidate caches
        runtime::forget_name_state(&paths, old_name);
        runtime::forget_name_state(&paths, undo_name);

        Ok(())
    }

    fn delete_computer(&self, computer: &str) -> Result<(), String> {
        let paths = self.paths()?;
        let runner = ProcessRunner;
        runner
            .run(&paths, &["remove".into(), computer.into()], Duration::from_secs(120))
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}
