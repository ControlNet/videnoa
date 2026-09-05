#!/usr/bin/env python3
"""Linux Docker smoke: synthetic Worker/media, HTTP progress and cross-mount move.

Uses only Python's standard library and an existing Controller image. Separate
bind mounts deliberately share a host device but have distinct mount IDs. No
production data or running containers are modified. The Worker is a test double;
this verifies orchestration/publication, not video processing or GPU behavior.
"""

import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request
from urllib.parse import unquote, urlsplit
import uuid


class SyntheticWorker(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def reply(self, value, status=200):
        data = value if isinstance(value, bytes) else json.dumps(value).encode()
        self.send_response(status)
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Content-Type", "application/octet-stream" if isinstance(value, bytes)
                         else "application/json")
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        state = self.server
        path = unquote(urlsplit(self.path).path)
        interface = {"inputs": [{"name": name, "port_type": "Path", "default_value": None}
                                for name in ("input", "output")], "outputs": []}
        if path == "/api/health":
            self.reply({"status": "ok"})
        elif path == "/api/workflows":
            self.reply([{"filename": "synthetic-smoke.json", "name": "Synthetic smoke",
                         "description": "Test-only workflow", "has_interface": True,
                         "workflow": {"interface": interface}}])
        elif path == "/api/presets":
            self.reply([])
        elif path == "/api/workflows/synthetic-smoke.json/interface":
            self.reply(interface)
        elif path == f"/api/jobs/{state.job_id}" and state.job:
            self.reply({**state.job, "status": "completed" if state.complete.is_set() else "running",
                        "progress": {"current_frame": 125, "total_frames": 1000,
                                     "fps": 25.0, "eta_seconds": 35.0}})
        elif path.startswith("/api/files/"):
            key = path.removeprefix("/api/files/")
            stat = key.endswith("/stat")
            key = key.removesuffix("/stat") if stat else key
            data = state.files.get(key)
            if data is None:
                self.reply({"error": "not_found"}, 404)
            elif stat:
                self.reply({"path": key, "size": len(data), "is_file": True, "is_dir": False})
            else:
                self.reply(data)
        else:
            self.reply({"error": "unexpected_route"}, 404)

    def do_PUT(self):
        key = unquote(urlsplit(self.path).path).removeprefix("/api/files/")
        data = self.rfile.read(int(self.headers["Content-Length"]))
        self.server.files[key] = data
        self.reply({"path": key, "size": len(data)})

    def do_POST(self):
        if self.path != "/api/run":
            self.reply({"error": "unexpected_route"}, 404)
            return
        state = self.server
        payload = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        state.runs += 1
        state.job = {"id": state.job_id, "created_at": "2026-09-06T00:00:00Z",
                     "workflow_name": payload["workflow_name"], "workflow_source": "workflow",
                     "params": payload["params"]}
        state.files[payload["params"]["output"]] = state.output
        self.reply({"id": state.job_id, "status": "queued",
                    "created_at": state.job["created_at"]}, 201)

    def do_DELETE(self):
        prefix = unquote(urlsplit(self.path).path).removeprefix("/api/files/")
        self.server.files = {key: data for key, data in self.server.files.items()
                             if key != prefix and not key.startswith(prefix + "/")}
        self.reply(b"", 204)


def docker(*args):
    return subprocess.check_output(["docker", *args], text=True).strip()


