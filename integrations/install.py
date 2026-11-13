#!/usr/bin/env python3
"""Install independent native integrations, preserving other user integrations."""
import argparse
import json
import os
from pathlib import Path
import shlex
import shutil
import sys

START = "# BEGIN Alloy agent monitoring\n"
END = "# END Alloy agent monitoring\n"
EVENTS = ["SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "PostToolUseFailure", "PermissionRequest", "Stop", "StopFailure", "Notification", "SessionEnd", "PreCompact"]


def backup_write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists():
        if path.read_text() == text:
            return
        shutil.copy2(path, str(path) + ".alloy-backup")
    temp = path.with_name(path.name + ".alloy-tmp")
    temp.write_text(text)
    temp.chmod(0o600)
    os.replace(temp, path)


def install(home, assets):
    target = home / ".alloy/integrations"
    target.mkdir(parents=True, exist_ok=True)
    for name in ["agent-hook.py", "opencode-plugin.js"]:
        source = (assets / name).read_text()
        backup_write(target / name, source)
    # The hook runtime is fixed at installation, including paths with spaces.
    command = shlex.quote(sys.executable) + " " + shlex.quote(str(target / "agent-hook.py"))
    claude = home / ".claude/settings.json"
    settings = json.loads(claude.read_text()) if claude.exists() else {}
    hooks = settings.setdefault("hooks", {})
    for event in EVENTS + ["Elicitation", "ElicitationResult"]:
        entries = hooks.setdefault(event, [])
        wanted = command + " claude"
        if not any(h.get("command") == wanted for entry in entries for h in entry.get("hooks", [])):
            entries.append({"hooks": [{"type": "command", "command": wanted, "timeout": 5}]})
    backup_write(claude, json.dumps(settings, ensure_ascii=False, indent=2) + "\n")
    kimi = home / ".kimi-code/config.toml"
    text = kimi.read_text() if kimi.exists() else ""
    if START in text:
        before, rest = text.split(START, 1)
        if END not in rest:
            raise ValueError("Unterminated Alloy integration block")
        text = before + rest.split(END, 1)[1]
    block = START
    for event in EVENTS + ["TurnStarted", "PermissionResult", "Interrupt", "SessionHeartbeat"]:
        block += "[[hooks]]\nevent = " + json.dumps(event) + "\ncommand = " + json.dumps(command + " kimi") + "\ntimeout = 5\n\n"
    block += END
    backup_write(kimi, text.rstrip() + "\n\n" + block)
    config = Path(os.environ.get("XDG_CONFIG_HOME", str(home / ".config")))
    backup_write(config / "opencode/plugins/alloy-awake.js", (target / "opencode-plugin.js").read_text())


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--home", type=Path, default=Path.home())
    parser.add_argument("--assets", type=Path, default=Path(__file__).resolve().parent if "__file__" in globals() else Path.cwd())
    args = parser.parse_args()
    install(args.home, args.assets)
    print("Installed Alloy native integrations. Restart Kimi, Claude Code and OpenCode to load them.")
