#!/usr/bin/env python3
"""Opt-in live AppKit/Paradev test of the installed Upyr binary.

Default is prepare-only. --run requires an unlocked Mac and existing Cua Driver
and physical-input permissions. The test owns its editor/raw PTY, aborts on
outside input, uses a private config copy with learning disabled, and restores
the normal Upyr process. No shell receives test keystrokes. The Chrome observer
uses a loopback-only HTTP server; editor applications retain their own behavior.
"""
from __future__ import annotations

import argparse
from collections import defaultdict
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import tomllib
import uuid

if __package__:
    from . import superset_input_probe as probe
    from .real_editor_probe import Editor
else:
    import superset_input_probe as probe
    from real_editor_probe import Editor

ROOT = Path(__file__).resolve().parents[1]
APP = Path('/Applications/Upyr.app')
BACKGROUND = APP / 'Contents/MacOS/upyr-background'
CONFIG = Path.home() / 'Library/Application Support/dev.Upyr.Upyr/config.toml'


def percentile(values, probability):
    """Nearest-rank percentile; small-N reports retain N and min/max."""
    ordered = sorted(values)
    if not ordered:
        return None
    return ordered[max(0, math.ceil(probability * len(ordered)) - 1)]


def aggregate(results):
    groups = defaultdict(list)
    for result in results:
        for checkpoint in result.get('checkpoints', []):
            if result['passed'] and checkpoint.get('passed') and checkpoint.get('timed'):
                groups[result['surface'] + '/' + result['case']].append(checkpoint['settled_ms'])
    return {name: dict(n=len(values), p50_ms=percentile(values, .5),
                      p95_ms=percentile(values, .95),
                      p99_ms=percentile(values, .99) if len(values) >= 100 else None,
                      min_ms=min(values), max_ms=max(values))
            for name, values in sorted(groups.items())}


def private_config(original, silent=False, auto_off=False):
    # Preserve every setting and section; this one test-only root setting
    # prevents repeated deliberate undo from learning permanent exceptions.
    import re
    text = original.decode()
    first_section = re.search(r'(?m)^\[', text)
    end = first_section.start() if first_section else len(text)
    root, sections = text[:end], text[end:]
    root, count = re.subn(r'(?m)^auto_correct_learning\s*=.*$', 'auto_correct_learning = false', root)
    if not count:
        root = 'auto_correct_learning = false\n' + root
    if auto_off:
        root, count = re.subn(r'(?m)^auto_correct\s*=.*$', 'auto_correct = false', root)
        if not count:
            root = 'auto_correct = false\n' + root
    text = root + sections
    if silent:
        text, count = re.subn(r'(?ms)(^\[sounds\]\n)(.*?)(?=^\[|\Z)',
                             lambda m: m[1] + re.sub(r'(?m)^enabled\s*=.*$', 'enabled = false', m[2]),
                             text)
        if not count or tomllib.loads(text)['sounds'].get('enabled') is not False:
            raise ValueError('silent probe requires an explicit [sounds].enabled setting')
    assert tomllib.loads(text)['auto_correct_learning'] is False
    return text.encode()


def read(path):
    return json.loads(path.read_text())


def wait_for(path, predicate, timeout=10):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            value = read(path)
            if predicate(value):
                return value
        except (OSError, ValueError):
            pass
        time.sleep(.02)
    raise RuntimeError(f'timeout waiting for owned fixture: {path}')


def driver(name, **kwargs):
    return json.loads(probe.run(['cua-driver', 'call', name, json.dumps(kwargs)], timeout=30))


def terminal_window(window_id, timeout=3, session=None):
    # A native fixture can open on a different Space from fullscreen Paradev.
    # Only an explicitly bound window may be brought back; automatic discovery
    # still requires a single already-visible window. Input always waits for it.
    deadline = time.monotonic() + timeout
    activated = False
    while True:
        candidates = [w for w in driver('list_windows')['windows']
                   if w.get('app_name') in ('Superset', 'Paradev')
                   and w.get('bounds', {}).get('height', 0) > 100
                   and (window_id is None or w['window_id'] == window_id)]
        choices = [w for w in candidates if w.get('is_on_screen')]
        if len(choices) == 1:
            return dict(pid=choices[0]['pid'], window_id=choices[0]['window_id'])
        if window_id is not None and not activated and len(candidates) == 1:
            window = dict(pid=candidates[0]['pid'], window_id=window_id)
            driver('get_window_state', **window, **({'session': session} if session else {}),
                   include_screenshot=False, max_depth=1, max_elements=1)
            driver('bring_to_front', **window)
            activated = True
        if len(choices) > 1 or time.monotonic() >= deadline:
            raise RuntimeError('Choose the exact visible Paradev window with --window-id')
        time.sleep(.1)


