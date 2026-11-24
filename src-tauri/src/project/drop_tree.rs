use super::model::Project;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DropFolder {
    pub name: String,
    pub parent_index: Option<usize>,
}

// Folder indexes describe only the current drop. Names never become filesystem paths.
pub(super) fn validate(folders: &[DropFolder]) -> Result<(), String> {
    for (index, folder) in folders.iter().enumerate() {
        let name = folder.name.trim();
        if folder.parent_index.is_some_and(|parent| parent >= index)
            || matches!(name, "" | "." | "..")
            || name.contains(['/', '\\'])
            || name.chars().any(char::is_control)
        {
            return Err("가져올 폴더의 이름 또는 경로를 확인해 주세요.".into());
        }
    }
    Ok(())
}

pub(super) fn insert(
    project: &mut Project,
    folders: &[DropFolder],
    destination: Option<Uuid>,
) -> Result<Vec<Uuid>, String> {
    project.require_folder(destination)?;
    let mut ids = Vec::with_capacity(folders.len());
    for folder in folders {
        let parent = folder.parent_index.map(|index| ids[index]).or(destination);
        let original = folder.name.trim();
        let mut name = original.to_owned();
        let mut suffix = 2;
        while project.has_folder_name(&name, parent, None) {
            name = format!("{original} ({suffix})");
            suffix += 1;
        }
        let id = Uuid::new_v4();
        project.create_folder(id, name, parent)?;
        ids.push(id);
    }
    Ok(ids)
}
