#!/usr/bin/env python3
"""Pipe interface to the store-owning service and its optional loopback TCP peer."""
import argparse
import json
import os
from pathlib import Path
import queue
import shlex
import subprocess
import threading
import uuid

STOPPED = "Local service stopped — no inspection data loaded"


def say(text):
    print(text, flush=True)


def say_review_status(value):
    for item in value["needs_review"]:
        say(f"Needs review: {item}")
    for item in value["resolved"]:
        say(f"Resolved: {item}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("store", type=Path)
    parser.add_argument("--service", type=Path, default=Path(__file__).parent / "target/debug/fieldcheck")
    parser.add_argument("--reply-barrier", type=Path, help="external journey control: pause AFTER commit, BEFORE reply")
    parser.add_argument("--writer", type=int, choices=[0, 1], default=0)
    network = parser.add_mutually_exclusive_group()
    network.add_argument("--listen", help="loopback IP:port for the fixed peer")
    network.add_argument("--connect", help="loopback IP:port; reconnect automatically")
    args = parser.parse_args()
    draft_path = args.store.with_name(args.store.name + ".draft.json")
    draft = json.loads(draft_path.read_text()) if draft_path.exists() else None
    events = queue.Queue()
    child = None
    loaded = False
    saving = False
    records = []

    def reader(process):
        for line in process.stdout:
            events.put(("service", process, json.loads(line)))
        process.wait()
        events.put(("stopped", process, None))

    def reopen():
        nonlocal child, loaded, records, saving
        if child is not None and child.poll() is None:
            if saving:
                say("Saving…")
                return
            # EOF lets the service finish its stdin loop and release its writer
            # lease before a replacement tries to open the same store.
            child.stdin.close()
            child.wait()
        elif saving:
            say("Save status unknown — reopen to check")
        saving = False
        loaded = False
        records = []
        command = [str(args.service.resolve()), str(args.store.resolve())]
        if args.reply_barrier:
            command.extend(["--reply-barrier", str(args.reply_barrier.resolve())])
        command.extend(["--writer", str(args.writer)])
        if args.listen:
            command.extend(["--listen", args.listen])
        if args.connect:
            command.extend(["--connect", args.connect])
        child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        threading.Thread(target=reader, args=(child,), daemon=True).start()

    def commands():
        for line in __import__("sys").stdin:
            events.put(("command", None, line.strip()))
        events.put(("command", None, "quit"))

    threading.Thread(target=commands, daemon=True).start()
    say('Commands: draft Pass|Fail "inspector" "note"; save; show; reopen; quit')
    reopen()
    try:
        while True:
            kind, process, value = events.get()
            if kind == "stopped":
                if process is not child:
                    continue
                if saving:
                    say("Save status unknown — reopen to check")
                saving = loaded = False
                records = []
                say(STOPPED)
            elif kind == "service":
                if process is not child:
                    continue
                if value.get("ready"):
                    loaded = True
                    records = value["records"]
                    say(value["checklist"]["text"])
                    say(f'Local service PID {value["pid"]}')
                    for record in records:
                        say(json.dumps(record, ensure_ascii=False))
                    say_review_status(value)
                    if draft:
                        match = next((r for r in records if json.loads(r["record"])["event_id"] == draft["event_id"]), None)
                        if match:
                            if json.loads(match["record"]) != draft:
                                loaded = False
                                say("Could not recover this checklist")
                            else:
                                say("Saved on this device")
                                draft = None
                                draft_path.unlink(missing_ok=True)
                elif "listening" in value:
                    say('Listening ' + value["listening"])
                elif value.get("status"):
                    records = value["records"]
                    say(value["network"])
                    say(value["delivery"])
                    say(json.dumps({"records": records, "draft": draft,
                                    "peer_confirmed": value["peer_confirmed"]}, ensure_ascii=False))
                    say_review_status(value)
                elif "saved" in value:
                    saving = False
                    saved = value["saved"]
                    if saved not in records:
                        records.append(saved)
                    say("Saved on this device")
                    say(json.dumps(saved, ensure_ascii=False))
                    draft = None
                    draft_path.unlink(missing_ok=True)
                elif "storage_error" in value:
                    saving = False
                    say("Not saved — local storage error; fix the storage problem, then reopen")
                    say(value["storage_error"])
                elif "recovery_error" in value:
                    loaded = False
                    records = []
                    messages = {
                        "missing_parent": "Store parent directory does not exist; create the parent and reopen",
                        "owned": "Another Fieldcheck process owns this store; close it and reopen",
                        "storage": "Could not open this store because of a storage error; fix the storage problem and reopen",
                        "configuration": "Store writer configuration does not match; reopen with the original writer",
                        "replay": "Could not recover this checklist; inspect the damaged store before reopening",
                        "startup": "Could not start this store; inspect its local files and reopen",
                    }
                    say(messages.get(value.get("recovery_kind"), "Could not start this store; inspect its local files and reopen"))
                    say(value["recovery_error"])
                else:
                    saving = False
                    say(value["error"])
            else:
                try:
                    words = shlex.split(value)
                    if not words:
                        continue
                    if words[0] == "quit":
                        break
                    if words[0] == "reopen":
                        reopen()
                        continue
                    # poll also prevents edits during the gap before the EOF event.
                    if not loaded or child.poll() is not None:
                        say(STOPPED)
                        continue
                    if saving:
                        say("Saving…")
                        continue
                    if words[0] == "draft" and len(words) == 4 and words[1] in ("Pass", "Fail") and words[2].strip():
                        if draft:
                            say("Existing draft retained; save or reopen to resolve it")
                            continue
                        candidate = dict(schema_version=1, checklist_version=1,
                                     item_id="north-yard/gate-3/latch", event_id=str(uuid.uuid4()),
                                     inspector=words[2], answer=words[1], note=words[3], observed_event_ids=[])
                        # Persist identity BEFORE submission; retain on all uncertain outcomes.
                        with draft_path.open("x") as output:
                            json.dump(candidate, output, ensure_ascii=False)
                            output.flush()
                            os.fsync(output.fileno())
                        directory = os.open(draft_path.parent, os.O_RDONLY)
                        try:
                            os.fsync(directory)
                        finally:
                            os.close(directory)
                        draft = candidate
                        say("Draft " + json.dumps(draft, ensure_ascii=False))
                    elif words == ["save"] and draft:
                        say("Saving…")
                        saving = True
                        try:
                            child.stdin.write(json.dumps(draft, ensure_ascii=False) + "\n")
                            child.stdin.flush()
                        except (BrokenPipeError, OSError):
                            say("Save status unknown — reopen to check")
                            saving = loaded = False
                            records = []
                            say(STOPPED)
                    elif words == ["show"]:
                        child.stdin.write('{"command":"status"}\n')
                        child.stdin.flush()
                    else:
                        say("Expected draft Pass|Fail \"inspector\" \"note\", save, show, reopen or quit")
                except (ValueError, OSError) as error:
                    say(f"Draft retained; interface error: {error}")
    finally:
        if child is not None and child.poll() is None:
            child.terminate()
            child.wait()


if __name__ == "__main__":
    main()
