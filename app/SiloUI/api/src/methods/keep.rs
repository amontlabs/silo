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

/// How an item both sides changed is settled.
#[derive(
    Clone, Copy, Debug, serde::Deserialize, PartialEq, Eq, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum KeepAnswer {
    /// The fork's version is written over main's.
    Fork,
    /// The item stays as main has it.
    Main,
    /// A repository only: the fork's commits arrive as a branch named `silo/<fork name>` and
    /// nothing is merged or checked out.
    Branch,
}

wire_struct! {
    ChangesKeepParams {
        /// The device that owns this; absent or `local` is this device.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
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
        /// The answer for each item both sides changed, by item id (the `id` of a conflict, or
        /// of an entry in `changes.diff`). A repository takes `branch` or `main`; every other
        /// item takes `fork` or `main`. An id that is not in the reviewed diff, or an answer the
        /// item does not allow, fails the keep and names the item. Databases take no answer.
        /// Choices are checked against the reviewed diff and are not part of the review digest.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        choices: Option<std::collections::BTreeMap<String, KeepAnswer>>,
        /// Item ids not to bring over even though they do not conflict. The same as answering
        /// `main`; the report calls them `skipped`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        skip: Option<Vec<String>>,
    }
}

wire_struct! {
    ChangesUndoParams {
        /// The device that owns this; absent or `local` is this device.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        /// The computer a keep changed.
        computer_id: String,
    }
}

wire_struct! {
    ChangesPromoteParams {
        /// The device that owns this; absent or `local` is this device.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
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
        /// `file`, `package`, `crontab`, `git`, `credentials` or `destination`.
        kind: String,
        item: String,
        detail: String,
        /// The id of the item, the key of its answer in `choices`; empty when no answer
        /// settles the conflict (credentials).
        #[serde(default)]
        id: String,
        /// The answers that settle it; empty when none does.
        #[serde(default)]
        allowed: Vec<KeepAnswer>,
        /// What changed in the fork, in a few words.
        #[serde(default)]
        fork_change: String,
        /// What changed on main, in a few words.
        #[serde(default)]
        main_change: String,
    }
}

wire_struct! {
    KeepRefusal {
        /// `choices-needed` (both sides changed items that have no answer yet: `conflicts` lists
        /// them with the answers each allows), `database`, `credentials`, `changed` or
        /// `main-changed`. Older engines also said `divergent` and `git`.
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
        /// What was done with each item, by id. Empty when the keep was refused.
        #[serde(default)]
        items: Vec<KeepItem>,
        /// Whether main was unchanged since the fork was made, so nothing could conflict.
        /// Absent when the keep stopped before main was looked at.
        #[serde(default)]
        fast_forward: Option<bool>,
    }
}

wire_struct! {
    /// What happened to one item.
    KeepItem {
        /// The item id.
        id: String,
        /// `file`, `package`, `repository`, `crontab`, `launch agent` or `database`.
        kind: String,
        label: String,
        /// `brought-over`, `left-as-main` (answered `main`), `branch` (a repository's commits
        /// left as a branch), `skipped` (listed in `skip`), `already-there` (main already holds
        /// it) or `not-combined` (a database).
        outcome: String,
        /// The same in words.
        detail: String,
    }
}

wire_struct! {
    /// The final result of `changes.keep`. `status` is `kept`, `dry-run` or `refused`; a
    /// refused keep did not change the target. A keep that needs answers is `refused` with the
    /// reason `choices-needed`.
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
        /// `running` or `stopped`: main as the undo left it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        main: Option<String>,
        /// `memory` when main resumed from the checkpoint's memory, else `stopped`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resumed_from: Option<String>,
        /// What the undo could not do, such as starting main.
        #[serde(default)]
        notes: Vec<String>,
    }
}

wire_struct! {
    /// Ask for a fork to be reviewed without keeping it: `changes.request` stores a
    /// [`FoldRequest`] and changes nothing else.
    ChangesRequestParams {
        /// The device that owns this; absent or `local` is this device.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        /// The fork to be reviewed.
        fork_id: String,
        /// The computer it would be kept into; defaults to the one the fork was made from.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_id: Option<String>,
        /// The `reviewDigest` of the `changes.diff` the request was made from.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        review_digest: Option<String>,
        /// A short text for the reviewer, usually the readable diff.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
        /// The bucket counts of the diff the request was made from.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        counts: Option<std::collections::BTreeMap<String, u64>>,
    }
}

