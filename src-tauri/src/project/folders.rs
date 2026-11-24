use super::model::Project;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SourceFolder {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<Uuid>,
    pub id: Uuid,
    pub name: String,
}

impl Project {
    pub(super) fn require_folder(&self, id: Option<Uuid>) -> Result<(), String> {
        if id.is_some_and(|id| !self.folders.iter().any(|f| f.id == id)) {
            return Err("폴더를 찾을 수 없습니다.".into());
        }
        Ok(())
    }

    pub(super) fn has_folder_name(
        &self,
        name: &str,
        parent: Option<Uuid>,
        except: Option<Uuid>,
    ) -> bool {
        let folded = name.to_lowercase();
        self.folders.iter().any(|f| {
            Some(f.id) != except && f.parent_id == parent && f.name.to_lowercase() == folded
        })
    }

    fn folder_name(
        &self,
        name: &str,
        parent: Option<Uuid>,
        except: Option<Uuid>,
    ) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() || name.chars().any(char::is_control) {
            return Err("폴더 이름을 입력해 주세요.".into());
        }
        if self.has_folder_name(name, parent, except) {
            return Err("이미 사용 중인 폴더 이름입니다.".into());
        }
        Ok(name.to_owned())
    }

    pub(super) fn create_folder(
        &mut self,
        id: Uuid,
        name: String,
        parent_id: Option<Uuid>,
    ) -> Result<(), String> {
        self.require_folder(parent_id)?;
        let name = self.folder_name(&name, parent_id, None)?;
        if self.folders.iter().any(|f| f.id == id) {
            return Err("이미 존재하는 폴더입니다.".into());
        }
        self.folders.push(SourceFolder {
            disk_key: None,
            id,
            name,
            parent_id,
        });
        Ok(())
    }

    pub(super) fn rename_folder(&mut self, id: Uuid, name: String) -> Result<(), String> {
        let index = self.folder_index(id)?;
        let name = self.folder_name(&name, self.folders[index].parent_id, Some(id))?;
        self.folders[index].name = name;
        Ok(())
    }

    fn folder_index(&self, id: Uuid) -> Result<usize, String> {
        self.folders
            .iter()
            .position(|f| f.id == id)
            .ok_or_else(|| "폴더를 찾을 수 없습니다.".into())
    }

    pub(super) fn move_folder(&mut self, id: Uuid, parent_id: Option<Uuid>) -> Result<(), String> {
        let index = self.folder_index(id)?;
        let mut ancestor = parent_id;
        let mut seen = HashSet::new();
        while let Some(parent) = ancestor {
            if parent == id || !seen.insert(parent) {
                return Err("폴더를 자기 자신이나 그 하위 폴더 안으로 이동할 수 없습니다.".into());
            }
            ancestor = self.folders[self.folder_index(parent)?].parent_id;
        }
        self.folder_name(&self.folders[index].name, parent_id, Some(id))?;
        self.folders[index].parent_id = parent_id;
        Ok(())
    }

    pub(super) fn remove_folder(&mut self, id: Uuid) -> Result<(), String> {
        let parent = self.folders[self.folder_index(id)?].parent_id;
        for child in self.folders.iter().filter(|f| f.parent_id == Some(id)) {
            self.folder_name(&child.name, parent, Some(id))
                .map_err(|_| {
                    format!(
                        "상위 위치에 ‘{}’ 폴더가 있습니다. 이름을 바꾼 뒤 삭제해 주세요.",
                        child.name
                    )
                })?;
        }
        for asset in &mut self.assets {
            if asset.folder_id == Some(id) {
                asset.folder_id = parent;
            }
        }
        for folder in &mut self.folders {
            if folder.parent_id == Some(id) {
                folder.parent_id = parent;
            }
        }
        self.folders.retain(|f| f.id != id);
        Ok(())
    }

    pub(super) fn move_sources(
        &mut self,
        ids: Vec<Uuid>,
        folder_id: Option<Uuid>,
    ) -> Result<(), String> {
        self.require_folder(folder_id)?;
        let ids: HashSet<_> = ids.into_iter().collect();
        let members: Vec<_> = self
            .assets
            .iter_mut()
            .filter(|a| !a.removed_from_bin && ids.contains(&a.id))
            .collect();
        if ids.is_empty() || members.len() != ids.len() {
            return Err("이동할 소스 카드를 다시 선택해 주세요.".into());
        }
        for asset in members {
            asset.folder_id = folder_id;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProjectStore;
    use serde_json::json;
    use std::fs;

    #[test]
    fn nested_folder_edits_preserve_hierarchy_on_move_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::new(dir.path().to_owned());
        let edit = |request| store.edit(serde_json::from_value(request).unwrap());
        let [a, b, child, leaf, other] = std::array::from_fn(|_| Uuid::new_v4());
        for (id, parent, name) in [
            (a, None, "시작"),
            (b, None, "마무리"),
            (child, Some(a), "사진"),
            (leaf, Some(child), "풍경"),
            (other, Some(b), "사진"),
        ] {
            edit(json!({"type":"createFolder","id":id,"parentId":parent,"name":name})).unwrap();
        }
        assert!(edit(json!({"type":"moveFolder","id":a,"parentId":leaf})).is_err());
        assert!(edit(json!({"type":"moveFolder","id":child,"parentId":b})).is_err());
        assert!(
            edit(json!({"type":"createFolder","id":Uuid::new_v4(),"parentId":a,"name":"사진"}))
                .is_err()
        );
        edit(json!({"type":"renameFolder","id":other,"name":"영상"})).unwrap();
        edit(json!({"type":"moveFolder","id":child,"parentId":b})).unwrap();
        let reopened = ProjectStore::new(dir.path().to_owned()).load().unwrap();
        let parent = |id| {
            reopened
                .folders
                .iter()
                .find(|f| f.id == id)
                .unwrap()
                .parent_id
        };
        assert_eq!(parent(child), Some(b));
        assert_eq!(parent(leaf), Some(child));
        let after = edit(json!({"type":"removeFolder","id":child}))
            .unwrap()
            .project;
        assert_eq!(
            after
                .folders
                .iter()
                .find(|f| f.id == leaf)
                .unwrap()
                .parent_id,
            Some(b)
        );
    }

    #[test]
    fn folders_survive_reload_and_preserve_legacy_source_references() {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::new(root.path().to_owned());
        let scene = Uuid::new_v4();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let folder = Uuid::new_v4();
        let original = json!({
            "title":"기존 프로젝트",
            "assets": ([a,b].map(|id| json!({"id":id,"name":"원본.png","kind":"image","extension":"png","durationMs":null,"hasAudio":false}))),
            "scenes":[{"id":scene,"revision":0,"text":"대본","caption":"","durationMs":4000,
                "visual":{"assetId":a,"inMs":0},"audio":null}]
        });
        fs::create_dir_all(store.media_dir()).unwrap();
        let file = store.media_dir().join(format!("{a}.png"));
        let bytes = include_bytes!("../../icons/32x32.png");
        fs::write(&file, bytes).unwrap();
        fs::write(store.directory().join("project.json"), original.to_string()).unwrap();
        let edit = |request| store.edit(serde_json::from_value(request).unwrap());
        assert!(store.load().unwrap().folders.is_empty());
        edit(json!({"type":"createFolder","id":folder,"name":"  장면  "})).unwrap();
        assert!(edit(json!({"type":"createFolder","id":Uuid::new_v4(),"name":"장면"})).is_err());
        assert!(
            edit(json!({"type":"moveSources","ids":[a,Uuid::new_v4()],"folderId":folder})).is_err()
        );
        assert!(store.load().unwrap().asset(a).unwrap().folder_id.is_none());
        edit(json!({"type":"moveSources","ids":[a,b],"folderId":folder})).unwrap();
        edit(json!({"type":"renameFolder","id":folder,"name":"인트로"})).unwrap();
        let reopened = ProjectStore::new(root.path().to_owned());
        let saved = reopened.load().unwrap();
        assert_eq!(saved.folders[0].name, "인트로");
        assert!(
            saved
                .assets
                .iter()
                .all(|asset| asset.folder_id == Some(folder))
        );
        let nested = Uuid::new_v4();
        edit(json!({"type":"createFolder","id":nested,"parentId":folder,"name":"배경"})).unwrap();
        edit(json!({"type":"moveSources","ids":[b],"folderId":nested})).unwrap();
        let moved = reopened.load().unwrap();
        assert_eq!(moved.asset(b).unwrap().folder_id, Some(nested));
        assert_eq!(moved.asset(a).unwrap().folder_id, Some(folder));
        edit(json!({"type":"removeFolder","id":nested})).unwrap();
        let project = reopened.load().unwrap();
        assert_eq!(project.folders[0].name, "인트로");
        edit(json!({"type":"moveSources","ids":[a],"folderId":null})).unwrap();
        let final_project = edit(json!({"type":"removeFolder","id":folder}))
            .unwrap()
            .project;
        assert!(final_project.folders.is_empty());
        assert!(
            final_project
                .assets
                .iter()
                .all(|asset| asset.folder_id.is_none() && !asset.removed_from_bin)
        );
        assert_eq!(
            serde_json::to_value(final_project.scenes).unwrap(),
            original["scenes"]
        );
        assert_eq!(fs::read(file).unwrap(), bytes);
    }
}
