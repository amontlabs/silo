//! `device.*`: facts about this Mac and the dependency report.

use std::collections::BTreeSet;

use serde_json::{json, Value};
use silo_api::methods::{
    computers::{ComputersListResult, LinuxComputerState, MacosComputerState},
    device::{
        ComputerResources, DependencyReportParams, DeviceFactsParams, DeviceResources,
        DeviceResourcesParams,
    },
    MethodName,
};
use silo_engine::{
    dependencies::{self, ProbePaths},
    device_facts, resources,
};

use crate::dispatcher::{DispatchError, HandlerOutput, RequestContext};

use super::{blocking, message_error, output, require_local_device, HandlerContext};

pub(super) async fn facts(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: DeviceFactsParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    // The Remote Login probe is a short network connect, so it runs off the async workers.
    let facts = blocking(|| Ok(device_facts::read())).await?;
    output(facts)
}

/// Every computer on the device with what it is configured to use.
fn resources_of(listed: &ComputersListResult) -> Vec<ComputerResources> {
    listed
        .linux
        .computers
        .iter()
        .map(|computer| ComputerResources {
            id: computer.configuration.id.clone(),
            name: computer.configuration.name.clone(),
            running: matches!(
                computer.state,
                LinuxComputerState::Running | LinuxComputerState::Starting
            ),
            cpus: u64::from(computer.configuration.cpus),
            memory_gib: u64::from(computer.configuration.memory_gib),
            disk_gib: Some(
                u64::from(computer.configuration.workspace_storage_gib)
                    .saturating_add(u64::from(computer.configuration.runtime_storage_gib)),
            ),
            workspace_storage_gib: Some(u64::from(computer.configuration.workspace_storage_gib)),
            runtime_storage_gib: Some(u64::from(computer.configuration.runtime_storage_gib)),
        })
        .chain(
            listed
                .macos
                .computers
                .iter()
                .map(|computer| ComputerResources {
                    id: computer.id.clone(),
                    name: computer.name.clone(),
                    running: matches!(
                        computer.state,
                        MacosComputerState::Running | MacosComputerState::Starting
                    ),
                    cpus: computer.cpus,
                    memory_gib: computer.memory_gib,
                    disk_gib: Some(computer.disk_gib),
                    workspace_storage_gib: None,
                    runtime_storage_gib: None,
                }),
        )
        .collect()
}

/// The computers a caller may see: all of them for the owner, otherwise those in `visible`.
fn shown_to(
    all: &[ComputerResources],
    visible: Option<&BTreeSet<String>>,
) -> Vec<ComputerResources> {
    all.iter()
        .filter(|computer| visible.is_none_or(|visible| visible.contains(&computer.id)))
        .cloned()
        .collect()
}

pub(super) async fn resources(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: DeviceResourcesParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    let (host, floors) = super::super::gate::host_and_floors(context)?;
    let internal = HandlerContext::for_owner_bridge(
        context.engine.clone(),
        RequestContext::owner("resources"),
    );
    // The measurements do not depend on the computers; when the list cannot be read (the
    // runtime is missing, say) they are still answered, with no computers named.
    // `route` reaches this handler, so the nested call is boxed.
    let all = match Box::pin(super::super::route(
        &internal,
        MethodName::ComputersList,
        &json!({}),
    ))
    .await
    {
        Ok(listed) => serde_json::from_value::<ComputersListResult>(listed.value)
            .map(|listed| resources_of(&listed))
            .map_err(|error| DispatchError::internal(error.to_string()))?,
        Err(_) => Vec::new(),
    };
    let shown = shown_to(&all, context.visible());
    let running = all.iter().filter(|computer| computer.running);
    let assessment = resources::assess(&host, &floors);
    output(DeviceResources {
        logical_cpus: host.logical_cpus,
        memory_total_bytes: host.memory_total,
        memory_available_bytes: host.memory_available,
        memory_pressure: host.pressure,
        disk_free_bytes: host.disk_free,
        disk_total_bytes: host.disk_total,
        running_computers: running.clone().count() as u64,
        running_memory_gib: running.clone().fold(0_u64, |sum, computer| {
            sum.saturating_add(computer.memory_gib)
        }),
        running_cpus: running.fold(0_u64, |sum, computer| sum.saturating_add(computer.cpus)),
        memory_floor_bytes: floors.memory,
        disk_floor_bytes: floors.disk,
        state: assessment.state,
        message: assessment.message,
        computers: shown,
    })
}

