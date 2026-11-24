use super::model::{Project, Scene, SceneSettings};
use serde::Serialize;
use std::collections::VecDeque;
use uuid::Uuid;

const LIMIT: usize = 100;
const CONFLICT: &str = "타임라인이 변경되어 이 작업을 되돌릴 수 없습니다.";

pub(super) enum Inverse {
    Guidelines {
        previous_selection: Option<Uuid>,
        before: Vec<super::guidelines::Style>,
        after: Vec<super::guidelines::Style>,
    },
    Group(Vec<Inverse>),
    Settings {
        id: Uuid,
        before: SceneSettings,
        after: SceneSettings,
    },
    Move {
        id: Uuid,
        index: usize,
        after: Vec<Uuid>,
    },
    Restore {
        scene: Scene,
        index: usize,
        after: Vec<Uuid>,
    },
    RemoveAdded {
        id: Uuid,
        initial: SceneSettings,
    },
}

pub(super) fn order(project: &Project) -> Vec<Uuid> {
    project.scenes.iter().map(|scene| scene.id).collect()
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UndoTarget {
    token: Uuid,
    scene_id: Option<Uuid>,
    removes_scene: bool,
}

#[derive(Default)]
pub(super) struct History(VecDeque<(UndoTarget, Inverse)>);

impl Inverse {
    fn scene_id(&self) -> Option<Uuid> {
        match self {
            Inverse::Guidelines { .. } => None,
            Inverse::Group(entries) => entries.iter().find_map(Inverse::scene_id),
            Inverse::Settings { id, .. }
            | Inverse::Move { id, .. }
            | Inverse::RemoveAdded { id, .. } => Some(*id),
            Inverse::Restore { scene, .. } => Some(scene.id),
        }
    }
}

impl History {
    pub fn push(&mut self, inverse: Inverse) {
        let scene_id = inverse.scene_id();
        if self.0.len() == LIMIT {
            self.0.pop_front();
        }
        let removes_scene = matches!(inverse, Inverse::RemoveAdded { .. });
        self.0.push_back((
            UndoTarget {
                token: Uuid::new_v4(),
                scene_id,
                removes_scene,
            },
            inverse,
        ));
    }

    pub fn target(&self) -> Option<UndoTarget> {
        self.0.back().map(|entry| entry.0)
    }

    pub fn apply(&self, token: Uuid, project: &mut Project) -> Result<(), String> {
        let (_, inverse) = self
            .0
            .back()
            .filter(|entry| entry.0.token == token)
            .ok_or(CONFLICT)?;
        Self::apply_inverse(inverse, project)
    }

    fn apply_inverse(inverse: &Inverse, project: &mut Project) -> Result<(), String> {
        match inverse {
            Inverse::Guidelines {
                before,
                after,
                previous_selection,
            } => {
                if project.guideline_styles != *after {
                    return Err(CONFLICT.into());
                }
                project.guideline_styles.clone_from(before);
                project.guidelines_revision += 1;
                if project
                    .selected_style_id
                    .is_some_and(|id| !before.iter().any(|s| s.id == id))
                {
                    project.selected_style_id = *previous_selection;
                }
            }
            Inverse::Group(entries) => {
                for entry in entries.iter().rev() {
                    Self::apply_inverse(entry, project)?;
                }
            }
            Inverse::Settings { id, before, after } => {
                let index = project.scene_index(*id)?;
                let scene = &mut project.scenes[index];
                if scene.settings != *after {
                    return Err(CONFLICT.into());
                }
                // The snapshot came from disk. Preserve even legacy trim/offset layouts exactly.
                scene.settings = before.clone();
                scene.revision += 1;
            }
            Inverse::Move { id, index, after } => {
                if order(project) != *after {
                    return Err(CONFLICT.into());
                }
                let current = project.scene_index(*id)?;
                let scene = project.scenes.remove(current);
                project.scenes.insert(*index, scene);
            }
            Inverse::Restore {
                scene,
                index,
                after,
            } => {
                if order(project) != *after {
                    return Err(CONFLICT.into());
                }
                let mut restored = scene.clone();
                restored.revision += 1;
                project.scenes.insert(*index, restored);
            }
            Inverse::RemoveAdded { id, initial } => {
                let index = project.scene_index(*id)?;
                if project.scenes[index].settings != *initial {
                    return Err(CONFLICT.into());
                }
                project.scenes.remove(index);
            }
        }
        Ok(())
    }

    pub fn pop_committed(&mut self) {
        self.0.pop_back();
    }
}
