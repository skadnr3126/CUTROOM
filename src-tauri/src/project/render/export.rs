use super::{
    Encoder, SceneStyle, bgm_mix, cache::PartCache, caption_font, join_segments, run,
    scene_command, seconds,
};
use crate::project::{ProjectStore, files, model::Project};
use std::{fs, ops::Range, path::Path};

pub(in crate::project) struct Export<'a> {
    pub(super) ffmpeg: &'a Path,
    pub(super) media: &'a Path,
    pub(super) project: &'a Project,
    pub(super) styles: &'a [SceneStyle<'a>],
    pub(super) cache_root: Option<&'a Path>,
}

#[derive(Debug, serde::Serialize)]
pub(in crate::project) struct ExportReport {
    pub(super) encoder: Encoder,
    pub(super) rendered: usize,
    pub(super) reused: usize,
}

impl ProjectStore {
    pub(in crate::project) fn export_mp4(
        &self,
        project: &Project,
        destination: &Path,
        range: Range<usize>,
        styles: &[SceneStyle],
    ) -> Result<ExportReport, String> {
        let ffmpeg = files::find_tool(&self.app_data, "ffmpeg")
            .ok_or("MP4 내보내기에 필요한 FFmpeg를 찾을 수 없습니다.")?;
        Export {
            ffmpeg: &ffmpeg,
            media: &self.media_dir(),
            project,
            styles,
            cache_root: Some(&self.app_data.join("mp4-render-cache")),
        }
        .write(destination, range)
    }
}

// Only a failed part encode should restart a GPU export on the CPU. Errors
// opening a project, validating media, mixing BGM or saving a file keep their cause.
enum AttemptError {
    Encode(String),
    Other(String),
}

impl From<String> for AttemptError {
    fn from(reason: String) -> Self {
        Self::Other(reason)
    }
}

fn with_cpu_fallback<T>(
    first: Encoder,
    mut attempt: impl FnMut(Encoder) -> Result<T, AttemptError>,
) -> Result<T, String> {
    match attempt(first) {
        Ok(result) => Ok(result),
        Err(AttemptError::Encode(gpu)) if first != Encoder::Cpu => match attempt(Encoder::Cpu) {
            Ok(result) => Ok(result),
            Err(AttemptError::Encode(cpu)) => {
                Err(format!("GPU 인코딩 오류: {gpu}\nCPU 재시도 오류: {cpu}"))
            }
            Err(AttemptError::Other(reason)) => Err(reason),
        },
        Err(AttemptError::Encode(reason) | AttemptError::Other(reason)) => Err(reason),
    }
}

