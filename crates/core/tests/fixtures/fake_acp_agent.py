#!/usr/bin/env python3
"""A scripted ACP agent for the tests. It answers a prompt by its text:

- plain text: streams "echo: <text>" in two chunks;
- "run: <command>": reports a tool call, asks permission, then reports the outcome;
- "slow": works until the turn is cancelled;
- "guidance": says whether Swarmed's guidance came along with the message;
- "fail": answers the prompt with an error;
- "mcp": names the MCP servers it was given; "mcp args" says what each runs;
- "guidance browser": says whether Swarmed's guidance told it of the task's browser;
- "model": names the model it was put on, or "fake/default" if none;
- "wake": ends its turn, then carries on by itself, as an agent whose timer went off does;
- "linger": ends its turn, then starts a tool and never says another word;
- "spawn": starts a sub-agent the way the protocol has it, as Codex does, which asks
  permission for "run: cat README.md" and hands back what it found;
- "spawn slow": the same, but the sub-agent works until its session is cancelled;
- "delegate": starts a sub-agent the way OpenCode does to a client that asked to hear of
  them, which works on after the turn and wakes the agent when it is done;
- "inline": the same, to a client that did not ask: the sub-agent's updates come on the
  agent's own stream, marked as the sub-agent's;
- "shell": leaves a shell running, as OpenCode does, and wakes when it ends;
- "printed": runs a command and sends what it printed only in `_meta`, as Codex does;
- "screenshot": calls a browser's screenshot tool, which returns a line of text and a PNG;
- "usage": says "hi" and ends with the usage codex-acp reports of a turn on a warm cache;
- "slow usage": works until the turn is cancelled, and ends with the same usage.

Set FAKE_ACP_LOAD_ONLY=1 to offer only session/load, which replays a history first.
Set FAKE_ACP_EFFORT=1 to have an effort setting, standing at medium.
Set FAKE_ACP_RESUME_NOTICE=1 to remark, as Codex does, on the model an earlier conversation
was opened with, right after opening it.
Set FAKE_ACP_MODES=a,b to have modes of its own by these names, standing at the first. An
echo then ends by naming the mode it was put into.
Set FAKE_ACP_TITLE_COMMAND=1 to title a shell call with the command itself, as some agents do.
Set FAKE_ACP_STAY=<file> to carry on after its input has closed, adding to that file as long
as it lives.
"""
import json
import os
import sys
import time

LOAD_ONLY = os.environ.get("FAKE_ACP_LOAD_ONLY") == "1"
MODES = [name for name in os.environ.get("FAKE_ACP_MODES", "").split(",") if name]
MODE = [{"id": "mode", "currentValue": MODES[0],
         "options": [{"value": name} for name in MODES]}] if MODES else []
TITLE_COMMAND = os.environ.get("FAKE_ACP_TITLE_COMMAND") == "1"
STAY = os.environ.get("FAKE_ACP_STAY")
# What an agent with an effort of its own says of it when a conversation opens.
EFFORT = [{"id": "reasoning_effort", "currentValue": "medium",
           "options": [{"value": "low"}, {"value": "medium"}, {"value": "high"}]}
          ] if os.environ.get("FAKE_ACP_EFFORT") == "1" else []
state = {"pending_prompt": None, "tool": None, "permission_id": None, "requests": 100,
         "subagents": False, "child_updates": False, "child": None}
CHILD = "acp-1:subagent:T1"
# As codex-acp says it of a turn on a warm cache: the input leaves out what was read from the
# cache, the output holds the reasoning, and the total adds up all but the thoughts.
WARM_USAGE = {"totalTokens": 24275, "inputTokens": 199, "cachedReadTokens": 24064,
              "cachedWriteTokens": 0, "outputTokens": 12, "thoughtTokens": 7}


def send(message):
    sys.stdout.write(json.dumps({"jsonrpc": "2.0", **message}) + "\n")
    sys.stdout.flush()


def update(session, body):
    send({"method": "session/update", "params": {"sessionId": session, "update": body}})


def chunk(session, text):
    update(session, {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}})


def child_notice(session, child, title, body):
    """What OpenCode tells a client that asked to hear of its sub-agents."""
    send({"method": "opencode/session/child_update", "params": {
        "rootSessionId": session, "childSessionId": child, "parentSessionId": session,
        "depth": 1, "title": title, **body}})


def marked(child, title, body):
    """A sub-agent's update as OpenCode marks it on the agent's own stream."""
    return {**body, "_meta": {"opencode/child-session": {
        "id": child, "parentID": "acp-1", "depth": 1, "title": title}}}


