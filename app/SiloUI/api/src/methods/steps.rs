//! Steps (the commands an agent runs on a computer through Silo) and the fork tree.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

wire_struct! {
    StepsListParams {
        /// The device that owns this; absent or `local` is this device.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        computer_id: String,
    }
}

wire_struct! {
    StepsListResult {
        steps: Vec<Step>,
    }
}

wire_struct! {
    /// Records a finished command as the computer's next step. The engine numbers it and
    /// links it to the step (and checkpoint) it follows.
    StepsRecordParams {
        /// The device that owns this; absent or `local` is this device.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        computer_id: String,
        /// The command as an argument vector; the engine joins and shortens it.
        command: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
        started_at: String,
        finished_at: String,
        /// The `step` checkpoint taken after the command, when one was.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        checkpoint_id: Option<String>,
    }
}

wire_struct! {
    StepsRecordResult {
        step: Step,
    }
}

wire_struct! {
    Step {
        id: String,
        computer_id: String,
        /// 1-based per computer and never reused.
        number: u32,
        /// The command, joined and cut to 200 characters.
        command: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
        started_at: String,
        finished_at: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        checkpoint_id: Option<String>,
        /// The step before this one on the same computer.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_step: Option<u32>,
        /// For a fork's first step: the checkpoint the fork was made from.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_checkpoint: Option<String>,
    }
}

/// `computers.tree` reads this device, or the one `deviceId` names.
pub type ComputersTreeParams = crate::methods::common::DeviceParams;

wire_struct! {
    ComputersTreeResult {
        nodes: Vec<TreeNode>,
    }
}

wire_struct! {
    TreeNode {
        id: String,
        name: String,
        /// The computer this one was forked from, after any promote in force.
        parent: Option<String>,
        from: TreeFrom,
        state: String,
        pending_changes: PendingChanges,
    }
}

/// What a fork was made from.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TreeFrom {
    /// Not a fork, or a fork with no record.
    Root,
    /// The parent's state when the fork was made.
    Main,
    Checkpoint {
        checkpoint_id: String,
    },
    Step {
        number: u32,
        checkpoint_id: String,
    },
    Recipe {
        hash: String,
    },
    /// A host folder; filled in by the `silo` command line, which keeps that record.
    Folder {
        path: String,
    },
}

wire_struct! {
    PendingChanges {
        /// The open review request for this fork, if there is one.
        request_id: Option<String>,
        /// Changed paths by bucket, when cheaply known.
        buckets: Option<BTreeMap<String, u64>>,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_origins_use_a_kind_tag() {
        let step = TreeFrom::Step {
            number: 3,
            checkpoint_id: "cp".into(),
        };
        let text = serde_json::to_string(&step).unwrap();
        assert_eq!(text, r#"{"kind":"step","number":3,"checkpointId":"cp"}"#);
        assert_eq!(
            serde_json::from_str::<TreeFrom>(r#"{"kind":"root"}"#).unwrap(),
            TreeFrom::Root
        );
    }

    #[test]
    fn a_record_needs_no_checkpoint() {
        let params: StepsRecordParams = serde_json::from_str(
            r#"{"computerId":"c","command":["ls"],"startedAt":"a","finishedAt":"b"}"#,
        )
        .unwrap();
        assert_eq!(params.checkpoint_id, None);
    }
}
