pub(crate) mod ass;
pub(crate) mod ass_files;
pub(crate) mod ass_preview;
#[cfg(test)]
mod ass_tests;
mod asset_filename;
mod caption_track;
pub(crate) mod capture;
pub(crate) mod chat_timeline;
mod clip_media;
mod composition_layout;
pub(crate) mod control;
mod cut_range;
mod drop_tree;
#[cfg(windows)]
pub(crate) mod file_drag;
mod files;
mod folders;
mod generated;
mod guidelines;
pub(crate) mod library;
pub(crate) mod media_drop;
pub(crate) mod media_tree;
mod model;
mod package;
mod render;
mod scene_tools;
mod source_work;
#[cfg(test)]
mod tests;
mod undo;

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;
use tempfile::NamedTempFile;
use uuid::Uuid;

use model::{Placement, Project, SceneSettings, VisualPlacement};
pub(crate) use source_work::{RenderedSource, SourceRecipe};
use undo::{History, Inverse, UndoTarget};

#[derive(Clone)]
pub(crate) struct ProjectStore {
    app_data: PathBuf,
    directory: PathBuf,
    pub(crate) id: Uuid,
    pub(crate) session: Uuid,
    history: Arc<Mutex<History>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectView {
    project: Project,
    media_dir: PathBuf,
    export_available: bool,
    media_extensions: &'static [&'static str],
    undo: Option<UndoTarget>,
}

#[derive(Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum Edit {
    SceneTool {
        scene_id: Uuid,
        expected: u32,
        tool: scene_tools::SceneTool,
    },
    CutClipRange {
        folder_id: Option<Uuid>,
        scene_id: Uuid,
        expected: u32,
        range: cut_range::ClipRange,
    },
    SaveGuidelineStyle {
        expected: u32,
        style: guidelines::StyleInput,
    },
    SelectGuidelineStyle {
        id: Option<Uuid>,
    },
    SetProductionGuidelines {
        expected: u32,
        text: String,
    },
    ApplyChatTimeline {
        folder_id: Option<Uuid>,
        job_id: Uuid,
        base: Vec<chat_timeline::SceneVersion>,
        operations: Vec<chat_timeline::Operation>,
    },
    Undo {
        token: Uuid,
    },
    MoveFolder {
        id: Uuid,
        parent_id: Option<Uuid>,
    },
    CreateFolder {
        parent_id: Option<Uuid>,
        id: Uuid,
        name: String,
    },
    RenameFolder {
        id: Uuid,
        name: String,
    },
    RemoveFolder {
        id: Uuid,
    },
    MoveSources {
        ids: Vec<Uuid>,
        folder_id: Option<Uuid>,
    },
    RenameSource {
        id: Uuid,
        name: String,
    },
    Rename {
        title: String,
    },
    AddScene {
        text: Option<String>,
    },
    RenameScene {
        id: Uuid,
        expected: u32,
        name: String,
    },
    UpdateScene {
        id: Uuid,
        expected: u32,
        settings: SceneSettings,
        title_card: Option<String>,
    },
    MoveScene {
        id: Uuid,
        target_id: Uuid,
        after: bool,
    },
    RemoveScene {
        id: Uuid,
        expected: u32,
    },
    RemoveSource {
        id: Uuid,
    },
}

impl ProjectStore {
    pub fn new(app_data: PathBuf) -> Self {
        let directory = app_data.join("project");
        Self::at(app_data, directory, Uuid::nil())
    }

    fn at(app_data: PathBuf, directory: PathBuf, id: Uuid) -> Self {
        Self {
            app_data,
            directory,
            id,
            session: Uuid::new_v4(),
            history: Default::default(),
        }
    }

    fn directory(&self) -> PathBuf {
        self.directory.clone()
    }

    pub(crate) fn media_dir(&self) -> PathBuf {
        self.directory().join("media")
    }

