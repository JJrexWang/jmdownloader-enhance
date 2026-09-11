use std::sync::Arc;
// events 仍在 lib.rs 通过 module 路径导出，但不再 derive tauri_specta::Event
use eyre::WrapErr;
use parking_lot::RwLock;
use tauri::{Manager, Wry};

// TODO: 用prelude来消除警告
use crate::commands::*;
use crate::config::Config;
use crate::downloader::download_manager::DownloadManager;
use crate::errors::install_custom_eyre_handler;
use crate::export::ComicExportLock;
use crate::jm_client::JmClient;
use crate::utils::DownloadedComicsIndex;

mod archive;
mod commands;
mod config;
mod downloader;
mod errors;
mod events;
mod export;
mod extensions;
mod jm_client;
mod logger;
mod responses;
mod service;
#[cfg(test)]
mod test_ctx;
mod text;
mod types;
mod utils;

fn generate_context() -> tauri::Context<Wry> {
    tauri::generate_context!()
}

// TODO: 添加Panic Doc
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    install_custom_eyre_handler().unwrap();

    let builder = tauri_specta::Builder::<Wry>::new()
        .commands(tauri_specta::collect_commands![
            greet,
            get_config,
            save_config,
            login,
            search,
            get_comic,
            get_favorite_folder,
            get_weekly_info,
            get_weekly,
            get_user_profile,
            create_download_task,
            create_download_tasks,
            pause_download_task,
            resume_download_task,
            delete_download_task,
            download_comic,
            download_all_favorites,
            update_downloaded_comics,
            show_path_in_file_manager,
            sync_favorite_folder,
            get_downloaded_comics,
            export_cbz,
            export_pdf,
            export_cbz_chapters,
            export_pdf_chapters,
            get_logs_dir_size,
            get_synced_comic,
            get_synced_comic_in_favorite,
            get_synced_comic_in_search,
            get_synced_comic_in_weekly,
            open_log_file,
        ]);
        // events 不再走 tauri-specta collect_events，由 AppContext::dispatch 统一管理
        // (桌面端：转发给 tauri::Emitter；HTTP 端：推到 broadcast channel)；

    #[cfg(debug_assertions)]
    builder
        .export(
            specta_typescript::Typescript::default()
                .bigint(specta_typescript::BigIntExportBehavior::Number)
                .formatter(specta_typescript::formatter::prettier)
                .header("// @ts-nocheck"), // 跳过检查
            "../src/bindings.ts",
        )
        .expect("Failed to export typescript bindings");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);

            // 先建 AppPaths（包含 data_dir / logs_dir），Config 和下载目录都依赖它
            let app_paths = service::AppPaths::from_app_handle(app.handle())?;
            std::fs::create_dir_all(&app_paths.data_dir).wrap_err(format!(
                "failed to create app data dir: {}",
                app_paths.data_dir.display()
            ))?;
            std::fs::create_dir_all(&app_paths.logs_dir).wrap_err(format!(
                "failed to create logs dir: {}",
                app_paths.logs_dir.display()
            ))?;
            app.manage(app_paths);

            let config = RwLock::new(Config::new(&app.state::<service::AppPaths>().data_dir)?);
            app.manage(config);

            // 构造一个共享的 `Arc<dyn AppContext>`：内部就是当前 AppHandle，
            // 但走 trait object 路径让所有下游（JmClient / DownloadManager / logger）
            // 都能透明换成 HTTP 适配器。
            let ctx: Arc<dyn service::AppContext> = Arc::new(app.handle().clone());

            let jm_client = JmClient::new(ctx.clone());
            app.manage(jm_client);

            let download_manager = DownloadManager::new(ctx.clone());
            app.manage(download_manager);

            let export_lock = ComicExportLock::new();
            app.manage(export_lock);

            let downloaded_comics_index = DownloadedComicsIndex::new();
            app.manage(downloaded_comics_index);

            logger::init(ctx.clone())?;

            Ok(())
        })
        .run(generate_context())
        .expect("error while running tauri application");
}
