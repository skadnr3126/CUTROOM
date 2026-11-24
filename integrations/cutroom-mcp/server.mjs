import { readFile } from "node:fs/promises";
import { createConnection } from "node:net";
import { join } from "node:path";
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";

const locator = join(process.env.LOCALAPPDATA, "com.namuk.tauri-app", "editor-control.json");
async function request(command) {
  let endpoint;
  try {
    endpoint = JSON.parse(await readFile(locator, "utf8"));
  } catch {
    throw new Error("연결 정보가 없습니다. 최신 컷룸 앱을 실행하세요.");
  }
  return new Promise((resolve, reject) => {
    const socket = createConnection({ host: "127.0.0.1", port: endpoint.port });
    socket.setEncoding("utf8");
    let response = "";
    socket.setTimeout(command.action === "export_scene" ? 30 * 60_000 : 90_000,
      () => socket.destroy(new Error("컷룸 응답 시간이 초과되었습니다. 타임라인을 조회해 반영 여부를 확인하세요.")));
    socket.on("error", reject);
    socket.on("connect", () => socket.write(JSON.stringify({ token: endpoint.token, command }) + "\n"));
    socket.on("data", chunk => { response += chunk; });
    socket.on("end", () => {
      try {
        const message = JSON.parse(response);
        if (message.error) throw new Error(message.error);
        resolve(message.result);
      } catch (error) { reject(error); }
    });
  });
}
const server = new McpServer({ name: "cutroom", version: "0.1.0" });
const sceneId = z.string().uuid().describe("ID from timeline");
const expected = z.number().int().nonnegative().describe("Current scene revision");
const atMs = z.number().int().nonnegative().describe("Time in milliseconds from scene start");
const cue = z.object({ startMs: z.number().int().nonnegative(), endMs: z.number().int().positive(), text: z.string().min(1) });
function tool(action, description, inputSchema) {
  server.registerTool(action, { description, inputSchema }, async args => {
    try {
      const data = await request({ ...args, action });
      return { content: [{ type: "text", text: JSON.stringify(data) }] };
    } catch (error) {
      return { isError: true, content: [{ type: "text", text: String(error.message ?? error) }] };
    }
  });
}
tool("timeline", "List the open Cutroom project's scenes, durations, IDs and revisions.", {});
tool("get_selection", "Read the active editor selection and timeline playhead, in milliseconds.", {});
tool("get_asset", "Resolve an asset in the open project to its media path, duration and audio metadata.", {assetId:z.string().uuid()});
tool("scene", "Read a scene including its captions, source placements and current revision.", { sceneId });
tool("captions", "Set the final complete timed caption list through the editor's undoable save. Read the scene first and preserve other captions. Optionally show the edited time.", {
  sceneId, expected, cues: z.array(cue), atMs: atMs.optional()
});
tool("seek", "Show the scene and move the live editor playhead.", { sceneId, atMs });
tool("add_caption", "Insert a caption at the scene playhead time using the editor's undoable insertion and existing subtitle style.", { sceneId, expected, atMs, text: z.string().min(1) });
tool("delete_caption", "Delete one timeline caption block by its exact scene-relative start and end times. ASS text lines grouped in this block are removed together. Undoable.", { sceneId, expected, startMs: atMs, endMs: z.number().int().positive() });
tool("import_ass", "Import an existing UTF-8 ASS subtitle file into the chosen scene, preserving styles, drawings and effects. Replaces its current subtitle track and is undoable.", { sceneId, expected, path: z.string().min(1) });
tool("register_output", "Register a finished local image, video or audio file in the intended Cutroom project. Get projectId and optional folderId from timeline before starting generation. Use one jobId per output and reuse it on retry. Write completed files at fresh paths under mediaDirectory; its subfolders are the source bin folders and are indexed automatically. Registering the same path binds the discovered asset to the job.", {
  projectId: z.string().uuid(), jobId: z.string().uuid(), path: z.string().min(1), title: z.string().min(1).optional(), folderId: z.string().uuid().optional()
});
tool("import_video", "Import a local MP4 into the live source bin.", { path: z.string().min(1) });
tool("replace_video", "Switch a single video to an imported equal-duration source, preserving timings and overlays.", { sceneId, expected, assetId: z.string().uuid() });
tool("export_scene", "Render this scene and its saved captions to a new local MP4. Existing output files are protected.", { sceneId, expected, path: z.string().min(1) });
await server.connect(new StdioServerTransport());
