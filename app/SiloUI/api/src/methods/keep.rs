//! Keep and undo: `changes.keep` and `changes.undo`.

use serde::{Deserialize, Serialize};

wire_struct! {
    /// The recipe's `keep:` section.
    KeepHooks {
        /// Services restarted after the changes are applied.
        #[serde(default)]
        restart: Vec<String>,
        /// Commands run once the code and files are in place (database migrations).
        #[serde(default)]
        migrate: Vec<String>,
        /// Commands run last.
        #[serde(default)]
        after: Vec<String>,
    }
}

wire_struct! {
    ChangesKeepParams {
        /// The fork whose changes are kept.
        fork_id: String,
        /// The computer to keep into; defaults to the one the fork was made from.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keep: Option<KeepHooks>,
        /// Check and report what a keep would do, changing nothing.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dry_run: Option<bool>,
    }
}

wire_struct! {
    ChangesUndoParams {
        /// The computer a keep changed.
        computer_id: String,
    }
}

wire_struct! {
    KeepConflict {
        /// `file`, `package`, `crontab`, `launch agent` or `git`.
        kind: String,
        item: String,
        detail: String,
    }
}

wire_struct! {
    KeepRefusal {
        /// `divergent`, `database` or `git`.
        reason: String,
        message: String,
        conflicts: Vec<KeepConflict>,
        hint: String,
    }
}

wire_struct! {
    KeepReport {
        fork: String,
        target: String,
        summary: Vec<String>,
        steps: Vec<String>,
        notes: Vec<String>,
        /// The before-keep checkpoint of the target.
        checkpoint: Option<String>,
    }
}

wire_struct! {
    /// The final result of `changes.keep`. `status` is `kept`, `dry-run` or `refused`; a
    /// refused keep did not change the target.
    ChangesKeepResult {
        status: String,
        report: KeepReport,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        refusal: Option<KeepRefusal>,
    }
}

wire_struct! {
    UndoneCheckpoint {
        id: String,
        name: String,
        created_at: String,
    }
}

wire_struct! {
    /// The final result of `changes.undo`. `status` is `undone` or `nothing-to-undo`.
    ChangesUndoResult {
        status: String,
        computer: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        recovered: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        checkpoint: Option<UndoneCheckpoint>,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keep_params_default_to_the_forks_source_and_no_hooks() {
        let params: ChangesKeepParams = serde_json::from_str(r#"{"forkId":"f"}"#).unwrap();
        assert_eq!(
            (params.target_id, params.keep, params.dry_run),
            (None, None, None)
        );
        let hooks: KeepHooks = serde_json::from_str(r#"{"restart":["web"]}"#).unwrap();
        assert!(hooks.migrate.is_empty() && hooks.after.is_empty());
    }

    #[test]
    fn a_refused_result_round_trips() {
        let value = serde_json::json!({
            "status": "refused",
            "report": {"fork": "f", "target": "m", "summary": [], "steps": [], "notes": [], "checkpoint": null},
            "refusal": {"reason": "database", "message": "m", "conflicts": [], "hint": "h"}
        });
        let result: ChangesKeepResult = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(&result).unwrap()["refusal"]["reason"],
            "database"
        );
    }
}
