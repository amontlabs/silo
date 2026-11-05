//! The recipe cache: one JSON file per built recipe under `<app data>/recipes/`.

use std::{
    fs,
    path::{Path, PathBuf},
};

use silo_api::methods::recipes::RecipeEntry;

pub struct Cache {
    root: PathBuf,
}

impl Cache {
    pub fn new(app_data: &Path) -> Self {
        Self {
            root: app_data.join("recipes"),
        }
    }

    fn file(&self, name: &str) -> Result<PathBuf, String> {
        if !super::valid_name(name) {
            return Err(format!("`{name}` is not a recipe name."));
        }
        Ok(self.root.join(format!("{name}.json")))
    }

    pub fn get(&self, name: &str) -> Result<Option<RecipeEntry>, String> {
        let path = self.file(name)?;
        match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text)
                .map(Some)
                .map_err(|error| format!("The cached recipe {name} is unreadable: {error}")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("Cannot read the cached recipe {name}: {error}")),
        }
    }

    /// Every readable entry, by name. A damaged file is skipped: it holds no usable build.
    pub fn list(&self) -> Vec<RecipeEntry> {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut found: Vec<RecipeEntry> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
            .filter_map(|entry| fs::read_to_string(entry.path()).ok())
            .filter_map(|text| serde_json::from_str(&text).ok())
            .collect();
        found.sort_by(|left, right| left.name.cmp(&right.name));
        found
    }

    /// Writes the entry whole: a reader sees the old file or the new one.
    pub fn put(&self, entry: &RecipeEntry) -> Result<(), String> {
        let path = self.file(&entry.name)?;
        fs::create_dir_all(&self.root)
            .map_err(|error| format!("Cannot create the recipe cache: {error}"))?;
        let text = serde_json::to_string_pretty(entry).map_err(|error| error.to_string())?;
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, text)
            .and_then(|()| fs::rename(&temporary, &path))
            .map_err(|error| format!("Cannot save the recipe {}: {error}", entry.name))
    }

    pub fn remove(&self, name: &str) -> Result<Option<RecipeEntry>, String> {
        let existing = self.get(name)?;
        if existing.is_some() {
            fs::remove_file(self.file(name)?)
                .map_err(|error| format!("Cannot remove the recipe {name}: {error}"))?;
        }
        Ok(existing)
    }
}
