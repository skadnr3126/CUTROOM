use super::{Project, ProjectStore, run_io, save};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::{
    AppHandle, Manager, Runtime, State,
    ipc::{CommandArg, CommandItem, InvokeError},
};
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    id: Uuid,
    path: PathBuf,
}

struct OpenProjects {
    recent: Vec<Entry>,
    current: ProjectStore,
    known: HashMap<Uuid, ProjectStore>,
    startup_notice: Option<String>,
}

#[cfg(test)]
mod tests;

pub(crate) struct Library {
    data: PathBuf,
    inner: Mutex<OpenProjects>,
}

fn atomic_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let mut tmp =
        tempfile::NamedTempFile::new_in(path.parent().ok_or("저장 위치를 확인해 주세요.")?)
            .map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut tmp, value).map_err(|e| e.to_string())?;
    tmp.flush()
        .and_then(|_| tmp.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    tmp.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

impl Library {
    pub fn open(data: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&data).map_err(|e| e.to_string())?;
        let legacy = ProjectStore::new(data.clone());
        let legacy_entry = Entry {
            id: Uuid::nil(),
            path: legacy.directory(),
        };
        let mut notice = None;
        let mut recent: Vec<Entry> = match fs::read(data.join("recent-projects.json")) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(list) => list,
                Err(_) => {
                    let backup = data.join(format!("recent-projects-{}.bak", Uuid::new_v4()));
                    fs::rename(data.join("recent-projects.json"), backup)
                        .map_err(|e| e.to_string())?;
                    notice=Some("최근 목록을 복구할 수 없어 기본 프로젝트를 열었습니다. 폴더 열기로 다시 등록할 수 있습니다.".into());
                    Vec::new()
                }
            },
            Err(e) if e.kind() == ErrorKind::NotFound => vec![],
            Err(e) => return Err(e.to_string()),
        };
        if !recent.iter().any(|e| e.id.is_nil()) {
            recent.push(legacy_entry);
        }
        let entry = &recent[0];
        let current = if entry.id.is_nil() && entry.path == legacy.directory() {
            legacy
        } else {
            let store = ProjectStore::at(data.clone(), entry.path.clone(), entry.id);
            if store.load().is_ok() || entry.id.is_nil() {
                store
            } else {
                notice =
                    Some("마지막 프로젝트 폴더를 열 수 없어 기본 프로젝트를 열었습니다.".into());
                legacy
            }
        };
        Ok(Self {
            data,
            inner: Mutex::new(OpenProjects {
                known: HashMap::from([(current.id, current.clone())]),
                recent,
                current,
                startup_notice: notice,
            }),
        })
    }

    pub fn current(&self) -> Result<ProjectStore, String> {
        Ok(self
            .inner
            .lock()
            .map_err(|_| "프로젝트 목록을 열 수 없습니다.")?
            .current
            .clone())
    }

    pub(crate) fn by_id(&self, id: Uuid) -> Result<ProjectStore, String> {
        let state = self
            .inner
            .lock()
            .map_err(|_| "프로젝트 상태를 확인할 수 없습니다.")?;
        if let Some(store) = state.known.get(&id) {
            return Ok(store.clone());
        }
        let entry = state
            .recent
            .iter()
            .find(|e| e.id == id)
            .ok_or("대상 프로젝트를 찾을 수 없습니다.")?;
        Ok(ProjectStore::at(self.data.clone(), entry.path.clone(), id))
    }

    fn activate(
        &self,
        path: PathBuf,
        expected: Uuid,
        name: Option<&str>,
        grant: impl FnOnce(&Path) -> Result<(), String>,
    ) -> Result<ProjectStore, String> {
        let mut state = self
            .inner
            .lock()
            .map_err(|_| "프로젝트 목록을 열 수 없습니다.")?;
        if state.current.session != expected {
            return Err("열린 프로젝트가 바뀌었습니다.".into());
        }
        let path = match name {
            Some(name) => create(&path, name)?,
            None => path,
        };
        let path = dunce::canonicalize(&path).map_err(|_| "프로젝트 폴더를 찾을 수 없습니다.")?;
        let entry = state
            .recent
            .iter()
            .find(|e| e.path == path || dunce::canonicalize(&e.path).is_ok_and(|p| p == path))
            .cloned()
            .unwrap_or_else(|| Entry {
                id: Uuid::new_v4(),
                path: path.clone(),
            });
        let mut store = state
            .known
            .get(&entry.id)
            .cloned()
            .unwrap_or_else(|| ProjectStore::at(self.data.clone(), path.clone(), entry.id));
        store.load()?;
        grant(&store.media_dir())?;
        let mut recent = state.recent.clone();
        recent.retain(|e| e.id != entry.id);
        recent.insert(0, entry);
        atomic_json(&self.data.join("recent-projects.json"), &recent)?;
        store.session = Uuid::new_v4();
        state.known.insert(store.id, store.clone());
        state.recent = recent;
        state.current = store.clone();
        state.startup_notice = None;
        Ok(store)
    }

    fn recent(&self) -> Result<Vec<Entry>, String> {
        Ok(self
            .inner
            .lock()
            .map_err(|_| "프로젝트 목록을 열 수 없습니다.")?
            .recent
            .clone())
    }
}

