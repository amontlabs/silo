//! Recipes: `recipes.build`, `recipes.list` and `recipes.remove`.
//!
//! A recipe is a clean machine spec. The engine builds one into a hidden computer, saves a
//! checkpoint of it, and forks from that checkpoint, so every build input is in
//! [`RecipeSpec`]. What happens in a fork (`quiet:`, `scrub:`, `keep:`) is not part of the
//! spec and never changes the build.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum RecipeOs {
    Linux,
    Macos,
}

wire_struct! {
    /// Packages per manager, installed in this order: apt, brew, pip, pipx, npm, cargo.
    RecipePackages {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        apt: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        brew: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        npm: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pip: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pipx: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        cargo: Vec<String>,
    }
}

impl Default for RecipePackages {
    fn default() -> Self {
        Self {
            apt: Vec::new(),
            brew: Vec::new(),
            npm: Vec::new(),
            pip: Vec::new(),
            pipx: Vec::new(),
            cargo: Vec::new(),
        }
    }
}

wire_struct! {
    /// A command started when the computer boots.
    RecipeService {
        name: String,
        run: String,
    }
}

wire_struct! {
    /// Everything that decides what a built recipe contains.
    RecipeSpec {
        os: RecipeOs,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cpus: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        memory_gib: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        disk_gib: Option<u32>,
        #[serde(default)]
        packages: RecipePackages,
        /// Commands run once, as root on Linux and as the account on macOS.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        setup: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        services: Vec<RecipeService>,
        /// Ports a fork publishes.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        ports: Vec<u16>,
    }
}

wire_struct! {
    RecipeBuildParams {
        /// The recipe's name (`linux-dev`, or a file under `.silo/recipes/`).
        name: String,
        spec: RecipeSpec,
        /// Build again even when the cache holds this spec.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        force: Option<bool>,
    }
}

wire_struct! {
    /// A built recipe: the hidden computer and the checkpoint forks start from.
    RecipeEntry {
        name: String,
        /// Content hash of the normalized spec.
        hash: String,
        /// The guest image version it was built on (`unknown` when none is pinned).
        image_version: String,
        os: RecipeOs,
        computer_id: String,
        computer_name: String,
        checkpoint_id: String,
        built_at: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        ports: Vec<u16>,
    }
}

wire_struct! {
    /// The final result of `recipes.build`.
    RecipeBuildResult {
        recipe: RecipeEntry,
        /// False when the cache already held this spec on this image.
        built: bool,
    }
}

wire_struct! {
    RecipesListResult {
        recipes: Vec<RecipeEntry>,
    }
}

wire_struct! {
    RecipeRemoveParams {
        name: String,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_minimal_spec_round_trips() {
        let spec: RecipeSpec = serde_json::from_str(r#"{"os":"linux"}"#).unwrap();
        assert_eq!(spec.os, RecipeOs::Linux);
        assert_eq!(spec.packages, RecipePackages::default());
        assert_eq!(
            serde_json::to_string(&spec).unwrap(),
            r#"{"os":"linux","packages":{}}"#
        );
    }
}
