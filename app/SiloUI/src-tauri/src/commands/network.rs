use crate::{network, runtime};
use tauri::AppHandle;

#[tauri::command]
pub(crate) async fn read_network_state(app: AppHandle) -> Result<network::State, String> {
    tokio::task::spawn_blocking(move || {
        let paths = runtime::runtime_paths(&app)
            .map_err(|_| "Could not read network services. Try again.")?;
        network::read_network_state(&paths)
    })
    .await
    .map_err(|_| "Could not read network services. Try again.".to_owned())?
}

#[tauri::command]
pub(crate) async fn save_network_port(
    app: AppHandle,
    computer: String,
    port: u16,
    host_port: Option<u16>,
    scheme: Option<String>,
) -> Result<network::State, String> {
    tokio::task::spawn_blocking(move || {
        let paths = runtime::runtime_paths(&app)
            .map_err(|_| "Could not read network services. Try again.")?;
        network::save_network_port(&paths, computer, port, host_port, scheme)
    })
    .await
    .map_err(|_| "Could not read network services. Try again.".to_owned())?
}

#[tauri::command]
pub(crate) async fn remove_network_port(
    app: AppHandle,
    computer: String,
    port: u16,
) -> Result<network::State, String> {
    tokio::task::spawn_blocking(move || {
        let paths = runtime::runtime_paths(&app)
            .map_err(|_| "Could not read network services. Try again.")?;
        network::remove_network_port(&paths, computer, port)
    })
    .await
    .map_err(|_| "Could not read network services. Try again.".to_owned())?
}

#[tauri::command]
pub(crate) async fn open_network_port(
    app: AppHandle,
    computer: String,
    port: u16,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let paths = runtime::runtime_paths(&app)
            .map_err(|_| "Could not read network services. Try again.")?;
        let url = network::network_port_url(&paths, computer, port)?;
        crate::applications::open_browser(&app, &url)
    })
    .await
    .map_err(|_| "Could not read network services. Try again.".to_owned())?
}
