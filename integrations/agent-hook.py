#!/usr/bin/env python3
"""Observe native hooks; never print output or affect the agent's decisions."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


def owner(provider):
    output = subprocess.check_output(
        ["/bin/ps", "-axo", "pid=,ppid=,comm="], text=True, timeout=2
    )
    processes = {}
    for line in output.splitlines():
        fields = line.strip().split(None, 2)
        if len(fields) == 3:
            processes[int(fields[0])] = (int(fields[1]), Path(fields[2]).name)
    names = {"kimi": {"kimi", "kimi-code"}, "claude": {"claude", "claude-code"}}[provider]
    pid = os.getppid()
    for _ in range(32):
        if pid not in processes:
            break
        parent, name = processes[pid]
        if name in names or any(name.startswith(n + " ") for n in names):
            started = subprocess.check_output(
                ["/bin/ps", "-p", str(pid), "-o", "lstart="], text=True, timeout=2, env={**os.environ, "LC_ALL": "C", "TZ": "UTC"}
            )
            return pid, " ".join(started.split())
        pid = parent
    raise ValueError("native agent process not found")


def status(payload):
    event = payload.get("hook_event_name")
    if event in {"SessionStart", "Stop", "StopFailure", "Interrupt", "SessionEnd"}:
        return "idle"
    if event in {"PermissionRequest", "Elicitation"}:
        return "waiting"
    if event == "Notification":
        if payload.get("notification_type") in {"permission_prompt", "elicitation_dialog"}:
            return "waiting"
        if payload.get("notification_type") == "idle_prompt":
            return "idle"
        return None
    if event == "PreToolUse" and payload.get("tool_name") in {"AskUserQuestion", "ask_user_question"}:
        return "waiting"
    if event in {"UserPromptSubmit", "TurnStarted", "PreToolUse", "PostToolUse", "PostToolUseFailure", "PermissionResult", "ElicitationResult", "PreCompact"}:
        return "working"
    return None


def main():
    provider = sys.argv[1]
    payload = json.load(sys.stdin)
    # Subagents have their own lifecycle. Never end or rename the root turn.
    if payload.get("agent_id"):
        return
    state = status(payload)
    session = payload.get("session_id")
    if not state or not isinstance(session, str) or not session:
        return
    pid, started = owner(provider)
    root = Path.home() / ".alloy/agents"
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    key = hashlib.sha256(session.encode()).hexdigest()
    path = root / f"{provider}-{pid}-{key}.json"
    title = payload.get("session_title", "")
    if not title and path.exists():
        title = json.loads(path.read_text()).get("title", "")
    value = dict(version=1, provider=provider, session_id=session, pid=pid,
                 process_started=started, status=state, title=title,
                 project=payload.get("cwd", ""), updated_at=time.time())
    fd, temp = tempfile.mkstemp(dir=root, prefix=".event-")
    try:
        with os.fdopen(fd, "w") as f:
            json.dump(value, f, ensure_ascii=False)
        os.replace(temp, path)
    finally:
        if os.path.exists(temp):
            os.unlink(temp)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        pass