    fn load(&self) -> Result<Project, String> {
        let file = self.directory().join("project.json");
        match fs::read(file) {
            Ok(data) => {
                let mut project: Project = serde_json::from_slice(&data)
                    .map_err(|_| "저장된 프로젝트를 읽을 수 없습니다.")?;
                if !generated::valid_assets(&project) {
                    return Err("저장된 프로젝트의 소스 정보가 올바르지 않습니다.".to_owned());
                }
                if !project.legacy_guidelines.is_empty() {
                    let default = project
                        .guideline_styles
                        .iter_mut()
                        .find(|style| style.id.is_nil())
                        .ok_or("기존 공통 지침의 기본 스타일을 찾을 수 없습니다.")?;
                    default.text = std::mem::take(&mut project.legacy_guidelines);
                }
                Ok(project)
            }
            Err(error)
                if error.kind() == io::ErrorKind::NotFound
                    && self.id.is_nil()
                    && self.directory == self.app_data.join("project") =>
            {
                Ok(Project::default())
            }
            Err(_) => Err("저장된 프로젝트를 열 수 없습니다.".to_owned()),
        }
    }

    fn view(&self, project: Project) -> ProjectView {
        let undo = self
            .history
            .lock()
            .ok()
            .and_then(|history| history.target());
        self.view_with_undo(project, undo)
    }

    fn view_with_undo(&self, project: Project, undo: Option<UndoTarget>) -> ProjectView {
        ProjectView {
            project,
            media_dir: self.media_dir(),
            media_extensions: files::MEDIA_EXTENSIONS,
            export_available: files::find_tool(&self.app_data, "ffmpeg").is_some(),
            undo,
        }
    }

    fn mutate(
        &self,
        change: impl FnOnce(&mut Project, &Path, &mut Vec<PathBuf>) -> Result<(), String>,
    ) -> Result<ProjectView, String> {
        self.commit(change).map(|project| self.view(project))
    }

    fn commit(
        &self,
        change: impl FnOnce(&mut Project, &Path, &mut Vec<PathBuf>) -> Result<(), String>,
    ) -> Result<Project, String> {
        self.commit_files(|project, media, added, _| change(project, media, added))
    }

    fn lock_project(&self) -> Result<fs::File, String> {
        let directory = self.directory();
        if directory != self.app_data.join("project") && !directory.join("project.json").is_file() {
            return Err("프로젝트 폴더를 찾을 수 없습니다.".into());
        }
        let media = self.media_dir();
        fs::create_dir_all(&media).map_err(|_| "프로젝트 저장 공간을 만들 수 없습니다.")?;
        let lock = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("project.lock"))
            .map_err(|_| "프로젝트 잠금을 열 수 없습니다.")?;
        lock.lock()
            .map_err(|_| "다른 프로젝트 저장 작업이 진행 중입니다.")?;

