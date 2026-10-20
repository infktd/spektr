#!/usr/bin/env python3
"""A fake spektr exec module (standard library only) for the engine's
exec-protocol tests. All data is fictional.

usage: fake.py <module-name> [--serve]

Behaviour depends on the input key: `boom` errors, `silent` exits without
`done`, `garbage` writes a non-JSON line, `slow` sleeps, anything else
reports a Lemmy account. If FAKE_MARKER is set, every run appends a line to
that file (tests use it to prove a blocked module never ran).
"""
import json
import os
import sys
import time

NAME = sys.argv[1]
SERVE = "--serve" in sys.argv


def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


def run(msg):
    job = msg["job"]
    key = msg["input"]["key"]
    marker = os.environ.get("FAKE_MARKER")
    if marker:
        with open(marker, "a") as f:
            f.write(f"{NAME} pid={os.getpid()} input={key}\n")
    if key == "boom":
        send({"type": "error", "job": job, "code": "upstream", "message": "upstream exploded", "retryable": False})
        return
    if key == "silent":
        sys.exit(3)
    if key == "garbage":
        sys.stdout.write("this is not json\n")
        sys.stdout.flush()
        return
    if key == "slow":
        time.sleep(30)
        return
    o = {"type": "observation", "job": job, "id": "o1",
         "source_url": f"https://lemmy.example/api/v3/user?username={key}",
         "content_type": "application/json", "raw": json.dumps({"name": key})}
    send(o)
    send({"type": "entity", "job": job, "ref": "e1",
          "entity": {"type": "account", "value": f"lemmy:{key}@lemmy.example"}, "evidence": ["o1"]})
    send({"type": "attr", "job": job, "target": "e1", "name": "display_name",
          "value": {"text": "Jordan M."}, "evidence": ["o1"]})
    send({"type": "attr", "job": job, "target": "e1", "name": "last_active",
          "value": {"time": "2026-10-16T09:12:00Z"}, "event": True, "evidence": ["o1"]})
    send({"type": "attr", "job": job, "target": "e1", "name": "token_seen",
          "value": {"bool": "FAKE_TOKEN" in msg["secrets"]}, "evidence": ["o1"]})
    send({"type": "edge", "job": job, "src": "$input", "predicate": "has_account", "dst": "e1", "evidence": ["o1"]})
    send({"type": "entity", "job": job, "ref": "i1",
          "entity": {"type": f"x.{NAME}.instance", "value": "Lemmy.Example"}, "evidence": ["o1"]})
    send({"type": "edge", "job": job, "src": "e1", "predicate": f"x.{NAME}.member_of_instance", "dst": "i1",
          "evidence": ["o1"]})
    send({"type": "progress", "job": job, "pct": 100, "note": "1/1 instances"})
    sys.stderr.write(f"token was {msg['secrets'].get('FAKE_TOKEN', '')}\n")
    send({"type": "done", "job": job})


def main():
    hello = json.loads(sys.stdin.readline())
    assert hello["type"] == "hello" and hello["protocol"] == 1, hello
    if SERVE:
        send({"type": "ready"})
    for line in sys.stdin:
        msg = json.loads(line)
        if msg["type"] == "run":
            run(msg)
        elif msg["type"] == "shutdown":
            break


main()
