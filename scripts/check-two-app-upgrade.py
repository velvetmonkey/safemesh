#!/usr/bin/env python3
"""Check the two-app demo's pre-frame upgrade warning against its code."""

import argparse
import ast
from pathlib import Path
import re


ROOT = Path(__file__).resolve().parents[1]


def refusal_text(source):
    tree = ast.parse(source)
    functions = [node for node in tree.body
                 if isinstance(node, ast.FunctionDef) and node.name == "unframe_record"]
    if len(functions) != 1:
        raise ValueError("expected one unframe_record function")
    messages = []
    for node in ast.walk(functions[0]):
        if not isinstance(node, ast.Raise) or not isinstance(node.exc, ast.Call):
            continue
        call = node.exc
        if not isinstance(call.func, ast.Name) or call.func.id != "ValueError":
            continue
        if len(call.args) != 1 or not isinstance(call.args[0], ast.JoinedStr):
            continue
        pieces = []
        for part in call.args[0].values:
            if isinstance(part, ast.Constant) and isinstance(part.value, str):
                pieces.append(part.value)
            elif (isinstance(part, ast.FormattedValue) and
                  isinstance(part.value, ast.Name) and part.value.id == "position"):
                pieces.append("{position}")
            else:
                raise ValueError("unexpected unframe_record refusal expression")
        messages.append("".join(pieces))
    if len(messages) != 1 or messages[0].count("{position}") != 1:
        raise ValueError("expected one position-bearing ValueError in unframe_record")
    return messages[0]


def missing_parts(root):
    readme = (root / "demos/two-app-inventory/README.md").read_text()
    node = (root / "demos/two-app-inventory/node.py").read_text()
    test = (root / "demos/two-app-inventory/test_sync.py").read_text()
    match = re.search(r"(?ms)^- \*\*Errors are not a recovery protocol\.\*\*(.*?)(?=^- \*\*|^## |\Z)", readme)
    bullet = re.sub(r"\s+", " ", match.group(1)) if match else ""
    missing = []
    if not (re.search(r"store .*written before PR 344|store .*earlier version.*before PR 344", bullet, re.I)
            and re.search(r"(?:no frame|without a frame) on its rows", bullet, re.I)
            and "accepted legacy row" in test):
        missing.append("pre-frame store and legacy-row assertion")
    if not re.search(r"(?:does not|will not) open after upgrade", bullet, re.I):
        missing.append("store does not open after upgrade")
    if not re.search(r"\bno migration\b", bullet, re.I):
        missing.append("no migration")
    try:
        refusal = refusal_text(node).replace("{position}", "1")
    except (SyntaxError, ValueError):
        missing.append("node.py refusal text")
    else:
        if refusal not in bullet:
            missing.append(f"refusal text from node.py: {refusal}")
    return missing


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    args = parser.parse_args()
    missing = missing_parts(args.root)
    for part in missing:
        print(f"MISSING {part}")
    print(f"TWO-APP UPGRADE PARTS 4, MISSING {len(missing)}")
    return 1 if missing else 0


if __name__ == "__main__":
    raise SystemExit(main())
