mod app_menu;
mod applications;
mod backup;
mod backup_controller;
mod bridge_error;
mod bundled_tools;
mod channel;
mod chatgpt_app;
mod clipboard;
#[cfg(test)]
mod command_permissions_tests;
mod computer_names;
mod computer_use;
mod creation_inputs;
mod dependencies;
mod desktop;
mod desktop_bridge;
mod desktop_proxy;
mod desktop_viewer;
#[cfg(debug_assertions)]
mod desktop_viewer_bench;
mod desktop_viewer_media;
mod device_identity;
mod editor;
mod files;
mod github;
#[cfg(test)]
mod github_build_tests;
mod github_http;
#[cfg(test)]
mod github_live_tests;
#[cfg(test)]
mod github_permissions_tests;
mod github_tokens;
mod health_watch;
mod host_push;
mod host_push_cache;
mod host_push_operations;
mod host_push_transport;
mod log_export;
mod log_retention;
mod macos_computers;
mod network;
mod notifications;
mod owned_tunnel;
mod pre_upgrade_backup;
mod preparation;
mod remote;
mod remote_access;
mod remote_network;
mod remote_ssh_access;
mod runtime;
mod runtime_migration;
mod secrets;
mod settings;
mod single_instance;
mod ssh_access;
mod ssh_connection;
mod startup;
mod status_panel;
mod sync;
mod system_integrations;
mod system_shutdown;
mod terminal;
#[cfg(test)]
mod test_support;
#[cfg(target_os = "macos")]
mod titlebar;
mod transfer;
mod tray;
mod updates;
mod viewer_clipboard;
mod viewer_shortcuts;
#[cfg(target_os = "macos")]
mod window_material;
mod working_account;

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
        Some(editor::TRANSPORT_MODE) => Some(editor::run_transport(&args[2..])),
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
            desktop::read_desktop_state,
            desktop::desktop_action,
            desktop::set_computer_use_approval,
            chatgpt_app::chatgpt_app_status,
            chatgpt_app::chatgpt_app_retry,
            preparation::read_preparation_status,
            preparation::retry_preparation,
            desktop_viewer::open_desktop,
            desktop_viewer::desktop_viewer_attach,
            desktop_viewer::desktop_viewer_detach,
            desktop_viewer_media::desktop_viewer_sound_support,
            desktop_viewer_media::desktop_viewer_sound_cancel,
            desktop_viewer_media::desktop_viewer_set_audio,
            desktop_viewer_media::desktop_viewer_reset_screen,
            desktop_viewer::desktop_viewer_clipboard,
            macos_computers::read_macos_computers,
            macos_computers::create_macos_computer,
            macos_computers::macos_computer_action,
            macos_computers::open_macos_display,
            macos_computers::macos_computer_clipboard,
            macos_computers::delete_macos_template,
            macos_computers::create_macos_checkpoint,
            macos_computers::restore_macos_checkpoint,
            macos_computers::fork_macos_checkpoint,
            macos_computers::delete_macos_checkpoint,
            app_menu::set_app_menu_state,
            app_menu::show_app_menu,
            updates::get_update_state,
            updates::check_for_update,
            updates::download_update,
            updates::install_update,
            updates::set_update_automatic_checks,
            updates::open_update_release,
            status_panel::open_main,
            status_panel::take_main_route,
            status_panel::hide_status,
            status_panel::resize_status,
            status_panel::quit_app,
            settings::enable_quit_confirmation,
            settings::answer_quit_request,
            tray::update_tray,
            host_push_operations::start_repository_push,
            host_push_operations::repository_push_status,
            host_push::dismiss_repository_push,
            files::list_computer_directory,
            transfer::choose_upload_files,
            transfer::upload_files,
            transfer::download_file,
            transfer::cancel_transfer,
            network::read_network_state,
            network::save_network_port,
            network::remove_network_port,
            network::open_network_port,
            ssh_access::read_ssh_access_state,
            ssh_access::save_ssh_access,
            ssh_connection::ssh_connection,
            secrets::read_secrets_state,
            secrets::save_secret,
            secrets::remove_secret,
            secrets::retry_secret,
            github::read_github_state,
            github::personal_token::save_github_personal_token,
            github::personal_token::remove_github_personal_token,
            github::connect_github,
            github::cancel_github_connection,
            github::reopen_github_authorization,
            github::manage_github_repositories,
            github::disconnect_github,
            github::set_github_access_enabled,
            github::save_github_configuration,
            github::retry_github_configuration,
            github::refresh_github_repositories,
            settings::initialize_settings,
            settings::read_settings,
            settings::update_settings,
            settings::update_onboarding_draft,
            settings::import_legacy_theme,
            settings::flush_settings,
            settings::begin_settings_flush,
            settings::complete_settings_flush,
            settings::cancel_settings_flush,
            settings::read_shutdown_state,
            remote::connections_status,
            remote::authorize_device,
            remote::setup_device_key,
            remote_network::remote_network_state,
            remote_ssh_access::remote_ssh_access_state,
            remote_ssh_access::remote_save_ssh_access,
            remote_network::remote_save_network_port,
            remote_network::remote_remove_network_port,
            remote_network::remote_open_network_port,
            remote::set_connections_enabled,
            remote::device_list,
            remote::connect_device,
            remote::remove_device,
            remote::device_snapshot,
            remote::remote_computer_action,
            remote::remote_checkpoint_action,
            remote::remote_upsert_computer,
            remote::remote_delete_computer,
            system_integrations::read_system_integrations,
            system_integrations::set_login_item,
            system_integrations::request_notification_authorization,
            notifications::deliver_notice,
            notifications::clear_computer_notices,
            system_integrations::open_integration_settings,
            system_integrations::show_integration_error,
            editor::read_editor_include_notice,
            applications::list_applications,
            applications::choose_application,
            dependencies::read_dependencies,
            backup_controller::read_backup_state,
            runtime_migration::read_runtime_migration_state,
            runtime_migration::retry_runtime_migration,
            runtime_migration::continue_after_migration_failure,
            pre_upgrade_backup::read_pre_upgrade_backup,
            pre_upgrade_backup::measure_pre_upgrade_backup,
            pre_upgrade_backup::delete_pre_upgrade_backup,
            pre_upgrade_backup::reveal_pre_upgrade_backup,
            pre_upgrade_backup::acknowledge_pre_upgrade_backup_notice,
            backup_controller::choose_backup_destination,
            backup_controller::choose_backup_archive,
            backup_controller::inspect_backup_archive,
            backup_controller::cancel_backup_inspection,
            backup_controller::reveal_backup_archive,
            backup_controller::start_backup,
            backup_controller::start_restore,
            backup_controller::cancel_backup_operation,
            backup_controller::dismiss_backup_operation,
            backup_controller::acknowledge_backup_result,
            runtime::read_application_state,
            runtime::storage::read_workspace_storage,
            runtime::storage::reclaim_workspace_storage,
            runtime::runtime_logs::query_computer_logs,
            log_export::export_computer_logs,
            log_export::cancel_log_export,
            runtime::read_application_shell,
            runtime::read_computer_configuration,
            runtime::configure_computer_identities,
            runtime::verify_computer_identities,
            runtime::computer_action,
            runtime::checkpoints::create_checkpoint,
            runtime::checkpoints::fork_checkpoint,
            runtime::checkpoints::restore_checkpoint,
            runtime::checkpoints::delete_checkpoint,
            runtime::checkpoints::abandon_restore,
            runtime::checkpoints::read_checkpoint_usage,
            runtime::read_setup_activity,
            runtime::read_operation_queue,
            runtime::cancel_operation,
            runtime::retry_computer_configuration,
            runtime::skip_computer_use_wait,
            runtime::change_computer_configuration
        ])
        .setup(|app| {
            // Tauri panics on a setup error. Explain the failure and exit instead.
            let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                runtime_migration::vocabulary::run(app.handle())?;
                settings::install(app.handle());
                system_shutdown::install(app.handle());
                let queue_app = app.handle().clone();
                runtime::OPERATIONS.set_listener(move || {
                    let _ = queue_app.emit("silo://operation-queue-changed", ());
                });
                runtime_migration::install(app.handle())?;
                computer_use::install(app.handle());
                pre_upgrade_backup::install(app.handle());
                remote::start(app.handle().clone());
                secrets::install(app.handle())?;
                github::install(app.handle());
                backup_controller::install(app.handle())?;
                runtime_migration::start_if_pending(app.handle())?;
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
                        updates::focused(&handle);
                    }
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        if tray::available(&handle) {
                            if let Some(window) = handle.get_webview_window("main") {
                                status_panel::report(window.hide());
                            }
                        } else {
                            settings::request_quit(&handle);
                        }
                    }
                });
                #[cfg(target_os = "macos")]
                window_material::install(&window)?;
                window.show()?;
                #[cfg(target_os = "macos")]
                titlebar::install(&window)?;
                notifications::install(app.handle());
                updates::install(app.handle())?;
                ssh_access::start_monitor(app.handle());
                editor::refresh_transports(app.handle());
                runtime::storage::start_monitor(app.handle());
                startup::install(app.handle());
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
    system_shutdown::install_terminate_handler(app.handle());
    app.run(|_app, _event| {
        if let tauri::RunEvent::Exit = &_event {
            settings::exit_backstop(_app);
            ssh_access::close_all();
            remote_network::close_all();
            transfer::close_all(std::time::Duration::from_secs(5));
        }
        if let tauri::RunEvent::ExitRequested { api, code, .. } = &_event {
            settings::prevent_exit_until_saved(_app, api, *code);
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
    settings::exit_without_shutdown(app);
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
