//! Recipes: `recipes.save`, `recipes.get`, `recipes.list`, `recipes.remove` and
//! `recipes.build`, and `computers.recipe`, which shows the recipe a computer was built from.
//!
//! A recipe is the text description of a computer. The engine stores named recipes, builds
//! one into a hidden computer, saves a checkpoint of it, and forks from that checkpoint, so
//! every build input is in [`RecipeSpec`]. What happens in a fork (`quiet`, `scrub`) and when
//! changes are merged into a computer (`keep`) is not part of the spec and never changes the
//! build, but it belongs to the same [`RecipeDefinition`].

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
        /// `false`: the computer, and every fork of it, starts no desktop (a Linux recipe;
        /// absent or `true` keeps the desktop on).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        desktop: Option<bool>,
    }
}

wire_struct! {
    /// A stored recipe: what builds the computer, and the rules that go with it.
    RecipeDefinition {
        spec: RecipeSpec,
        /// Process names and units stopped in a fork of a computer built from this recipe.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        quiet: Vec<String>,
        /// Absolute paths removed from such a fork.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        scrub: Vec<String>,
        /// Run when changes are merged into a computer built from this recipe.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keep: Option<super::keep::KeepHooks>,
        /// The command that tests the project; a note for people and agents, never run by Silo.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        test: Option<String>,
        /// Settings files merged into the computer from forks. Merging appends to it.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        files: Vec<String>,
    }
}

wire_struct! {
    RecipeSaveParams {
        /// The device that owns this; absent or `local` is this device.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        /// Lowercase letters, digits, `-` and `_`. Saving over a name replaces that recipe;
        /// computers already built from it keep the text they were built from.
        name: String,
        recipe: RecipeDefinition,
    }
}

wire_struct! {
    RecipeGetParams {
        /// The device that owns this; absent or `local` is this device.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        name: String,
    }
}

wire_struct! {
    /// A stored recipe with what the engine knows about it.
    RecipeDocument {
        name: String,
        /// Shipped with Silo (`linux-dev`) and not saved by anyone. Saving a recipe with this
        /// name replaces it until that recipe is removed.
        built_in: bool,
        /// Content hash of the normalized spec on the current image.
        hash: String,
        recipe: RecipeDefinition,
        /// The build forks start from, when one exists for this exact spec.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        built: Option<RecipeEntry>,
    }
}

wire_struct! {
    /// One line of the recipe list.
    RecipeSummary {
        name: String,
        built_in: bool,
        os: RecipeOs,
        hash: String,
        /// A build of this exact spec exists.
        built: bool,
    }
}

wire_struct! {
    /// The recipe a computer was built from, as it was when it was built. Forks have their
    /// source's record.
    RecipeRecord {
        name: String,
        /// Content hash of the normalized spec the computer was built from.
        hash: String,
        built_at: String,
        recipe: RecipeDefinition,
    }
}

wire_struct! {
    ComputerRecipeResult {
        /// Absent for a computer that was not built from a recipe.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        recipe: Option<RecipeRecord>,
    }
}

wire_struct! {
    RecipeBuildParams {
        /// The device that owns this; absent or `local` is this device.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        /// The recipe's name: a stored recipe (`linux-dev` is always there).
        name: String,
        /// Build this spec instead of the stored recipe's.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spec: Option<RecipeSpec>,
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
        /// The builds forks start from.
        recipes: Vec<RecipeEntry>,
        /// Every stored recipe, built or not.
        #[serde(default)]
        saved: Vec<RecipeSummary>,
    }
}

wire_struct! {
    RecipeRemoveParams {
        /// The device that owns this; absent or `local` is this device.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
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
