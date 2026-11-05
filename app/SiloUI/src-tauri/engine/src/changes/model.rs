//! The manifest and diff types the capture code uses, behind one boundary.
//!
//! This is a minimal stand-in with the contract's signatures. The real `changes::manifest`
//! and `changes::diff` replace it by turning this file into
//! `pub use super::{diff::{diff, Diff}, ignore::IgnoreList, manifest::Manifest};`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Typed records of a manifest (the stand-in keeps the file records only).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Manifest {
    files: BTreeMap<String, String>,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest, String> {
        if !text.starts_with("META\tformat\t2\n") {
            return Err("The manifest is not format 2.".to_owned());
        }
        let mut files = BTreeMap::new();
        for line in text.lines() {
            let fields: Vec<&str> = line.split('\t').collect();
            if matches!(fields.first(), Some(&"F") | Some(&"L")) && fields.len() >= 3 {
                files.insert(fields[1].to_owned(), fields[2..].join("\t"));
            }
        }
        Ok(Manifest { files })
    }
}

#[derive(Clone, Debug, Default)]
pub struct IgnoreList;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<String>,
}

impl Diff {
    pub fn render(&self, max_lines: Option<usize>) -> String {
        let mut lines: Vec<String> = Vec::new();
        for (mark, paths) in [("+", &self.added), ("-", &self.removed), ("~", &self.changed)] {
            lines.extend(paths.iter().map(|path| format!("{mark} {path}")));
        }
        if lines.is_empty() {
            return "No changes.".to_owned();
        }
        if let Some(limit) = max_lines.filter(|limit| *limit < lines.len()) {
            let more = lines.len() - limit;
            lines.truncate(limit);
            lines.push(format!("... +{more} more"));
        }
        lines.join("\n")
    }
}

pub fn diff(
    base: &Manifest,
    current: &Manifest,
    baseline: Option<&Diff>,
    _ignore: &IgnoreList,
) -> Diff {
    let mut result = Diff::default();
    for (path, signature) in &current.files {
        match base.files.get(path) {
            None => result.added.push(path.clone()),
            Some(before) if before != signature => result.changed.push(path.clone()),
            Some(_) => {}
        }
    }
    result.removed = base
        .files
        .keys()
        .filter(|path| !current.files.contains_key(*path))
        .cloned()
        .collect();
    if let Some(noise) = baseline {
        for list in [&mut result.added, &mut result.removed, &mut result.changed] {
            list.retain(|path| {
                !noise.added.contains(path)
                    && !noise.removed.contains(path)
                    && !noise.changed.contains(path)
            });
        }
    }
    result
}
