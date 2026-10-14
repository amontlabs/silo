//! The pure conversions on small documents.
use super::convert::*;
use serde_json::{json, Value};

fn converted(convert: fn(&mut Value) -> Converted, mut value: Value) -> (Value, bool) {
    let changed = convert(&mut value).unwrap();
    (value, changed)
}

#[test]
fn a_converted_document_converts_to_itself() {
    let (once, changed) = converted(
        settings,
        json!({"settings": {"sandboxOrder": ["a"]}, "onboardingDraft": null}),
    );
    assert!(changed);
    let (twice, changed) = converted(settings, once.clone());
    assert!(!changed);
    assert_eq!(once, twice);
}

#[test]
fn an_old_key_replaces_a_new_one_present_beside_it() {
    let (value, _) = converted(
        update_resume,
        json!({"machines": [{"id": "1"}], "computers": [{"id": "stale"}]}),
    );
    assert_eq!(value, json!({"computers": [{"id": "1"}]}));
}

#[test]
fn unexpected_shapes_are_reported_not_converted() {
    assert_eq!(github(&mut json!([1])), Err(Unexpected));
    assert_eq!(github(&mut json!({"workspaces": "x"})), Err(Unexpected));
    assert_eq!(secrets(&mut json!({"secrets": [1]})), Err(Unexpected));
    assert_eq!(
        computers_metadata(&mut json!({"machines": {}})),
        Err(Unexpected)
    );
    assert_eq!(network(&mut json!({"mappings": [[]]})), Err(Unexpected));
}

#[test]
fn a_configuration_of_another_kind_keeps_its_kind() {
    let (value, changed) = converted(
        computers_metadata,
        json!({"machines": [{"kind": "other", "id": "1"}]}),
    );
    assert!(changed);
    assert_eq!(value, json!({"computers": [{"kind": "other", "id": "1"}]}));
}

#[test]
fn a_draft_without_the_optional_parts_converts() {
    let (value, _) = converted(
        settings,
        json!({"onboardingDraft": {"currentStep": "github", "machines": [],
               "unfinishedMachineEditor": null, "workspaceSelections": {},
               "workspaceIdentities": {}}}),
    );
    assert_eq!(
        value,
        json!({"onboardingDraft": {"currentStep": "github", "computers": [],
               "unfinishedComputerEditor": null, "computerSelections": {},
               "computerIdentities": {}}})
    );
}

#[test]
fn only_the_setup_steps_a_build_wrote_are_renamed() {
    let event = |step: &str| json!({"phase": "workspaces", "step": step, "workspace": "dev"});
    let (value, changed) = converted(
        setup_activity,
        json!([
            event("workspace-image-wait"),
            event("workspace-unknown"),
            event("image-ready")
        ]),
    );
    assert!(changed);
    let steps: Vec<_> = value
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["step"].as_str().unwrap())
        .collect();
    assert_eq!(
        steps,
        ["computer-image-wait", "workspace-unknown", "image-ready"]
    );
}
