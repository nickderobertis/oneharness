#!/usr/bin/env python3
"""Byte-counting HTTP CONNECT proxy — the rejected candidate, kept as evidence.

    connect_proxy.py --port-file PATH --tally PATH [--bind ADDR]

Tunnels CONNECT host:port (and plain-HTTP absolute-URI requests) and, on
SIGTERM, writes the tunnelled byte totals per destination to --tally as JSON.
Run a launch with HTTPS_PROXY/HTTP_PROXY pointing here and compare the tally
with netmeasure.py's namespace counters: whatever the namespace saw beyond the
tally's legs went around the proxy.
"""

import argparse
import json
import signal
import socket
import sys
import threading
from collections import defaultdict

tally = defaultdict(lambda: {"connections": 0, "to_client": 0, "from_client": 0})
lock = threading.Lock()


def pump(src, dst, key, field):
    try:
        while True:
            data = src.recv(65536)
            if not data:
                break
            dst.sendall(data)
            with lock:
                tally[key][field] += len(data)
    except OSError:
        pass
    finally:
        try:
            dst.shutdown(socket.SHUT_WR)
        except OSError:
            pass


def handle(client):
    head = b""
    while b"\r\n\r\n" not in head:
        data = client.recv(4096)
        if not data:
            client.close()
            return
        head += data
    line = head.split(b"\r\n", 1)[0].decode()
    method, target, _ = line.split(" ", 2)
    rest = head.split(b"\r\n\r\n", 1)[1]
    if method == "CONNECT":
        host, port = target.rsplit(":", 1)
        key = target
    else:  # absolute-URI plain HTTP
        hostport = target.split("/", 3)[2]
        host, _, port = hostport.partition(":")
        port = port or "80"
        key = "http://" + hostport
    try:
        upstream = socket.create_connection((host, int(port)), timeout=30)
        upstream.settimeout(None)
    except OSError:
        client.sendall(b"HTTP/1.1 502 Bad Gateway\r\n\r\n")
        client.close()
        return
    with lock:
        tally[key]["connections"] += 1
    if method == "CONNECT":
        client.sendall(b"HTTP/1.1 200 Connection established\r\n\r\n")
        if rest:
            upstream.sendall(rest)
    else:
        upstream.sendall(head)
    threading.Thread(target=pump, args=(upstream, client, key, "to_client"), daemon=True).start()
    pump(client, upstream, key, "from_client")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--bind", default="127.0.0.1")
    parser.add_argument("--port-file", required=True)
    parser.add_argument("--tally", required=True)
    args = parser.parse_args()
    listener = socket.socket()
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind((args.bind, 0))
    listener.listen(128)

    def finish(*_):
        with lock:
            totals = {"to_client": sum(v["to_client"] for v in tally.values()),
                      "from_client": sum(v["from_client"] for v in tally.values()),
                      "by_destination": dict(tally)}
        with open(args.tally, "w") as out:
            json.dump(totals, out)
        sys.exit(0)

    signal.signal(signal.SIGTERM, finish)
    with open(args.port_file, "w") as out:
        out.write(str(listener.getsockname()[1]))
    while True:
        conn, _ = listener.accept()
        threading.Thread(target=handle, args=(conn,), daemon=True).start()


if __name__ == "__main__":
    main()
