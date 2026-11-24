import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFile, mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "vite";

if (process.platform === "win32") {
  const root = fileURLToPath(new URL(".", import.meta.url));
  const output = join(root, "bundle");
  execFileSync(process.env.ComSpec, ["/d", "/c", "npm.cmd ci --omit=dev --no-audit --no-fund"], { cwd: root, stdio: "inherit" });
  await build({
    configFile: false,
    root,
    publicDir: false,
    ssr: { noExternal: true },
    build: {
      ssr: join(root, "server.mjs"),
      outDir: output,
      emptyOutDir: false,
      license: { fileName: "licenses-js.md" },
      rolldownOptions: { output: { format: "cjs", entryFileNames: "server.cjs", codeSplitting: false } },
    },
  });
  const arch = ({ x86_64: "x64", aarch64: "arm64", i686: "x86", ia32: "x86" })[process.env.TAURI_ENV_ARCH ?? process.arch]
    ?? process.env.TAURI_ENV_ARCH ?? process.arch;
  const sums = {
    x64: "4a191082e9706866830379d577d2cc8a50c776fcd9436a8d061b063ed0d9a0e3",
    arm64: "c1856817f611738c77534efbff8a5a17990593eab24046bc8d2c23a77a160d13",
    x86: "9c01245683c87e3f81eedfbad82c1665611749627cd1b0b5a42921db9361b4a6",
  };
  if (!sums[arch]) throw Error("Unsupported Windows target: " + arch);
  const version = "v22.23.5";
  const name = `node-${version}-win-${arch}`;
  const scratch = await mkdtemp(join(tmpdir(), "cutroom-runtime-"));
  try {
    const zip = join(scratch, "node.zip");
    execFileSync("curl.exe", ["--fail", "--location", "--silent", "--show-error", "--retry", "2", "--output", zip,
      `https://nodejs.org/dist/${version}/${name}.zip`], { stdio: "inherit" });
    if (createHash("sha256").update(await readFile(zip)).digest("hex") !== sums[arch]) throw Error("Node.js checksum mismatch");
    execFileSync("tar.exe", ["-xf", zip, "-C", scratch, name + "/node.exe", name + "/LICENSE"]);
    await copyFile(join(scratch,name,"node.exe"), join(output,"cutroom-node.exe"));
    await copyFile(join(scratch,name,"LICENSE"), join(output,"LICENSE-node.txt"));
  } finally {
    await rm(scratch, { recursive: true, force: true });
  }
}
