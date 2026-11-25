#!/usr/bin/env python3
"""A stand-in Chrome for the browser tests: enough of the DevTools protocol, over a WebSocket.

Started as Swarmed starts Chrome. It listens on a port of 127.0.0.1 it picks, writes it into
`DevToolsActivePort` in its profile as Chrome does, and answers what Swarmed asks:

- one page at first, on the address it was started with;
- navigating, with its history; an address with `console` in it logs a line, one with
  `refused` fails as Chrome does;
- pictures of the page shown once asked for, one more after each acknowledgement;
- tabs opened and closed, dialogs answered, `Browser.close`;
- Chrome's inspect mode: while it is on, a press of the mouse picks a `<button id="save">`
  whose description, box, accessibility, HTML, script and picture it answers for.

Over HTTP it answers `/json/version` and `/json/list` as Chrome does, with addresses built from
the `Host` the client sent. Every connection that asked to discover targets hears of tabs opened,
changed and closed, whichever connection did it, as with Chrome.

What it is told to do with the mouse and the keys is written into `fake-input` in its profile,
one JSON line each, and what it was given to download into, into `fake-downloads`. The inspect
modes it is put in go into `fake-inspect`, and what a pick read it with into `fake-pick`. A file
`fake-fail` in its profile has it end at once, saying why, as a Chrome that cannot start does,
and a file `fake-slow` has it take two seconds to listen, as one on a busy machine does.
"""

import base64
import hashlib
import json
import os
import socket
import struct
import sys
import threading
import time

GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"

profile = None
start = "about:blank"
for arg in sys.argv[1:]:
    if arg.startswith("--user-data-dir="):
        profile = arg.split("=", 1)[1]
    elif not arg.startswith("--"):
        start = arg

if profile is None:
    sys.stderr.write("no --user-data-dir\n")
    sys.exit(2)

if os.path.exists(os.path.join(profile, "fake-fail")):
    sys.stderr.write("[0101/000000.000:ERROR] the profile is in use by another Chrome\n")
    sys.exit(21)

if os.path.exists(os.path.join(profile, "fake-slow")):
    time.sleep(2)

lock = threading.Lock()
discovering = []
pages = {}
order = []
counter = [0]


def new_page(url):
    counter[0] += 1
    page_id = "T%d" % counter[0]
    pages[page_id] = {"history": [url], "current": 0, "casting": False, "frames": 0, "size": (800, 600),
                      "inspecting": False}
    order.append(page_id)
    return page_id


def info(page_id):
    page = pages[page_id]
    url = page["history"][page["current"]]
    title = "Page at " + url if url != "about:blank" else ""
    return {"targetId": page_id, "type": "page", "url": url, "title": title, "attached": False}


new_page(start)


class Connection:
    def __init__(self, sock):
        self.sock = sock
        self.write_lock = threading.Lock()

    def send(self, message):
        data = json.dumps(message).encode()
        head = bytes([0x81])
        if len(data) < 126:
            head += bytes([len(data)])
        elif len(data) < 65536:
            head += bytes([126]) + struct.pack(">H", len(data))
        else:
            head += bytes([127]) + struct.pack(">Q", len(data))
        with self.write_lock:
            self.sock.sendall(head + data)

    def event(self, method, params, session=None):
        message = {"method": method, "params": params}
        if session:
            message["sessionId"] = session
        self.send(message)

    def read_exactly(self, count):
        data = b""
        while len(data) < count:
            chunk = self.sock.recv(count - len(data))
            if not chunk:
                raise EOFError
            data += chunk
        return data

    def read(self):
        first, second = self.read_exactly(2)
        length = second & 0x7F
        if length == 126:
            length = struct.unpack(">H", self.read_exactly(2))[0]
        elif length == 127:
            length = struct.unpack(">Q", self.read_exactly(8))[0]
        mask = self.read_exactly(4) if second & 0x80 else b"\0\0\0\0"
        payload = bytearray(self.read_exactly(length))
        for i in range(length):
            payload[i] ^= mask[i % 4]
        if first & 0x0F == 0x8:
            raise EOFError
        return payload.decode()


def broadcast(method, params):
    for listener in list(discovering):
        try:
            listener.event(method, params)
        except OSError:
            discovering.remove(listener)


def frame(conn, page_id, session):
    page = pages[page_id]
    if not page["casting"] or page["frames"] >= 40:
        return
    page["frames"] += 1
    width, height = page["size"]
    data = base64.b64encode(("frame %d %dx%d" % (page["frames"], width, height)).encode()).decode()
    meta = {"deviceWidth": width, "deviceHeight": height, "pageScaleFactor": 1, "offsetTop": 0}
    conn.event("Page.screencastFrame", {"data": data, "metadata": meta, "sessionId": page["frames"]}, session)


def page_of(session):
    return session[2:] if session and session.startswith("S-") else None


