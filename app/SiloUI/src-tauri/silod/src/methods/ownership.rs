use silo_api::methods::MethodName;

use crate::dispatcher::method_name;

pub(super) fn owns_prefix(method: MethodName, prefixes: &[&str]) -> bool {
    let name = method_name(method);
    prefixes.iter().any(|prefix| name.starts_with(prefix))
}
