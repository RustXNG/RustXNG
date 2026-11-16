#!/usr/bin/env python3
"""Run one stdio MCP session against a rustxng binary (stdlib only).

This is the part of the agent setup an HTTP check cannot see: the JSON-RPC
handshake, the tool list and one tool call. It contacts no external network
when the caller passes settings that point at a local fake engine.
"""

from __future__ import annotations

import json
import os
import select
import subprocess
import time


class McpSmokeError(RuntimeError):
    """The MCP session did not behave the way an agent expects."""


def _read_response(stream, expected_id, timeout):
    """Wait for the reply with `expected_id`, skipping notifications."""
    deadline = time.monotonic() + timeout
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return None
        ready, _, _ = select.select([stream], [], [], remaining)
        if not ready:
            return None
        line = stream.readline()
        if not line:
            return None
        message = json.loads(line)
        if message.get("id") == expected_id:
            return message


def run(binary, settings_path, workdir, env, query, known_url):
    """Initialize, list the tools and search; raise McpSmokeError on drift."""
    log_path = os.path.join(workdir, "mcp.log")
    with open(log_path, "wb") as log:
        process = subprocess.Popen(
            [binary, "mcp", "--config", settings_path],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=log,
            text=True,
            bufsize=1,
            env=env,
            cwd=workdir,
        )
        try:
            def send(message):
                process.stdin.write(json.dumps(message) + "\n")
                process.stdin.flush()

            send({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "rustxng-smoke", "version": "0"},
                },
            })
            init = _read_response(process.stdout, 1, 20.0)
            if not init or "result" not in init:
                raise McpSmokeError("initialize failed: " + repr(init))
            name = (init["result"].get("serverInfo") or {}).get("name")
            if name != "rustxng":
                raise McpSmokeError("serverInfo.name is " + repr(name))

            send({"jsonrpc": "2.0", "method": "notifications/initialized"})

            send({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
            listing = _read_response(process.stdout, 2, 20.0)
            tools = [
                tool.get("name")
                for tool in (listing or {}).get("result", {}).get("tools", [])
            ]
            for expected in ("search", "read_page"):
                if expected not in tools:
                    raise McpSmokeError(
                        "tools/list lacks " + repr(expected) + ": " + repr(tools)
                    )

            send({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {"name": "search", "arguments": {"query": query, "limit": 3}},
            })
            call = _read_response(process.stdout, 3, 30.0)
            result = (call or {}).get("result") or {}
            if result.get("isError"):
                raise McpSmokeError("search returned an error: " + repr(call)[:400])
            structured = result.get("structuredContent") or {}
            results = structured.get("results") or []
            urls = [item.get("url") for item in results]
            if known_url not in urls:
                raise McpSmokeError("search did not return " + known_url + ": " + repr(urls))
        finally:
            try:
                process.stdin.close()
            except OSError:
                pass
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=10)
    if process.returncode not in (0, None):
        with open(log_path, "rb") as handle:
            detail = handle.read().decode("utf-8", "replace")[-2000:]
        raise McpSmokeError("MCP exited with " + repr(process.returncode) + ": " + detail)