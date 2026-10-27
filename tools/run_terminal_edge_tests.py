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
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT / 'target/terminal-edge-report.json')
    args = parser.parse_args()
    command = ['cargo', 'test', '--workspace', '--all-targets', '--locked', '--offline', '--', '--nocapture']
    result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, timeout=300)
    output = result.stdout + result.stderr
    totals = [tuple(map(int, match)) for match in re.findall(
        r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;', output)]
    version = re.search(r'(?m)^version = "([^"]+)"', (ROOT / 'Cargo.toml').read_text()).group(1)
    coverage = {}
    for field, marker in [('curated_evaluations', 'curated evaluations'),
                          ('generated_sequences', 'generated command sequences'),
                          ('control_format_checks', 'control/format injection checks')]:
        match = re.search(r'(\d+) ' + re.escape(marker), output)
        coverage[field] = int(match.group(1)) if match else None
    report = dict(version=version, completed_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  status='passed' if result.returncode == 0 else 'failed',
                  command=command, passed=sum(t[0] for t in totals),
                  failed=sum(t[1] for t in totals), ignored=sum(t[2] for t in totals),
                  focused_checks=[line for line in output.splitlines() if any(marker in line for marker in (
                      'curated evaluations', 'generated command sequences', 'injection checks', 'macOS layout mapping:'))],
                  coverage=coverage, no_gui_input=True, interactive_probe_requested=False)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    log = args.output.with_suffix('.log')
    log.write_text(output)
    print(json.dumps(report, ensure_ascii=False, indent=2))
    print(f'Full test log: {log}')
    return result.returncode


if __name__ == '__main__':
    raise SystemExit(main())
