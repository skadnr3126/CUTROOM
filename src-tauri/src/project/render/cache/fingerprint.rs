use super::super::{Project, SceneSettings, SceneStyle};
use super::content::{Stamp, content_hash, input_file, stamp};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    env,
    fs::{self, File},
    io,
    path::{Path, PathBuf},
};

// Bump when changing the meaning of rendered pixels, audio, or stream parameters.
const RECIPE: &str = concat!("cutroom-h264-parts/1/", env!("CARGO_PKG_VERSION"));

#[derive(Default)]
pub(super) struct Dependencies(Vec<(PathBuf, Stamp)>);

impl Dependencies {
    // An external edit while FFmpeg is working must not publish the result under
    // the old content key. Reopen by path to detect a replacement as well.
    pub(super) fn current(&self) -> bool {
        self.0.iter().all(|(path, before)| {
            input_file(path)
                .and_then(|f| stamp(&f))
                .is_ok_and(|after| after == *before)
        })
    }
}

pub(super) struct Inputs {
    tool: [u8; 32],
    font: Option<[u8; 32]>,
    installed_fonts: Option<[u8; 32]>,
}

impl Inputs {
    pub(super) fn new(ffmpeg: &Path, staged_font: Option<&Path>) -> io::Result<Self> {
        let digest = |path: &Path| content_hash(path).map(|(_, hash)| hash);
        Ok(Self {
            tool: digest(ffmpeg)?,
            font: staged_font.map(digest).transpose()?,
            installed_fonts: None,
        })
    }

    pub(super) fn key(
        &mut self,
        project: &Project,
        media: &Path,
        piece: &SceneSettings,
        style: SceneStyle<'_>,
        ass: Option<(&str, u64)>,
        encoder: &str,
    ) -> io::Result<(String, Dependencies)> {
        let needs_system_fonts = ass.is_some()
            || (self.font.is_none()
                && (!piece.caption.is_empty()
                    || !piece.caption_cues.is_empty()
                    || !style.side_text.is_empty()
                    || !style.caption_changes.is_empty()));
        let font_library = if needs_system_fonts {
            if self.installed_fonts.is_none() {
                self.installed_fonts = Some(font_inventory()?);
            }
            self.installed_fonts
        } else {
            None
        };
        let mut assets = BTreeMap::new();
        let mut deps = Dependencies::default();
        let referenced = piece
            .visual
            .iter()
            .map(|v| v.source.asset_id)
            .chain(piece.audio.iter().map(|a| a.asset_id))
            .chain(piece.overlays.iter().map(|o| o.asset_id));
        for id in referenced {
            if assets.contains_key(&id) {
                continue;
            }
            let asset = project.asset(id).map_err(io::Error::other)?;
            let path = asset.path_in(media);
            let (before, digest) = content_hash(&path)?;
            assets.insert(id, (asset.kind, asset.duration_ms, asset.has_audio, digest));
            deps.0.push((path, before));
        }
        // Only render inputs belong here. A timeline reorder, BGM change or a
        // renamed section should reuse the exact same pre-mix MP4.
        let recipe = (
            RECIPE,
            self.tool,
            self.font,
            font_library,
            encoder,
            style,
            (
                piece.duration_ms,
                &piece.visual,
                &piece.audio,
                &piece.overlays,
            ),
            (&piece.caption, &piece.caption_cues, ass),
            assets,
        );
        let bytes = serde_json::to_vec(&recipe)?;
        let key = Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        Ok((key, deps))
    }
}

// Libass resolves ASS typefaces and fallback glyphs against this computer's
// font collection. A font install, removal or in-place replacement invalidates
// captioned parts, without rereading every system font for every scene.
fn font_inventory() -> io::Result<[u8; 32]> {
    #[cfg(windows)]
    let mut dirs: Vec<_> = [
        env::var_os("WINDIR").map(|p| PathBuf::from(p).join("Fonts")),
        env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Microsoft/Windows/Fonts")),
    ]
    .into_iter()
    .flatten()
    .collect();
    #[cfg(unix)]
    let mut dirs = {
        let mut roots = vec![
            PathBuf::from("/usr/share/fonts"),
            PathBuf::from("/usr/local/share/fonts"),
            PathBuf::from("/System/Library/Fonts"),
            PathBuf::from("/Library/Fonts"),
        ];
        if let Some(home) = env::var_os("HOME") {
            let home = PathBuf::from(home);
            roots.extend([
                home.join(".fonts"),
                home.join(".local/share/fonts"),
                home.join("Library/Fonts"),
            ]);
        }
        if let Some(data) = env::var_os("XDG_DATA_HOME") {
            roots.push(PathBuf::from(data).join("fonts"));
        }
        roots
    };
    let mut files = BTreeMap::new();
    let mut visited = HashSet::new();
    while let Some(dir) = dirs.pop() {
        let canonical = match fs::canonicalize(&dir) {
            Ok(path) => path,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        if !visited.insert(canonical) {
            continue;
        }
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                dirs.push(path);
            } else if path
                .extension()
                .and_then(|v| v.to_str())
                .is_some_and(|ext| {
                    ["ttf", "ttc", "otf", "otc", "fon", "dfont", "pfa", "pfb"]
                        .iter()
                        .any(|e| ext.eq_ignore_ascii_case(e))
                })
            {
                files.insert(path.clone(), stamp(&File::open(path)?)?);
            }
        }
    }
    Ok(Sha256::digest(serde_json::to_vec(&files)?).into())
}
