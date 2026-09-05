#!/usr/bin/env python3
"""Exercise an actual Controller binary with isolated, synthetic filesystem fixtures.

Uses only Python's standard library. Authentication material is generated in memory
and never printed. No production workspace, media, or credentials are used.
"""

import argparse
import json
from pathlib import Path
import secrets
import socket
import sqlite3
import subprocess
import tempfile
import time
import urllib.error
import urllib.request


def available_port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


def run(binary):
    with tempfile.TemporaryDirectory(prefix="controller-architecture-smoke-") as directory:
        root = Path(directory)
        workspace = root / "controller-workspace"
        media = root / "media"
        workspace.mkdir()
        media.mkdir()
        source = media / "E08.mkv"
        destination = media / "E08.AI.mp4"
        source.write_bytes(b"synthetic archive smoke media")
        port = available_port()
        host = f"127.0.0.1:{port}"
        cookie = None
        csrf = None
        password = secrets.token_urlsafe(32)
        process = None

        def request(method, path, body=None, scheme="https", authenticated=True):
            headers = {"Host": host, "Origin": f"{scheme}://{host}"}
            if authenticated and cookie:
                headers["Cookie"] = cookie
                headers["x-csrf-token"] = csrf
            if path == "/api/tasks":
                headers["Idempotency-Key"] = secrets.token_hex(16)
            if body is not None:
                headers["Content-Type"] = "application/json"
            data = json.dumps(body).encode() if body is not None else None
            message = urllib.request.Request(f"http://{host}{path}", data=data, headers=headers, method=method)
            try:
                response = urllib.request.urlopen(message, timeout=10)
            except urllib.error.HTTPError as error:
                response = error
            with response:
                return response.status, response.headers, json.load(response)

        def start(overrides):
            nonlocal process
            process = subprocess.Popen([str(binary), *overrides], cwd=workspace,
                                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            for _ in range(100):
                assert process.poll() is None, "Controller exited during startup"
                try:
                    if request("GET", "/api/health", authenticated=False)[0] == 200:
                        return
                except urllib.error.URLError:
                    pass
                time.sleep(0.1)
            raise AssertionError("Controller did not become healthy")

        def stop():
            if process and process.poll() is None:
                process.terminate()
                try:
                    assert process.wait(timeout=35) == 0, "Controller shutdown failed"
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
                    raise

        def settings_request(settings):
            return {"version": settings["version"], "server": settings["server"],
                    "auth": {key: settings[key] for key in
                             ("secure_cookie", "session_absolute_seconds", "session_idle_seconds")},
                    "scheduler": settings["scheduler"], "timeouts": settings["timeouts"],
                    "retry": settings["retry"]}

        try:
            start(["--host", "127.0.0.1", "--port", str(port)])
            status, _, initial = request("GET", "/api/auth/setup", authenticated=False)
            assert status == 200 and not initial["initialized"]
            status, headers, _ = request("POST", "/api/auth/setup",
                                         {"password": password, "password_confirmation": password},
                                         authenticated=False)
            assert status == 200, "HTTPS first setup with defaults failed"
            cookie = headers["Set-Cookie"].split(";", 1)[0]
            csrf = headers["x-csrf-token"]
            status, _, settings = request("GET", "/api/settings")
            assert status == 200 and settings["server"]["port"] == port
            changed = settings_request(settings)
            changed["auth"]["secure_cookie"] = True
            changed["scheduler"]["paused"] = True
            changed["scheduler"]["prefetch_per_worker"] = 2
            status, _, saved = request("PUT", "/api/settings", changed)
            assert status == 200 and saved["scheduler"]["paused"]
            assert request("GET", "/api/settings")[0] == 401, "old policy cookie was retained"
            status, headers, _ = request("POST", "/api/auth/login", {"password": password}, authenticated=False)
            assert status == 200 and "; Secure" in headers["Set-Cookie"]
            cookie = headers["Set-Cookie"].split(";", 1)[0]
            csrf = headers["x-csrf-token"]
            assert request("PUT", "/api/settings", settings_request(saved), scheme="http")[0] == 403
            assert request("PUT", "/api/settings", changed)[0] == 409, "stale generation was accepted"

            task = {"input_path": str(source), "output_path": str(destination),
                    "workflow": "synthetic-smoke-workflow", "source": "api", "priority": 0}
            status, _, created = request("POST", "/api/tasks", task)
            assert status == 201, "external media intake failed"
            assert created["input_path"] == str(source) and created["output_path"] == str(destination)
            assert not destination.exists() and list(media.iterdir()) == [source]
            assert request("POST", "/api/tasks", {**task, "input_path": str(workspace / "data/controller.toml")})[0] == 400
            assert request("POST", "/api/tasks", {**task, "output_path": str(workspace / "data/private.mp4")})[0] == 400
            destination.write_bytes(b"synthetic pre-existing output sentinel")
            assert request("POST", "/api/tasks", task)[0] == 400
            assert destination.read_bytes() == b"synthetic pre-existing output sentinel"

            config_file = workspace / "data/controller.toml"
            document = config_file.read_text()
            assert "prefetch_per_worker = 2" in document
            with sqlite3.connect(workspace / "data/controller.sqlite3") as database:
                assert database.execute("SELECT server_port, paused, prefetch_per_worker, configuration_initialized, config_document, pending_config_document FROM controller_settings").fetchone() == (3001, 0, 1, 0, "", None)
                database.execute("UPDATE controller_settings SET prefetch_per_worker = 99, paused = 0")
            edited = document.replace("prefetch_per_worker = 2", "prefetch_per_worker = 7")
            config_file.write_text(edited)
            assert request("GET", "/api/settings")[2]["scheduler"]["prefetch_per_worker"] == 2
            stop()
            assert config_file.read_text() == edited, "shutdown overwrote a manual TOML edit"
            start([])
            status, _, restarted = request("GET", "/api/settings")
            assert status == 200, "Secure session did not survive restart"
            assert restarted["scheduler"]["prefetch_per_worker"] == 7
            assert restarted["scheduler"]["paused"] and restarted["server"]["port"] == port
            assert request("GET", f"/api/tasks/{created['id']}")[0] == 200
            print("PASS: archive startup, HTTPS setup/Secure transition, Settings CAS/TOML, external/private paths, graceful restart, durable sessions/tasks")
        finally:
            stop()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    run(parser.parse_args().binary.resolve(strict=True))