impl Export<'_> {
    pub(super) fn write(
        &self,
        destination: &Path,
        range: Range<usize>,
    ) -> Result<ExportReport, String> {
        self.write_with_encoder(destination, range, Encoder::detect(self.ffmpeg))
    }

    pub(super) fn write_with_encoder(
        &self,
        destination: &Path,
        range: Range<usize>,
        first: Encoder,
    ) -> Result<ExportReport, String> {
        if range.is_empty() || range.end > self.project.scenes.len() {
            return Err("내보낼 대본 구간을 확인해 주세요.".into());
        }
        if destination.try_exists().map_err(|e| e.to_string())? {
            return Err("같은 이름의 파일이 있습니다. 새 이름으로 내보내 주세요.".into());
        }
        for scene in &self.project.scenes[range.clone()] {
            self.project.validate_settings(&scene.settings)?;
        }
        with_cpu_fallback(first, |encoder| {
            self.attempt(destination, range.clone(), encoder)
        })
    }

    fn attempt(
        &self,
        destination: &Path,
        range: Range<usize>,
        encoder: Encoder,
    ) -> Result<ExportReport, AttemptError> {
        // Each attempt owns its partial files; the final concat always uses one H.264 profile.
        let staging = tempfile::tempdir()
            .map_err(|e| format!("내보내기 작업 폴더를 만들 수 없습니다. {e}"))?;
        let work = staging.path();
        let staged_font = work.join("caption.ttf");
        let font = caption_font().is_some_and(|source| fs::copy(source, &staged_font).is_ok());
        let mut cache = self.cache_root.and_then(|root| {
            PartCache::open(root, self.ffmpeg, font.then_some(staged_font.as_path()))
        });
        let mut report = ExportReport {
            encoder,
            rendered: 0,
            reused: 0,
        };
        let mut concat = String::new();
        let start_ms = self.project.scenes[..range.start]
            .iter()
            .map(|s| s.settings.duration_ms)
            .sum::<u64>();
        let mut end_ms = start_ms;
        for index in range {
            let scene = &self.project.scenes[index].settings;
            let style = self.styles.get(index).copied().unwrap_or_default();
            let ass = scene
                .caption_ass
                .as_ref()
                .map(|track| track.source.as_str());
            if let Some(source) = ass {
                fs::write(work.join("scene.ass"), source).map_err(|e| e.to_string())?;
            }
            let mut elapsed = 0;
            for (piece, settings) in scene.segments().iter().enumerate() {
                let file = format!("part-{index:05}-{piece}.mp4");
                let local = work.join(&file);
                let entry = cache.as_mut().and_then(|cache| {
                    cache
                        .key(
                            self.project,
                            self.media,
                            settings,
                            style,
                            ass.map(|s| (s, elapsed)),
                            encoder.cache_id(),
                        )
                        .map(|key| (cache, key))
                });
                if entry
                    .as_ref()
                    .is_some_and(|(cache, key)| cache.restore(key, &local))
                {
                    report.reused += 1;
                } else {
                    let mut command = scene_command(
                        self.ffmpeg,
                        work,
                        self.media,
                        self.project,
                        settings,
                        font,
                        style,
                        &file,
                        ass.map(|_| elapsed),
                        encoder,
                    )?;
                    run(&mut command).map_err(|e| {
                        AttemptError::Encode(format!(
                            "구간 {}을 렌더링할 수 없습니다. {e}",
                            index + 1
                        ))
                    })?;
                    if let Some((cache, key)) = entry {
                        cache.publish(&key, &local);
                    }
                    report.rendered += 1;
                }
                elapsed += settings.duration_ms;
                concat.push_str(&format!("file '{file}'\n"));
                if !self.project.bgm.is_empty() {
                    // Keep BGM anchors on the editor clock even at rounded AAC/video boundaries.
                    concat.push_str(&format!("duration {}\n", seconds(settings.duration_ms)));
                }
            }
            end_ms += scene.duration_ms;
        }
        fs::write(work.join("segments.txt"), concat).map_err(|e| e.to_string())?;
        join_segments(self.ffmpeg, work, destination, |cmd| {
            bgm_mix::configure(cmd, self.media, self.project, start_ms..end_ms)
        })?;
        if let Some(cache) = cache {
            cache.prune();
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_only_a_failed_gpu_part_and_reports_the_successful_encoder() {
        let mut attempts = Vec::new();
        let saved = with_cpu_fallback(Encoder::Nvenc, |encoder| {
            attempts.push(encoder);
            if encoder == Encoder::Cpu {
                Ok(encoder)
            } else {
                Err(AttemptError::Encode("GPU busy".into()))
            }
        })
        .unwrap();
        assert_eq!(saved, Encoder::Cpu);
        assert_eq!(attempts, [Encoder::Nvenc, Encoder::Cpu]);
        attempts.clear();
        let failure = with_cpu_fallback(Encoder::Nvenc, |encoder| {
            attempts.push(encoder);
            Err::<(), _>(AttemptError::Other("output is full".into()))
        });
        assert_eq!(failure.unwrap_err(), "output is full");
        assert_eq!(attempts, [Encoder::Nvenc]);
    }
}
