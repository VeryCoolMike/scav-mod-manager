mod auto_account;
mod bepinex;
mod commands;
mod error;
mod game;
mod gamebanana;
mod mods;
mod nexus;
mod nexus_sso;
mod nxm;
mod profiles;
mod state;
mod updates;

use state::AppState;
use tauri::{AppHandle, Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();

    // Single-instance MUST be registered first so nxm:// links from a second
    // launch are forwarded to the already-running window (Windows/Linux).
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if let Some(url) = args.iter().find(|a| a.starts_with("nxm://")) {
                handle_nxm_url(app, url.clone());
            }
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.set_focus();
            }
        }));
    }

    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let app_state = AppState::init().map_err(|e| e.to_string())?;
            app.manage(app_state);

            #[cfg(desktop)]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                let handle = app.handle().clone();
                app.deep_link().on_open_url(move |event| {
                    for url in event.urls() {
                        handle_nxm_url(&handle, url.to_string());
                    }
                });
                // Register the nxm scheme at runtime (needed for dev + Linux).
                let _ = app.deep_link().register("nxm");

                // A link may have launched the app the very first time.
                if let Ok(Some(urls)) = app.deep_link().get_current() {
                    for url in urls {
                        handle_nxm_url(app.handle(), url.to_string());
                    }
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::detect_games,
            commands::validate_game_path,
            commands::set_game_path,
            commands::bepinex_status,
            commands::bepinex_install,
            commands::bepinex_uninstall,
            commands::nexus_sso_login,
            commands::nexus_logout,
            commands::nexus_validate,
            commands::nexus_browse,
            commands::nexus_updated,
            commands::nexus_mod_details,
            commands::nexus_mod_files,
            commands::nexus_endorse,
            commands::nexus_tracked,
            commands::install_nxm,
            commands::install_mod_file,
            commands::gb_browse,
            commands::gb_search,
            commands::gb_mod_files,
            commands::gb_install,
            commands::list_installed,
            commands::set_mod_enabled,
            commands::uninstall_mod,
            commands::sync_mods,
            commands::list_profiles,
            commands::create_profile,
            commands::delete_profile,
            commands::clone_profile,
            commands::switch_profile,
            commands::export_profile_bundle,
            commands::import_profile_bundle,
            commands::export_profile_code,
            commands::import_profile_code,
            commands::check_updates,
            commands::launch_game,
            commands::auto_create_account,
            commands::auto_poll_verification,
            commands::auto_full_register,
            commands::nexus_auto_download,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Scav Mod Manager");
}

/// Resolve an nxm link in the background and notify the UI of the result.
fn handle_nxm_url(app: &AppHandle, url: String) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = app.emit("nxm://start", &url);
        let result = {
            let state = app.state::<AppState>();
            nxm::handle(&state, &url).await
        };
        match result {
            Ok(installed) => {
                let _ = app.emit("nxm://done", installed);
            }
            Err(e) => {
                let _ = app.emit("nxm://error", e.to_string());
            }
        }
        if let Some(win) = app.get_webview_window("main") {
            let _ = win.set_focus();
        }
    });
}