#[derive(
    Clone, Copy, Debug, serde::Deserialize, PartialEq, Eq, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum FoldRequestStatus {
    /// Waiting for a review.
    Open,
    /// A keep of it started and has not been recorded as finished.
    Keeping,
    /// Kept into its target.
    Kept,
    /// The fork was dropped without keeping it.
    ThrownAway,
}

/// Why a request was closed without being answered.
#[derive(
    Clone, Copy, Debug, serde::Deserialize, PartialEq, Eq, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum FoldRequestClosedReason {
    /// The fork was deleted.
    ForkDeleted,
    /// The computer the fork would be kept into was deleted.
    TargetDeleted,
}

wire_struct! {
    /// A recorded request to review a fork. The engine keeps it until it is answered.
    FoldRequest {
        id: String,
        fork_id: String,
        /// The computer the fork would be kept into.
        target: String,
        created_at: String,
        /// The digest of the changes the requester reviewed; empty when none was given.
        review_digest: String,
        summary: String,
        status: FoldRequestStatus,
        /// How many changes each bucket (`code`, `packages`, `settings`, `database`, `other`)
        /// held when the request was made; empty when it was not given.
        #[serde(default)]
        counts: std::collections::BTreeMap<String, u64>,
        /// The checkpoint of the target taken before the keep, once the request is `kept`;
        /// `changes.undo` goes back to it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keep_id: Option<String>,
        /// When the request was kept or thrown away.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resolved_at: Option<String>,
        /// When a keep of the request started, in milliseconds since the epoch; set while the
        /// status is `keeping`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keeping_since_ms: Option<u64>,
        /// Why a `thrown-away` request was closed when no one answered it: the fork or the
        /// target computer was deleted.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        closed_reason: Option<FoldRequestClosedReason>,
        /// The approval that asks the owner to merge this request, when a computer's agent
        /// asked for the merge.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        approval_id: Option<String>,
    }
}

wire_struct! {
    ChangesRequestResult {
        request: FoldRequest,
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
                params.review_digest,
                params.choices,
                params.skip
            ),
            (None, None, None, None, None, None, None)
        );
        let hooks: KeepHooks = serde_json::from_str(r#"{"restart":["web"]}"#).unwrap();
        assert!(hooks.migrate.is_empty() && hooks.after.is_empty());
    }

    #[test]
    fn a_fold_request_params_need_only_the_fork() {
        let params: ChangesRequestParams = serde_json::from_str(r#"{"forkId":"f"}"#).unwrap();
        assert_eq!(
            (params.target_id, params.review_digest, params.summary),
            (None, None, None)
        );
        let record: FoldRequest = serde_json::from_value(serde_json::json!({
            "id": "fr-1", "forkId": "f", "target": "m", "createdAt": "t",
            "reviewDigest": "d", "summary": "s", "status": "open"
        }))
        .unwrap();
        assert_eq!(record.status, FoldRequestStatus::Open);
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

    #[test]
    fn choices_and_a_choices_needed_refusal_round_trip() {
        let params: ChangesKeepParams = serde_json::from_str(
            r#"{"forkId":"f","choices":{"repo:/root/app":"branch","file:/etc/x":"fork"},"skip":["package:dpkg:jq"]}"#,
        )
        .unwrap();
        let choices = params.choices.unwrap();
        assert_eq!(choices["repo:/root/app"], KeepAnswer::Branch);
        assert_eq!(params.skip.unwrap(), ["package:dpkg:jq"]);
        let value = serde_json::json!({
            "status": "refused",
            "report": {"fork": "f", "target": "m", "summary": [], "steps": [], "notes": [], "checkpoint": null, "fastForward": false},
            "refusal": {"reason": "choices-needed", "message": "m", "hint": "h", "conflicts": [
                {"kind": "git", "item": "/root/app", "detail": "d", "id": "repo:/root/app",
                 "allowed": ["branch", "main"], "forkChange": "new commits", "mainChange": "its own changes"}
            ]}
        });
        let result: ChangesKeepResult = serde_json::from_value(value).unwrap();
        let refusal = result.refusal.unwrap();
        assert_eq!(
            refusal.conflicts[0].allowed,
            [KeepAnswer::Branch, KeepAnswer::Main]
        );
        assert_eq!(result.report.fast_forward, Some(false));
        let old: KeepConflict =
            serde_json::from_str(r#"{"kind":"file","item":"/a","detail":"d"}"#).unwrap();
        assert!(old.id.is_empty() && old.allowed.is_empty());
    }
}
