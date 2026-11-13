import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import tomllib
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parent


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


hook = load("agent_hook", "agent-hook.py")
installer = load("installer", "install.py")


class Integrations(unittest.TestCase):
    def test_installer_preserves_configuration_and_is_idempotent(self):
        with tempfile.TemporaryDirectory() as temp:
            home = Path(temp)
            (home / ".claude").mkdir()
            original = {"theme": "dark", "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "existing-hook"}]}]}}
            (home / ".claude/settings.json").write_text(json.dumps(original))
            (home / ".kimi-code").mkdir()
            (home / ".kimi-code/config.toml").write_text('default_model = "keep"\n[[hooks]]\nevent = "Stop"\ncommand = "existing-hook"\n')
            with patch.dict(os.environ, {"XDG_CONFIG_HOME": str(home / ".config")}):
                installer.install(home, ROOT)
                paths = [home / ".claude/settings.json", home / ".kimi-code/config.toml"]
                first = [p.read_text() for p in paths]
                installer.install(home, ROOT)
                self.assertEqual(first, [p.read_text() for p in paths])
            settings = json.loads(first[0])
            self.assertEqual(settings["theme"], "dark")
            self.assertEqual(len(settings["hooks"]["Stop"]), 2)
            kimi = tomllib.loads(first[1])
            self.assertEqual(kimi["default_model"], "keep")
            self.assertEqual(kimi["hooks"][0]["command"], "existing-hook")
            self.assertEqual(len(kimi["hooks"]), 16)
            self.assertTrue((home / ".config/opencode/plugins/alloy-awake.js").is_file())

    def test_hook_lifecycle_writes_only_metadata_and_remains_silent(self):
        with tempfile.TemporaryDirectory() as temp:
            home = Path(temp)
            with patch.object(Path, "home", return_value=home), patch.object(hook, "owner", return_value=(123, "start")), patch.object(sys, "argv", ["hook", "claude"]):
                for event, expected in [("UserPromptSubmit", "working"), ("PermissionRequest", "waiting"), ("PostToolUse", "working"), ("Stop", "idle"), ("StopFailure", "idle"), ("SessionEnd", "idle")]:
                    data = dict(hook_event_name=event, session_id="one", session_title="Task", cwd="/project", prompt="PRIVATE PROMPT", tool_input={"secret": "PRIVATE TOOL"})
                    with patch.object(sys, "stdin", io.StringIO(json.dumps(data))), patch.object(sys, "stdout", new_callable=io.StringIO) as output:
                        hook.main()
                        self.assertEqual(output.getvalue(), "")
                    files = list((home / ".alloy/agents").glob("*.json"))
                    self.assertEqual(len(files), 1)
                    text = files[0].read_text()
                    record = json.loads(text)
                    self.assertEqual(record["status"], expected)
                    self.assertEqual(record["session_id"], "one")
                    self.assertNotIn("PRIVATE", text)
                # A subagent stopping cannot clear the foreground session.
                with patch.object(sys, "stdin", io.StringIO(json.dumps(dict(data, agent_id="child")))):
                    hook.main()
                self.assertEqual(len(list((home / ".alloy/agents").glob("*.json"))), 1)

    def test_waiting_question_and_unknown_notification(self):
        self.assertEqual(hook.status({"hook_event_name":"PreToolUse", "tool_name":"AskUserQuestion"}), "waiting")
        self.assertIsNone(hook.status({"hook_event_name":"Notification", "notification_type":"unrelated"}))
        self.assertIsNone(hook.status({"hook_event_name":"SubagentStop"}))

    def test_titles_never_determine_hook_status(self):
        for title in ["", "Thinking...", "Working...", "idle", "已完成"]:
            self.assertEqual(hook.status({"hook_event_name":"UserPromptSubmit", "session_title":title}), "working")
            self.assertEqual(hook.status({"hook_event_name":"Stop", "session_title":title}), "idle")
            self.assertIsNone(hook.status({"hook_event_name":"Unknown", "session_title":title}))

    def test_owner_selects_nearest_agent_process(self):
        with patch.object(os, "getppid", return_value=4), patch.object(hook.subprocess, "check_output", side_effect=["1 0 launchd\n2 1 /bin/claude\n3 2 /bin/claude\n4 3 /bin/sh\n", "Fri Nov 13  9:10:00 2026\n"]):
            self.assertEqual(hook.owner("claude"), (3, "Fri Nov 13 9:10:00 2026"))


if __name__ == "__main__":
    unittest.main()
