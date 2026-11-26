use super::clip_volume::ClipVolume;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub(super) const MIN_DURATION_MS: u64 = 100;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Project {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bgm: Vec<super::bgm::Clip>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub media_tree: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_transaction: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub folders: Vec<super::folders::SourceFolder>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timeline_jobs: Vec<Uuid>,
    pub title: String,
    #[serde(default, rename = "productionGuidelines", skip_serializing)]
    pub legacy_guidelines: String,
    #[serde(default = "super::guidelines::initial_styles")]
    pub guideline_styles: Vec<super::guidelines::Style>,
    #[serde(default = "super::guidelines::default_selection")]
    pub selected_style_id: Option<Uuid>,
    #[serde(default)]
    pub guidelines_revision: u32,
    pub scenes: Vec<Scene>,
    pub assets: Vec<Asset>,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            bgm: Vec::new(),
            media_tree: false,
            media_transaction: None,
            folders: Vec::new(),
            timeline_jobs: Vec::new(),
            title: "새 영상 프로젝트".to_owned(),
            legacy_guidelines: String::new(),
            guideline_styles: super::guidelines::initial_styles(),
            selected_style_id: super::guidelines::default_selection(),
            guidelines_revision: 0,
            scenes: Vec::new(),
            assets: Vec::new(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Scene {
    pub id: Uuid,
    pub revision: u32,
    #[serde(flatten)]
    pub settings: SceneSettings,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SceneSettings {
    pub text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub caption: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption_ass: Option<super::ass::AssTrack>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caption_cues: Vec<super::caption_track::CaptionCue>,
    pub duration_ms: u64,
    pub visual: Option<VisualPlacement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clips: Vec<SceneClip>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overlays: Vec<Overlay>,
    pub audio: Option<Placement>,
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SceneClip {
    pub id: Uuid,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub gap_ms: u64,
    pub duration_ms: u64,
    #[serde(flatten)]
    pub visual: VisualPlacement,
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Overlay {
    #[serde(default, skip_serializing_if = "ClipVolume::is_original")]
    pub volume: ClipVolume,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track: Option<u32>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sound: bool,
    pub id: Uuid,
    pub asset_id: Uuid,
    pub start_ms: u64,
    pub in_ms: u64,
    pub duration_ms: u64,
    // Size is a percentage of the canvas. Position is a percentage of remaining space.
    pub size: u8,
    pub x: u8,
    pub y: u8,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

pub(super) fn scene_name(input: &str) -> Result<String, String> {
    let name = input.trim();
    if name.chars().any(char::is_control) {
        return Err("구간 이름을 한 줄로 입력해 주세요.".into());
    }
    Ok(name.to_owned())
}

impl SceneSettings {
    fn sequence_duration(&self) -> Option<u64> {
        self.clips.iter().try_fold(0_u64, |end, clip| {
            end.checked_add(clip.gap_ms)?.checked_add(clip.duration_ms)
        })
    }

    fn overlay_end(&self) -> u64 {
        self.overlays
            .iter()
            .map(|o| o.start_ms.saturating_add(o.duration_ms))
            .max()
            .unwrap_or(0)
    }

    pub fn segments(&self) -> Vec<Self> {
        if self.clips.is_empty() {
            return vec![self.clone()];
        }
        let mut elapsed = 0;
        let mut pieces = Vec::new();
        let mut push = |visual, duration_ms: u64, start: u64| {
            let overlays = self
                .overlays
                .iter()
                .filter_map(|o| {
                    let begin = start.max(o.start_ms);
                    let end = (start + duration_ms).min(o.start_ms + o.duration_ms);
                    if begin >= end {
                        return None;
                    }
                    Some(Overlay {
                        start_ms: begin - start,
                        duration_ms: end - begin,
                        in_ms: o.in_ms + (begin - o.start_ms),
                        ..*o
                    })
                })
                .collect();
            pieces.push(Self {
                visual,
                duration_ms,
                overlays,
                caption: self.caption.clone(),
                caption_ass: None, // ASS uses the full scene clock during segmented export.
                caption_cues: super::caption_track::window(&self.caption_cues, start, duration_ms),
                name: String::new(),
                notes: String::new(),
                text: String::new(),
                clips: Vec::new(),
                audio: self.audio.map(|a| Placement {
                    in_ms: a.in_ms.saturating_add(start),
                    ..a
                }),
            });
        };
        for clip in &self.clips {
            if clip.gap_ms > 0 {
                push(None, clip.gap_ms, elapsed);
                elapsed += clip.gap_ms;
            }
            push(Some(clip.visual), clip.duration_ms, elapsed);
            elapsed += clip.duration_ms;
        }
        if elapsed < self.duration_ms {
            push(None, self.duration_ms - elapsed, elapsed);
        }
        pieces
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Placement {
    pub asset_id: Uuid,
    pub in_ms: u64,
    #[serde(default, skip_serializing_if = "ClipVolume::is_original")]
    pub volume: ClipVolume,
}

impl Placement {
    pub(super) fn new(asset_id: Uuid, in_ms: u64) -> Self {
        Self {
            asset_id,
            in_ms,
            volume: ClipVolume::default(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VisualPlacement {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<ClipFrame>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub muted: bool,
    #[serde(flatten)]
    pub source: Placement,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trim: Option<VideoTrim>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ClipFrame {
    pub size: u8,
    pub x: u8,
    pub y: u8,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VideoTrim {
    pub offset_ms: u64,
    pub out_ms: u64,
}

impl VisualPlacement {
    pub fn trimmed_window(self, scene_ms: u64) -> Option<(u64, u64)> {
        self.trim.map(|trim| {
            let available = scene_ms.saturating_sub(trim.offset_ms);
            let duration = trim.out_ms.saturating_sub(self.source.in_ms).min(available);
            (trim.offset_ms, duration)
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Asset {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk: Option<super::media_tree::DiskStamp>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub missing: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<Uuid>,
    pub id: Uuid,
    pub name: String,
    pub extension: String,
    pub kind: Kind,
    pub duration_ms: Option<u64>,
    pub has_audio: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_job_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub removed_from_bin: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Kind {
    Video,
    Image,
    Audio,
}

impl Kind {
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension {
            "mp4" | "mov" | "webm" | "m4v" => Some(Self::Video),
            "png" | "jpg" | "jpeg" | "webp" => Some(Self::Image),
            "wav" | "mp3" | "m4a" | "aac" | "ogg" | "flac" => Some(Self::Audio),
            _ => None,
        }
    }
}

impl Asset {
    pub fn project_relative_file(&self) -> std::borrow::Cow<'_, str> {
        self.project_file
            .as_deref()
            .map(std::borrow::Cow::Borrowed)
            .unwrap_or_else(|| format!("media/{}.{}", self.id, self.extension).into())
    }

    pub fn path_in(&self, media: &std::path::Path) -> std::path::PathBuf {
        if self.missing {
            return media
                .parent()
                .unwrap_or(media)
                .join(".cutroom-offline")
                .join(self.id.to_string());
        }
        self.expected_path(media)
    }
    pub fn expected_path(&self, media: &std::path::Path) -> std::path::PathBuf {
        match &self.project_file {
            Some(relative) => media.parent().unwrap_or(media).join(relative),
            None => media.join(format!("{}.{}", self.id, self.extension)),
        }
    }
}

impl Project {
    pub fn add_scene(&mut self, text: String) {
        let duration_ms = (text.chars().count().div_ceil(7) as u64).clamp(4, 60) * 1_000;
        self.scenes.push(Scene {
            id: Uuid::new_v4(),
            revision: 0,
            settings: SceneSettings {
                text,
                name: String::new(),
                notes: String::new(),
                caption: String::new(),
                caption_ass: None,
                caption_cues: Vec::new(),
                duration_ms,
                visual: None,
                clips: Vec::new(),
                overlays: Vec::new(),
                audio: None,
            },
        });
    }

    pub fn scene_index(&self, id: Uuid) -> Result<usize, String> {
        self.scenes
            .iter()
            .position(|scene| scene.id == id)
            .ok_or_else(|| "대본 구간을 찾을 수 없습니다.".to_owned())
    }

    pub fn asset(&self, id: Uuid) -> Result<&Asset, String> {
        self.assets
            .iter()
            .find(|asset| asset.id == id)
            .ok_or_else(|| "프로젝트에서 연결할 소스를 찾을 수 없습니다.".to_owned())
    }

    pub fn edit_scene(
        &mut self,
        id: Uuid,
        expected: u32,
        mut settings: SceneSettings,
    ) -> Result<(), String> {
        let index = self.scene_index(id)?;
        if self.scenes[index].revision != expected {
            return Err(
                "요청을 보낸 뒤 이 구간이 변경되었습니다. 최신 구간을 기준으로 다시 작업해 주세요."
                    .to_owned(),
            );
        }
        let previous = &self.scenes[index].settings;
        if previous.name != settings.name {
            settings.name = scene_name(&settings.name)?;
        }
        if !settings.clips.is_empty() {
            settings.duration_ms = settings
                .sequence_duration()
                .ok_or("클립 길이 합계를 확인해 주세요.")?;
        }
        if (previous.visual != settings.visual || previous.duration_ms != settings.duration_ms)
            && let Some(clip) = &mut settings.visual
        {
            let asset = self.asset(clip.source.asset_id)?;
            if asset.kind == Kind::Video {
                let out_ms = clip
                    .trim
                    .map(|trim| trim.out_ms)
                    .or(asset.duration_ms)
                    .unwrap_or_else(|| clip.source.in_ms.saturating_add(settings.duration_ms));
                clip.trim = Some(VideoTrim {
                    offset_ms: 0,
                    out_ms,
                });
                settings.duration_ms = out_ms.saturating_sub(clip.source.in_ms);
            }
        }
        if settings.overlays.iter().any(|c| c.track.is_some()) {
            for (index, clip) in settings.overlays.iter_mut().enumerate() {
                clip.track.get_or_insert(index as u32 + 1);
            }
            settings.overlays.sort_by_key(|c| (c.track, c.start_ms));
        }
        settings.duration_ms = settings.duration_ms.max(settings.overlay_end());
        if settings.duration_ms < previous.duration_ms
            && settings.caption_cues == previous.caption_cues
        {
            settings.caption_cues =
                super::caption_track::window(&settings.caption_cues, 0, settings.duration_ms);
        }
        self.validate_settings(&settings)?;
        let scene = &mut self.scenes[index];
        scene.settings = settings;
        scene.revision += 1;
        Ok(())
    }

    pub fn validate_settings(&self, settings: &SceneSettings) -> Result<(), String> {
        if let Some(track) = &settings.caption_ass {
            track.validate()?;
            if !settings.caption.is_empty() || !settings.caption_cues.is_empty() {
                return Err("ASS 자막과 기본 자막이 겹칩니다.".into());
            }
        }
        super::caption_track::validate(&settings.caption_cues, settings.duration_ms)?;
        if !settings.caption_cues.is_empty() && !settings.caption.is_empty() {
            return Err("구간 자막과 시간 지정 자막을 동시에 저장할 수 없습니다.".into());
        }
        let mut layer_ids = std::collections::HashSet::new();
        let mut track_ends = std::collections::HashMap::new();
        for (index, layer) in settings.overlays.iter().enumerate() {
            let track = layer.track.unwrap_or(index as u32 + 1);
            let previous_end =
                track_ends.insert(track, layer.start_ms.saturating_add(layer.duration_ms));
            if track == 0 || previous_end.is_some_and(|end| end > layer.start_ms) {
                return Err("같은 트랙에서 두 클립의 시간이 겹칩니다.".into());
            }
            let asset = self.asset(layer.asset_id)?;
            let min = asset
                .duration_ms
                .unwrap_or(MIN_DURATION_MS)
                .clamp(1, MIN_DURATION_MS);
            if !layer_ids.insert(layer.id)
                || asset.kind == Kind::Audio
                || !(10..=100).contains(&layer.size)
                || layer.x > 100
                || layer.y > 100
                || layer.duration_ms < min
                || layer.start_ms.saturating_add(layer.duration_ms) > settings.duration_ms
                || (asset.kind == Kind::Image && layer.in_ms != 0)
                || (asset.kind == Kind::Video
                    && asset
                        .duration_ms
                        .is_some_and(|d| layer.in_ms.saturating_add(layer.duration_ms) > d))
            {
                return Err("겹침 레이어의 소스, 시간 또는 위치를 확인해 주세요.".into());
            }
        }
        if !settings.clips.is_empty() {
            let mut ids = std::collections::HashSet::new();
            let total = settings.sequence_duration();
            if settings.visual.is_some()
                || total.map(|ms| ms.max(settings.overlay_end())) != Some(settings.duration_ms)
                || settings.duration_ms > 86_400_000
            {
                return Err("구간과 클립 길이를 확인해 주세요.".into());
            }
            for clip in &settings.clips {
                if layer_ids.contains(&clip.id) || !ids.insert(clip.id) {
                    return Err("클립 ID가 중복되었습니다.".into());
                }
                self.validate_settings(&SceneSettings {
                    duration_ms: clip.duration_ms,
                    visual: Some(clip.visual),
                    audio: settings.audio,
                    name: String::new(),
                    notes: String::new(),
                    text: String::new(),
                    caption: String::new(),
                    caption_ass: None,
                    caption_cues: Vec::new(),
                    clips: Vec::new(),
                    overlays: Vec::new(),
                })?;
            }
            return Ok(());
        }
        if settings
            .visual
            .and_then(|v| v.frame)
            .is_some_and(|f| !(10..=100).contains(&f.size) || f.x > 100 || f.y > 100)
        {
            return Err("클립 크기와 위치를 확인해 주세요.".into());
        }
        let min_duration = settings
            .visual
            .and_then(|clip| self.asset(clip.source.asset_id).ok())
            .filter(|asset| asset.kind == Kind::Video)
            .and_then(|asset| asset.duration_ms)
            .map_or(MIN_DURATION_MS, |source_ms| {
                source_ms.clamp(1, MIN_DURATION_MS)
            });
        if !(min_duration..=86_400_000).contains(&settings.duration_ms) {
            return Err("구간 길이는 0.1초에서 24시간 사이여야 합니다.".to_owned());
        }
        for (placement, audio_track) in [
            (settings.visual.map(|clip| clip.source), false),
            (settings.audio, true),
        ] {
            let Some(placement) = placement else { continue };
            let asset = self.asset(placement.asset_id)?;
            if (asset.kind == Kind::Audio) != audio_track {
                return Err(
                    "영상 트랙에는 영상·이미지, 음성 트랙에는 오디오를 넣어 주세요.".to_owned(),
                );
            }
            if (asset.kind == Kind::Image && placement.in_ms != 0)
                || asset
                    .duration_ms
                    .is_some_and(|duration| placement.in_ms >= duration)
            {
                return Err("시작 위치는 소스 재생 시간 안에 있어야 합니다.".to_owned());
            }
            if !audio_track && let Some(trim) = settings.visual.and_then(|clip| clip.trim) {
                if asset.kind != Kind::Video {
                    return Err("영상 클립에서 시작점과 끝점을 조절할 수 있습니다.".to_owned());
                }
                if trim.out_ms <= placement.in_ms
                    || asset
                        .duration_ms
                        .is_some_and(|duration| trim.out_ms > duration)
                {
                    return Err(
                        "끝점은 시작점보다 뒤에 있고 소스 재생 시간 안에 있어야 합니다.".to_owned(),
                    );
                }
            }
        }
        Ok(())
    }
}
