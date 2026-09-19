#!/usr/bin/env python3
"""Local pipe-only interface. The child process exclusively owns the SafeMesh store."""
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


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("store", type=Path)
    parser.add_argument("--service", type=Path, default=Path(__file__).parent / "target/debug/fieldcheck")
    parser.add_argument("--reply-barrier", type=Path, help="external journey control: pause AFTER commit, BEFORE reply")
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
        nonlocal child, loaded, records
        if child is not None and child.poll() is None:
            say("Service already running")
            return
        loaded = False
        records = []
        command = [str(args.service.resolve()), str(args.store.resolve())]
        if args.reply_barrier:
            command.append(str(args.reply_barrier.resolve()))
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
                    say("Not saved — local storage error")
                    say(value["storage_error"])
                elif "recovery_error" in value:
                    loaded = False
                    records = []
                    say("Could not recover this checklist")
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
                        say(json.dumps({"records": records, "draft": draft}, ensure_ascii=False))
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