def navigated(conn, page_id, session):
    page = pages[page_id]
    url = page["history"][page["current"]]
    conn.event("Page.frameStartedLoading", {"frameId": page_id}, session)
    broadcast("Target.targetInfoChanged", {"targetInfo": info(page_id)})
    conn.event("Page.frameNavigated", {"frame": {"id": page_id, "url": url}}, session)
    if "console" in url:
        args = [{"type": "string", "value": "hello from"}, {"type": "string", "value": url}]
        conn.event("Runtime.consoleAPICalled", {"type": "log", "args": args, "timestamp": time.time() * 1000}, session)
    conn.event("Page.frameStoppedLoading", {"frameId": page_id}, session)


def note(name, value):
    with open(os.path.join(profile, name), "a") as out:
        out.write(json.dumps(value) + "\n")


def answer(conn, method, params, session):
    page_id = page_of(session)
    page = pages.get(page_id) if page_id else None
    if method == "Browser.setDownloadBehavior":
        note("fake-downloads", params.get("downloadPath"))
        return {}
    if method == "Target.setDiscoverTargets":
        for known in list(order):
            conn.event("Target.targetCreated", {"targetInfo": info(known)})
        discovering.append(conn)
        return {}
    if method == "Target.getTargets":
        return {"targetInfos": [info(known) for known in order]}
    if method == "Target.getTargetInfo":
        if params["targetId"] not in pages:
            raise KeyError("No target with given id found")
        return {"targetInfo": info(params["targetId"])}
    if method == "Target.attachToTarget":
        if params["targetId"] not in pages:
            raise KeyError("No target with given id found")
        return {"sessionId": "S-" + params["targetId"]}
    if method in ("Page.enable", "Runtime.enable", "Log.enable", "Page.bringToFront", "Page.reload", "Page.stopLoading"):
        return {}
    if method == "Target.createTarget":
        created = new_page(params.get("url", "about:blank"))
        broadcast("Target.targetCreated", {"targetInfo": info(created)})
        return {"targetId": created}
    if method == "Target.closeTarget":
        closed = params["targetId"]
        if closed in pages:
            del pages[closed]
            order.remove(closed)
            broadcast("Target.targetDestroyed", {"targetId": closed})
        return {"success": True}
    if method == "Browser.close":
        return {}
    if page is None:
        raise KeyError("'%s' wasn't found" % method)
    return answer_on_page(conn, method, params, session, page_id, page)


def answer_on_page(conn, method, params, session, page_id, page):
    if method == "Page.getNavigationHistory":
        entries = [{"id": i + 1, "url": url, "title": url} for i, url in enumerate(page["history"])]
        return {"currentIndex": page["current"], "entries": entries}
    if method == "Page.navigate":
        if "refused" in params["url"]:
            return {"frameId": page_id, "errorText": "net::ERR_CONNECTION_REFUSED"}
        del page["history"][page["current"] + 1:]
        page["history"].append(params["url"])
        page["current"] += 1
        navigated(conn, page_id, session)
        return {"frameId": page_id, "loaderId": "L"}
    if method == "Page.navigateToHistoryEntry":
        page["current"] = params["entryId"] - 1
        navigated(conn, page_id, session)
        return {}
    if method == "Emulation.setDeviceMetricsOverride":
        page["size"] = (params["width"], params["height"])
        return {}
    if method == "Page.startScreencast":
        page["casting"] = True
        frame(conn, page_id, session)
        return {}
    if method == "Page.stopScreencast":
        page["casting"] = False
        return {}
    if method == "Page.screencastFrameAck":
        frame(conn, page_id, session)
        return {}
    if method.startswith("Input."):
        note("fake-input", {"method": method, "params": params, "page": page_id})
        if page["inspecting"] and params.get("type") == "mousePressed":
            conn.event("Overlay.inspectNodeRequested", {"backendNodeId": PICKED}, session)
        return {}
    if method.startswith(("Overlay.", "DOM.", "Accessibility.", "Runtime.callFunctionOn",
                          "Runtime.releaseObject", "Page.createIsolatedWorld",
                          "Page.getLayoutMetrics", "Page.captureScreenshot")):
        return inspecting(method, params, page)
    if method == "Page.handleJavaScriptDialog":
        conn.event("Page.javascriptDialogClosed", {"result": params.get("accept", False)}, session)
        return {}
    raise KeyError("'%s' wasn't found" % method)


PICKED = 42

