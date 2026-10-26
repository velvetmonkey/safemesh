#!/usr/bin/env python3
"""Check Diagnose prose against errors from damaged gold-path Rust stores."""

import argparse
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parents[1]
PAGE = ROOT / "docs/src/content/docs/persist-and-restart.md"
FIXTURE = ROOT / "examples/gold-path/rust"
CASES = ("truncate_half", "flip_middle", "wrong_schema", "changed_tag", "append_frame_byte")


def run(command, cwd, log_dir, name, env):
    result = subprocess.run(command, cwd=cwd, env=env, capture_output=True, text=True)
    (log_dir / f"{name}.stdout").write_text(result.stdout)
    (log_dir / f"{name}.stderr").write_text(result.stderr)
    exit_file = log_dir / f"{name}.exit"
    exit_file.write_text(f"{result.returncode}\n")
    return int(exit_file.read_text()), result.stdout, result.stderr


def instrument(source):
    replacements = (
        "Counter::restart_counter(&counter_dir, config).unwrap()",
        "Members::restart_utf8_set(&members_dir, config).unwrap()",
    )
    for old in replacements:
        if source.count(old) != 1:
            raise ValueError(f"fixture restart expression changed: {old}")
        new = old.removesuffix(".unwrap()") + (
            '.unwrap_or_else(|e| { eprintln!("RESTART_DEBUG: {e:?}"); '
            'eprintln!("RESTART_DISPLAY: {e}"); std::process::exit(101) })'
        )
        source = source.replace(old, new)
    return source


def prepare_binary(work, env):
    fixture = work / "examples/gold-path/rust"
    (fixture / "src").mkdir(parents=True)
    for name in ("Cargo.toml", "Cargo.lock"):
        shutil.copy2(FIXTURE / name, fixture / name)
    (fixture / "src/main.rs").write_text(instrument((FIXTURE / "src/main.rs").read_text()))
    (work / "rust").symlink_to(ROOT / "rust", target_is_directory=True)
    shutil.copy2(ROOT / "rust-toolchain.toml", work)
    target = Path(env.get("CARGO_TARGET_DIR", FIXTURE / "target")).resolve()
    env["CARGO_TARGET_DIR"] = str(target)
    code, _, stderr = run(
        ["cargo", "build", "--quiet", "--locked", "--manifest-path", str(fixture / "Cargo.toml")],
        work, work, "build", env,
    )
    if code:
        raise RuntimeError(f"build exit {code}: {stderr.strip()}")
    binary = target / "debug/safemesh-first-use"
    if not binary.is_file():
        raise RuntimeError(f"built binary absent: {binary}")
    return binary


def mutate(name, store):
    path = store / "members/writer-0.journal"
    other = store / "counter/writer-0.journal"
    if not path.is_file() or not other.is_file():
        raise ValueError("persist did not create the observed writer journal files")
    data = bytearray(path.read_bytes())
    if name == "truncate_half":
        data = data[:len(data) // 2]
    elif name == "flip_middle":
        data[len(data) // 2] ^= 1
    elif name == "wrong_schema":
        data = bytearray(other.read_bytes())
    elif name == "changed_tag":
        data[36] ^= 1
    elif name == "append_frame_byte":
        end = 36 + int.from_bytes(data[32:36], "little")
        data[end:end] = b"\xff"
        data[32:36] = (end - 36 + 1).to_bytes(4, "little")
    elif name == "no_change":
        pass  # A test-only sentinel: the real restart must succeed.
    else:
        raise ValueError(f"unknown case {name}")
    path.write_bytes(data)


def diagnose_section(page):
    text = page.read_text()
    match = re.search(r"^## Diagnose a failed Rust exercise\s*$\n(.*?)(?=^## |\Z)", text, re.M | re.S)
    if not match:
        raise ValueError("Diagnose section absent")
    return " ".join(match.group(1).replace("`", "").split())


def history_display_on_page(section, variant, display):
    folded_display = " ".join(display.split())
    return any(f"{variant}{context} (Display: {folded_display})" in section
               for context in ("", " on restart"))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--page", type=Path, default=PAGE)
    parser.add_argument("--cases", default=",".join(CASES))
    args = parser.parse_args()
    names = args.cases.split(",") if args.cases else []
    if any(name not in (*CASES, "no_change") for name in names):
        print("UNREAD unknown case")
        return 2
    try:
        section = diagnose_section(args.page)
        with tempfile.TemporaryDirectory(prefix="diagnose-history-") as folder:
            work = Path(folder)
            env = os.environ.copy()
            binary = prepare_binary(work, env)
            named = missing = 0
            for name in names:
                case = work / "cases" / name
                case.mkdir(parents=True)
                code, _, stderr = run([str(binary), "persist"], case, case, "persist", env)
                if code:
                    raise RuntimeError(f"persist {name} exit {code}: {stderr.strip()}")
                store = case / ".gold-rust"
                store.rename(case / "pristine")
                shutil.copytree(case / "pristine", store)
                mutate(name, store)
                code, _, stderr = run([str(binary), "restart"], case, case, "restart", env)
                debug = re.search(r"^RESTART_DEBUG: (History\([^\n]+\))$", stderr, re.M)
                display = re.search(r"^RESTART_DISPLAY: ([^\n]+)$", stderr, re.M)
                variant = debug.group(1) if debug else "other"
                on_page = (code != 0 and debug is not None and display is not None
                           and history_display_on_page(section, variant, display.group(1)))
                named += int(on_page)
                missing += int(not on_page)
                print(f"case {name}: {variant} on page {'yes' if on_page else 'no'}", flush=True)
            print(f"DIAGNOSE HISTORY CASES {len(names)}, NAMED {named}, MISSING {missing}")
            return int(missing != 0 or len(names) < 3)
    except (OSError, ValueError, RuntimeError) as error:
        print(f"UNREAD {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
