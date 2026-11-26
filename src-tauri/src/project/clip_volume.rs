use super::model::{Kind, Project, Scene, VisualPlacement};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8")]
pub(crate) struct ClipVolume(u8);

impl ClipVolume {
    pub(super) fn percent(self) -> u8 {
        self.0
    }

    pub(super) fn is_original(&self) -> bool {
        *self == Self::default()
    }
}

impl Default for ClipVolume {
    fn default() -> Self {
        Self(100)
    }
}

impl TryFrom<u8> for ClipVolume {
    type Error = &'static str;

    fn try_from(percent: u8) -> Result<Self, Self::Error> {
        if percent <= 100 {
            Ok(Self(percent))
        } else {
            Err("클립 음량은 0~100% 사이의 정수로 입력해 주세요.")
        }
    }
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum ClipVolumeTarget {
    Clip { clip_id: Uuid },
    SceneAudio {},
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ClipVolumeInfo {
    pub(super) asset_id: Uuid,
    pub(super) volume: ClipVolume,
    pub(super) muted: bool,
}

impl From<VisualPlacement> for ClipVolumeInfo {
    fn from(visual: VisualPlacement) -> Self {
        Self {
            asset_id: visual.source.asset_id,
            volume: visual.source.volume,
            muted: visual.muted,
        }
    }
}

enum Slot {
    LegacyVisual,
    Clip(usize),
    Overlay(usize),
    SceneAudio,
}

impl ClipVolumeTarget {
    fn resolve(&self, project: &Project, scene: &Scene) -> Result<(Slot, ClipVolumeInfo), String> {
        let settings = &scene.settings;
        let (slot, info) = match *self {
            Self::SceneAudio {} => {
                let audio = settings
                    .audio
                    .ok_or("이 구간에 연결된 오디오가 없습니다.")?;
                (
                    Slot::SceneAudio,
                    ClipVolumeInfo {
                        asset_id: audio.asset_id,
                        volume: audio.volume,
                        muted: false,
                    },
                )
            }
            Self::Clip { clip_id } => {
                if clip_id == scene.id
                    && settings.clips.is_empty()
                    && let Some(visual) = settings.visual
                {
                    (Slot::LegacyVisual, visual.into())
                } else if let Some((i, clip)) = settings
                    .clips
                    .iter()
                    .enumerate()
                    .find(|(_, clip)| clip.id == clip_id)
                {
                    (Slot::Clip(i), clip.visual.into())
                } else if let Some((i, overlay)) = settings
                    .overlays
                    .iter()
                    .enumerate()
                    .find(|(_, overlay)| overlay.id == clip_id)
                {
                    (
                        Slot::Overlay(i),
                        ClipVolumeInfo {
                            asset_id: overlay.asset_id,
                            volume: overlay.volume,
                            muted: !overlay.sound,
                        },
                    )
                } else {
                    return Err("이 구간에서 클립을 찾을 수 없습니다.".into());
                }
            }
        };
        let asset = project.asset(info.asset_id)?;
        if asset.kind != Kind::Audio && !(asset.kind == Kind::Video && asset.has_audio) {
            return Err("원본 소리가 있는 클립만 음량을 조절할 수 있습니다.".into());
        }
        Ok((slot, info))
    }

    pub(super) fn get(&self, project: &Project, scene: &Scene) -> Result<ClipVolumeInfo, String> {
        self.resolve(project, scene).map(|(_, info)| info)
    }

    pub(super) fn set(
        &self,
        project: &mut Project,
        scene_index: usize,
        volume: ClipVolume,
    ) -> Result<bool, String> {
        let (slot, current) = self.resolve(project, &project.scenes[scene_index])?;
        if volume == current.volume {
            return Ok(false);
        }
        let settings = &mut project.scenes[scene_index].settings;
        match slot {
            Slot::LegacyVisual => settings.visual.as_mut().unwrap().source.volume = volume,
            Slot::Clip(i) => settings.clips[i].visual.source.volume = volume,
            Slot::Overlay(i) => settings.overlays[i].volume = volume,
            Slot::SceneAudio => settings.audio.as_mut().unwrap().volume = volume,
        }
        Ok(true)
    }
}