def subagent_update(session, body):
    if state["subagents"]:
        update(session, {"sessionUpdate": "subagent_update", "sessionId": CHILD, **body})


def spawn(session, slow):
    """A sub-agent as the protocol has it: announced, then at work under its own session."""
    chunk(session, "I will ask a helper.")
    subagent_update(session, {"capabilities": {}})
    subagent_update(session, {"title": "Explorer 1", "description": "explorer"})
    update(session, {"sessionUpdate": "session_message", "messageId": "m1", "senderSessionId": session,
                     "recipientSessionId": CHILD, "content": [{"type": "text", "text": "Find the README."}]})
    subagent_update(session, {"state": {"state": "running"}})
    chunk(CHILD, "Looking.")
    # What it uses is its own, not the agent's.
    update(CHILD, {"sessionUpdate": "usage_update", "used": 99, "size": 100})
    state["child"] = "slow" if slow else "asking"
    if slow:
        return
    update(CHILD, {"sessionUpdate": "tool_call", "toolCallId": "child-call", "title": "Terminal",
                   "kind": "execute", "status": "pending", "rawInput": {"command": "cat README.md"}})
    state["requests"] += 1
    state["permission_id"] = state["requests"]
    state["session"] = session
    send({"id": state["requests"], "method": "session/request_permission", "params": {
        "sessionId": CHILD,
        "toolCall": {"toolCallId": "child-call", "title": "Terminal", "kind": "execute",
                     "rawInput": {"command": "cat README.md"}},
        "options": [{"optionId": "allow", "name": "Allow", "kind": "allow_once"},
                    {"optionId": "reject", "name": "Reject", "kind": "reject_once"}]}})


def child_answered(result):
    session = state["session"]
    allowed = result["outcome"].get("optionId") == "allow"
    update(CHILD, {"sessionUpdate": "tool_call_update", "toolCallId": "child-call",
                   "status": "completed" if allowed else "failed",
                   "content": [{"type": "content", "content": {"type": "text", "text": "demo"}}]})
    chunk(CHILD, "Found it: demo.")
    subagent_update(session, {"state": {"state": "idle", "stopReason": "end_turn"}})
    state["child"] = None
    chunk(session, "The helper is done.")
    update(session, {"sessionUpdate": "usage_update", "used": 1, "size": 2})
    finish()


def subagent_call(session, call, description):
    update(session, {"sessionUpdate": "tool_call", "toolCallId": call, "name": "subagent",
                     "title": "subagent", "kind": "think", "status": "pending", "rawInput": {}})
    update(session, {"sessionUpdate": "tool_call_update", "toolCallId": call, "status": "in_progress",
                     "kind": "think", "title": "subagent",
                     "rawInput": {"agent": "explore", "description": description, "prompt": "Look for it."}})


def subagent_returns(session, call, child):
    update(session, {"sessionUpdate": "tool_call_update", "toolCallId": call, "status": "completed",
                     "content": [{"type": "content", "content": {"type": "text",
                                  "text": "The subagent is working in the background (sessionID: %s)." % child}}],
                     "rawOutput": {"metadata": {"sessionID": child, "status": "running"}}})


def delegate(session):
    """A sub-agent as OpenCode has it, told of in notifications of its own."""
    if not state["child_updates"]:
        chunk(session, "You did not ask to hear of sub-agents.")
        return finish()
    title = "Find auth code"
    subagent_call(session, "call_sub", title)
    child_notice(session, "ses_C", title, {"type": "status", "status": "created"})
    child_notice(session, "ses_C", title, {"type": "status", "status": "running"})
    subagent_returns(session, "call_sub", "ses_C")
    child_notice(session, "ses_C", title, {"type": "update", "update": marked("ses_C", title, {
        "sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Reading."}})})
    child_notice(session, "ses_C", title, {"type": "update", "update": marked("ses_C", title, {
        "sessionUpdate": "tool_call", "toolCallId": "ses_C:call_1", "title": title + ": read", "kind": "read",
        "status": "pending", "rawInput": {"path": "auth.rs"}})})
    child_notice(session, "ses_C", title, {"type": "update", "update": marked("ses_C", title, {
        "sessionUpdate": "tool_call_update", "toolCallId": "ses_C:call_1", "status": "completed",
        "content": [{"type": "content", "content": {"type": "text", "text": "fn login"}}]})})
    chunk(session, "It works on it.")
    finish()
    time.sleep(0.2)
    child_notice(session, "ses_C", title, {"type": "update", "update": marked("ses_C", title, {
        "sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Found it in auth.rs."}})})
    child_notice(session, "ses_C", title, {"type": "status", "status": "completed"})
    # Its end wakes the agent, which reads what it handed back.
    chunk(session, "The helper found it.")
    update(session, {"sessionUpdate": "usage_update", "used": 3, "size": 4})