def run(image):
    with tempfile.TemporaryDirectory(prefix="controller-mount-smoke-") as directory:
        root = Path(directory)
        data, media = root / "data", root / "media"
        data.mkdir()
        media.mkdir()
        assert data.stat().st_dev == media.stat().st_dev
        source = media / "synthetic-input.mkv"
        source.write_bytes(b"synthetic input; not a video")
        destination = media / "synthetic-output.mp4"
        worker = ThreadingHTTPServer(("127.0.0.1", 0), SyntheticWorker)
        worker.job_id, worker.job, worker.runs = str(uuid.uuid4()), None, 0
        worker.files = {}
        worker.complete = threading.Event()
        worker.output = b"synthetic verified output; not a video\n" * 65536
        thread = threading.Thread(target=worker.serve_forever, daemon=True)
        thread.start()
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            port = listener.getsockname()[1]
        origin = f"http://127.0.0.1:{port}"
        container = "controller-mount-smoke-" + uuid.uuid4().hex[:12]
        cookie, csrf = None, None

        def request(method, path, body=None):
            headers = {"Origin": origin, "Content-Type": "application/json"}
            if cookie:
                headers.update({"Cookie": cookie, "x-csrf-token": csrf})
            if method == "POST" and path == "/api/tasks":
                headers["Idempotency-Key"] = uuid.uuid4().hex
            message = urllib.request.Request(origin + path, method=method, headers=headers,
                                             data=json.dumps(body).encode() if body is not None else None)
            with urllib.request.urlopen(message, timeout=30) as response:
                return response.headers, json.load(response)

        def await_task(task_id, predicate):
            deadline = time.monotonic() + 60
            while time.monotonic() < deadline:
                try:
                    task = request("GET", f"/api/tasks/{task_id}")[1]["task"]
                except urllib.error.URLError:
                    time.sleep(0.2)
                    continue
                assert task["status"] != "failed", task.get("failure")
                if predicate(task):
                    return task
                time.sleep(0.2)
            raise AssertionError(f"Task did not reach expected state: {task['status']}")

        try:
            docker("run", "-d", "--name", container, "--network", "host",
                   "--user", f"{os.getuid()}:{os.getgid()}",
                   "-v", f"{data}:/workspace/data", "-v", f"{media}:/media",
                   image, "--host", "127.0.0.1", "--port", str(port))
            for _ in range(100):
                try:
                    if request("GET", "/api/health")[1] == {"status": "ok"}:
                        break
                except (OSError, urllib.error.URLError):
                    time.sleep(0.1)
            else:
                raise AssertionError("Controller startup timed out")
            mounts = docker("exec", container, "cat", "/proc/self/mountinfo").splitlines()
            identities = {parts[4]: (parts[0], parts[2]) for parts in map(str.split, mounts)
                          if parts[4] in ("/workspace/data", "/media")}
            assert identities["/workspace/data"][0] != identities["/media"][0]
            assert identities["/workspace/data"][1] == identities["/media"][1]

            password = secrets.token_urlsafe(32)
            headers, _ = request("POST", "/api/auth/setup",
                                 {"password": password, "password_confirmation": password})
            cookie = headers["Set-Cookie"].split(";", 1)[0]
            csrf = headers["x-csrf-token"]
            request("POST", "/api/workers", {"name": "Synthetic smoke worker", "enabled": True,
                                            "api_url": f"http://127.0.0.1:{worker.server_port}",
                                            "compute_slots": 1})
            task = request("POST", "/api/tasks", {"input_path": "/media/synthetic-input.mkv",
                           "output_path": "/media/synthetic-output.mp4", "workflow": "synthetic-smoke.json",
                           "source": "api", "priority": 0})[1]
            task_id = task["id"]
            progress = await_task(task_id, lambda item: item["status"] == "processing"
                                  and item["progress"]["percent"] == 12.5)["progress"]
            assert progress["frames_per_second"] == 25.0 and progress["eta_seconds"] == 35
            assert not destination.exists()
            worker.complete.set()
            completed = await_task(task_id, lambda item: item["status"] == "completed")
            assert completed["attempt_count"] == 1 and worker.runs == 1
            assert hashlib.sha256(destination.read_bytes()).digest() == hashlib.sha256(worker.output).digest()
            assert set(media.iterdir()) == {source, destination}
            assert not (data / task_id).exists(), "Private task artifacts were not cleaned up"
            assert not worker.files, "Remote workspace was not cleaned up"
            docker("restart", container)
            await_task(task_id, lambda item: item["status"] == "completed")
            assert worker.runs == 1, "Restart recomputed an already published task"
            print("PASS: HTTP progress/FPS/ETA, same-device distinct bind mounts, copy/delete publication, "
                  "exact output bytes, no sibling staging, local/remote cleanup, restart without AI replay")
        finally:
            docker("rm", "-f", container)
            worker.shutdown()
            worker.server_close()
            thread.join()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", default="videnoa-controller:backend-qa")
    run(parser.parse_args().image)
