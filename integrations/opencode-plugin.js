// Managed by Alloy. Native OpenCode events only; no terminal or Herdr access.
import { mkdir, writeFile, rename } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";

export const AlloyAwakePlugin = async ({ client, directory }) => {
  const root = join(homedir(), ".alloy", "agents");
  let started;
  try {
    started = execFileSync("/bin/ps", ["-p", String(process.pid), "-o", "lstart="], { encoding: "utf8", timeout: 2000, env: { ...process.env, LC_ALL: "C", TZ: "UTC" } }).trim().split(/\s+/).join(" ");
  } catch { return {}; }
  const sessions = new Map();
  let chain = Promise.resolve();
  let sequence = 0;
  const report = (id, state) => {
    if (!id) return Promise.resolve();
    const next = chain.then(async () => {
      let info = sessions.get(id);
      if (!info) {
        const response = await client.session.get({ path: { id } });
        info = response.data;
        if (!info) return;
        sessions.set(id, info);
      }
      // Child sessions must not appear as independent foreground tasks.
      if (info.parentID) return;
      const key = createHash("sha256").update(id).digest("hex");
      const path = join(root, `opencode-${process.pid}-${key}.json`);
      const temp = `${path}.${sequence++}.tmp`;
      await mkdir(root, { recursive: true, mode: 0o700 });
      try {
        await writeFile(temp, JSON.stringify({ version: 1, provider: "opencode", session_id: id, pid: process.pid, process_started: started, status: state, title: info.title || "", project: info.directory || directory || "", updated_at: Date.now() / 1000 }), { mode: 0o600 });
        await rename(temp, path);
      } catch { /* Observation failures must never affect the agent. */ }
    });
    chain = next.catch(() => {});
    return chain;
  };
  return {
    event: async ({ event }) => {
      const p = event.properties || {};
      if (p.info?.id && event.type.startsWith("session.")) sessions.set(p.info.id, p.info);
      const id = p.sessionID || p.info?.id;
      if (event.type === "session.status") {
        const kind = typeof p.status === "string" ? p.status : p.status?.type;
        if (["busy", "retry"].includes(kind)) await report(id, "working");
        else if (kind === "idle") await report(id, "idle");
      } else if (["session.idle", "session.error", "session.deleted"].includes(event.type)) {
        await report(id, "idle");
      } else if (["permission.asked", "question.asked"].includes(event.type)) {
        await report(id, "waiting");
      } else if (["permission.replied", "question.replied", "question.rejected"].includes(event.type)) {
        await report(id, "working");
      }
    },
  };
};
