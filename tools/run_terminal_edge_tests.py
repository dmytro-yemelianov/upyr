#!/usr/bin/env python3
"""Run offline headless tests and write a machine-readable regression report.

Never activates an app or posts keyboard/mouse events. macOS also reads the
installed layout mapping on the test process's main thread without selecting a
layout or changing preferences. Superset, clipboard and user config are unused.
"""
from __future__ import annotations

import argparse
import datetime
import json
from pathlib import Path
import platform
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT / 'target/terminal-edge-report.json')
    parser.add_argument('--timeout', type=int, default=300, help='Cargo timeout in seconds (default: 300)')
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error('--timeout must be positive')
    command = ['cargo', 'test', '--workspace', '--all-targets', '--locked', '--offline', '--', '--nocapture']
    failure = None
    try:
        result = subprocess.run(command, cwd=ROOT, capture_output=True, encoding='utf-8', errors='replace', timeout=args.timeout)
        returncode = result.returncode
        output = result.stdout + result.stderr
        failure = f'Cargo exited with status {returncode}' if returncode else None
    except subprocess.TimeoutExpired as error:
        def decoded(value: str | bytes | None) -> str:
            return value.decode('utf-8', errors='replace') if isinstance(value, bytes) else value or ''
        failure = f'Cargo timed out after {args.timeout} seconds'
        output = decoded(error.stdout) + decoded(error.stderr) + '\n' + failure + '\n'
        returncode = 124
    except OSError as error:
        failure = f'Could not start Cargo: {error}'
        output = failure + '\n'
        returncode = 127
    totals = [tuple(map(int, match)) for match in re.findall(
        r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;', output)]
    version = re.search(r'(?m)^version = "([^"]+)"', (ROOT / 'Cargo.toml').read_text(encoding='utf-8')).group(1)
    coverage = {}
    for field, marker in [('curated_evaluations', 'curated evaluations'),
                          ('generated_sequences', 'generated command sequences'),
                          ('control_format_checks', 'control/format injection checks')]:
        match = re.search(r'(\d+) ' + re.escape(marker), output)
        coverage[field] = int(match.group(1)) if match else None
    report = dict(version=version, completed_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  status='passed' if returncode == 0 else 'failed',
                  platform=platform.system(), source_commit=subprocess.run(
                      ['git', 'rev-parse', 'HEAD'], cwd=ROOT, capture_output=True,
                      encoding='utf-8', errors='replace', check=False).stdout.strip(),
                  returncode=returncode, failure=failure,
                  command=command, passed=sum(t[0] for t in totals),
                  failed=sum(t[1] for t in totals), ignored=sum(t[2] for t in totals),
                  focused_checks=[line for line in output.splitlines() if any(marker in line for marker in (
                      'curated evaluations', 'generated command sequences', 'injection checks', 'macOS layout mapping:'))],
                  coverage=coverage, no_gui_input=True, interactive_probe_requested=False)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    log = args.output.with_suffix('.log')
    log.write_text(output, encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False, indent=2))
    print(f'Full test log: {log}')
    return returncode


if __name__ == '__main__':
    raise SystemExit(main())
