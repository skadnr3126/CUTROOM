use serde::Deserialize;
use tempfile::tempdir;
use uuid::Uuid;

use super::{
    ProjectStore, ProjectView,
    composition_layout::{CaptionChange, Layout, VisualKeyframe, validate_motion},
    files::{find_tool, inspect_asset},
    model::{Kind, Placement, Project, Scene, SceneSettings, VisualPlacement},
    render,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SourceRecipe {
    folder_id: Option<Uuid>,
    job_id: Uuid,
    asset_ids: Vec<Uuid>,
    name: String,
    clips: Vec<SourceClip>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SourceClip {
    duration_ms: u64,
    visual_asset_id: Option<Uuid>,
    visual_in_ms: u64,
    audio_asset_id: Option<Uuid>,
    audio_in_ms: u64,
    caption: String,
    #[serde(default)]
    photo_motion: render::PhotoMotion,
    #[serde(default)]
    layout: Layout,
    #[serde(default)]
    visual_keyframes: Vec<VisualKeyframe>,
    #[serde(default)]
    side_text: String,
    #[serde(default)]
    caption_changes: Vec<CaptionChange>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RenderedSource {
    pub(super) view: ProjectView,
    pub(super) asset_id: Uuid,
}

impl SourceRecipe {
    fn composition(&self, project: &Project) -> Result<Project, String> {
        if self.clips.is_empty() {
            return Err("영상으로 만들 컷이 필요합니다.".to_owned());
        }
        let placement = |id: Option<Uuid>, in_ms| -> Result<_, String> {
            let Some(asset_id) = id else { return Ok(None) };
            if !self.asset_ids.contains(&asset_id) {
                return Err("편집안에는 첨부한 소스만 사용할 수 있습니다.".to_owned());
            }
            project.asset(asset_id)?;
            Ok(Some(Placement { asset_id, in_ms }))
        };
        let mut has_content = false;
        let mut scenes = Vec::with_capacity(self.clips.len());
        for (index, clip) in self.clips.iter().enumerate() {
            let visual = placement(clip.visual_asset_id, clip.visual_in_ms)?;
            let audio = placement(clip.audio_asset_id, clip.audio_in_ms)?;
            validate_motion(&clip.visual_keyframes, clip.duration_ms)?;
            if ((clip.layout != Layout::Full || !clip.visual_keyframes.is_empty())
                && visual.is_none())
                || (clip.layout == Layout::Full && !clip.side_text.trim().is_empty())
            {
                return Err(format!(
                    "{}번째 컷의 좌우 배치와 시각 소스를 확인해 주세요.",
                    index + 1
                ));
            }
            let mut previous = 0;
            for change in &clip.caption_changes {
                if change.start_ms <= previous || change.start_ms >= clip.duration_ms {
                    return Err(format!(
                        "{}번째 컷의 자막 변경 시각을 순서대로 지정해 주세요.",
                        index + 1
                    ));
                }
                previous = change.start_ms;
            }
            has_content |= visual.is_some()
                || audio.is_some()
                || (self.asset_ids.is_empty()
                    && (!clip.caption.trim().is_empty()
                        || clip
                            .caption_changes
                            .iter()
                            .any(|c| !c.text.trim().is_empty())));
            if clip.photo_motion != render::PhotoMotion::None
                && !visual.is_some_and(|source| {
                    project
                        .asset(source.asset_id)
                        .is_ok_and(|asset| asset.kind == Kind::Image)
                })
            {
                return Err(format!(
                    "{}번째 컷의 확대·축소에는 사진이 필요합니다.",
                    index + 1
                ));
            }
            let settings = SceneSettings {
                name: String::new(),
                notes: String::new(),
                clips: Vec::new(),
                overlays: Vec::new(),
                text: String::new(),
                caption: clip.caption.clone(),
                caption_ass: None,
                caption_cues: Vec::new(),
                duration_ms: clip.duration_ms,
                visual: visual.map(|source| VisualPlacement {
                    frame: None,
                    muted: false,
                    source,
                    trim: None,
                }),
                audio,
            };
            project.validate_settings(&settings).map_err(|reason| {
                format!("편집안의 {}번째 컷을 확인해 주세요. {reason}", index + 1)
            })?;
            if let Some(source) = visual {
                let asset = project.asset(source.asset_id)?;
                if asset.kind == Kind::Video
                    && asset.duration_ms.is_some_and(|duration| {
                        source.in_ms.saturating_add(clip.duration_ms) > duration
                    })
                {
                    return Err(format!(
                        "편집안의 {}번째 컷이 영상 소스의 길이를 넘습니다.",
                        index + 1
                    ));
                }
            }
            scenes.push(Scene {
                id: Uuid::new_v4(),
                revision: 0,
                settings,
            });
        }
        if !has_content {
            return Err(
                "첨부한 소스를 사용하거나 대본만 보낼 때는 화면에 표시할 문구를 넣어 주세요."
                    .to_owned(),
            );
        }
        Ok(Project {
            bgm: Vec::new(),
            media_tree: project.media_tree,
            media_transaction: None,
            folders: project.folders.clone(),
            timeline_jobs: Vec::new(),
            title: self.name.clone(),
            legacy_guidelines: String::new(),
            guideline_styles: project.guideline_styles.clone(),
            selected_style_id: project.selected_style_id,
            guidelines_revision: project.guidelines_revision,
            scenes,
            assets: project.assets.clone(),
        })
    }

    fn file_name(&self) -> String {
        let raw = self.name.trim();
        let stem = raw.strip_suffix(".mp4").unwrap_or(raw);
        let stem = stem
            .chars()
            .filter(|c| !c.is_control() && !"/\\:*?\"<>|".contains(*c))
            .take(110)
            .collect::<String>();
        let stem = stem.trim_matches(|c: char| c.is_whitespace() || c == '.');
        format!(
            "{}.mp4",
            if stem.is_empty() {
                "AI 편집본"
            } else {
                stem
            }
        )
    }
}

impl ProjectStore {
    pub(super) fn render_source(&self, recipe: SourceRecipe) -> Result<RenderedSource, String> {
        let project = self.load()?;
        if let Some(asset) = project
            .assets
            .iter()
            .find(|asset| asset.source_job_id == Some(recipe.job_id))
        {
            let asset_id = asset.id;
            return Ok(RenderedSource {
                view: self.view(project),
                asset_id,
            });
        }
        let composition = recipe.composition(&project)?;
        let ffmpeg =
            find_tool(&self.app_data, "ffmpeg").ok_or("편집본을 만들려면 FFmpeg가 필요합니다.")?;
        let ffprobe = find_tool(&self.app_data, "ffprobe");
        let staging = tempdir().map_err(|_| "편집본을 만들 작업 공간을 열 수 없습니다.")?;
        let output = staging.path().join("source.mp4");
        let styles: Vec<_> = recipe
            .clips
            .iter()
            .map(|clip| render::SceneStyle {
                motion: clip.photo_motion,
                layout: clip.layout,
                keyframes: &clip.visual_keyframes,
                side_text: &clip.side_text,
                caption_changes: &clip.caption_changes,
                center_text: clip.visual_asset_id.is_none() && recipe.asset_ids.is_empty(),
            })
            .collect();
        render::export_styled(&ffmpeg, &self.media_dir(), &composition, &output, &styles)?;
        let mut asset_id = Uuid::nil();
        let view = self.mutate(|project, media, added| {
            if let Some(asset) = project
                .assets
                .iter()
                .find(|asset| asset.source_job_id == Some(recipe.job_id))
            {
                asset_id = asset.id;
                return Ok(());
            }
            let mut asset = inspect_asset(&output, &recipe.file_name(), ffprobe.as_deref())?;
            asset.folder_id = recipe.folder_id;
            asset.source_job_id = Some(recipe.job_id);
            asset_id = super::generated::publish(
                project,
                media,
                added,
                asset,
                super::generated::Input::File(&output),
            )?;
            Ok(())
        })?;
        Ok(RenderedSource { view, asset_id })
    }
}
