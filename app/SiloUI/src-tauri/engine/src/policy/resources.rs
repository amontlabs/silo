//! The sizes of a computer, and whether a new computer asks for more than its maker has.

use serde_json::Value;

/// A computer's CPUs, memory and storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resources {
    /// A Linux computer has a ceiling above its current size and two storage areas; a macOS
    /// computer's ceiling is its size and its one disk counts as workspace storage.
    pub linux: bool,
    pub cpus: u64,
    pub max_cpus: u64,
    pub memory_gib: u64,
    pub max_memory_gib: u64,
    pub workspace_gib: u64,
    pub runtime_gib: u64,
}

fn number(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}

impl Resources {
    /// A Linux computer's `configuration`, as the request names it or `computers.list` shows it.
    pub fn of_linux(configuration: &Value) -> Option<Self> {
        Some(Self {
            linux: true,
            cpus: number(configuration, "cpus")?,
            max_cpus: number(configuration, "maxCPUs")?,
            memory_gib: number(configuration, "memoryGiB")?,
            max_memory_gib: number(configuration, "maxMemoryGiB")?,
            workspace_gib: number(configuration, "workspaceStorageGiB")?,
            runtime_gib: number(configuration, "runtimeStorageGiB")?,
        })
    }

    /// A macOS computer, as `computers.list` shows it or a create request names it.
    pub fn of_macos(computer: &Value) -> Option<Self> {
        let (cpus, memory) = (number(computer, "cpus")?, number(computer, "memoryGiB")?);
        Some(Self {
            linux: false,
            cpus,
            max_cpus: cpus,
            memory_gib: memory,
            max_memory_gib: memory,
            workspace_gib: number(computer, "diskGiB")?,
            runtime_gib: 0,
        })
    }

    fn storage_gib(&self) -> u64 {
        self.workspace_gib + self.runtime_gib
    }

    /// What `self`, asked for a new computer, has more of than `own`. Two Linux computers are
    /// compared size by size; otherwise CPUs, memory and the storage in all.
    pub fn exceeding(&self, own: &Resources) -> Vec<String> {
        let mut more = Vec::new();
        let mut check = |what: &str, asked: u64, has: u64| {
            if asked > has {
                more.push(format!("{what} {asked} (it has {has})"));
            }
        };
        check("CPUs", self.cpus, own.cpus);
        check("memory in GiB", self.memory_gib, own.memory_gib);
        if self.linux && own.linux {
            check("maximum CPUs", self.max_cpus, own.max_cpus);
            check(
                "maximum memory in GiB",
                self.max_memory_gib,
                own.max_memory_gib,
            );
            check(
                "workspace storage in GiB",
                self.workspace_gib,
                own.workspace_gib,
            );
            check("runtime storage in GiB", self.runtime_gib, own.runtime_gib);
        } else {
            check("storage in GiB", self.storage_gib(), own.storage_gib());
        }
        more
    }

    /// The sizes in a sentence.
    pub fn describe(&self) -> String {
        if self.linux {
            format!(
                "{} CPUs (up to {}), {} GiB of memory (up to {}), {} GiB of workspace storage and {} GiB of runtime storage",
                self.cpus,
                self.max_cpus,
                self.memory_gib,
                self.max_memory_gib,
                self.workspace_gib,
                self.runtime_gib
            )
        } else {
            format!(
                "{} CPUs, {} GiB of memory and a {} GiB disk",
                self.cpus, self.memory_gib, self.workspace_gib
            )
        }
    }
}
