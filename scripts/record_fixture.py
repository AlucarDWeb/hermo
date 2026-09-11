#!/usr/bin/env python
"""Record a protocol fixture from a live Hermes dashboard (PLAN.md T0a / W0).

Connects to the loopback gateway, runs synthetic innocuous prompts and appends
every *inbound* JSON-RPC frame, verbatim and one per line, to
hermes_core/tests/fixtures/events.jsonl. Outbound frames go to stderr only.

Secrets: the session token never enters the fixture or the repo; it is read
from /tmp/hermo-session-token (or passed via HERMO_SESSION_TOKEN).
"""

import asyncio
import json
import os
import sys
import time

import websockets  # type: ignore

import fixture_redact

PORT = os.environ.get("HERMO_PORT", "9121")
WS_URL = f"ws://127.0.0.1:{PORT}/api/ws"
HERE = os.path.dirname(os.path.abspath(__file__))
FIXTURE = os.path.join(HERE, "..", "hermes_core", "tests", "fixtures", "events.jsonl")

histogram: dict[str, int] = {}
event_queue: asyncio.Queue = asyncio.Queue()


def log_outbound(obj) -> None:
    label = obj.get("method", "?") if "id" in obj else "response"
    print(f"--> {label}", file=sys.stderr)


async def send(ws, obj) -> None:
    await ws.send(json.dumps(obj, separators=(",", ":")))
    log_outbound(obj)


def record(line: str) -> None:
    # Redacted on the way in: a session.info frame carries the live system prompt
    # (user memory + profile), the skill list and the absolute cwd.
    with open(FIXTURE, "a") as f:
        f.write(fixture_redact.redact_line(line) + "\n")


async def drain(ws, pending_responses) -> None:
    """Single consumer: append every inbound frame to the fixture, classify it,
    resolve pending response futures and enqueue events."""
    async for raw in ws:
        for line in raw.split("\n"):
            line = line.strip()
            if not line:
                continue
            record(line)
            try:
                obj = json.loads(line)
            except json.JSONDecodeError:
                continue
            rid = obj.get("id")
            if rid is not None and rid in pending_responses and not pending_responses[rid].done():
                pending_responses[rid].set_result(obj)
                histogram[f"<result {rid}>"] = histogram.get(f"<result {rid}>", 0) + 1
                print(f"<-- result {rid}", file=sys.stderr)
                continue
            params = obj.get("params") or {}
            etype = params.get("type") if obj.get("method") == "event" else "<other>"
            histogram[etype] = histogram.get(etype, 0) + 1
            print(f"<-- {etype}", file=sys.stderr)
            if obj.get("method") == "event":
                await event_queue.put(obj)


async def request(ws, pending_responses, rid, method, params, timeout=60.0):
    fut = asyncio.get_running_loop().create_future()
    pending_responses[rid] = fut
    await send(ws, {"jsonrpc": "2.0", "id": rid, "method": method, "params": params})
    try:
        return await asyncio.wait_for(fut, timeout)
    finally:
        pending_responses.pop(rid, None)


async def wait_for_event(etype, timeout=180.0, since=None):
    """Return the next enqueued event of the given type for session `since`."""
    deadline = time.monotonic() + timeout
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError(f"event {etype} not seen within {timeout}s")
        try:
            obj = await asyncio.wait_for(event_queue.get(), timeout=remaining)
        except asyncio.TimeoutError:
            continue
        params = obj.get("params") or {}
        if params.get("type") == etype and (since is None or params.get("session_id") == since):
            return obj


async def main() -> None:
    token = os.environ.get("HERMO_SESSION_TOKEN") or open("/tmp/hermo-session-token").read().strip()
    os.makedirs(os.path.dirname(FIXTURE), exist_ok=True)
    if os.path.exists(FIXTURE):
        os.remove(FIXTURE)
    pending: dict[str, asyncio.Future] = {}

    async with websockets.connect(f"{WS_URL}?token={token}", max_size=2**24) as ws:
        drain_task = asyncio.create_task(drain(ws, pending))
        await asyncio.sleep(0.5)  # gateway.ready lands via drain

        # session.create
        resp = await request(ws, pending, "r-create", "session.create", {"cols": 48})
        sid = resp["result"]["session_id"]
        print(f"session: {sid}", file=sys.stderr)
        await wait_for_event("session.info", since=sid)

        # a) plain turn
        await send(ws, {"jsonrpc": "2.0", "id": "r-plain", "method": "prompt.submit",
                        "params": {"session_id": sid,
                                   "text": "Reply with exactly this sentence and nothing else: hello from the fixture recording."}})
        await wait_for_event("message.complete", timeout=240, since=sid)

        # b) tool turn — read-only shell command
        await send(ws, {"jsonrpc": "2.0", "id": "r-tool", "method": "prompt.submit",
                        "params": {"session_id": sid,
                                   "text": "Use your shell tool to run exactly `echo hermo-fixture-tool-check`, then tell me the output in one short sentence."}})
        await wait_for_event("tool.complete", timeout=300, since=sid)
        await wait_for_event("message.complete", timeout=300, since=sid)

        # c) approval — NOT forced: this host's tool policy auto-allows read
        #     shell commands, so no approval.request appears on the wire. Forcing
        #     an approval would require a non-innocuous command; documented in
        #     the report and synthesised in events_synthetic.jsonl instead.

        # d) clarify turn
        await send(ws, {"jsonrpc": "2.0", "id": "r-clarify", "method": "prompt.submit",
                        "params": {"session_id": sid,
                                   "text": "Before anything else, use your clarify tool to ask me one question: which colour do I prefer, blue or green? Wait for my answer, then reply in one short sentence."}})
        clar = await wait_for_event("clarify.request", timeout=300, since=sid)
        payload = clar["params"]["payload"]
        req_id = payload.get("request_id")
        qid = None
        if isinstance(payload.get("questions"), list) and payload["questions"]:
            qid = payload["questions"][0].get("qid")
        answer_params = {"session_id": sid, "request_id": req_id, "answer": "green"}
        if qid:
            answer_params["question_id"] = qid
        await send(ws, {"jsonrpc": "2.0", "id": "r-clarify-resp", "method": "clarify.respond",
                        "params": answer_params})
        await wait_for_event("message.complete", timeout=300, since=sid)

        # e) busy submit: long turn, second submit ~2 s in
        await send(ws, {"jsonrpc": "2.0", "id": "r-long", "method": "prompt.submit",
                        "params": {"session_id": sid,
                                   "text": "Count slowly from 1 to 20, one number per line, pacing yourself. Synthetic prompt for protocol recording."}})
        await asyncio.sleep(2.0)
        try:
            await request(ws, pending, "r-busy", "prompt.submit",
                          {"session_id": sid, "text": "second submit while the first turn runs"}, timeout=30)
        except asyncio.TimeoutError:
            print("busy-submit: no response in 30s", file=sys.stderr)
        await wait_for_event("message.complete", timeout=600, since=sid)

        drain_task.cancel()


if __name__ == "__main__":
    asyncio.run(main())
    print("\n== event histogram ==")
    for k, v in sorted(histogram.items()):
        print(f"{k:40s} {v}")