def running_upyr():
    rows = subprocess.check_output(['ps', '-axo', 'pid=,comm='], text=True).splitlines()
    return [int(parts[0]) for row in rows if len(parts := row.strip().split(None, 1)) == 2
            and parts[1] == str(BACKGROUND)]


def stop_upyr(pids):
    for pid in pids:
        os.kill(pid, signal.SIGTERM)
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline and any(pid in running_upyr() for pid in pids):
        time.sleep(.05)
    if any(pid in running_upyr() for pid in pids):
        raise RuntimeError('Upyr did not stop; refusing to run two input monitors')


def main(args):
    if sys.platform != 'darwin':
        raise RuntimeError('live probe requires macOS')
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    helper = output / 'physical-input'
    fixture = output / 'native-fixture'
    textedit = output / 'textedit-fixture'
    sources = [('superset-input-probe.swift', helper), ('typing-latency-fixture.swift', fixture)]
    if args.surface in ('textedit', 'editors'):
        sources.append(('textedit-latency-fixture.swift', textedit))
    for source, binary in sources:
        probe.run(['swiftc', '-O', '-warnings-as-errors', str(ROOT / 'tools' / source), '-o', str(binary)], timeout=60)
    initial = json.loads(probe.run([str(helper)]))
    config_bytes = CONFIG.read_bytes()
    tested_binary = getattr(args, 'binary', None) or BACKGROUND
    tested_binary = tested_binary.resolve()
    config = tomllib.loads(config_bytes.decode())
    cases = read(ROOT / 'tests/fixtures/typing-latency-cases.json')['cases']
    if getattr(args, 'continuous_pairs', None) is not None:
        if not 1 <= args.continuous_pairs <= 1000:
            raise ValueError('--continuous-pairs must be between 1 and 1000')
        for case in cases:
            if case['name'].startswith('continuous-alternating-'):
                case['actions'][0]['text'] = 'dbghfdktyyz continue ' * args.continuous_pairs
                case['actions'][-1]['expect'] = 'виправлення continue ' * args.continuous_pairs
    if getattr(args, 'burst', False):
        actions, expected = [], ''
        for index in range(100):
            for prefix, trigger, suffix, converted, layout in [
                ('dbg','h','fdktyyz ','виправлення ','Ukrainian-PC'),
                ('con','t','inue ','continue ','ABC'),
            ]:
                mark = f'{index}-{layout}'
                actions += [dict(text=prefix,gap=20),dict(text=trigger,gap=0,mark=mark),
                            dict(expect=expected+converted[:4],layout='com.apple.keylayout.'+layout,
                                 since=mark,name=mark,timeout=3000),dict(text=suffix,gap=20)]
                expected += converted
        actions.append(dict(expect=expected,layout='com.apple.keylayout.ABC'))
        cases = [dict(name='alternating-200-corrections',source='com.apple.keylayout.ABC',actions=actions)]
    if args.case:
        unknown = set(args.case) - {case['name'] for case in cases}
        if unknown:
            raise ValueError(f'unknown cases: {sorted(unknown)}')
        cases = [case for case in cases if case['name'] in args.case]
    if getattr(args, 'auto_off', False):
        if not cases or any(not c['name'].startswith('continuous-alternating-') for c in cases):
            raise ValueError('--auto-off control requires explicit continuous-alternating cases')
        for case in cases:
            case['actions'][-1] = dict(expect=case['actions'][0]['text'],layout=case['source'])
    report = dict(status='prepared', completed_at=None, preflight=initial,
                  binary_sha256=hashlib.sha256(tested_binary.read_bytes()).hexdigest(),
                  binary_path=str(tested_binary),
                  config_sha256=hashlib.sha256(config_bytes).hexdigest(),
                  timing='CGEvent key-down to first editor-text + layout confirmation; Globe starts at Fn release; nominal 2ms polling; excludes pixel presentation',
                  config_overrides={'auto_correct_learning': False},
                  delays={k: v for k, v in config.items() if k.endswith('_delay_ms')},
                  cases=[case['name'] for case in cases], results=[], latency={})
    if getattr(args, 'continuous_pairs', None) is not None:
        report['continuous_pairs'] = args.continuous_pairs
    if getattr(args, 'silent', False):
        report['config_overrides']['sounds.enabled'] = False
    if getattr(args, 'auto_off', False):
        report['config_overrides']['auto_correct'] = False
    if args.surface in ('textedit', 'chrome', 'cursor', 'editors'):
        report['observer'] = 'TextEdit AX readback (5ms); Chrome local DOM input observer + loopback HTTP; Cursor document events + private extension; 10ms focus heartbeat. Transport delay is included.'

    def save():
        report['completed_at'] = datetime.now(timezone.utc).isoformat()
        report['latency'] = aggregate(report['results'])
        probe.write_json(output / 'report.json', report)

    save()
    if not args.run:
        print(json.dumps(dict(status='prepared', report=str(output/'report.json'), preflight=initial), ensure_ascii=False))
        return 0
    permissions = probe.run(['cua-driver', 'permissions', 'status'])
    allowed = all(any(label in line and 'granted' in line for line in permissions.splitlines())
                  for label in ('Accessibility:', 'Screen Recording:'))
    if not allowed or not initial['postAccess']:
        report.update(status='blocked', reason='Existing Accessibility/Screen Recording/input permissions are required; no input sent')
        save(); print(report['reason']); return 2
    if initial.get('screenLocked') or initial['bundle'] == 'com.apple.loginwindow':
        report.update(status='blocked', reason='Mac is locked; no app stopped or input sent')
        save(); print(report['reason']); return 2
    if not config.get('auto_correct') or not config.get('switch_layout'):
        raise RuntimeError('Enable automatic correction and layout switching before this test')
    surfaces = (['native', 'terminal'] if args.surface == 'both' else
                ['textedit', 'chrome', 'cursor'] if args.surface == 'editors' else [args.surface])
    if 'terminal' in surfaces and (not args.workspace or 'com.superset.desktop' not in config.get('terminal_typing_apps', [])):
        raise RuntimeError('terminal probe requires --workspace and the existing com.superset.desktop opt-in')
    preview = None
    stopped = False
    session = 'upyr-latency-' + uuid.uuid4().hex[:10]
    private = output / 'private-config.toml'
    private.write_bytes(private_config(config_bytes, getattr(args, 'silent', False),
                                      getattr(args, 'auto_off', False))); private.chmod(0o600)
    original_pids = running_upyr()
    if len(original_pids) != 1:
        raise RuntimeError('Expected exactly one installed Upyr background process')
    driver('start_session', session=session)
    report['status'] = 'running'
    save()
    interrupted = False
    try:
        stop_upyr(original_pids); stopped = True
        log = (output / 'upyr.log').open('w')
        preview = subprocess.Popen([str(tested_binary)], env={**os.environ, 'UPYR_CONFIG': str(private), 'RUST_LOG': 'upyr=debug'}, stdout=log, stderr=subprocess.STDOUT)
        time.sleep(.8)
        if preview.poll() is not None:
            raise RuntimeError('private-config Upyr process did not start')
        report['upyr_pid'] = preview.pid
        for surface in surfaces:
            scratch = output / (surface + '-' + uuid.uuid4().hex[:8])
            scratch.mkdir()
            probe.write_json(scratch/'control.json', dict(nonce='initial'))
            child = pane = original_pane = editor = None
            try:
                if surface == 'native':
                    child = subprocess.Popen([str(fixture), str(scratch)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                    state = wait_for(scratch/'state.json', lambda s: s.get('ready'))
                    window = dict(pid=state['pid'], window_id=state['window'])
                elif surface in ('textedit', 'chrome', 'cursor'):
                    editor = Editor(surface, scratch, driver, wait_for, session, textedit,
                                    existing_cursor=getattr(args, 'existing_cursor', False))
                    window = editor.window
                else:
                    import shlex
                    # Focusing a pane only selects its tab inside that workspace;
                    # the desktop may still be displaying another workspace.
                    probe.cli('workspaces', 'open', args.workspace, '--local')
                    window = terminal_window(args.window_id, session=session)
                    layout = probe.cli('panes', 'list', '--workspace', args.workspace)
                    original_pane = next(t['activePaneId'] for t in layout['tabs'] if t['active'])
                    command = shlex.join([sys.executable, str(ROOT/'tools/superset_input_probe.py'), '--fixture', str(scratch)])
                    created = probe.cli('panes', 'new-tab', '--workspace', args.workspace, '--command', command)
                    pane = created['paneId']
                    probe.cli('panes', 'focus', '--workspace', args.workspace, '--pane', pane)
                    wait_for(scratch/'state.json', lambda s: s.get('ready'))
                driver('get_window_state', **window, session=session, include_screenshot=False)
                driver('bring_to_front', **window)
                time.sleep(.2)

                def snapshot():
                    image = scratch/'fixture.png'
                    deadline = time.monotonic() + 5
                    while True:
                        if pane:
                            layout = probe.cli('panes', 'list', '--workspace', args.workspace)
                            if not any(t['active'] and t['activePaneId'] == pane for t in layout['tabs']):
                                raise RuntimeError('Owned terminal pane lost focus')
                        driver('get_window_state', **window, session=session, screenshot_out_file=str(image),
                               include_accessibility_tree=surface in ('native', 'textedit'))
                        if not pane:
                            return
                        marker = json.loads(probe.run([str(helper), 'marker', str(image), read(scratch/'state.json')['nonce'][:8]]))
                        if marker['ownedFixtureVisible']:
                            return
                        if time.monotonic() >= deadline:
                            raise RuntimeError('Owned raw-input marker is not visible; no input sent')
                        time.sleep(.1)  # PTY ready state can precede a Spaces/renderer repaint.

                def send(actions, source=None):
                    snapshot()
                    nonce = read(scratch/'state.json')['nonce']
                    plan = dict(actions=actions, upyrPID=preview.pid, finalWait=400,
                                clipboardRestoreTimeout=max(3000, config.get('restore_delay_ms', 250) + 1000))
                    if source:
                        plan['source'] = source
                    probe.write_json(scratch/'plan.json', plan)
                    # Long continuous cases intentionally exceed a minute.
                    # Allow their requested gaps plus bounded per-key observer
                    # overhead; the helper still checks outside input per key.
                    typing_seconds = sum(len(a.get('text', '')) * (a.get('gap', 0) / 1000 + .02) for a in actions)
                    timeout = max(180 if getattr(args, 'burst', False) else 60, 30 + typing_seconds)
                    return json.loads(probe.run([str(helper), str(window['pid']), str(window['window_id']), str(scratch), nonce, str(scratch/'plan.json')], timeout=timeout))

                for case in cases:
                    if case.get('surface') == 'terminal' and surface != 'terminal':
                        continue
                    if case.get('surface') == 'native' and surface == 'terminal':
                        continue
                    repetitions = args.repetitions if case.get('latency') else 1
                    for repetition in range(repetitions):
                        nonce = uuid.uuid4().hex
                        probe.write_json(scratch/'control.json', dict(nonce=nonce))
                        wait_for(scratch/'state.json', lambda s: s['nonce'] == nonce)
                        time.sleep(.25)
                        send([dict(key=53), dict(key=36)], case['source'])
                        nonce += '-start'
                        probe.write_json(scratch/'control.json', dict(nonce=nonce))
                        wait_for(scratch/'state.json', lambda s: s['nonce'] == nonce)
                        time.sleep(.15)
                        observed = send(case['actions'])
                        time.sleep(.4)  # clipboard cleanup is outside measured editing latency
                        state = read(scratch/'state.json')
                        checks = observed['checkpoints']
                        passed = (len(checks) == sum('expect' in action for action in case['actions'])
                                  and bool(checks) and all(check.get('passed') for check in checks)
                                  and state['text'] == checks[-1]['observed']
                                  and observed['source'] == checks[-1]['source']
                                  and observed['clipboardContentRestored']
                                  and (surface != 'terminal' or observed['clipboardUnchanged']))
                        result = dict(surface=surface, case=case['name'], repetition=repetition, passed=passed,
                                      text=state['text'], enters=state['enters'], **observed)
                        report['results'].append(result); save()
                        shown = {k: result[k] for k in ('surface','case','repetition','passed')}
                        shown.update(characters=len(state['text']), checkpoints=len(checks), latency=report['latency'])
                        if not passed:
                            shown['failures'] = [c for c in checks if not c.get('passed')]
                            shown['clipboardDiagnostic'] = observed.get('clipboardDiagnostic')
                        print(json.dumps(shown, ensure_ascii=False), flush=True)
                        snapshot()
                probe.write_json(scratch/'control.json', dict(nonce='stop', stop=True))
            finally:
                probe.write_json(scratch/'control.json', dict(nonce='stop', stop=True))
                if editor:
                    editor.close()
                if child:
                    try: child.wait(timeout=3)
                    except subprocess.TimeoutExpired: child.terminate(); child.wait(timeout=3)
                if pane:
                    layout = probe.cli('panes', 'list', '--workspace', args.workspace)
                    owned_active = any(t['active'] and t['activePaneId'] == pane for t in layout['tabs'])
                    probe.cli('panes', 'close', '--workspace', args.workspace, '--pane', pane)
                    if owned_active and original_pane:
                        probe.cli('panes', 'focus', '--workspace', args.workspace, '--pane', original_pane)
        report['status'] = 'passed' if report['results'] and all(r['passed'] for r in report['results']) else 'failed'
    except BaseException as error:
        interrupted = isinstance(error, KeyboardInterrupt) or any(marker in str(error) for marker in (
            'test interrupted or owned fixture lost focus', 'Owned terminal pane lost focus'))
        report.update(status='aborted', reason=str(error))
        raise
    finally:
        try:
            if preview and preview.poll() is None:
                preview.terminate()
                try: preview.wait(timeout=5)
                except subprocess.TimeoutExpired: preview.kill(); preview.wait(timeout=3)
            if stopped:
                subprocess.run(['open', str(APP)], check=True)
            # Do not take focus/layout back from someone who interrupted the run.
            if not interrupted:
                probe.run([str(helper), 'select', initial['source']])
            if not interrupted and initial.get('window'):
                driver('get_window_state', pid=initial['frontPID'], window_id=initial['window'], session=session, include_screenshot=False)
                driver('bring_to_front', pid=initial['frontPID'], window_id=initial['window'])
        finally:
            try:
                driver('end_session', session=session)
            finally:
                report['config_unchanged'] = CONFIG.read_bytes() == config_bytes
                save()
    return 0 if report['status'] == 'passed' else 1


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run', action='store_true', help='send input to owned test surfaces; requires an unlocked Mac')
    parser.add_argument('--binary', type=Path, help='test a candidate background executable; restore the installed app afterward')
    parser.add_argument('--surface', choices=['native','terminal','both','textedit','chrome','cursor','editors'], default='both')
    parser.add_argument('--repetitions', type=int, default=10)
    parser.add_argument('--case', action='append')
    parser.add_argument('--burst', action='store_true', help='200 alternating corrections without resetting the editor; retains cold sample')
    parser.add_argument('--continuous-pairs', type=int, help='override continuous cases with 1..1000 word pairs, without waiting for each correction')
    parser.add_argument('--silent', action='store_true', help='disable sound only in the private probe config for a matched experiment')
    parser.add_argument('--auto-off', action='store_true', help='continuous-input control: disable autocorrection only in the private config and expect unchanged physical text')
    parser.add_argument('--existing-cursor', action='store_true', help='open only an owned development window in the signed-in Cursor profile; never stop the shared app')
    parser.add_argument('--workspace', default=os.environ.get('SUPERSET_WORKSPACE_ID',''))
    parser.add_argument('--window-id', type=int)
    parser.add_argument('--output', type=Path, default=ROOT/'target/typing-latency-report')
    args = parser.parse_args()
    if not 1 <= args.repetitions <= 100:
        parser.error('--repetitions must be 1..100')
    raise SystemExit(main(args))