        Ok(lock)
    }

    fn commit_files(
        &self,
        change: impl FnOnce(
            &mut Project,
            &Path,
            &mut Vec<PathBuf>,
            &mut Vec<PathBuf>,
        ) -> Result<(), String>,
    ) -> Result<Project, String> {
        let directory = self.directory();
        let media = self.media_dir();
        let _lock = self.lock_project()?;
        let mut project = self.load()?;
        let marker = project.media_transaction;
        media_tree::recover(&directory, marker)?;
        let layout = if project.media_tree {
            Some(media_tree::Layout::capture(&project)?)
        } else {
            None
        };
        let mut journal = media_tree::Journal::new(&directory);
        let mut added_files = Vec::new();
        let mut retired = Vec::new();
        let result = change(&mut project, &media, &mut added_files, &mut retired).and_then(|_| {
            if let Some(before) = layout {
                media_tree::materialize(&directory, &mut project, &before, &mut journal, false)?;
            }
            if let Some(id) = journal.marker() {
                project.media_transaction = Some(id);
            }
            save(&directory, &project)
        });
        journal.finish(if result.is_ok() {
            project.media_transaction
        } else {
            marker
        })?;
        if let Err(error) = result {
            for file in added_files {
                let _ = fs::remove_file(file);
            }
            return Err(error);
        }
        for old in retired {
            if let Err(e) = fs::remove_file(&old) {
                eprintln!("이전 파일 이름 정리 실패 {}: {e}", old.display());
            }
        }
        Ok(project)
    }

    fn edit(&self, edit: Edit) -> Result<ProjectView, String> {
        let staged = self.stage_cut_sources(&edit)?;
        let mut history = self
            .history
            .lock()
            .map_err(|_| "편집 기록을 열 수 없습니다.")?;
        let undoing = matches!(edit, Edit::Undo { .. });
        let mut inverse = None;
        let project = self.commit_files(|project, media, added, _retired| {
            let mut removed = Vec::new();
            match edit {
                Edit::SceneTool {
                    scene_id,
                    expected,
                    tool,
                } => {
                    inverse = tool.apply(project, scene_id, expected)?;
                }
                Edit::CutClipRange {
                    scene_id,
                    expected,
                    range,
                    ..
                } => {
                    inverse = Some(range.apply(project, scene_id, expected, &mut removed)?);
                }
                Edit::SaveGuidelineStyle { expected, style } => {
                    inverse = project.save_style(expected, style)?
                }
                Edit::SelectGuidelineStyle { id } => project.select_style(id)?,

                Edit::SetProductionGuidelines { expected, text } => {
                    inverse = project.set_production_guidelines(expected, text)?;
                }
                Edit::ApplyChatTimeline {
                    job_id,
                    base,
                    operations,
                    ..
                } => {
                    inverse =
                        chat_timeline::apply(project, job_id, &base, operations, &mut removed)?;
                }
                Edit::Undo { token } => history.apply(token, project)?,
                Edit::Rename { title } => {
                    if title.trim().is_empty() {
                        return Err("프로젝트 이름을 입력해 주세요.".to_owned());
                    }
                    project.title = title.trim().to_owned();
                }
                Edit::AddScene { text } => {
                    let scene_text = match text.as_deref().map(str::trim) {
                        Some("") => return Err("대본에서 추가할 내용을 선택해 주세요.".to_owned()),
                        Some(text) => text.to_owned(),
                        None => String::new(),
                    };
                    project.add_scene(scene_text);
                    let scene = project.scenes.last().unwrap();
                    inverse = Some(Inverse::RemoveAdded {
                        id: scene.id,
                        initial: scene.settings.clone(),
                    });
                }
                Edit::RenameScene { id, expected, name } => {
                    let index = project.scene_index(id)?;
                    let scene = &mut project.scenes[index];
                    if scene.revision != expected {
                        return Err("요청 이후 구간이 변경되었습니다. 다시 확인해 주세요.".into());
                    }
                    inverse = chat_timeline::rename_scene(scene, name)?;
                }
                Edit::UpdateScene {
                    id,
                    expected,
                    mut settings,
                    title_card,
                } => {
                    if let Some(image) = title_card {
                        let asset = files::write_title(media, &image)?;
                        added.push(asset.path_in(media));
                        let card = VisualPlacement {
                            frame: None,
                            muted: false,
                            source: Placement {
                                asset_id: asset.id,
                                in_ms: 0,
                            },
                            trim: None,
                        };
                        if let Some(first) = settings.clips.first_mut() {
                            first.visual = card;
                        } else {
                            settings.visual = Some(card);
                        }
                        project.assets.push(asset);
                    }
                    let index = project.scene_index(id)?;
                    let before = project.scenes[index].settings.clone();
                    project.edit_scene(id, expected, settings)?;
                    let after = &project.scenes[index].settings;
                    if *after != before {
                        inverse = Some(Inverse::Settings {
                            id,
                            before,
                            after: after.clone(),
                        });
                    }
                }
                Edit::MoveScene {
                    id,
                    target_id,
                    after,
                } => {
                    let from = project.scene_index(id)?;
                    let target = project.scene_index(target_id)?;
                    if from != target {
                        let insertion = target + usize::from(after) - usize::from(from < target);
                        let moved = project.scenes.remove(from);
                        project.scenes.insert(insertion, moved);
                        if from != insertion {
                            inverse = Some(Inverse::Move {
                                id,
                                index: from,
                                after: undo::order(project),
                            });
                        }
                    }
                }
                Edit::RemoveScene { id, expected } => {
                    let index = project.scene_index(id)?;
                    if project.scenes[index].revision != expected {
                        return Err("구간이 변경되었습니다. 최신 구간을 확인해 주세요.".to_owned());
                    }
                    let scene = project.scenes.remove(index);
                    inverse = Some(Inverse::Restore {
                        scene,
                        index,
                        after: undo::order(project),
                    });
                }
                Edit::RemoveSource { id } => {
                    let asset = project
                        .assets
                        .iter_mut()
                        .find(|asset| asset.id == id)
                        .ok_or("삭제할 소스 카드를 찾을 수 없습니다.")?;
                    asset.removed_from_bin = true;
                    project.detach_style_reference(id);
                }
                Edit::CreateFolder {
                    id,
                    name,
                    parent_id,
                } => project.create_folder(id, name, parent_id)?,
                Edit::MoveFolder { id, parent_id } => project.move_folder(id, parent_id)?,
                Edit::RenameFolder { id, name } => project.rename_folder(id, name)?,
                Edit::RemoveFolder { id } => project.remove_folder(id)?,
                Edit::MoveSources { ids, folder_id } => project.move_sources(ids, folder_id)?,
                Edit::RenameSource { id, name } => {
                    let name = name.trim();
                    if name.is_empty() || name.chars().any(char::is_control) {
                        return Err("소스 이름을 입력해 주세요.".into());
                    }
                    project
                        .assets
                        .iter_mut()
                        .find(|a| a.id == id && !a.removed_from_bin)
                        .ok_or("소스를 찾을 수 없습니다.")?
                        .name = name.into();
                }
            }
            if !removed.is_empty() {
                staged.publish(&removed, project, media, added)?;
            }
            Ok(())
        })?;
        if undoing {
            history.pop_committed();
        }
        if let Some(entry) = inverse {
            history.push(entry);
        }
        Ok(self.view_with_undo(project, history.target()))
    }


}

