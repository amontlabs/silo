//! Typed engine requests: the method and its parameters travel together, so a
//! caller cannot name a method or build parameters that the registry does not
//! declare.
//!
//! Constructors live in [`super::requests`], generated from the registry.

use std::marker::PhantomData;

use serde_json::Value;

use super::registry::METHOD_REGISTRY;
use super::{MethodKind, MethodName, MethodParams};

mod sealed {
    pub trait Sealed {}
}

/// Marker for the kind of a method, mirroring [`MethodKind`].
pub trait Kind: sealed::Sealed {
    const KIND: MethodKind;
}

/// Zero-sized kind markers carried by [`Request`].
pub mod kind {
    use super::{sealed::Sealed, Kind, MethodKind};

    macro_rules! kinds {
        ($($name:ident),*) => {$(
            #[derive(Clone, Copy, Debug)]
            pub enum $name {}
            impl Sealed for $name {}
            impl Kind for $name {
                const KIND: MethodKind = MethodKind::$name;
            }
        )*};
    }

    kinds!(Query, Command, Job, Stream, Subscription);
}

/// A method call: registry method, its typed parameters, and its kind.
#[derive(Clone, Debug)]
pub struct Request<K> {
    params: MethodParams,
    _kind: PhantomData<K>,
}

impl<K: Kind> Request<K> {
    pub(crate) fn new(params: MethodParams) -> Self {
        debug_assert_eq!(
            METHOD_REGISTRY
                .iter()
                .find(|entry| entry.method == params.method())
                .map(|entry| entry.kind),
            Some(K::KIND)
        );
        Self {
            params,
            _kind: PhantomData,
        }
    }

    pub fn method(&self) -> MethodName {
        self.params.method()
    }

    /// The wire name of the method.
    pub fn name(&self) -> &'static str {
        self.params.method().name()
    }

    pub fn kind(&self) -> MethodKind {
        K::KIND
    }

    pub fn params(&self) -> &MethodParams {
        &self.params
    }

    /// The parameters object exactly as the wire carries it.
    pub fn params_value(&self) -> Value {
        let mut tagged = serde_json::to_value(&self.params)
            .expect("registry parameters are plain serializable data");
        tagged
            .get_mut("params")
            .map(Value::take)
            .unwrap_or_else(|| Value::Object(Default::default()))
    }
}

/// Source of `requests.rs`: one typed constructor per registry entry.
pub fn requests_source() -> String {
    let mut modules = std::collections::BTreeSet::new();
    for entry in METHOD_REGISTRY {
        if let Some((module, _)) = entry.params_type.split_once("::") {
            if module != "transport" {
                modules.insert(module);
            }
        }
    }
    let mut out = String::from(
        "//! Typed request constructors, one per registry method.\n\
         //!\n\
         //! Generated from `METHOD_REGISTRY` by `requests_source`; do not edit.\n\
         //! Regenerate with `cargo run -p silo-api --example generate_requests`.\n\n",
    );
    out.push_str("use crate::transport;\n");
    for module in modules {
        out.push_str(&format!("use super::{module};\n"));
    }
    out.push_str("use super::request::{kind, Request};\nuse super::MethodParams;\n");
    for entry in METHOD_REGISTRY {
        let function = snake_case(entry.name);
        let kind = format!("{:?}", entry.kind);
        let variant = format!("{:?}", entry.method);
        let one_line = format!(
            "pub fn {function}(params: {}) -> Request<kind::{kind}> {{",
            entry.params_type
        );
        out.push_str(&format!("\n/// `{}`\n", entry.name));
        if one_line.len() <= 100 {
            out.push_str(&one_line);
            out.push('\n');
        } else {
            out.push_str(&format!(
                "pub fn {function}(\n    params: {},\n) -> Request<kind::{kind}> {{\n",
                entry.params_type
            ));
        }
        out.push_str(&format!(
            "    Request::new(MethodParams::{variant}(params))\n}}\n"
        ));
    }
    out
}

fn snake_case(name: &str) -> String {
    let mut out = String::new();
    for character in name.chars() {
        match character {
            '.' => out.push('_'),
            upper if upper.is_ascii_uppercase() => {
                out.push('_');
                out.push(upper.to_ascii_lowercase());
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::methods::common::{ComputerTargetParams, EmptyParams};
    use crate::methods::requests;

    #[test]
    fn committed_requests_match_the_registry() {
        assert_eq!(include_str!("requests.rs"), requests_source());
    }

    #[test]
    fn constructor_names_are_unique_valid_identifiers() {
        let source = requests_source();
        let mut seen = std::collections::HashSet::new();
        for entry in METHOD_REGISTRY {
            let function = snake_case(entry.name);
            assert!(
                function
                    .chars()
                    .all(|c| (c.is_ascii_alphanumeric() && !c.is_ascii_uppercase()) || c == '_'),
                "{function}"
            );
            assert!(seen.insert(function.clone()), "duplicate {function}");
            assert_eq!(source.matches(&format!("pub fn {function}(")).count(), 1);
        }
        assert_eq!(seen.len(), METHOD_REGISTRY.len());
    }

    #[test]
    fn requests_carry_name_kind_and_wire_params() {
        let start = requests::computers_start(ComputerTargetParams {
            device_id: None,
            computer_id: "c1".into(),
        });
        assert_eq!(start.name(), "computers.start");
        assert_eq!(start.kind(), MethodKind::Job);
        assert_eq!(
            start.params_value(),
            serde_json::json!({"computerId": "c1"})
        );
        let list = requests::connections_devices_list(EmptyParams {});
        assert_eq!(list.name(), "connections.devices.list");
        assert_eq!(list.params_value(), serde_json::json!({}));
    }

    #[test]
    fn names_round_trip_through_the_registry() {
        for entry in METHOD_REGISTRY {
            assert_eq!(MethodName::from_name(entry.name), Some(entry.method));
            assert_eq!(entry.method.name(), entry.name);
        }
        assert_eq!(MethodName::from_name("nope"), None);
    }
}
