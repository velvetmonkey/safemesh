#!/usr/bin/env python3
"""Fail when a release gate or a current version identity is missing."""

import argparse
from pathlib import Path
import re
import sys
import tomllib
import json

ROOT = Path(__file__).resolve().parent.parent


def check(path: Path) -> list[str]:
    text = path.read_text()
    required = {
        'tag trigger': r'tags:\s*\["v\*\.\*\.\*"\]',
        'read-only default': r'permissions:\s*\n\s+contents:\s*read',
        'version tag': r'test "\$GITHUB_REF_NAME" = "v\$version"',
        'exact tag commit': r'git rev-parse "\$\{GITHUB_REF_NAME\}\^\{commit\}".*\$GITHUB_SHA',
        'exact-commit CI': r'head_sha=\$GITHUB_SHA.*event=push.*\.head_sha == \$sha and \.head_branch == "main" and \.conclusion == "success"',
        'required human reviewer': r'environments/release-publish.*required_reviewers.*reviewer\.login == "velvetmonkey"',
        'published release refusal': r'Refuse an already published release.*release-state.*= true',
        'asset script': r'build-release-assets\.sh',
        'draft release': r'gh release create.*--draft --verify-tag',
        'notes file': r'--notes-file.*docs/releases/v0\.2\.0\.md',
        'download draft': r'gh release download.*\n.*cmp SHA256SUMS.*\n.*sha256sum --check SHA256SUMS',
        'draft byte comparison': r'cmp "\$name" "\$RUNNER_TEMP/original/\$name"',
        'publish environment': r'publish:\s*\n\s+needs: verify-draft\s*\n\s+environment: release-publish',
        'draft state before publish': r'Publish only the verified draft.*isDraft.*gh release edit.*--draft=false',
    }
    errors = [name for name, pattern in required.items() if not re.search(pattern, text, re.S)]
    for name in ('safemesh-crdt', 'safemesh-ffi', 'safemesh-python', 'safemesh-wasm'):
        data = tomllib.loads((ROOT / 'rust/crates' / name / 'Cargo.toml').read_text())
        if data['package']['version'] != '0.2.0':
            errors.append(f'{name} version')
        if name != 'safemesh-crdt' and data['dependencies']['safemesh-crdt']['version'] != '0.2.0':
            errors.append(f'{name} core dependency')
    if tomllib.loads((ROOT / 'rust/crates/safemesh-python/pyproject.toml').read_text())['project']['version'] != '0.2.0':
        errors.append('Python package version')
    if json.loads((ROOT / 'web/package.json').read_text())['version'] != '0.2.0':
        errors.append('web package version')
    return errors


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--file', type=Path, default=ROOT / '.github/workflows/release.yml')
    args = parser.parse_args()
    errors = check(args.file)
    if errors:
        print('Release gate missing: ' + ', '.join(errors), file=sys.stderr)
        sys.exit(1)
    print('Release gates and current versions present')
