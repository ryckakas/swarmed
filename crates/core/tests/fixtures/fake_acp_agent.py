#!/usr/bin/env python3
"""A scripted ACP agent for the tests. It answers a prompt by its text:

- plain text: streams "echo: <text>" in two chunks;
- "run: <command>": reports a tool call, asks permission, then reports the outcome;
- "slow": works until the turn is cancelled;
- "guidance": says whether Swarmed's guidance came along with the message;
- "fail": answers the prompt with an error;
- "mcp": names the MCP servers it was given.

Set FAKE_ACP_LOAD_ONLY=1 to offer only session/load, which replays a history first.
Set FAKE_ACP_RESUME_NOTICE=1 to remark, as Codex does, on the model an earlier conversation
was opened with, right after opening it.
"""
import json
import os
import sys

LOAD_ONLY = os.environ.get("FAKE_ACP_LOAD_ONLY") == "1"
state = {"pending_prompt": None, "tool": None, "permission_id": None, "requests": 100}


def send(message):
    sys.stdout.write(json.dumps({"jsonrpc": "2.0", **message}) + "\n")
    sys.stdout.flush()


def update(session, body):
    send({"method": "session/update", "params": {"sessionId": session, "update": body}})


def chunk(session, text):
    update(session, {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}})


def finish(stop_reason="end_turn"):
    send({"id": state["pending_prompt"], "result": {"stopReason": stop_reason}})
    state["pending_prompt"] = None


def on_prompt(request_id, params):
    session = params["sessionId"]
    text = params["prompt"][0]["text"]
    state["pending_prompt"] = request_id
    if text == "guidance":
        # Whatever came with the message besides the user's own words.
        extra = [block["text"] for block in params["prompt"][1:]]
        chunk(session, "guidance: " + ("yes" if any("html-view" in e for e in extra) else "none"))
        finish()
    elif text == "mcp":
        chunk(session, "servers: " + ", ".join(state.get("servers", [])))
        finish()
    elif text == "key":
        chunk(session, "key: " + os.environ.get("OPENROUTER_API_KEY", "none"))
        finish()
    elif text == "slow":
        chunk(session, "working")
    elif text == "fail":
        send({"id": request_id, "error": {"code": -32000, "message": "auth_required"}})
        state["pending_prompt"] = None
    elif text.startswith("run:"):
        command = text[4:].strip()
        state["tool"] = "call-1"
        chunk(session, "I will run it.")
        # As some real agents do: announce the call first, send its arguments in an update.
        update(session, {"sessionUpdate": "tool_call", "toolCallId": "call-1", "title": "Terminal",
                         "kind": "execute", "status": "pending", "rawInput": {}})
        update(session, {"sessionUpdate": "tool_call_update", "toolCallId": "call-1",
                         "status": "in_progress", "rawInput": {"command": command}})
        state["requests"] += 1
        state["permission_id"] = state["requests"]
        send({"id": state["requests"], "method": "session/request_permission", "params": {
            "sessionId": session,
            "toolCall": {"toolCallId": "call-1", "title": "Terminal", "rawInput": {"command": command}},
            "options": [{"optionId": "allow", "name": "Allow", "kind": "allow_once"},
                        {"optionId": "reject", "name": "Reject", "kind": "reject_once"}]}})
        state["session"] = session
    else:
        chunk(session, "echo: ")
        chunk(session, text)
        update(session, {"sessionUpdate": "usage_update", "used": 1, "size": 2,
                         "cost": {"amount": 0.5, "currency": "USD"}})
        finish()


def on_permission_answer(result):
    session = state["session"]
    outcome = result["outcome"]
    allowed = outcome.get("optionId") == "allow"
    update(session, {"sessionUpdate": "tool_call_update", "toolCallId": state["tool"],
                     "status": "completed" if allowed else "failed",
                     "content": [{"type": "content", "content": {"type": "text",
                                                               "text": "ok" if allowed else outcome["outcome"]}}]})
    chunk(session, "ran it" if allowed else "not allowed")
    finish()


def describe(server):
    kind = server.get("type", "stdio")
    secrets = server.get("headers") or server.get("env") or []
    return "%s(%s, %d set)" % (server["name"], kind, len(secrets))


def open_session(request_id, method, params):
    state["servers"] = [describe(server) for server in params.get("mcpServers", [])]
    if method == "session/new":
        if not os.path.isdir(params["cwd"]):
            return send({"id": request_id, "error": {"code": -32602, "message": "no such directory"}})
        return send({"id": request_id, "result": {
            "sessionId": "acp-1",
            "configOptions": [{"id": "model", "currentValue": "fake/default"}]}})
    if method == "session/load":
        # A real agent replays the conversation before it answers.
        chunk(params["sessionId"], "replayed history")
    send({"id": request_id, "result": {}})
    if method == "session/resume" and os.environ.get("FAKE_ACP_RESUME_NOTICE") == "1":
        chunk(params["sessionId"], "Warning: resuming with another model. ")


for line in sys.stdin:
    message = json.loads(line)
    method, request_id = message.get("method"), message.get("id")
    params = message.get("params", {})
    if method == "initialize":
        capabilities = {"loadSession": True} if LOAD_ONLY else {"sessionCapabilities": {"resume": {}}}
        if os.environ.get("FAKE_ACP_NO_HTTP") != "1":
            capabilities["mcpCapabilities"] = {"http": True}
        send({"id": request_id, "result": {"protocolVersion": 1, "agentCapabilities": capabilities}})
    elif method in ("session/new", "session/load", "session/resume"):
        open_session(request_id, method, params)
    elif method == "session/set_config_option":
        send({"id": request_id, "result": {}})
    elif method == "session/prompt":
        on_prompt(request_id, params)
    elif method == "session/cancel":
        finish("cancelled")
    elif method is None and request_id == state["permission_id"]:
        on_permission_answer(message["result"])
