import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

const path = name => fileURLToPath(new URL(name, import.meta.url));
const client = new Client({ name: "cutroom-guide-check", version: "1" });
try {
  await client.connect(new StdioClientTransport({
    command: path("bundle/cutroom-node.exe"), args: [path("bundle/server.cjs")]
  }));
  const guide = await readFile(path("WORKFLOW.md"), "utf8");
  const entry = await readFile(path("../codex-workspace/AGENTS.md"), "utf8");
  assert.equal(await readFile(path("bundle/AGENTS.md"), "utf8"), entry);
  const result = await client.callTool({ name: "workspace_guide", arguments: {} });
  assert.ok(!result.isError && result.content[0]?.type === "text");
  assert.equal(result.content[0].text, guide);
  const { tools } = await client.listTools();
  for (const { name } of tools) {
    assert.ok((guide + entry).includes("`" + name + "`"), "Document this MCP tool: " + name);
  }
  console.log("Verified workspace instructions and", tools.length, "documented MCP tools.");
} finally {
  await client.close();
}
