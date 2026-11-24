use super::checked;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
};
use uuid::Uuid;

const JOURNAL: &str = ".cutroom-media-transaction";
#[derive(Serialize, Deserialize)]
enum Step {
    Move(String, String),
    Create(String),
    RemoveEmpty(String),
}

pub(in crate::project) struct Journal<'a> {
    root: &'a Path,
    id: Uuid,
    file: Option<File>,
}
impl<'a> Journal<'a> {
    pub fn new(root: &'a Path) -> Self {
        Self {
            root,
            id: Uuid::new_v4(),
            file: None,
        }
    }
    pub fn marker(&self) -> Option<Uuid> {
        self.file.as_ref().map(|_| self.id)
    }
    fn record(&mut self, step: &Step) -> Result<(), String> {
        if self.file.is_none() {
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(self.root.join(JOURNAL))
                .map_err(|e| e.to_string())?;
            writeln!(f, "{}", self.id).map_err(|e| e.to_string())?;
            self.file = Some(f);
        }
        let f = self.file.as_mut().unwrap();
        let line = serde_json::to_string(step).map_err(|e| e.to_string())?;
        writeln!(f, "{line}")
            .and_then(|_| f.sync_data())
            .map_err(|e| e.to_string())
    }
    pub fn mkdir(&mut self, rel: &str) -> Result<(), String> {
        let path = checked(self.root, rel)?;
        if path.is_dir() {
            return Ok(());
        }
        self.record(&Step::Create(rel.into()))?;
        fs::create_dir(path).map_err(|e| format!("폴더 생성 실패 ({rel}): {e}"))
    }
    pub fn relocate(&mut self, from: &str, to: &str) -> Result<(), String> {
        if from == to {
            return Ok(());
        }
        let old = checked(self.root, from)?;
        let new = checked(self.root, to)?;
        if new.try_exists().map_err(|e| e.to_string())? {
            return Err(format!("같은 이름이 이미 있습니다. {to}"));
        }
        if !old.try_exists().map_err(|e| e.to_string())? {
            return Err(format!("파일을 찾을 수 없습니다. {from}"));
        }
        self.record(&Step::Move(from.into(), to.into()))?;
        fs::rename(old, new).map_err(|e| format!("이동 실패 ({from} → {to}): {e}"))
    }
    pub fn remove_empty(&mut self, rel: &str) -> Result<(), String> {
        let path = checked(self.root, rel)?;
        if !path.exists() {
            return Ok(());
        }
        self.record(&Step::RemoveEmpty(rel.into()))?;
        fs::remove_dir(path).map_err(|e| e.to_string())
    }
    pub fn finish(self, committed: Option<Uuid>) -> Result<(), String> {
        let root = self.root;
        let worked = self.file.is_some();
        drop(self);
        if worked {
            recover(root, committed)?;
        }
        Ok(())
    }
}

pub(in crate::project) fn recover(root: &Path, committed: Option<Uuid>) -> Result<(), String> {
    let path = root.join(JOURNAL);
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    // Only complete, flushed records can precede a filesystem operation.
    let end = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |n| n + 1);
    let mut lines = std::str::from_utf8(&bytes[..end])
        .map_err(|e| e.to_string())?
        .lines();
    let id = lines
        .next()
        .map(Uuid::parse_str)
        .transpose()
        .map_err(|e| e.to_string())?;
    if id != committed {
        for line in lines.rev() {
            match serde_json::from_str::<Step>(line).map_err(|e| e.to_string())? {
                Step::Move(from, to) => {
                    let old = checked(root, &from)?;
                    let new = checked(root, &to)?;
                    if !old.exists() && new.exists() {
                        fs::rename(new, old).map_err(|e| e.to_string())?;
                    } else if old.exists() && new.exists() {
                        return Err(format!(
                            "경로 충돌로 이동 복구를 중단했습니다. {from}, {to}"
                        ));
                    }
                }
                Step::Create(dir) => {
                    let p = checked(root, &dir)?;
                    if p.is_dir() {
                        fs::remove_dir(p).map_err(|e| e.to_string())?;
                    }
                }
                Step::RemoveEmpty(dir) => {
                    let p = checked(root, &dir)?;
                    if !p.exists() {
                        fs::create_dir(p).map_err(|e| e.to_string())?;
                    }
                }
            }
        }
    }
    fs::remove_file(path).map_err(|e| e.to_string())
}
