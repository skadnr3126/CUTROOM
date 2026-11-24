mod editor_bridge;
mod project;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(windows)]
    let builder = builder.plugin(tauri_plugin_global_shortcut::Builder::new().build());
    builder
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data = app.path().app_local_data_dir()?;
            let library = project::library::Library::open(data.clone())?;
            let active = library.current()?;
            if active.media_dir() == data.join("project").join("media") {
                std::fs::create_dir_all(active.media_dir())?;
            }
            project::library::grant_media(app.handle(), &active.media_dir())?;
            app.manage(project::media_drop::DropInbox::default());
            app.manage(project::capture::recording::RecordingInbox::default());
            app.manage(library);
            if let Err(error) = editor_bridge::start(app.handle().clone(), &data) {
                eprintln!("컷룸 제어 연결을 열 수 없습니다. {error}");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            project::library::project_session,
            project::library::project_recent,
            project::library::project_switch,
            project::library::project_save_draft,
            project::project_load,
            project::project_sync_media,
            project::project_relink_media,
            project::project_edit,
            project::read_script_file,
            project::project_import_media,
            #[cfg(windows)]
            project::file_drag::drag_bin_files,
            project::capture::region::capture_region,
            project::capture::recording::begin_recording,
            project::capture::recording::append_recording,
            project::capture::recording::finish_recording,
            project::capture::recording::discard_recording,
            project::media_drop::begin_media_drop,
            project::media_drop::append_media_drop,
            project::media_drop::end_media_drop,
            project::project_caption_font,
            project::ass_preview::ass_preview_sheet,
            project::ass::ass_ranges,
            project::ass_files::ass_save_file,
            project::ass::ass_rows,
            project::ass::ass_update,
            project::project_render_source,
            project::project_clip_thumbnail,
            project::project_save_clip,
            project::project_export
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
