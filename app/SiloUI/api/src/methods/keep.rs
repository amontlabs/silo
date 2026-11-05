//! Keep and undo: `changes.keep` and `changes.undo`.

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
        /// Carry database changes even though no migration step covers them.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        data_ok: Option<bool>,
        /// The `reviewDigest` of the `changes.diff` the user reviewed; the keep refuses when the
        /// fork's changes no longer match it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        review_digest: Option<String>,
    }
}

wire_struct! {
    ChangesUndoParams {
        /// The computer a keep changed.
        computer_id: String,
    }
}

wire_struct! {
    ChangesPromoteParams {
        /// The fork that replaces main.
        fork_id: String,
        /// The computer it replaces; defaults to the one the fork was made from.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_id: Option<String>,
        /// Check and report what a promote would do, changing nothing.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dry_run: Option<bool>,
    }
}

wire_struct! {
    PromoteReport {
        fork: String,
        target: String,
        /// The name the old main has afterwards.
        archive: String,
        steps: Vec<String>,
        notes: Vec<String>,
        /// The before-promote checkpoint of the old main.
        checkpoint: Option<String>,
    }
}

wire_struct! {
    PromoteRefusal {
        /// `busy`, `os` or `unsupported`.
        reason: String,
        message: String,
    }
}

wire_struct! {
    /// The final result of `changes.promote`. `status` is `promoted`, `dry-run` or `refused`; a
    /// refused promote changed nothing.
    ChangesPromoteResult {
        status: String,
        report: PromoteReport,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        refusal: Option<PromoteRefusal>,
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
        /// The digest of the fork's changes the keep acted on.
        #[serde(default)]
        review_digest: Option<String>,
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
        /// What changed on the computer after the keep; the undo discards it.
        #[serde(default)]
        changed_since: Vec<String>,
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
            (
                params.target_id,
                params.keep,
                params.dry_run,
                params.data_ok,
                params.review_digest
            ),
            (None, None, None, None, None)
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
