import assert from "node:assert/strict";
import { readFile, readdir, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const temp = await mkdtemp(join(tmpdir(), "alloy-plugin-"));
const oldHome = process.env.HOME;
process.env.HOME = temp;
try {
  const source = await readFile(new URL("./opencode-plugin.js", import.meta.url), "utf8");
  const { AlloyAwakePlugin } = await import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}`);
  const client = { session: { get: async ({ path: { id } }) => ({ data: { id, title: `Task ${id}`, directory: "/project", ...(id === "child" ? { parentID: "root" } : {}) } }) } };
  const plugin = await AlloyAwakePlugin({ client, directory: "/fallback" });
  const event = (type, properties) => plugin.event({ event: { type, properties } });
  const records = async () => {
    const root = join(temp, ".alloy/agents");
    return Promise.all((await readdir(root)).filter(f => f.endsWith(".json")).map(async f => JSON.parse(await readFile(join(root, f), "utf8"))));
  };
  for (const [type, properties, state] of [
    ["session.status", { status: { type: "busy" } }, "working"],
    ["permission.asked", {}, "waiting"],
    ["permission.replied", {}, "working"],
    ["question.asked", {}, "waiting"],
    ["question.replied", {}, "working"],
    ["session.status", { status: { type: "retry" } }, "working"],
    ["session.status", { status: { type: "idle" } }, "idle"],
    ["session.error", {}, "idle"],
  ]) {
    await event(type, { sessionID: "root", ...properties });
    const [record] = await records();
    assert.equal(record.status, state);
    assert.equal(record.title, "Task root");
    assert.equal(record.pid, process.pid);
    assert.ok(record.process_started);
  }
  for (const title of ["Thinking...", "Working...", "已完成", ""]) {
    await event("session.updated", { info: { id: "root", title, directory: "/project" } });
    assert.equal((await records()).find(r => r.session_id === "root").status, "idle", "a title update is not a lifecycle event");
  }
  await event("session.status", { sessionID: "child", status: { type: "busy" } });
  assert.equal((await records()).length, 1, "child sessions must not replace roots");
  // Multiple foreground sessions survive concurrent event delivery.
  await Promise.all(["one", "two"].map(sessionID => event("session.status", { sessionID, status: { type: "busy" } })));
  assert.equal((await records()).length, 3);
  await event("session.updated", { info: { id: "one", title: "Renamed", directory: "/new" } });
  await event("session.status", { sessionID: "one", status: { type: "busy" } });
  assert.equal((await records()).find(r => r.session_id === "one").title, "Renamed");
  console.log("OpenCode native event integration passed");
} finally {
  if (oldHome === undefined) delete process.env.HOME;
  else process.env.HOME = oldHome;
  await rm(temp, { recursive: true, force: true });
}
