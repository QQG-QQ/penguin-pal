#!/usr/bin/env python3
import json
import sys
import time


TOOLS = [
    {
        "name": "penguin_status",
        "description": "Return a small local status payload for PenguinPal MCP connectivity checks.",
        "inputSchema": {
            "type": "object",
            "properties": {},
            "additionalProperties": False,
        },
    },
    {
        "name": "echo_note",
        "description": "Echo a short note. This is a harmless local read-only demo tool.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "text": {"type": "string"},
            },
            "additionalProperties": False,
        },
    },
]


def send_response(request_id, result=None, error=None):
    response = {"jsonrpc": "2.0", "id": request_id}
    if error is not None:
        response["error"] = error
    else:
        response["result"] = result
    print(json.dumps(response, ensure_ascii=False), flush=True)


def handle_request(message):
    request_id = message.get("id")
    method = message.get("method")
    params = message.get("params") or {}

    if method == "initialize":
        send_response(
            request_id,
            {
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "PenguinPal FastMCP Local Tools", "version": "0.1.0"},
            },
        )
        return

    if method == "tools/list":
        send_response(request_id, {"tools": TOOLS})
        return

    if method == "tools/call":
        tool_name = params.get("name")
        arguments = params.get("arguments") or {}
        if tool_name == "penguin_status":
            send_response(
                request_id,
                {
                    "content": [
                        {
                            "type": "text",
                            "text": "PenguinPal local MCP server is ready.",
                        }
                    ],
                    "isError": False,
                    "checkedAt": int(time.time()),
                },
            )
            return
        if tool_name == "echo_note":
            send_response(
                request_id,
                {
                    "content": [
                        {
                            "type": "text",
                            "text": str(arguments.get("text", "")),
                        }
                    ],
                    "isError": False,
                },
            )
            return
        send_response(
            request_id,
            error={"code": -32601, "message": f"Unknown tool: {tool_name}"},
        )
        return

    if request_id is not None:
        send_response(
            request_id,
            error={"code": -32601, "message": f"Unknown method: {method}"},
        )


def main():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            handle_request(json.loads(line))
        except Exception as error:
            send_response(None, error={"code": -32603, "message": str(error)})


if __name__ == "__main__":
    main()
