import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFile, mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const build = "ffmpeg-9.0.2-essentials_build";
const sha256 = "60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba";
const output = fileURLToPath(new URL("./bundle/", import.meta.url));
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
} finally {
  await rm(scratch, { recursive: true, force: true });
}
