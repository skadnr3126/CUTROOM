import { readFile } from "node:fs/promises";
import { createConnection } from "node:net";
import { dirname, join } from "node:path";
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
    const rendersVideo = command.action === "export_scene" ||
      (command.action === "cut_clip_range" && command.disposition === "library");
    socket.setTimeout(rendersVideo ? 30 * 60_000 : 90_000,
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
server.registerTool("workspace_guide", {
  description: "Read the editing workflow shipped with this Cutroom MCP server, including captions, media and portable projects.",
  inputSchema: {},
  annotations: { readOnlyHint: true }
}, async () => {
  const guide = await readFile(join(dirname(process.argv[1]), "WORKFLOW.md"), "utf8");
  return { content: [{ type: "text", text: guide }] };
});
const sceneId = z.string().uuid().describe("ID from timeline");
const expected = z.number().int().nonnegative().describe("Current scene revision");
const atMs = z.number().int().nonnegative().describe("Time in milliseconds from scene start");
const captionText = z.string().min(1).describe("The entire caption, with a newline character between display lines. All lines share this caption's timing.");
const cue = z.object({ startMs: z.number().int().nonnegative(), endMs: z.number().int().positive(), text: captionText });
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
const bgmAnchor = z.object({sceneId, offsetMs:atMs.nullable().describe("0 = scene start, null = follow the scene end, or a scene-relative millisecond position")}).strict();
const bgmClip = z.object({
  id:z.string().uuid().describe("Stable BGM clip ID. Generate once for an addition and reuse when retrying."),
  assetId:z.string().uuid().describe("Audio asset from list_bgm"),
  start:bgmAnchor, end:bgmAnchor,
  volume:z.number().int().min(0).max(100),
  fadeInMs:z.number().int().nonnegative(), fadeOutMs:z.number().int().nonnegative(),
  sourceRange:z.object({inMs:z.number().int().nonnegative(), outMs:z.number().int().positive()}).strict().nullable().optional()
    .describe("Loop this excerpt of the original song in milliseconds. Omit or use null for the whole song.")
}).strict();
const bgmProject = z.string().uuid().describe("Open project ID returned by list_bgm");
tool("list_bgm", "List music in the source bin and all timeline BGM clips with their editable settings. Use timeline for scene IDs.", {});
tool("set_bgm", "Add or update one timeline BGM. Unless a range is requested, place one source duration from the start, capped at timeline end. Use list_bgm and timeline to calculate the numeric end offset. For new clips use a fresh UUID, volume 35 and zero fades unless requested. To update, copy the previous clip from list_bgm to expected and change only the desired fields in clip. Undoable.", {
  projectId:bgmProject, clip:bgmClip, expected:bgmClip.nullable().optional()
});
tool("delete_bgm", "Remove one timeline BGM clip, preserving its source music in the bin. Pass the clip previously read from list_bgm. Undoable.", {
  projectId:bgmProject, expected:bgmClip
});
tool("get_selection", "Read the active editor selection and timeline playhead, in milliseconds.", {});
tool("get_asset", "Resolve an asset in the open project to its media path, duration and audio metadata.", {assetId:z.string().uuid()});
tool("scene", "Read a scene including its captions, source placements and current revision.", { sceneId });
const editClip = {
  projectId: z.string().uuid().describe("Open project ID from timeline"),
  sceneId, expected
};
const clipId = z.string().uuid().describe("ID from scene.clips or scene.overlays. For a single scene.visual with no clips, use sceneId.");
const clipMs = z.number().int().nonnegative().describe("Milliseconds from the clip's timeline start, including trim.offsetMs if present");
tool("split_clip", "Split a video or image clip at one time and keep both parts on the timeline. No media file is created. Read scene before editing and again for the new clip IDs. Undoable.", {
  ...editClip, clipId, atMs: clipMs
});
tool("cut_clip_range", "Cut a clip-relative interval [startMs, endMs) from one video clip. Leave at least 100 ms on both sides, with both cut points inside the visible video. Choose library to save the cut footage as a new MP4, or discard to remove it from the timeline. For scene-relative deletions across clip edges, use delete_track_range. Undoable.", {
  ...editClip,
  range: z.object({ clipId, startMs: clipMs, endMs: clipMs }).strict(),
  disposition: z.enum(["discard", "library"]),
  folderId: z.string().uuid().optional().describe("Destination bin folder from timeline for library cuts. Omit to save at the bin root.")
});
tool("delete_track_range", "Delete a scene-relative interval [startMs, endMs) on one visual track, including across clip boundaries. Fully covered clips are removed. Each surviving piece must meet the clip minimum (normally 100 ms). Following clips touching the cut move together up to the next gap. Source files are preserved. Undoable as one edit. Read scene for track numbers and the current revision.", {
  ...editClip,
  track: z.number().int().min(0).max(0xffffffff).describe("0 = main visual track. For overlays use scene.overlays[].track, or its 1-based array index if track is absent."),
  startMs: atMs,
  endMs: z.number().int().positive().describe("Exclusive end in milliseconds from scene start, greater than startMs and at most the scene duration.")
});
tool("rename_scene", "Rename a timeline scene. Read its revision from timeline first. An empty name restores the default numbered label. Undoable.", {
  sceneId, expected, name: z.string().describe("New single-line scene name")
});
tool("move_scene", "Move a timeline scene immediately before or after another scene. Use the IDs from timeline. Undoable.", {
  sceneId, targetId: z.string().uuid().describe("ID of the reference scene"), after: z.boolean().describe("true for after, false for before")
});
tool("captions", "Set the final complete timed caption list through the editor's undoable save. Read the scene first and preserve other captions. Optionally show the edited time.", {
  sceneId, expected, cues: z.array(cue), atMs: atMs.optional()
});
tool("seek", "Show the scene and move the live editor playhead.", { sceneId, atMs });
tool("add_caption", "Insert one caption at the scene playhead time using the editor's undoable insertion and existing subtitle style. Send the full multiline text in one call.", { sceneId, expected, atMs, text: captionText });
tool("delete_caption", "Delete one timeline caption block by its exact scene-relative start and end times. ASS text lines grouped in this block are removed together. Undoable.", { sceneId, expected, startMs: atMs, endMs: z.number().int().positive() });
tool("import_ass", "Import an existing UTF-8 ASS subtitle file into the chosen scene, preserving styles, drawings and effects. When authoring the file, put each multiline caption in one Dialogue event with \\N between its display lines. Replaces the current subtitle track and is undoable.", { sceneId, expected, path: z.string().min(1) });
tool("register_output", "Register a finished local image, video or audio file in the intended Cutroom project. Get projectId and optional folderId from timeline before starting generation. Use one jobId per output and reuse it on retry. Write completed files at fresh paths under mediaDirectory; its subfolders are the source bin folders and are indexed automatically. Registering the same path binds the discovered asset to the job.", {
  projectId: z.string().uuid(), jobId: z.string().uuid(), path: z.string().min(1), title: z.string().min(1).optional(), folderId: z.string().uuid().optional()
});
tool("import_video", "Import a local MP4 into the live source bin.", { path: z.string().min(1) });
tool("replace_video", "Switch a single video to an imported equal-duration source, preserving timings and overlays.", { sceneId, expected, assetId: z.string().uuid() });
tool("export_scene", "Render this scene and its saved captions to a new local MP4. Existing output files are protected.", { sceneId, expected, path: z.string().min(1) });
server.connect(new StdioServerTransport()).catch(error => {
  console.error(error);
  process.exitCode = 1;
});
