#!/usr/bin/env python3
"""A stand-in Chrome for the browser tests: enough of the DevTools protocol, over a WebSocket.

Started as Swarmed starts Chrome. It listens on a port of 127.0.0.1 it picks, writes it into
`DevToolsActivePort` in its profile as Chrome does, and answers what Swarmed asks:

- one page at first, on the address it was started with;
- navigating, with its history; an address with `console` in it logs a line, one with
  `refused` fails as Chrome does;
- pictures of the page shown once asked for, one more after each acknowledgement;
- tabs opened and closed, dialogs answered, `Browser.close`.

What it is told to do with the mouse and the keys is written into `fake-input` in its profile,
one JSON line each, and what it was given to download into, into `fake-downloads`. A file
`fake-fail` in its profile has it end at once, saying why, as a Chrome that cannot start does.
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

lock = threading.Lock()
pages = {}
order = []
counter = [0]


def new_page(url):
    counter[0] += 1
    page_id = "T%d" % counter[0]
    pages[page_id] = {"history": [url], "current": 0, "casting": False, "frames": 0, "size": (800, 600)}
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
    conn.event("Target.targetInfoChanged", {"targetInfo": info(page_id)})
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
        conn.event("Target.targetCreated", {"targetInfo": info(created)})
        return {"targetId": created}
    if method == "Target.closeTarget":
        closed = params["targetId"]
        if closed in pages:
            del pages[closed]
            order.remove(closed)
            conn.event("Target.targetDestroyed", {"targetId": closed})
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
        return {}
    if method == "Page.handleJavaScriptDialog":
        conn.event("Page.javascriptDialogClosed", {"result": params.get("accept", False)}, session)
        return {}
    raise KeyError("'%s' wasn't found" % method)


def serve(sock):
    request = b""
    while b"\r\n\r\n" not in request:
        chunk = sock.recv(1024)
        if not chunk:
            return
        request += chunk
    key = ""
    for line in request.decode().split("\r\n"):
        if line.lower().startswith("sec-websocket-key:"):
            key = line.split(":", 1)[1].strip()
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