pub(super) async fn dependencies(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: DependencyReportParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    dependencies::validate_request_id(&params.request_id).map_err(message_error)?;
    let runtime_host = context.silod_context().runtime_host();
    let executable_dir = runtime_host
        .bundled_tools_directory()
        .map_err(message_error)?;
    let resource_dir = context.engine_context().paths().resources.clone();
    let frameworks_dir = executable_dir
        .parent()
        .map(|contents| contents.join("Frameworks"));
    let paths = ProbePaths::new(executable_dir, resource_dir, frameworks_dir);
    // The runtime library on Linux lives under the resources, where the runtime loads it.
    #[cfg(target_os = "linux")]
    let paths = {
        let library = silo_engine::runtime::bundled_runtime_library(
            &paths.executable_dir.join("msb"),
            &paths.resource_dir,
            runtime_host.runtime_package_kind(),
        );
        paths.with_runtime_library(library)
    };
    // Three probe groups run one after another, each under its own budget, so a report can
    // take close to a minute.
    let report = blocking(move || Ok(dependencies::read_report(params.request_id, paths))).await?;
    let report: silo_api::methods::device::DependencyReport = serde_json::to_value(report)
        .and_then(serde_json::from_value)
        .map_err(|error| DispatchError::internal(error.to_string()))?;
    output(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn computer(id: &str, running: bool, memory_gib: u64) -> ComputerResources {
        ComputerResources {
            id: id.into(),
            name: format!("name-{id}"),
            running,
            cpus: 2,
            memory_gib,
            disk_gib: Some(20),
            workspace_storage_gib: Some(10),
            runtime_storage_gib: Some(10),
        }
    }

    fn listed() -> ComputersListResult {
        let linux = json!({
            "kind": "linux",
            "configuration": {"id": "lin", "name": "box", "cpus": 2, "maxCPUs": 2, "memoryGiB": 4,
                "maxMemoryGiB": 4, "workspaceStorageGiB": 30, "runtimeStorageGiB": 12},
            "purpose": "", "state": "running", "stateDetail": "", "canDismissError": false,
            "freshness": "fresh", "repositories": [], "files": [], "ports": [], "logs": [],
            "githubRepositories": [], "secretNames": []
        });
        let macos = json!({
            "kind": "macos", "id": "mac", "name": "mac", "cpus": 4, "memoryGiB": 8, "diskGiB": 64,
            "osVersion": null, "state": "stopped", "progress": null, "detail": null, "note": null,
            "displayOpen": false, "installed": true, "setupComplete": true,
            "needsPersonalizing": false, "checkpoints": [], "checkpointOperation": null,
            "pendingRestore": null
        });
        serde_json::from_value(json!({
            "linux": {"runtimeRepair": null, "computers": [linux], "activities": [],
                "computerConfigurationOperation": null, "repositoryPushOperations": [],
                "github": null, "secrets": []},
            "macos": {"supported": true, "unsupportedReason": null, "computers": [macos],
                "template": null, "minDiskGiB": 64}
        }))
        .unwrap()
    }

    #[test]
    fn a_linux_computer_shows_its_workspace_and_runtime_storage_and_a_macos_computer_its_disk() {
        let rows = resources_of(&listed());
        let linux = rows.iter().find(|row| row.id == "lin").unwrap();
        assert_eq!(linux.disk_gib, Some(42));
        assert_eq!(linux.workspace_storage_gib, Some(30));
        assert_eq!(linux.runtime_storage_gib, Some(12));
        let mac = rows.iter().find(|row| row.id == "mac").unwrap();
        assert_eq!(mac.disk_gib, Some(64));
        assert_eq!(mac.workspace_storage_gib, None);
        assert_eq!(mac.runtime_storage_gib, None);
    }

    #[test]
    fn a_computer_sees_the_disk_size_of_only_the_computers_it_may_see() {
        let all = resources_of(&listed());
        let visible: BTreeSet<String> = ["mac"].map(String::from).into();
        let shown = shown_to(&all, Some(&visible));
        assert_eq!(shown.len(), 1);
        assert_eq!((shown[0].id.as_str(), shown[0].disk_gib), ("mac", Some(64)));
    }

    #[test]
    fn a_computers_list_holds_only_itself_and_the_computers_it_made() {
        let all = [
            computer("main", true, 8),
            computer("fork", false, 4),
            computer("stranger", true, 16),
        ];
        let visible: BTreeSet<String> = ["main", "fork"].map(String::from).into();
        let ids = |list: Vec<ComputerResources>| -> Vec<String> {
            list.into_iter().map(|computer| computer.id).collect()
        };
        assert_eq!(ids(shown_to(&all, Some(&visible))), ["main", "fork"]);
        assert_eq!(ids(shown_to(&all, None)), ["main", "fork", "stranger"]);
        assert!(shown_to(&all, Some(&BTreeSet::new())).is_empty());
    }
}
