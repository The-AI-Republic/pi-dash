# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Offline smoke: one real engine process refreshes auth after a simulated expiry.

Invoked by the ignored native test with its actual rendered engine config.
No external model, session, or network service is used.
"""

import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import queue
import subprocess
import threading


def run(engine, config_home, token_file):
    token_file = Path(token_file)
    token_file.write_text("smoke-old")
    authorizations = []

    class Gateway(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_POST(self):
            self.rfile.read(int(self.headers.get("Content-Length", "0")))
            authorizations.append(self.headers.get("Authorization"))
            if len(authorizations) == 2:
                # Simulate expiry while the engine is alive. The desktop's
                # background refresh has already atomically replaced its file.
                temporary = token_file.with_suffix(".tmp")
                temporary.write_text("smoke-new")
                temporary.replace(token_file)
                self.send_response(401)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(
                    b'{"error":{"message":"expired","type":"invalid_request_error","code":"invalid_api_key"}}'
                )
                return
            if authorizations[-1] != (
                "Bearer smoke-old" if len(authorizations) == 1 else "Bearer smoke-new"
            ):
                self.send_error(403)
                return
            item = {
                "id": "msg_smoke",
                "type": "message",
                "role": "assistant",
                "status": "completed",
                "content": [{"type": "output_text", "text": "OK", "annotations": []}],
            }
            response = {
                "id": "resp_smoke",
                "object": "response",
                "created_at": 0,
                "status": "completed",
                "output": [item],
                "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2},
            }
            events = [
                {
                    "type": "response.created",
                    "response": {**response, "status": "in_progress", "output": []},
                },
                {
                    "type": "response.output_item.added",
                    "output_index": 0,
                    "item": {**item, "status": "in_progress", "content": []},
                },
                {
                    "type": "response.output_text.delta",
                    "item_id": "msg_smoke",
                    "output_index": 0,
                    "content_index": 0,
                    "delta": "OK",
                },
                {"type": "response.output_item.done", "output_index": 0, "item": item},
                {"type": "response.completed", "response": response},
            ]
            payload = "".join(
                f"event: {event['type']}\ndata: {json.dumps(event)}\n\n" for event in events
            ).encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

    server = ThreadingHTTPServer(("127.0.0.1", 0), Gateway)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    environment = {**os.environ, "CODEX_HOME": str(config_home)}
    environment.pop("PIDASH_GATEWAY_TOKEN", None)
    command = [
        engine,
        "app-server",
        "--strict-config",
        "-c",
        f'model_providers.pidash.base_url="http://127.0.0.1:{server.server_port}/v1"',
    ]
    process = subprocess.Popen(
        command,
        env=environment,
        cwd=config_home,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    messages = queue.Queue()
    errors = []

    def read_stdout():
        for line in process.stdout:
            messages.put(json.loads(line))
        messages.put({"process_exited": True})

    def read_stderr():
        errors.extend(process.stderr)

    threading.Thread(target=read_stdout, daemon=True).start()
    threading.Thread(target=read_stderr, daemon=True).start()

    def send(message):
        process.stdin.write(json.dumps(message) + "\n")
        process.stdin.flush()

    def wait(predicate):
        while True:
            message = messages.get(timeout=45)
            assert "process_exited" not in message, "".join(errors)
            assert "error" not in message, message
            if predicate(message):
                return message

    try:
        send(
            {
                "id": 1,
                "method": "initialize",
                "params": {"clientInfo": {"name": "pidash-auth-smoke", "version": "1"}},
            }
        )
        wait(lambda message: message.get("id") == 1)
        send({"method": "initialized", "params": {}})
        send(
            {
                "id": 2,
                "method": "thread/start",
                "params": {
                    "cwd": str(config_home),
                    "model": "gpt-5-codex",
                    "sandbox": "read-only",
                    "approvalPolicy": "never",
                },
            }
        )
        thread = wait(lambda message: message.get("id") == 2)["result"]["thread"]["id"]
        for request_id in [3, 4]:
            send(
                {
                    "id": request_id,
                    "method": "turn/start",
                    "params": {
                        "threadId": thread,
                        "input": [{"type": "text", "text": "Say OK. Do not use tools."}],
                    },
                }
            )
            wait(lambda message: message.get("id") == request_id)
            completed = wait(lambda message: message.get("method") == "turn/completed")
            assert completed["params"]["turn"]["status"] == "completed", completed
        assert authorizations == ["Bearer smoke-old", "Bearer smoke-old", "Bearer smoke-new"], (
            authorizations
        )
        assert process.poll() is None, "engine unexpectedly exited"
        print(
            "PASS: same engine process completed two turns, refreshed after 401, and used the rotated token"
        )
    finally:
        process.kill()
        process.wait(timeout=10)
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--engine", required=True)
    parser.add_argument("--config-home", required=True)
    parser.add_argument("--token-file", required=True)
    arguments = parser.parse_args()
    run(arguments.engine, arguments.config_home, arguments.token_file)