impl<'de, R: Runtime> CommandArg<'de, R> for ProjectStore {
    fn from_command(item: CommandItem<'de, R>) -> Result<Self, InvokeError> {
        let store = item.message.webview().state::<Library>().current()?;
        let token = item
            .message
            .headers()
            .get("x-cutroom-session")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| Uuid::parse_str(s).ok());
        if token != Some(store.session) {
            return Err("이 요청을 시작한 프로젝트가 닫혔습니다.".into());
        }
        Ok(store)
    }
}

pub(crate) fn grant_media(app: &AppHandle, path: &Path) -> Result<(), String> {
    let scope = app.asset_protocol_scope();
    let root = path.parent().ok_or("프로젝트 폴더를 찾을 수 없습니다.")?;
    scope
        .allow_directory(root.join(".cutroom-retained"), false)
        .map_err(|e| e.to_string())?;
    scope.allow_directory(path, true).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn project_session(library: State<'_, Library>) -> Result<Value, String> {
    let state = library
        .inner
        .lock()
        .map_err(|_| "프로젝트 상태를 확인할 수 없습니다.")?;
    let store = &state.current;
    let draft_path = store.directory().join("script-draft.json");
    let mut notice = state.startup_notice.clone();
    let draft = match fs::read(&draft_path) {
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(value) if value["text"].is_string() && value["locations"].is_object() => Some(value),
            _ => {
                let backup = store
                    .directory()
                    .join(format!("script-draft-{}.bak", Uuid::new_v4()));
                fs::rename(&draft_path, &backup).map_err(|e| e.to_string())?;
                notice = Some(format!(
                    "대본 초안의 형식을 읽을 수 없어 원본을 보관했습니다. {}",
                    backup.display()
                ));
                None
            }
        },
        Err(e) if e.kind() == ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    Ok(
        json!({"id":store.id,"session":store.session,"path":store.directory(),"legacy":store.id.is_nil(),"draft":draft,"notice":notice}),
    )
}

#[tauri::command]
pub(crate) async fn project_save_draft(store: ProjectStore, draft: Value) -> Result<(), String> {
    run_io(move || {
        if store.directory() == store.app_data.join("project") {
            fs::create_dir_all(store.directory()).map_err(|e| e.to_string())?;
        }
        atomic_json(&store.directory().join("script-draft.json"), &draft)
    })
    .await
}

#[tauri::command]
pub(crate) async fn project_recent(library: State<'_, Library>) -> Result<Value, String> {
    let recent = library.recent()?;
    let data = library.data.clone();
    run_io(move || {
        let mut items = Vec::new();
        for entry in recent {
            let store = ProjectStore::at(data.clone(), entry.path.clone(), entry.id);
            let title = store.load().ok().map(|p| p.title);
            items.push(json!({"id":entry.id,"path":entry.path,"title":title}));
        }
        Ok(json!(items))
    })
    .await
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub(crate) enum Open {
    Create { name: String },
    Browse,
    Recent { id: Uuid },
}

fn create(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let name = name.trim();
    if name.is_empty()
        || name.ends_with('.')
        || name
            .chars()
            .any(|c| c.is_control() || r#"\/:*?"<>|"#.contains(c))
        || name == ".."
    {
        return Err("폴더 이름으로 사용할 프로젝트 이름을 입력해 주세요.".into());
    }
    let path = parent.join(name);
    fs::create_dir(&path).map_err(|e| {
        if e.kind() == ErrorKind::AlreadyExists {
            "같은 이름의 폴더가 있습니다. 다른 이름을 입력해 주세요.".into()
        } else {
            e.to_string()
        }
    })?;
    let project = Project {
        media_tree: true,
        title: name.into(),
        ..Default::default()
    };
    if let Err(error) = fs::create_dir(path.join("media")) {
        let _ = fs::remove_dir(&path);
        return Err(error.to_string());
    }
    if let Err(error) = save(&path, &project) {
        let _ = fs::remove_dir(path.join("media"));
        let _ = fs::remove_dir(&path);
        return Err(error);
    }
    Ok(path)
}

#[tauri::command]
pub(crate) async fn project_switch(
    app: AppHandle,
    store: ProjectStore,
    action: Open,
) -> Result<bool, String> {
    run_io(move || {
        let library = app.state::<Library>();
        let (path, name) = match action {
            Open::Recent { id } => (
                library
                    .recent()?
                    .into_iter()
                    .find(|e| e.id == id)
                    .ok_or("최근 프로젝트를 찾을 수 없습니다.")?
                    .path,
                None,
            ),
            choice => {
                let title = if matches!(choice, Open::Create { .. }) {
                    "새 프로젝트 폴더를 만들 위치"
                } else {
                    "project.json이 있는 프로젝트 폴더 열기"
                };
                let Some(folder) = app.dialog().file().set_title(title).blocking_pick_folder()
                else {
                    return Ok(false);
                };
                let path = folder
                    .into_path()
                    .map_err(|_| "로컬 폴더를 선택해 주세요.")?;
                (
                    path,
                    if let Open::Create { name } = choice {
                        Some(name)
                    } else {
                        None
                    },
                )
            }
        };
        library.activate(path, store.session, name.as_deref(), |media| grant_media(&app, media))?;
        Ok(true)
    })
    .await
}