def inline(session):
    """A sub-agent as OpenCode has it, to a client that did not ask to hear of them."""
    title = "Count lines"
    subagent_call(session, "call_in", title)
    update(session, marked("ses_I", title, {"sessionUpdate": "agent_message_chunk",
                                            "content": {"type": "text", "text": "Counting."}}))
    update(session, marked("ses_I", title, {"sessionUpdate": "tool_call", "toolCallId": "ses_I:call_1",
                                            "title": title + ": wc", "kind": "execute", "status": "completed",
                                            "rawInput": {"command": "wc -l"}}))
    subagent_returns(session, "call_in", "ses_I")
    chunk(session, "Asked.")
    finish()


def shell(session):
    """A shell left running, as OpenCode reports one: its call ends, and nothing tells its end."""
    update(session, {"sessionUpdate": "tool_call", "toolCallId": "call_sh", "title": "shell", "kind": "execute",
                     "status": "pending", "rawInput": {}})
    update(session, {"sessionUpdate": "tool_call_update", "toolCallId": "call_sh", "status": "in_progress",
                     "title": "pnpm dev", "rawInput": {"command": "pnpm dev", "background": True}})
    update(session, {"sessionUpdate": "tool_call_update", "toolCallId": "call_sh", "status": "completed",
                     "content": [{"type": "content", "content": {"type": "text",
                                  "text": "Command moved to the background (shell ID: sh_X)."}}],
                     "rawOutput": {"metadata": {"status": "running", "truncated": False, "shellID": "sh_X"}}})
    chunk(session, "It runs.")
    finish()
    if os.environ.get("FAKE_ACP_SHELL_ENDS") == "1":
        time.sleep(0.2)
        chunk(session, "The dev server stopped.")
        update(session, {"sessionUpdate": "usage_update", "used": 3, "size": 4})


def printed(session):
    """A command whose output comes only in `_meta`, as Codex sends it to most clients."""
    update(session, {"sessionUpdate": "tool_call", "toolCallId": "call_ls", "title": "ls", "kind": "execute",
                     "status": "in_progress", "rawInput": {"command": "ls"},
                     "content": [{"type": "terminal", "terminalId": "call_ls"}]})
    for data in ("README.md\n", "src\n"):
        update(session, {"sessionUpdate": "tool_call_update", "toolCallId": "call_ls",
                         "_meta": {"terminal_output_delta": {"data": data, "terminal_id": "call_ls"}}})
    update(session, {"sessionUpdate": "tool_call_update", "toolCallId": "call_ls", "status": "completed",
                     "_meta": {"terminal_exit": {"exit_code": 0, "signal": None, "terminal_id": "call_ls"}}})
    finish()


# A PNG of two by two pixels, in base64.
SCREENSHOT = ("iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAE0lEQVR42mPQiDoBRAxA/"
              "OHDBwAlUgav0sDuFwAAAABJRU5ErkJggg==")


def screenshot(session):
    """A tool call whose content holds an image, as an MCP tool's result reaches ACP."""
    update(session, {"sessionUpdate": "tool_call", "toolCallId": "call_shot",
                     "title": "browser_take_screenshot", "kind": "other", "status": "in_progress",
                     "rawInput": {"type": "png"}})
    update(session, {"sessionUpdate": "tool_call_update", "toolCallId": "call_shot",
                     "status": "completed", "content": [
                         {"type": "content", "content": {"type": "text", "text": "Took the screenshot"}},
                         {"type": "content", "content": {"type": "image", "data": SCREENSHOT,
                                                          "mimeType": "image/png"}},
                     ]})
    chunk(session, "The page looks right.")
    finish()