fn save(directory: &Path, project: &Project) -> Result<(), String> {
    let mut file =
        NamedTempFile::new_in(directory).map_err(|_| "프로젝트를 저장할 수 없습니다.")?;
    serde_json::to_writer_pretty(&mut file, project)
        .map_err(|_| "프로젝트를 기록할 수 없습니다.")?;
    file.flush()
        .and_then(|_| file.as_file().sync_all())
        .map_err(|_| "프로젝트를 기록할 수 없습니다.")?;
    file.persist(directory.join("project.json"))
        .map_err(|_| "프로젝트를 저장할 수 없습니다.")?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn project_load(store: ProjectStore) -> Result<ProjectView, String> {
    run_io(move || Ok(store.view(store.reconcile_media(true)?.0))).await
}

#[tauri::command]
pub(crate) async fn project_sync_media(store: ProjectStore) -> Result<Option<ProjectView>, String> {
    run_io(move || {
        let (p, changed) = store.sync_media()?;
        Ok(changed.then(|| store.view(p)))
    })
    .await
}

#[tauri::command]
pub(crate) async fn project_relink_media(
    app: AppHandle,
    store: ProjectStore,
    asset_id: Uuid,
) -> Result<Option<ProjectView>, String> {
    run_io(move || {
        let Some(file) = app
            .dialog()
            .file()
            .set_title("누락된 소스에 다시 연결할 파일")
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let path = file.into_path().map_err(|_| "로컬 파일을 선택해 주세요.")?;
        store
            .mutate(|p, media, added| {
                let i = p
                    .assets
                    .iter()
                    .position(|a| a.id == asset_id && a.missing && !a.removed_from_bin)
                    .ok_or("다시 연결할 누락 소스가 없습니다.")?;
                let old = p.assets.remove(i);
                if old.expected_path(media).exists()
                    && !p
                        .assets
                        .iter()
                        .any(|a| !a.missing && a.project_file == old.project_file)
                {
                    return Err("원래 파일이 복원되었습니다. 보관함을 다시 열어 주세요.".into());
                }
                let name = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .ok_or("파일 이름을 읽을 수 없습니다.")?;
                let probe = files::find_tool(&store.app_data, "ffprobe");
                let mut a = files::inspect_asset(&path, name, probe.as_deref())?;
                if a.kind != old.kind {
                    return Err("같은 종류의 미디어 파일을 선택해 주세요.".into());
                }
                a.id = old.id;
                a.name = old.name;
                a.source_job_id = old.source_job_id;
                a.folder_id = old.folder_id;
                generated::publish(p, media, added, a, generated::Input::File(&path))?;
                Ok(())
            })
            .map(Some)
    })
    .await
}

#[tauri::command]
pub(crate) async fn project_edit(store: ProjectStore, edit: Edit) -> Result<ProjectView, String> {
    run_io(move || store.edit(edit)).await
}

#[tauri::command]
pub(crate) async fn read_script_file(app: AppHandle) -> Result<Option<String>, String> {
    run_io(move || {
        let Some(file) = app
            .dialog()
            .file()
            .add_filter("대본 텍스트", &["txt", "md"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let path = file
            .into_path()
            .map_err(|_| "이 기기에 있는 대본 파일을 선택해 주세요.")?;
        let text = files::read_script(&path)?;
        if text.trim().is_empty() {
            return Err("대본 파일에 불러올 텍스트가 없습니다.".to_owned());
        }
        Ok(Some(text))
    })
    .await
}

#[tauri::command]
pub(crate) async fn project_import_media(
    app: AppHandle,
    folder_id: Option<Uuid>,
    store: ProjectStore,
) -> Result<Option<ProjectView>, String> {
    run_io(move || {
        let Some(files) = app
            .dialog()
            .file()
            .add_filter("영상, 이미지, 음성", files::MEDIA_EXTENSIONS)
            .blocking_pick_files()
        else {
            return Ok(None);
        };
        if files.is_empty() {
            return Ok(None);
        }
        let probe = files::find_tool(&store.app_data, "ffprobe");
        store
            .mutate(|project, media, added| {
                project.require_folder(folder_id)?;
                for file in files {
                    let path = file
                        .into_path()
                        .map_err(|_| "이 기기에 있는 소스 파일을 선택해 주세요.")?;
                    if let Some(asset)=media_tree::registered_file(project,media,&path)? {
                        if folder_id.is_some(){asset.folder_id=folder_id;}
                        continue;
                    }
                    let mut asset = files::copy_asset(&path, media, probe.as_deref())?;
                    asset.folder_id = folder_id;
                    added.push(asset.path_in(media));
                    project.assets.push(asset);
                }
                Ok(())
            })
            .map(Some)
    })
    .await
}

#[tauri::command]
pub(crate) async fn project_caption_font() -> Result<tauri::ipc::Response, String> {
    run_io(|| {
        let path = render::caption_font().ok_or("자막에 사용할 한글 글꼴을 찾을 수 없습니다.")?;
        let bytes = fs::read(path).map_err(|_| "자막 글꼴을 읽을 수 없습니다.")?;
        Ok(tauri::ipc::Response::new(bytes))
    })
    .await
}

#[tauri::command]
pub(crate) async fn project_render_source(
    store: ProjectStore,
    recipe: SourceRecipe,
) -> Result<RenderedSource, String> {
    run_io(move || store.render_source(recipe)).await
}

#[tauri::command]
pub(crate) async fn project_clip_thumbnail(
    store: ProjectStore,
    asset_id: Uuid,
    at_ms: u64,
) -> Result<String, String> {
    run_io(move || store.clip_thumbnail(asset_id, at_ms)).await
}

#[tauri::command]
pub(crate) async fn project_save_clip(
    store: ProjectStore,
    scene_id: Uuid,
    expected: u32,
    clip_id: Option<Uuid>,
    folder_id: Option<Uuid>,
) -> Result<RenderedSource, String> {
    run_io(move || store.save_clip(scene_id, expected, clip_id, folder_id)).await
}

#[tauri::command]
pub(crate) async fn project_export(app: AppHandle, store: ProjectStore) -> Result<bool, String> {
    run_io(move || {
        let ffmpeg = files::find_tool(&store.app_data, "ffmpeg")
            .ok_or("FFmpeg를 찾을 수 없습니다. 설치한 뒤 컷룸을 다시 열어 주세요.")?;
        let project = store.load()?;
        if project.scenes.is_empty() {
            return Err("내보낼 대본 구간을 추가해 주세요.".to_owned());
        }
        let Some(file) = app
            .dialog()
            .file()
            .add_filter("MP4 영상", &["mp4"])
            .set_file_name("컷룸 편집본.mp4")
            .blocking_save_file()
        else {
            return Ok(false);
        };
        let path = file
            .into_path()
            .map_err(|_| "이 기기의 저장 위치를 선택해 주세요.")?;
        let path = path.with_extension("mp4");
        render::export(&ffmpeg, &store.media_dir(), &project, &path)?;
        Ok(true)
    })
    .await
}

async fn run_io<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(job)
        .await
        .map_err(|_| "프로젝트 작업을 완료할 수 없습니다.".to_owned())?
}
