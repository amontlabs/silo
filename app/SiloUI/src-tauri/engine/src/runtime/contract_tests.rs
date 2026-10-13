use std::path::Path;

use serde::Serialize;
use serde_json::Value;

pub(crate) fn assert_fixture(name: &str, actual: impl Serialize) {
    let actual = serde_json::to_value(actual).unwrap();
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../src/test/contracts")
        .join(name);
    if std::env::var("SILO_UPDATE_CONTRACT_FIXTURES").as_deref() == Ok("1") {
        std::fs::write(
            &path,
            format!("{}\n", serde_json::to_string_pretty(&actual).unwrap()),
        )
        .unwrap();
    }
    let expected: Value = serde_json::from_slice(&std::fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "Cannot read {}: {error}. See src/test/contracts/README.md.",
            path.display()
        )
    }))
    .unwrap();
    assert_eq!(
        actual, expected,
        "Native wire contract changed: {name}. Review before regenerating."
    );
}
