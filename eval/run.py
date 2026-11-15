#!/usr/bin/env python3
"""Score a local model on questions.jsonl with and without the offline library.

Talks to any OpenAI-compatible server (llamafile / llama-server on
127.0.0.1:8080). "With library", the model gets the tools `zim-mcp serve`
offers over MCP, exactly as any MCP client would. Standard library only.

    python3 eval/run.py --zim /path/to/zim [--zim-mcp zim-mcp] [--url http://127.0.0.1:8080] [--limit N]
"""

import argparse, json, subprocess, sys, urllib.request
from pathlib import Path

SYSTEM = "Answer briefly."


class Mcp:
    """Minimal MCP stdio client for `zim-mcp serve`: the eval uses exactly the
    tools, descriptions and outputs any MCP client gets."""

    def __init__(self, zim_mcp, zim):
        self.p = subprocess.Popen([zim_mcp, "serve", zim, "--no-library"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        self.id = 0
        init = self.call("initialize", {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "eval", "version": "1"}})
        self.instructions = init.get("instructions", "")  # what an MCP host shows the model
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        self.tools = [
            {"type": "function", "function": {"name": t["name"], "description": t.get("description", ""), "parameters": t["inputSchema"]}}
            for t in self.call("tools/list", {})["tools"]
        ]

    def send(self, msg):
        self.p.stdin.write(json.dumps(msg) + "\n")
        self.p.stdin.flush()

    def call(self, method, params):
        self.id += 1
        self.send({"jsonrpc": "2.0", "id": self.id, "method": method, "params": params})
        while True:
            msg = json.loads(self.p.stdout.readline())
            if msg.get("id") == self.id:
                return msg.get("result") or {"content": [{"type": "text", "text": str(msg.get("error"))}]}

    def run(self, name, args):
        r = self.call("tools/call", {"name": name, "arguments": args})
        return "\n".join(c.get("text", "") for c in r.get("content", []))


def chat(url, messages, tools=None):
    body = {"messages": messages, "temperature": 0}
    if tools:
        body["tools"] = tools
    req = urllib.request.Request(f"{url}/v1/chat/completions", json.dumps(body).encode(), {"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=600) as r:
        return json.load(r)["choices"][0]["message"]


def ask(url, q, mcp=None, max_calls=6):
    system = f"{SYSTEM} {mcp.instructions}" if mcp else SYSTEM
    messages = [{"role": "system", "content": system}, {"role": "user", "content": q}]
    for _ in range(max_calls + 1):
        msg = chat(url, messages, mcp.tools if mcp else None)
        calls = msg.get("tool_calls") or []
        if not calls:
            return msg.get("content") or "", [m for m in messages if m["role"] == "tool"]
        messages.append(msg)
        for c in calls:
            try:
                args = json.loads(c["function"].get("arguments") or "{}")
            except json.JSONDecodeError:
                args = {}
            messages.append({"role": "tool", "tool_call_id": c.get("id", ""), "content": mcp.run(c["function"]["name"], args)})
    return "", messages


def correct(answer, expect):
    a = answer.lower()
    return any(e.lower() in a for e in expect)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--url", default="http://127.0.0.1:8080")
    ap.add_argument("--zim", required=True)
    ap.add_argument("--zim-mcp", default="zim-mcp")
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--out", default="eval/results.json")
    a = ap.parse_args()
    qs = [json.loads(l) for l in open(Path(__file__).with_name("questions.jsonl"))]
    if a.limit:
        qs = qs[: a.limit]
    mcp = Mcp(a.zim_mcp, a.zim)
    results, score = [], {"without": 0, "with": 0}
    for i, q in enumerate(qs, 1):
        plain, _ = ask(a.url, q["q"])
        lib, tool_msgs = ask(a.url, q["q"], mcp)
        ok = {"without": correct(plain, q["expect"]), "with": correct(lib, q["expect"])}
        for k in ok:
            score[k] += ok[k]
        results.append({"q": q["q"], "without": plain, "with": lib, "tool_calls": len(tool_msgs), **{f"ok_{k}": v for k, v in ok.items()}})
        print(f"{i:2}/{len(qs)} without:{'✓' if ok['without'] else '✗'} with:{'✓' if ok['with'] else '✗'} ({len(tool_msgs)} tool calls)  {q['q']}", flush=True)
    n = len(qs)
    print(f"\nwithout library: {score['without']}/{n}   with library: {score['with']}/{n}")
    Path(a.out).write_text(json.dumps({"score": score, "n": n, "results": results}, indent=2, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
