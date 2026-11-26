use super::files::media_command;
use std::{
    path::Path,
    process::Stdio,
    thread,
    time::{Duration, Instant},
};

// FFmpeg lists compiled-in encoders even on computers without their drivers.
// Require actual packets with the caller's output settings; a stalled driver
// must not block a software fallback.
pub(super) fn supports(ffmpeg: &Path, size: &str, options: &[&str]) -> bool {
    let Ok(mut child) = media_command(ffmpeg)
        .args([
            "-nostdin",
            "-v",
            "error",
            "-abort_on",
            "empty_output",
            "-f",
            "lavfi",
            "-i",
            &format!("color=s={size}:r=30"),
            "-frames:v",
            "2",
            "-an",
        ])
        .args(options)
        .args(["-f", "null", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}
