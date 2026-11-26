use super::super::encoder_probe;
use std::{path::Path, process::Command};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub(in crate::project) enum Encoder {
    Nvenc,
    Qsv,
    Amf,
    Cpu,
}

impl Encoder {
    pub(super) fn detect(ffmpeg: &Path) -> Self {
        [Self::Nvenc, Self::Qsv, Self::Amf]
            .into_iter()
            .find(|encoder| encoder.can_encode(ffmpeg))
            .unwrap_or(Self::Cpu)
    }

    // Part of the segment cache key. Revise the ID when its output settings change.
    pub(super) fn cache_id(&self) -> &'static str {
        match self {
            Self::Nvenc => "h264-nvenc-p5-cq22-v1",
            Self::Qsv => "h264-qsv-medium-icq22-v1",
            Self::Amf => "h264-amf-balanced-cqp-v1",
            Self::Cpu => "h264-x264-fast-crf19-v1",
        }
    }

    pub(super) fn add_output_args(&self, cmd: &mut Command) {
        cmd.args(self.output_args());
    }

    fn output_args(self) -> &'static [&'static str] {
        match self {
            Self::Nvenc => &[
                "-c:v",
                "h264_nvenc",
                "-preset",
                "p5",
                "-tune",
                "hq",
                "-rc",
                "vbr",
                "-cq",
                "22",
                "-b:v",
                "0",
                "-pix_fmt",
                "yuv420p",
            ],
            Self::Qsv => &[
                "-c:v",
                "h264_qsv",
                "-preset",
                "medium",
                "-global_quality",
                "22",
                "-pix_fmt",
                "nv12",
            ],
            Self::Amf => &[
                "-c:v",
                "h264_amf",
                "-usage",
                "transcoding",
                "-quality",
                "balanced",
                "-rc",
                "cqp",
                "-qp_i",
                "20",
                "-qp_p",
                "22",
                "-qp_b",
                "24",
                "-pix_fmt",
                "yuv420p",
            ],
            Self::Cpu => &[
                "-c:v", "libx264", "-preset", "fast", "-crf", "19", "-pix_fmt", "yuv420p",
                "-threads", "4",
            ],
        }
    }

    fn can_encode(self, ffmpeg: &Path) -> bool {
        encoder_probe::supports(ffmpeg, "1280x720", self.output_args())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_working_hardware_probe_selects_software() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(
            Encoder::detect(&temp.path().join("unavailable-ffmpeg")),
            Encoder::Cpu
        );
    }

    #[test]
    #[ignore = "Requires CUTROOM_TEST_TOOLS with FFmpeg"]
    fn selected_encoder_and_software_can_both_encode_export_frames() {
        let ffmpeg = std::path::PathBuf::from(std::env::var_os("CUTROOM_TEST_TOOLS").unwrap())
            .join(format!("ffmpeg{}", std::env::consts::EXE_SUFFIX));
        let selected = Encoder::detect(&ffmpeg);
        eprintln!("MP4 export encoder: {selected:?}");
        assert!(selected.can_encode(&ffmpeg), "{selected:?}");
        assert!(Encoder::Cpu.can_encode(&ffmpeg));
    }
}
