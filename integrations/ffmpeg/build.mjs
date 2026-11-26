import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFile, mkdtemp, readFile, rm, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { delimiter, isAbsolute, join } from "node:path";
import { fileURLToPath } from "node:url";

const build = "ffmpeg-9.0.2-essentials_build";
const sha256 = "60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba";
const output = fileURLToPath(new URL("./bundle/", import.meta.url));
if (process.argv.includes("--dev")) {
  if (process.platform !== "win32" || process.env.TAURI_ENV_PLATFORM !== "windows") process.exit(0);
  const dirs = [output];
  if (process.env.LOCALAPPDATA) dirs.push(join(process.env.LOCALAPPDATA, "com.namuk.tauri-app", "tools"));
  dirs.push(...(process.env.PATH ?? "").split(delimiter).filter(isAbsolute));
  async function available(name) {
    for (const dir of dirs) {
      if ((await stat(join(dir, name)).catch(() => null))?.isFile()) return true;
    }
    return false;
  }
  if (await available("ffmpeg.exe") && await available("ffprobe.exe")) process.exit(0);
  console.log("컷룸 개발 환경에 FFmpeg와 FFprobe를 준비합니다.");
}
const scratch = await mkdtemp(join(tmpdir(), "cutroom-ffmpeg-"));
try {
  const zip = join(scratch, "ffmpeg.zip");
  execFileSync("curl.exe", ["--fail", "--location", "--silent", "--show-error", "--retry", "2", "--output", zip,
    `https://github.com/GyanD/codexffmpeg/releases/download/9.0.2/${build}.zip`], { stdio: "inherit" });
  if (createHash("sha256").update(await readFile(zip)).digest("hex") !== sha256) {
    throw Error("Unexpected FFmpeg archive checksum");
  }
  const files = ["bin/ffmpeg.exe", "bin/ffprobe.exe", "LICENSE", "README.txt"];
  execFileSync("tar.exe", ["-xf", zip, "-C", scratch, ...files.map(f => build + "/" + f)]);
  for (const file of files) {
    await copyFile(join(scratch, build, file), join(output, file.replace(/^bin\//, "")));
  }
} catch (error) {
  if (!process.argv.includes("--dev")) throw error;
  console.warn("FFmpeg 준비에 실패했습니다. 영상 기능을 사용하려면 네트워크 연결을 확인하고 개발판을 다시 실행하세요.", error.message);
} finally {
  await rm(scratch, { recursive: true, force: true });
}
