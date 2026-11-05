//! Typed parser for the guest state manifest (format 1 and 2, see `docs/v1/PHASE2_CONTRACT.md`).
//!
//! One record per line, tab separated. `\t`, `\n` and `\\` inside a field are escaped.
//! Unknown record types are skipped so a newer collector does not break an older reader;
//! a known record with too few fields is an error, because a truncated stream would
//! otherwise read as deleted files.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Highest manifest format this reader understands.
pub const MAX_FORMAT: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "manifest line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ManifestError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    File,
    Link,
}

/// An `F` (regular file) or `L` (symlink) record. `digest` is the sha256 (`-` when the
/// file was too large to hash) for files and the link target for links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub kind: FileKind,
    pub size: String,
    pub mtime: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRepo {
    pub head: String,
    pub branch: String,
    pub dirty: u64,
    pub untracked: u64,
}

/// A gitignored file inside a repo (`GIG`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GigEntry {
    pub repo: String,
    pub size: String,
    pub mtime: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgTable {
    pub digest: String,
    pub rows: Option<u64>,
}

/// Database records (`DB`, `DBT`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Databases {
    /// database name -> schema digest
    pub pg_schema: BTreeMap<String, String>,
    /// (database, table) -> data digest and row count
    pub pg_tables: BTreeMap<(String, String), PgTable>,
    /// file path -> file digest
    pub sqlite: BTreeMap<String, String>,
}

/// A macOS launch agent or daemon (`LAGENT`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchAgent {
    pub digest: String,
    pub path: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    pub files: BTreeMap<String, FileEntry>,
    /// Pruned trees: path -> entry count (`None` when not counted).
    pub pdirs: BTreeMap<String, Option<u64>>,
    /// (manager, name) -> version
    pub pkg: BTreeMap<(String, String), String>,
    /// (manager, name) of manually installed packages
    pub pkgm: BTreeSet<(String, String)>,
    /// (manager, name) -> direct requirements (`PKGD`, optional dependency data)
    pub pkgd: BTreeMap<(String, String), Vec<String>>,
    pub git: BTreeMap<String, GitRepo>,
    /// repo -> (path, status code) of dirty and untracked files
    pub gitf: BTreeMap<String, BTreeMap<String, String>>,
    /// absolute path -> entry
    pub gig: BTreeMap<String, GigEntry>,
    pub cron: BTreeMap<String, String>,
    /// user -> crontab file path
    pub cronf: BTreeMap<String, String>,
    pub svc: BTreeMap<String, String>,
    /// (proto, port) -> process
    pub listen: BTreeMap<(String, String), String>,
    pub proc: BTreeSet<String>,
    pub meta: BTreeMap<String, String>,
    pub db: Databases,
    /// (scope, label) -> agent
    pub lagent: BTreeMap<(String, String), LaunchAgent>,
    /// path -> (manager, package)
    pub own: BTreeMap<String, (String, String)>,
}

