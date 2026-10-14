mod app_menu;
mod commands;
pub(crate) use silo_engine::*;
#[cfg(test)]
mod command_permissions_tests;
#[cfg(debug_assertions)]
mod desktop_viewer_bench;
mod engine_context;
#[cfg(test)]
mod github_build_tests;
#[cfg(test)]
mod github_permissions_tests;
pub(crate) use commands::macos_computers;
pub(crate) use commands::macos_remote;
pub(crate) use commands::macos_remote_viewer;
mod single_instance;
mod status_panel;
#[cfg(target_os = "macos")]
mod titlebar;
mod tray;
mod viewer_shortcuts;
#[cfg(target_os = "macos")]
mod window_material;

use tauri::{Emitter, Manager, WindowEvent};

fn main() {
    #[cfg(target_os = "linux")]
    if std::env::current_exe().is_ok_and(|path| path == std::path::Path::new("/usr/bin/silo-ui"))
        && std::path::Path::new(system_integrations::PACKAGE_UPDATE_MARKER).exists()
    {
        system_integrations::explain_unfinished_package_update();
        return;
    }

    // The bundle identifier embedded at build time decides the channel, including
    // for the bridge modes below that run without a window or Tauri runtime.
    let context = tauri::generate_context!();
    channel::init(&context.config().identifier);
    let args: Vec<_> = std::env::args().collect();
    let bridge = match args.get(1).map(String::as_str) {
        Some("--remote-bridge") => Some(remote::run_bridge()),
        Some("--remote-guest") => Some(match remote::guest_stream_params(&args[2..]) {
            Ok(params) => remote::run_remote_stream(&args[2], "guest.ssh", params),
            Err(error) => Err(error),
        }),
        Some(editor::TRANSPORT_MODE) => Some(commands::editor::run_transport(&args[2..])),
        _ => None,
    };
    if let Some(result) = bridge {
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    // Plugins initialize while the app is built, in registration order, and the
    // setup hook runs only after that. A second launch therefore exits inside
    // the single-instance plugin before any migration, Connections or computer work.
    let app = tauri::Builder::default()
        .plugin(single_instance::plugin())
        .on_page_load(|webview, _| {
            #[cfg(target_os = "macos")]
            if webview.label() == "main" {
                if let Some(window) = webview.get_webview_window("main") {
                    window_material::sync_accessibility(&window);
                }
            }
            #[cfg(not(target_os = "macos"))]
            let _ = webview;
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            commands::desktop::read_desktop_state,
            commands::desktop::desktop_action,
            commands::desktop::set_computer_use_approval,
            commands::chatgpt_app::chatgpt_app_status,
            commands::chatgpt_app::chatgpt_app_retry,
            commands::preparation::read_preparation_status,
            commands::preparation::retry_preparation,
            commands::desktop_viewer::open_desktop,
            commands::desktop_viewer::desktop_viewer_attach,
            commands::desktop_viewer::desktop_viewer_detach,
            commands::desktop_viewer_media::desktop_viewer_sound_support,
            commands::desktop_viewer_media::desktop_viewer_sound_cancel,
            commands::desktop_viewer_media::desktop_viewer_set_audio,
            commands::desktop_viewer_media::desktop_viewer_reset_screen,
            commands::viewer_clipboard::desktop_viewer_clipboard,
            commands::macos_computers::read_macos_computers,
            commands::macos_computers::create_macos_computer,
            commands::macos_computers::macos_computer_action,
            commands::macos_computers::open_macos_display,
            commands::macos_computers::macos_computer_clipboard,
            commands::macos_computers::delete_macos_template,
            commands::macos_computers::create_macos_checkpoint,
            commands::macos_computers::restore_macos_checkpoint,
            commands::macos_computers::fork_macos_checkpoint,
            commands::macos_computers::delete_macos_checkpoint,
            commands::macos_remote::remote_macos_snapshot,
            commands::macos_remote::remote_macos_create,
            commands::macos_remote::remote_macos_action,
            commands::macos_remote_viewer::open_macos_remote_display,
            commands::macos_remote_viewer::macos_display_session,
            commands::macos_remote_viewer::macos_display_resize,
            app_menu::set_app_menu_state,
            app_menu::show_app_menu,
            commands::updates::get_update_state,
            commands::updates::check_for_update,
            commands::updates::download_update,
            commands::updates::install_update,
            commands::updates::set_update_automatic_checks,
            commands::updates::open_update_release,
            status_panel::open_main,
            status_panel::take_main_route,
            status_panel::hide_status,
            status_panel::resize_status,
            status_panel::quit_app,
            commands::settings::enable_quit_confirmation,
            commands::settings::answer_quit_request,
            tray::update_tray,
            commands::host_push::start_repository_push,
            commands::host_push::repository_push_status,
            commands::host_push::dismiss_repository_push,
            commands::files::list_computer_directory,
            commands::transfer::choose_upload_files,
            commands::transfer::upload_files,
            commands::transfer::download_file,
            commands::transfer::cancel_transfer,
            commands::network::read_network_state,
            commands::network::save_network_port,
            commands::network::remove_network_port,
            commands::network::open_network_port,
            commands::ssh_access::read_ssh_access_state,
            commands::ssh_access::save_ssh_access,
            commands::ssh_access::ssh_connection,
            commands::secrets::read_secrets_state,
            commands::secrets::save_secret,
            commands::secrets::remove_secret,
            commands::secrets::retry_secret,
            commands::github::read_github_state,
            commands::github::save_github_personal_token,
            commands::github::remove_github_personal_token,
            commands::github::connect_github,
            commands::github::cancel_github_connection,
            commands::github::reopen_github_authorization,
            commands::github::manage_github_repositories,
            commands::github::disconnect_github,
            commands::github::set_github_access_enabled,
            commands::github::save_github_configuration,
            commands::github::retry_github_configuration,
            commands::github::refresh_github_repositories,
            commands::settings::initialize_settings,
            commands::settings::read_settings,
            commands::settings::update_settings,
            commands::settings::update_onboarding_draft,
            commands::settings::import_legacy_theme,
            commands::settings::flush_settings,
            commands::settings::begin_settings_flush,
            commands::settings::complete_settings_flush,
            commands::settings::cancel_settings_flush,
            commands::settings::read_shutdown_state,
            commands::remote::connections_status,
            commands::remote::authorize_device,
            commands::remote::setup_device_key,
            commands::remote::remote_network_state,
            commands::remote::remote_ssh_access_state,
            commands::remote::remote_save_ssh_access,
            commands::remote::remote_save_network_port,
            commands::remote::remote_remove_network_port,
            commands::remote::remote_open_network_port,
            commands::remote::set_connections_enabled,
            commands::remote::device_list,
            commands::remote::connect_device,
            commands::remote::remove_device,
            commands::remote::device_snapshot,
            commands::remote::remote_computer_action,
            commands::remote::remote_checkpoint_action,
            commands::remote::remote_upsert_computer,
            commands::remote::remote_delete_computer,
            commands::system_integrations::read_system_integrations,
            commands::system_integrations::set_login_item,
            commands::system_integrations::request_notification_authorization,
            commands::notifications::deliver_notice,
            commands::notifications::clear_computer_notices,
            commands::system_integrations::open_integration_settings,
            commands::system_integrations::show_integration_error,
            commands::editor::read_editor_include_notice,
            commands::applications::list_applications,
            commands::applications::choose_application,
            commands::dependencies::read_dependencies,
            commands::backup_controller::read_backup_state,
            commands::runtime_migration::read_runtime_migration_state,
            commands::runtime_migration::retry_runtime_migration,
            commands::runtime_migration::continue_after_migration_failure,
            commands::pre_upgrade_backup::read_pre_upgrade_backup,
            commands::pre_upgrade_backup::measure_pre_upgrade_backup,
            commands::pre_upgrade_backup::delete_pre_upgrade_backup,
            commands::pre_upgrade_backup::reveal_pre_upgrade_backup,
            commands::pre_upgrade_backup::acknowledge_pre_upgrade_backup_notice,
            commands::backup_controller::choose_backup_destination,
            commands::backup_controller::choose_backup_archive,
            commands::backup_controller::inspect_backup_archive,
            commands::backup_controller::cancel_backup_inspection,
            commands::backup_controller::reveal_backup_archive,
            commands::backup_controller::start_backup,
            commands::backup_controller::start_restore,
            commands::backup_controller::cancel_backup_operation,
            commands::backup_controller::dismiss_backup_operation,
            commands::backup_controller::acknowledge_backup_result,
            commands::runtime::read_application_state,
            commands::runtime_storage::read_workspace_storage,
            commands::runtime_storage::reclaim_workspace_storage,
            commands::runtime_logs::query_computer_logs,
            commands::log_export::export_computer_logs,
            commands::log_export::cancel_log_export,
            commands::runtime::read_application_shell,
            commands::runtime::read_computer_configuration,
            commands::runtime::configure_computer_identities,
            commands::runtime::verify_computer_identities,
            commands::runtime::computer_action,
            commands::runtime_checkpoints::create_checkpoint,
            commands::runtime_checkpoints::fork_checkpoint,
            commands::runtime_checkpoints::restore_checkpoint,
            commands::runtime_checkpoints::delete_checkpoint,
            commands::runtime_checkpoints::abandon_restore,
            commands::runtime_checkpoints::read_checkpoint_usage,
            commands::runtime::read_setup_activity,
            commands::runtime::read_operation_queue,
            commands::runtime::cancel_operation,
            commands::runtime::retry_computer_configuration,
            commands::runtime::skip_computer_use_wait,
            commands::runtime::change_computer_configuration
        ])
        .setup(|app| {
            // Tauri panics on a setup error. Explain the failure and exit instead.
            let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                engine_context::install(app)?;
                commands::creation_inputs::install(app.handle())?;
                runtime_migration::vocabulary::run()?;
                commands::system_shutdown::install(app.handle());
                let queue_app = app.handle().clone();
                runtime::OPERATIONS.set_listener(move || {
                    let _ = queue_app.emit("silo://operation-queue-changed", ());
                });
                commands::runtime_migration::install(app.handle())?;
                // Checkpoint records from before automatic checkpoints become manual ones,
                // once, before anything reads them (and before the macOS sweep).
                {
                    let migration = app.state::<std::sync::Arc<runtime_migration::Controller>>();
                    let context = silo_engine::context::current()
                        .ok_or("The engine context is not installed.")?;
                    silo_engine::checkpoints::upgrade_layouts(context.as_ref(), &migration)?;
                }
                commands::computer_use::install(app.handle());
                pre_upgrade_backup::install();
                commands::remote::start(app.handle())?;
                commands::secrets::install(app.handle())?;
                commands::github::install(app.handle())?;
                commands::backup_controller::install(app.handle())?;
                commands::runtime_migration::start_if_pending(app.handle())?;
                status_panel::install(app.handle())?;
                #[cfg(debug_assertions)]
                desktop_viewer_bench::start(app.handle());
                tray::install(app.handle())?;
                app_menu::install(app.handle())?;
                let window = app
                    .get_webview_window("main")
                    .expect("main window is configured");
                #[cfg(target_os = "linux")]
                window.set_decorations(false)?;
                let handle = app.handle().clone();
                window.on_window_event(move |event| {
                    if matches!(event, WindowEvent::Focused(true)) {
                        commands::updates::focused(&handle);
                    }
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        if tray::available(&handle) {
                            if let Some(window) = handle.get_webview_window("main") {
                                status_panel::report(window.hide());
                            }
                        } else {
                            commands::settings::request_quit(&handle);
                        }
                    }
                });
                #[cfg(target_os = "macos")]
                window_material::install(&window)?;
                window.show()?;
                #[cfg(target_os = "macos")]
                titlebar::install(&window)?;
                commands::notifications::install(app.handle());
                commands::updates::install(app.handle())?;
                commands::ssh_access::start_monitor(app.handle());
                commands::editor::refresh_transports(app.handle());
                commands::runtime_storage::start_monitor(app.handle());
                commands::startup::install(app.handle());
                Ok(())
            })();
            if let Err(error) = result {
                startup_failed(app.handle(), &error.to_string());
            }
            Ok(())
        })
        .build(context)
        .unwrap_or_else(|error| {
            eprintln!("Silo could not start: {error}");
            std::process::exit(1);
        });
    // tao installs its AppKit delegate while the event loop is created; add the
    // terminate handler before AppKit finishes launching.
    #[cfg(target_os = "macos")]
    commands::system_shutdown::install_terminate_handler(app.handle());
    app.run(|_app, _event| {
        if let tauri::RunEvent::Exit = &_event {
            commands::settings::exit_backstop(_app);
            ssh_access::close_all();
            remote_network::close_all();
            transfer::close_all(std::time::Duration::from_secs(5));
        }
        if let tauri::RunEvent::ExitRequested { api, code, .. } = &_event {
            commands::settings::prevent_exit_until_saved(_app, api, *code);
        }
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen {
            has_visible_windows: false,
            ..
        } = _event
        {
            status_panel::report(status_panel::open_main(_app.clone(), None));
        }
    });
}

/// Setup stopped part-way, so some native state the UI relies on is missing. Stop
/// the UI from using it, explain the failure, and exit without the Quit path: it
/// would stop computers that this process never managed.
fn startup_failed(app: &tauri::AppHandle, error: &str) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    let name = channel::current().product_name();
    eprintln!("{name} could not start: {error}");
    commands::settings::exit_without_shutdown(app);
    if let Ok(blank) = "about:blank".parse::<tauri::Url>() {
        for window in app.webview_windows().values() {
            let _ = window.navigate(blank.clone());
        }
    }
    app.dialog()
        .message(format!("{name} could not start.\n\n{error}"))
        .title(name)
        .kind(MessageDialogKind::Error)
        .show(|_| std::process::exit(1));
}
