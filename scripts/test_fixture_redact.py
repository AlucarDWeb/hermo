"""Unit tests for scripts/fixture_redact.py (pytest-free, plain unittest).

Run from scripts/:  python3 -m unittest test_fixture_redact
"""

import json
import unittest

import fixture_redact as fr


class RedactFrameTest(unittest.TestCase):
    def test_nested_and_recursive_redaction(self):
        frame = {
            "jsonrpc": "2.0",
            "method": "event",
            "params": {
                "type": "session.info",
                "payload": {
                    "info": {
                        "system_prompt": "secret prompt",
                        "systemPrompt": "secret prompt camel",
                        "skills": {"a": 1},
                        "mcp_servers": ["one"],
                        "cwd": "/home/me/secret-project",
                        "model": "keep-me",
                    }
                },
            },
        }
        out = fr.redact_frame(frame)
        payload = out["params"]["payload"]["info"]
        self.assertEqual(payload["system_prompt"], fr.REDACTED)
        self.assertEqual(payload["systemPrompt"], fr.REDACTED)
        self.assertEqual(payload["skills"], {})
        self.assertEqual(payload["mcp_servers"], [])
        self.assertEqual(payload["cwd"], "/redacted")
        self.assertEqual(payload["model"], "keep-me")

    def test_session_info_with_every_field_present(self):
        frame = {
            "jsonrpc": "2.0",
            "id": "r-create",
            "result": {
                "session_id": "6d1e5129",
                "stored_session_id": "20260911_130514_77a77f",
                "session_key": "sess_key_abc",
                "conversation_id": "conv_123",
                "info": {
                    "system_prompt": "the whole profile",
                    "skills": {"tts": ["text_to_speech"]},
                    "mcp_servers": [],
                    "skill_commands": ["cmd"],
                    "cwd": "/home/me/Dev/x",
                    "session_id": "20260911_130514_77a77f",
                },
            },
        }
        out = fr.redact_frame(frame)
        result = out["result"]
        self.assertEqual(result["stored_session_id"], fr.REDACTED)
        self.assertEqual(result["session_key"], fr.REDACTED)
        self.assertEqual(result["conversation_id"], fr.REDACTED)
        # every session_id string value is redacted (key kept for shape)
        self.assertEqual(result["stored_session_id"], fr.REDACTED)
        self.assertEqual(result["session_key"], fr.REDACTED)
        self.assertEqual(result["conversation_id"], fr.REDACTED)
        self.assertEqual(result["session_id"], fr.REDACTED)
        info = result["info"]
        self.assertEqual(info["system_prompt"], fr.REDACTED)
        self.assertEqual(info["skills"], {})
        self.assertEqual(info["mcp_servers"], [])
        self.assertEqual(info["skill_commands"], [])
        self.assertEqual(info["cwd"], "/redacted")
        self.assertEqual(info["session_id"], fr.REDACTED)

    def test_frame_without_params_returned_unchanged(self):
        line = '{"jsonrpc":"2.0","id":"r1","result":{"status":"redirected"}}'
        out = fr.redact_line(line)
        self.assertEqual(out, line)

    def test_invalid_json_returned_as_is(self):
        raw = "not json at all {"
        self.assertEqual(fr.redact_line(raw), raw)

    def test_event_session_id_value_redacted_key_kept(self):
        frame = {
            "jsonrpc": "2.0",
            "method": "event",
            "params": {
                "type": "session.title",
                "session_id": "6d1e5129",
                "payload": {"session_id": "20260911_130514_77a77f", "title": "t"},
            },
        }
        out = fr.redact_frame(frame)
        self.assertEqual(out["params"]["session_id"], fr.REDACTED)
        self.assertEqual(out["params"]["payload"]["session_id"], fr.REDACTED)
        self.assertIn("session_id", out["params"])
        self.assertIn("session_id", out["params"]["payload"])

    def test_widened_field_list_and_prompt_type_guard(self):
        frame = {
            "params": {
                "payload": {
                    "instructions": "system instructions",
                    "working_directory": "/home/me/proj",
                    "tools": {"shell": ["bash"]},
                    "prompt": "a prompt body",
                    "usage": {"prompt": 1234, "completion": 56},
                }
            }
        }
        payload = fr.redact_frame(frame)["params"]["payload"]
        self.assertEqual(payload["instructions"], fr.REDACTED)
        self.assertEqual(payload["working_directory"], fr.REDACTED)
        self.assertEqual(payload["tools"], {})
        self.assertEqual(payload["prompt"], fr.REDACTED)
        # `usage.prompt` is a numeric token count and must survive untouched
        self.assertEqual(payload["usage"]["prompt"], 1234)
        self.assertEqual(payload["usage"]["completion"], 56)

    def test_redaction_is_idempotent(self):
        # Redact the JSON *text*: redact_frame mutates in place, so comparing two
        # calls on the same dict object would be tautological (review finding).
        raw = json.dumps(
            {
                "params": {
                    "payload": {
                        "system_prompt": "p",
                        "cwd": "/home/me/x",
                        "session_id": "20260911_130514_77a77f",
                        "skills": {"a": [1]},
                        "tools": {"shell": ["bash"]},
                    }
                }
            }
        )
        once = fr.redact_line(raw)
        twice = fr.redact_line(once)
        self.assertEqual(once, twice)
        self.assertEqual(fr.redact_line(twice), twice)
        self.assertNotIn("20260911_", once)
        self.assertNotIn("/home/me", once)


if __name__ == "__main__":
    unittest.main()
