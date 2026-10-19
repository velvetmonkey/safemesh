#!/usr/bin/env python3
"""Require release notes for each relevant first-parent pull request merge."""

import argparse
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path.cwd()


def git(*args: str) -> str:
    return subprocess.run(['git', *args], cwd=ROOT, check=True, text=True, capture_output=True).stdout


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument('tag')
    parser.add_argument('--rev', default=None)
    args = parser.parse_args()
    rev = args.rev or args.tag
    notes = ROOT / 'docs' / 'releases' / f'{args.tag}.md'
    if not notes.is_file():
        print(f'MISSING NOTES: docs/releases/{args.tag}.md')
        return 1
    try:
        prev = git('describe', '--tags', '--abbrev=0', f'{rev}^').strip()
        commits = git('log', '--first-parent', '--format=%H %s', f'{prev}..{rev}').splitlines()
        merged = set()
        optional = set()
        uncounted = []
        for entry in commits:
            commit, subject = entry.split(' ', 1)
            match = re.match(r'Merge pull request #(\d+) ', subject)
            if not match:
                uncounted.append((commit, subject))
                continue
            paths = git('diff', '--name-only', f'{commit}^1', commit).splitlines()
            if paths and all(path.startswith('docs/releases/') for path in paths):
                optional.add(int(match.group(1)))
                continue
            merged.add(int(match.group(1)))
    except subprocess.CalledProcessError as exc:
        print(f'GIT ERROR: {exc.stderr.strip()}', file=sys.stderr)
        return 1
    listed = {int(n) for n in re.findall(r'^- PR #(\d+):', notes.read_text(), re.M)}
    missing = sorted(merged - listed)
    extra = sorted(listed - merged - optional)
    for n in missing:
        print(f'MISSING PR #{n}')
    for n in extra:
        print(f'EXTRA PR #{n}')
    for commit, subject in uncounted:
        print(f'UNCOUNTED COMMIT {commit} {subject}')
    if missing or extra or uncounted:
        return 1
    print(f'Release notes complete for {args.tag}: {len(listed)} PRs')
    return 0


if __name__ == '__main__':
    sys.exit(main())