# The smallest start of a PNG: what Chrome's pictures start with.
PNG = base64.b64encode(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR").decode()


def inspecting(method, params, page):
    """Inspect mode, and what a pick reads of the one element there is to pick."""
    if method == "Overlay.setInspectMode":
        page["inspecting"] = params["mode"] == "searchForNode"
        note("fake-inspect", params["mode"])
        return {}
    if method in ("Overlay.enable", "Overlay.disable", "Overlay.hideHighlight", "DOM.enable",
                  "DOM.disable", "Runtime.releaseObject"):
        return {}
    if params.get("backendNodeId", PICKED) != PICKED:
        raise KeyError("No node with given id found")
    if method == "DOM.describeNode":
        return {"node": {"nodeId": 0, "backendNodeId": PICKED, "nodeType": 1, "nodeName": "BUTTON",
                         "localName": "button", "attributes": ["id", "save", "class", "btn primary"]}}
    if method == "DOM.getOuterHTML":
        return {"outerHTML": '<button id="save" class="btn primary">Save</button>'}
    if method == "Accessibility.getPartialAXTree":
        return {"nodes": [{"nodeId": "1", "backendDOMNodeId": PICKED, "role": {"type": "role", "value": "button"},
                           "name": {"type": "computedString", "value": "Save"}}]}
    if method == "DOM.getBoxModel":
        return {"model": {"border": [40, 100, 160, 100, 160, 140, 40, 140], "width": 120, "height": 40}}
    if method == "Page.createIsolatedWorld":
        note("fake-pick", {"world": params.get("worldName")})
        return {"executionContextId": 7}
    if method == "DOM.resolveNode":
        note("fake-pick", {"resolved in": params.get("executionContextId")})
        return {"object": {"type": "object", "subtype": "node", "objectId": "object-42"}}
    if method == "Runtime.callFunctionOn":
        wanted = params["arguments"][0]["value"]
        styles = [[name, {"color": "rgb(255, 0, 0)", "font-size": "14px"}.get(name, "")] for name in wanted]
        value = {"selector": "#save", "text": "Save", "styles": styles}
        return {"result": {"type": "object", "value": value}}
    if method == "Page.getLayoutMetrics":
        view = {"pageX": 0, "pageY": 300, "clientWidth": 800, "clientHeight": 600}
        return {"cssVisualViewport": view}
    if method == "Page.captureScreenshot":
        note("fake-pick", {"clip": params.get("clip")})
        return {"data": PNG}
    raise KeyError("'%s' wasn't found" % method)


def answer_http(sock, path, host):
    """What Chrome answers over plain HTTP, its addresses built from the `Host` asked for."""
    path = path.rstrip("/")
    if path == "/json/version":
        body = {"Browser": "FakeChrome/1.0", "Protocol-Version": "1.3",
                "webSocketDebuggerUrl": "ws://%s/devtools/browser/fake-%d" % (host, os.getpid())}
    elif path in ("/json", "/json/list"):
        with lock:
            body = [dict(info(known), webSocketDebuggerUrl="ws://%s/devtools/page/%s" % (host, known))
                    for known in order]
    else:
        sock.sendall(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        sock.close()
        return
    data = json.dumps(body).encode()
    sock.sendall(("HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=UTF-8\r\n"
                  "Content-Length: %d\r\nConnection: close\r\n\r\n" % len(data)).encode() + data)
    sock.close()


def serve(sock):
    request = b""
    while b"\r\n\r\n" not in request:
        chunk = sock.recv(1024)
        if not chunk:
            return
        request += chunk
    key = ""
    host = ""
    upgrade = False
    lines = request.decode().split("\r\n")
    for line in lines:
        if line.lower().startswith("sec-websocket-key:"):
            key = line.split(":", 1)[1].strip()
        if line.lower().startswith("host:"):
            host = line.split(":", 1)[1].strip()
        if line.lower().startswith("upgrade:") and "websocket" in line.lower():
            upgrade = True
    if not upgrade:
        return answer_http(sock, lines[0].split(" ")[1], host)
    accept = base64.b64encode(hashlib.sha1((key + GUID).encode()).digest()).decode()
    sock.sendall(("HTTP/1.1 101 WebSocket Protocol Handshake\r\nUpgrade: websocket\r\n"
                  "Connection: Upgrade\r\nSec-WebSocket-Accept: %s\r\n\r\n" % accept).encode())
    conn = Connection(sock)
    while True:
        try:
            message = json.loads(conn.read())
        except (EOFError, OSError):
            return
        method, session = message["method"], message.get("sessionId")
        try:
            with lock:
                result = answer(conn, method, message.get("params", {}), session)
            reply = {"id": message["id"], "result": result}
        except KeyError as refused:
            reply = {"id": message["id"], "error": {"code": -32000, "message": refused.args[0]}}
        if session:
            reply["sessionId"] = session
        conn.send(reply)
        if method == "Browser.close":
            sock.close()
            os._exit(0)


listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
listener.bind(("127.0.0.1", 0))
listener.listen(8)
port = listener.getsockname()[1]
written = os.path.join(profile, "DevToolsActivePort.partial")
with open(written, "w") as out:
    out.write("%d\n/devtools/browser/fake-%d\n" % (port, os.getpid()))
os.rename(written, os.path.join(profile, "DevToolsActivePort"))

while True:
    accepted, _ = listener.accept()
    threading.Thread(target=serve, args=(accepted,), daemon=True).start()