def finish(stop_reason="end_turn", usage=None):
    result = {"stopReason": stop_reason}
    if usage is not None:
        result["usage"] = usage
    send({"id": state["pending_prompt"], "result": result})
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
    elif text == "mcp args":
        # What each server runs, never what its variables hold.
        runs = ["%s: %s" % (server["name"], " ".join([server.get("command", "")] + server.get("args", [])))
                for server in state.get("given", [])]
        chunk(session, "; ".join(runs))
        finish()
    elif text == "guidance browser":
        extra = [block["text"] for block in params["prompt"][1:]]
        chunk(session, "browser: " + ("yes" if any("SWARMED_BROWSER" in e for e in extra) else "none"))
        finish()
    elif text == "model":
        chunk(session, "model: " + state.get("model", "fake/default"))
        finish()
    elif text == "key":
        chunk(session, "key: " + os.environ.get("OPENROUTER_API_KEY", "none"))
        finish()
    elif text == "wake":
        chunk(session, "I will tell you when the timer goes off.")
        finish()
        time.sleep(0.2)
        update(session, {"sessionUpdate": "tool_call_update", "toolCallId": "timer",
                         "title": "Timer", "status": "completed"})
        chunk(session, "The timer went off.")
        update(session, {"sessionUpdate": "usage_update", "used": 3, "size": 4})
    elif text == "linger":
        chunk(session, "Done for now.")
        finish()
        time.sleep(0.2)
        update(session, {"sessionUpdate": "tool_call", "toolCallId": "late", "title": "Watching",
                         "kind": "other", "status": "in_progress", "rawInput": {"for": "ever"}})
    elif text == "slow":
        chunk(session, "working")
    elif text in ("spawn", "spawn slow"):
        spawn(session, text == "spawn slow")
    elif text == "delegate":
        delegate(session)
    elif text == "inline":
        inline(session)
    elif text == "shell":
        shell(session)
    elif text == "printed":
        printed(session)
    elif text == "screenshot":
        screenshot(session)
    elif text == "usage":
        chunk(session, "hi")
        finish(usage=WARM_USAGE)
    elif text == "slow usage":
        state["usage_on_cancel"] = True
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
        title = command if TITLE_COMMAND else "Terminal"
        send({"id": state["requests"], "method": "session/request_permission", "params": {
            "sessionId": session,
            "toolCall": {"toolCallId": "call-1", "title": title, "kind": "execute",
                         "rawInput": {"command": command}},
            "options": [{"optionId": "allow", "name": "Allow", "kind": "allow_once"},
                        {"optionId": "reject", "name": "Reject", "kind": "reject_once"}]}})
        state["session"] = session
    else:
        chunk(session, "echo: ")
        chunk(session, text)
        if MODES:
            chunk(session, " (mode: %s)" % state.get("mode", MODES[0]))
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
    state["given"] = params.get("mcpServers", [])
    if method == "session/new":
        if not os.path.isdir(params["cwd"]):
            return send({"id": request_id, "error": {"code": -32602, "message": "no such directory"}})
        return send({"id": request_id, "result": {
            "sessionId": "acp-1",
            "configOptions": [{"id": "model", "currentValue": "fake/default"}] + EFFORT + MODE}})
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
        asked = params.get("clientCapabilities", {})
        # As Codex and OpenCode read what a client asks for.
        state["subagents"] = isinstance(asked.get("subagents"), dict)
        state["child_updates"] = asked.get("_meta", {}).get("opencode/child-session-updates") is True
        capabilities = {"loadSession": True} if LOAD_ONLY else {"sessionCapabilities": {"resume": {}}}
        if os.environ.get("FAKE_ACP_NO_HTTP") != "1":
            capabilities["mcpCapabilities"] = {"http": True}
        send({"id": request_id, "result": {"protocolVersion": 1, "agentCapabilities": capabilities}})
    elif method in ("session/new", "session/load", "session/resume"):
        open_session(request_id, method, params)
    elif method == "session/set_config_option":
        if params.get("configId") in ("mode", "model"):
            state[params["configId"]] = params.get("value")
        send({"id": request_id, "result": {}})
    elif method == "session/prompt":
        on_prompt(request_id, params)
    elif method == "session/cancel" and params.get("sessionId") == CHILD:
        # Cancelling a sub-agent's session stops what it does, and not its starter's turn.
        if state["child"] == "slow":
            subagent_update("acp-1", {"state": {"state": "idle", "stopReason": "cancelled"}})
            state["child"] = None
            chunk("acp-1", "The helper was stopped.")
            finish()
    elif method == "session/cancel":
        usage = WARM_USAGE if state.pop("usage_on_cancel", False) else None
        finish("cancelled", usage=usage)
    elif method is None and request_id == state["permission_id"] and state["child"] == "asking":
        child_answered(message["result"])
    elif method is None and request_id == state["permission_id"]:
        on_permission_answer(message["result"])

# Its input has closed, which asks it to go. An agent that does not take the hint carries on.
while STAY:
    with open(STAY, "a") as beat:
        beat.write(".")
    time.sleep(0.05)
