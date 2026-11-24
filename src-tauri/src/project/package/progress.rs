use serde::Serialize;
use std::io::{self, Read, Write};

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Bytes {
    pub percent: u8,
    pub written_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "stage", rename_all = "camelCase")]
pub(crate) enum PackageProgress {
    Packing(Bytes),
    Finalizing,
    Extracting(Bytes),
    Verifying,
    Opening,
}

pub(super) struct ByteTransfer<F> {
    progress: Bytes,
    report: F,
    buffer: Vec<u8>,
}

impl<F: FnMut(Bytes)> ByteTransfer<F> {
    pub fn new(total_bytes: u64, mut report: F) -> Self {
        let progress = Bytes {
            percent: 0,
            written_bytes: 0,
            total_bytes,
        };
        report(progress);
        Self {
            progress,
            report,
            buffer: vec![0; 256 * 1024],
        }
    }

    pub fn copy(
        &mut self,
        mut input: impl Read,
        mut output: impl Write,
        size: u64,
        name: &str,
    ) -> Result<(), String> {
        let mut remaining = size;
        let p = &mut self.progress;
        if size > p.total_bytes - p.written_bytes {
            return Err(format!("패키지 작업 중 파일 크기가 변경되었습니다. {name}"));
        }
        loop {
            let n = match input.read(&mut self.buffer) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                result => result.map_err(super::err)?,
            };
            if n == 0 {
                break;
            }
            remaining = remaining
                .checked_sub(n as u64)
                .ok_or_else(|| format!("파일 크기가 일치하지 않습니다. {name}"))?;
            output.write_all(&self.buffer[..n]).map_err(super::err)?;
            p.written_bytes += n as u64;
            let percent = (u128::from(p.written_bytes) * 100 / u128::from(p.total_bytes)) as u8;
            if percent != p.percent {
                p.percent = percent;
                (self.report)(*p);
            }
        }
        if remaining != 0 {
            return Err(format!("파일 크기가 일치하지 않습니다. {name}"));
        }
        Ok(())
    }

    pub fn complete(&self) -> bool {
        self.progress.written_bytes == self.progress.total_bytes
    }
}