fn unescape(s: &str) -> String {
    if !s.contains('\\') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('\\') => out.push('\\'),
            Some(o) => {
                out.push('\\');
                out.push(o);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn num(s: &str) -> Option<u64> {
    s.parse().ok()
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest, ManifestError> {
        let mut m = Manifest::default();
        for (idx, raw) in text.lines().enumerate() {
            if raw.is_empty() {
                continue;
            }
            let line = idx + 1;
            let f: Vec<String> = raw.split('\t').map(unescape).collect();
            let need = |n: usize| -> Result<(), ManifestError> {
                if f.len() < n {
                    Err(ManifestError {
                        line,
                        message: format!("{} record needs {} fields, found {}", f[0], n, f.len()),
                    })
                } else {
                    Ok(())
                }
            };
            match f[0].as_str() {
                "F" | "L" => {
                    need(5)?;
                    let kind = if f[0] == "F" {
                        FileKind::File
                    } else {
                        FileKind::Link
                    };
                    m.files.insert(
                        f[1].clone(),
                        FileEntry {
                            kind,
                            size: f[2].clone(),
                            mtime: f[3].clone(),
                            digest: f[4].clone(),
                        },
                    );
                }
                "P" => {
                    need(3)?;
                    m.pdirs.insert(f[1].clone(), num(&f[2]));
                }
                "PKG" => {
                    need(4)?;
                    m.pkg.insert((f[1].clone(), f[2].clone()), f[3].clone());
                }
                "PKGM" => {
                    need(3)?;
                    m.pkgm.insert((f[1].clone(), f[2].clone()));
                }
                "PKGD" => {
                    need(4)?;
                    let deps = f[3]
                        .split(',')
                        .filter(|d| !d.is_empty())
                        .map(str::to_owned)
                        .collect();
                    m.pkgd.insert((f[1].clone(), f[2].clone()), deps);
                }
                "GIT" => {
                    need(6)?;
                    m.git.insert(
                        f[1].clone(),
                        GitRepo {
                            head: f[2].clone(),
                            branch: f[3].clone(),
                            dirty: num(&f[4]).unwrap_or(0),
                            untracked: num(&f[5]).unwrap_or(0),
                        },
                    );
                }
                "GITF" => {
                    need(4)?;
                    m.gitf
                        .entry(f[1].clone())
                        .or_default()
                        .insert(f[3].clone(), f[2].clone());
                }
                "GIG" => {
                    need(6)?;
                    let path = if f[2].starts_with('/') {
                        f[2].clone()
                    } else {
                        format!("{}/{}", f[1].trim_end_matches('/'), f[2])
                    };
                    m.gig.insert(
                        path,
                        GigEntry {
                            repo: f[1].clone(),
                            size: f[3].clone(),
                            mtime: f[4].clone(),
                            digest: f[5].clone(),
                        },
                    );
                }
                "CRON" => {
                    need(3)?;
                    m.cron.insert(f[1].clone(), f[2].clone());
                }
                "CRONF" => {
                    need(3)?;
                    m.cronf.insert(f[1].clone(), f[2].clone());
                }
                "SVC" => {
                    need(3)?;
                    m.svc.insert(f[1].clone(), f[2].clone());
                }
                "LISTEN" => {
                    need(4)?;
                    m.listen.insert((f[1].clone(), f[2].clone()), f[3].clone());
                }
                "PROC" => {
                    need(2)?;
                    m.proc.insert(f[1].clone());
                }
                "META" => {
                    need(3)?;
                    if f[1] == "format" {
                        let v = num(&f[2]).unwrap_or(u64::MAX);
                        if v > u64::from(MAX_FORMAT) {
                            return Err(ManifestError {
                                line,
                                message: format!(
                                    "manifest format {} is newer than this reader ({})",
                                    f[2], MAX_FORMAT
                                ),
                            });
                        }
                    }
                    m.meta.insert(f[1].clone(), f[2].clone());
                }
                "DB" => {
                    need(4)?;
                    match f[1].as_str() {
                        "postgres" if f[3] == "schema" => {
                            need(5)?;
                            m.db.pg_schema.insert(f[2].clone(), f[4].clone());
                        }
                        "sqlite" => {
                            m.db.sqlite.insert(f[2].clone(), f[3].clone());
                        }
                        _ => {}
                    }
                }
                "DBT" => {
                    need(6)?;
                    if f[1] == "postgres" {
                        m.db.pg_tables.insert(
                            (f[2].clone(), f[3].clone()),
                            PgTable {
                                digest: f[4].clone(),
                                rows: num(&f[5]),
                            },
                        );
                    }
                }
                "LAGENT" => {
                    need(5)?;
                    m.lagent.insert(
                        (f[1].clone(), f[2].clone()),
                        LaunchAgent {
                            digest: f[3].clone(),
                            path: f[4].clone(),
                        },
                    );
                }
                "OWN" => {
                    need(4)?;
                    m.own.insert(f[3].clone(), (f[1].clone(), f[2].clone()));
                }
                _ => {}
            }
        }
        Ok(m)
    }

    pub fn home(&self) -> Option<&str> {
        self.meta
            .get("home")
            .map(String::as_str)
            .filter(|h| !h.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_record_type() {
        let text = "META\tformat\t2\nMETA\thome\t/root\n\
F\t/root/a b\t3\t100\tabc\nL\t/root/l\t1\t100\t/root/a b\nP\t/root/.npm\t10\nP\t/x\t-\n\
PKG\tdpkg\tjq\t1.7\nPKGM\tdpkg\tjq\nPKGD\tdpkg\tjq\tlibjq1,libonig5\n\
GIT\t/root/app\tdeadbeef\tmain\t1\t2\nGITF\t/root/app\t_M\tsrc/a.rs\n\
GIG\t/root/app\t/root/app/.env\t5\t9\tfff\nGIG\t/root/app\t.env.local\t5\t9\t-\n\
CRON\troot\tabc\nCRONF\troot\t/var/spool/cron/crontabs/root\nSVC\tnginx.service\tenabled\n\
LISTEN\ttcp\t3001\tnode\nPROC\tnode\n\
DB\tpostgres\tapp\tschema\t11\nDBT\tpostgres\tapp\tusers\t22\t7\nDB\tsqlite\t/root/x.db\t33\n\
LAGENT\tuser\tdev.x\t44\t/Users/a/Library/LaunchAgents/dev.x.plist\nOWN\tdpkg\tjq\t/usr/bin/jq\n";
        let m = Manifest::parse(text).unwrap();
        assert_eq!(m.files["/root/a b"].size, "3");
        assert_eq!(m.files["/root/l"].kind, FileKind::Link);
        assert_eq!(m.pdirs["/root/.npm"], Some(10));
        assert_eq!(m.pdirs["/x"], None);
        assert_eq!(m.pkg[&("dpkg".into(), "jq".into())], "1.7");
        assert!(m.pkgm.contains(&("dpkg".into(), "jq".into())));
        assert_eq!(
            m.pkgd[&("dpkg".into(), "jq".into())],
            vec!["libjq1", "libonig5"]
        );
        assert_eq!(m.git["/root/app"].untracked, 2);
        assert_eq!(m.gitf["/root/app"]["src/a.rs"], "_M");
        assert!(m.gig.contains_key("/root/app/.env"));
        assert!(m.gig.contains_key("/root/app/.env.local"));
        assert_eq!(m.cronf["root"], "/var/spool/cron/crontabs/root");
        assert_eq!(m.listen[&("tcp".into(), "3001".into())], "node");
        assert_eq!(m.db.pg_schema["app"], "11");
        assert_eq!(
            m.db.pg_tables[&("app".into(), "users".into())].rows,
            Some(7)
        );
        assert_eq!(m.db.sqlite["/root/x.db"], "33");
        assert_eq!(m.lagent[&("user".into(), "dev.x".into())].digest, "44");
        assert_eq!(m.own["/usr/bin/jq"].1, "jq");
        assert_eq!(m.home(), Some("/root"));
    }

    #[test]
    fn unescapes_fields() {
        let m = Manifest::parse("F\t/a\\tb\\nc\\\\d\t1\t2\t-\n").unwrap();
        assert!(m.files.contains_key("/a\tb\nc\\d"));
    }

    #[test]
    fn rejects_truncated_records_and_newer_formats() {
        let e = Manifest::parse("META\tformat\t1\nF\t/a\t1\n").unwrap_err();
        assert_eq!(e.line, 2);
        assert!(Manifest::parse("META\tformat\t3\n").is_err());
        assert!(Manifest::parse("ZZZ\tfuture\trecord\n").is_ok());
    }
}
