use std::{collections::BTreeMap, path::Path};

pub const KEYS: [&str; 3] = [
    "SILO_GITHUB_APP_SLUG",
    "SILO_GITHUB_CLIENT_ID",
    "SILO_GITHUB_CLIENT_SECRET",
];

// Environment values override the ignored local file, including empty values:
// an empty CI secret must fail rather than silently use another configuration.
pub fn configuration(
    path: &Path,
    environment: impl Fn(&str) -> Option<String>,
) -> Result<BTreeMap<String, String>, String> {
    let local: BTreeMap<String, serde_json::Value> = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| {
            "github-build.local.json must be a JSON object with string values.".to_owned()
        })?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(_) => return Err("Cannot read github-build.local.json.".into()),
    };
    let mut values = BTreeMap::new();
    for key in KEYS {
        let value = match environment(key) {
            Some(env_val) => env_val,
            None => match local.get(key) {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(_) => {
                    return Err(format!(
                        "Missing or invalid {key}. Configure app/SiloUI/github-build.local.json or set the build environment. See docs/SiloUI-RELEASES.md."
                    ));
                }
                None => String::new(),
            },
        };
        // Never include values in diagnostics. Newlines would also inject Cargo directives.
        if value.trim().is_empty() || value.contains(['\r', '\n', '\0']) || value.trim() != value {
            return Err(format!(
                "Missing or invalid {key}. Configure app/SiloUI/github-build.local.json or set the build environment. See docs/SiloUI-RELEASES.md."
            ));
        }
        values.insert(key.to_owned(), value);
    }
    Ok(values)
}
