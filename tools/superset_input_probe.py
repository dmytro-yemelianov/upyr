#!/usr/bin/env python3
"""Opt-in live macOS regression probe; opens and closes its own Superset tab.

Requires Upyr running with automatic/terminal/slash correction enabled for
com.superset.desktop, the default undo shortcut, Accessibility, swiftc and
cua-driver with existing Accessibility/Screen Recording grants. Never sends
commands to an existing terminal or alters Upyr preferences. Stop typing and
clicking during the run; external input aborts physical-key delivery.
"""
from __future__ import annotations

import argparse
import codecs
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import time
import uuid


def write_json(path: Path, value: dict) -> None:
    staged = path.with_suffix('.new')
    staged.write_text(json.dumps(value, ensure_ascii=False))
    staged.replace(path)


def fixture(root: Path) -> None:
    """A raw input sink: no eval, subprocesses, shell or command submission."""
    import select
    import termios
    import tty

    fd = sys.stdin.fileno()
    old = termios.tcgetattr(fd)
    text: list[str] = []
    cursor = enters = 0
    nonce = ''
    decoder = codecs.getincrementaldecoder('utf-8')()
    escape = ''
    deadline = time.monotonic() + 900
    state = root / 'state.json'

    def publish(ready: bool = True) -> None:
        write_json(state, dict(pid=os.getpid(), nonce=nonce, ready=ready,
                               text=''.join(text), cursor=cursor, enters=enters))
        visible = ''.join(text).replace('\n', '↵')
        try:
            bits = f'{int(nonce[:8], 16):032b}'
        except ValueError:
            bits = '0' * 32
        colors = {'0': '40;180;255', '1': '255;40;120', 'g': '80;255;80'}
        barcode = ''.join('\x1b[48;2;' + colors[bit] + 'm  \x1b[0m ' for bit in 'g' + bits + 'g')
        sys.stdout.write('\x1b[H\x1b[2KUpyr isolated probe: this field never executes commands.\r\n'
                         '\x1b[2K' + barcode + '\r\n\x1b[2KPROBE> ' + visible + '\x1b[J')
        if len(text) > cursor:
            sys.stdout.write(f'\x1b[{len(text) - cursor}D')
        sys.stdout.flush()

    try:
        tty.setraw(fd)
        sys.stdout.write('\x1b]0;Upyr isolated input probe\x07\r\n'
                         'Upyr input test: this field never executes commands.\r\n')
        publish()
        while time.monotonic() < deadline:
            control = json.loads((root / 'control.json').read_text())
            if control.get('stop'):
                break
            if control['nonce'] != nonce:
                nonce = control['nonce']
                text = list(control.get('seed', ''))
                cursor = len(text)
                enters = 0
                escape = ''
                decoder.reset()
                publish()
            if not select.select([fd], [], [], .025)[0]:
                if escape == '\x1b':
                    escape = ''
                continue
            raw = os.read(fd, 4096)
            if not raw:
                break
            for char in decoder.decode(raw):
                if escape:
                    escape += char
                    if escape in ('\x1b[D', '\x1b[C', '\x1b[H', '\x1b[F'):
                        cursor = {'D': max(0, cursor - 1), 'C': min(len(text), cursor + 1),
                                  'H': 0, 'F': len(text)}[char]
                        escape = ''
                    elif len(escape) > 16 or (char.isalpha() and len(escape) > 2):
                        escape = ''
                    elif not '\x1b['.startswith(escape) and not escape.startswith('\x1b['):
                        escape = ''
                    continue
                if char == '\x1b':
                    escape = char
                elif char in ('\x7f', '\b'):
                    if cursor:
                        cursor -= 1
                        del text[cursor]
                elif char in ('\r', '\n'):
                    enters += 1
                    text.insert(cursor, '\n')
                    cursor += 1
                elif char >= ' ':
                    text.insert(cursor, char)
                    cursor += 1
            publish()
    finally:
        publish(False)
        termios.tcsetattr(fd, termios.TCSADRAIN, old)
        sys.stdout.write('\r\nProbe stopped.\r\n')


def run(args: list[str], *, timeout: int = 20) -> str:
    result = subprocess.run(args, capture_output=True, text=True, timeout=timeout)
    if result.returncode:
        raise RuntimeError(f'{args[0]} failed ({result.returncode}): {result.stderr}{result.stdout}')
    return result.stdout


def cli(*args: str) -> dict:
    return json.loads(run(['superset', *args, '--json']))


def main(workspace: str, override_pid: int | None = None) -> None:
    if sys.platform != 'darwin' or not workspace:
        raise RuntimeError('macOS and --workspace (or SUPERSET_WORKSPACE_ID) are required')
    permissions = run(['cua-driver', 'permissions', 'status'])
    if not all(any(label in line and 'granted' in line for line in permissions.splitlines())
               for label in ('Accessibility:', 'Screen Recording:')):
        raise RuntimeError('Grant Cua Driver permissions before running this probe')
    root = Path(tempfile.mkdtemp(prefix='upyr-superset-'))
    helper = root / 'physical-input'
    run(['swiftc', '-O', '-warnings-as-errors', '-o', str(helper),
         str(Path(__file__).with_name('superset-input-probe.swift'))], timeout=60)
    before = json.loads(run([str(helper)]))
    if not before['postAccess']:
        raise RuntimeError('Physical-input permission missing')
    initial = cli('panes', 'list', '--workspace', workspace)
    original_pane = next(t['activePaneId'] for t in initial['tabs'] if t['active'])
    upyr_pid = override_pid or int(run(['pgrep', '-f', '^/Applications/Upyr.app/Contents/MacOS/upyr-background run$']))
    config = Path.home() / 'Library/Application Support/dev.Upyr.Upyr/config.toml'
    config_before = config.read_bytes()
    write_json(root / 'control.json', dict(nonce='initial'))
    command = shlex.join([sys.executable, str(Path(__file__).resolve()), '--fixture', str(root)])
    created = cli('panes', 'new-tab', '--workspace', workspace, '--command', command)
    pane = created['paneId']
    results: list[dict] = []
    session = 'upyr-probe-' + root.name[-8:]
    print(f'evidence={root}', flush=True)
    try:
        run(['cua-driver', 'call', 'start_session', json.dumps(dict(session=session))])
        cli('panes', 'focus', '--workspace', workspace, '--pane', pane)
        deadline = time.monotonic() + 12
        while not (root / 'state.json').exists() and time.monotonic() < deadline:
            time.sleep(.1)
        if not (root / 'state.json').exists():
            raise RuntimeError('Owned raw-input fixture did not start')
        windows = json.loads(run(['cua-driver', 'call', 'list_windows', '{}']))['windows']
        candidates = [w for w in windows if w.get('is_on_screen') and w.get('app_name') in ('Superset', 'Paradev')
                      and w.get('bounds', {}).get('height', 0) > 100]
        if len(candidates) != 1:
            raise RuntimeError('Expected one visible Superset window; select its workspace first')
        window = candidates[0]
        run(['cua-driver', 'call', 'bring_to_front', json.dumps(dict(pid=window['pid'], window_id=window['window_id']))])
        time.sleep(.4)


        def state() -> dict:
            return json.loads((root / 'state.json').read_text())

        def check_pane() -> None:
            layout = cli('panes', 'list', '--workspace', workspace)
            if not any(t['active'] and t['activePaneId'] == pane for t in layout['tabs']):
                raise RuntimeError('Owned pane is no longer active')

        def keys(actions: list[dict], source: str | None = None) -> dict:
            check_pane()
            image = root / 'fixture.png'
            run(['cua-driver', 'call', 'get_window_state', json.dumps(dict(pid=window['pid'], window_id=window['window_id'], session=session, screenshot_out_file=str(image), include_accessibility_tree=False))])
            visible = json.loads(run([str(helper), 'marker', str(image), state()['nonce'][:8]]))
            if not visible['ownedFixtureVisible']:
                raise RuntimeError('Owned fixture marker is not visible; no physical input sent')
            plan = dict(actions=actions, upyrPID=upyr_pid)
            if source:
                plan['source'] = source
            write_json(root / 'plan.json', plan)
            return json.loads(run([str(helper), str(window['pid']), str(window['window_id']),
                                   str(root), state()['nonce'], str(root / 'plan.json')]))

        abc, ukr = 'com.apple.keylayout.ABC', 'com.apple.keylayout.Ukrainian-PC'
        scenarios = [
            ('slash-after-escape', ukr, [dict(key=53), dict(text='/command'), dict(wait=500)], ['/command'], abc),
            ('slash-after-reset-undo', ukr, [dict(key=53), dict(text='/c'), dict(wait=500), dict(undo=True)], ['.с'], ukr),
            ('leading-bracket-short', abc, [dict(text='[ez '), dict(wait=500)], ['хуя '], ukr),
            ('leading-bracket-short-stem', abc, [dict(text='[eq '), dict(wait=500)], ['хуй '], ukr),
            ('leading-semicolon-short', abc, [dict(text=';er '), dict(wait=500)], ['жук '], ukr),
            ('leading-right-bracket', abc, [dict(text='];f '), dict(wait=500)], ['їжа '], ukr),
            ('leading-quote', abc, [dict(text="'yjn "), dict(wait=500)], ['єнот '], ukr),
            ('leading-period', abc, [dict(text='.yfr '), dict(wait=500)], ['юнак '], ukr),
            ('native-backslash-map', abc, [dict(text='\\fyjr '), dict(wait=500)], ['ʼанок '], ukr),
            ('native-backquote-ge', abc, [dict(text='`fyjr '), dict(wait=500)], ['ґанок '], ukr),
            ('preserved-comma', abc, [dict(text='ghbdsn, '), dict(wait=500)], ['привіт, '], ukr),
            ('preserved-dots', abc, [dict(text='ghbdsn... '), dict(wait=500)], ['привіт... '], ukr),
            ('slash-after-reset-fast', ukr, [dict(key=53), dict(text='/command', gap=15), dict(wait=500)], ['/command'], abc),
            ('literal-dot-env', abc, [dict(text='.env '), dict(wait=500)], ['.env '], abc),
            ('literal-bracket-identifier', abc, [dict(text='[raw '), dict(wait=500)], ['[raw '], abc),
            ('literal-semicolon-command', abc, [dict(text=';ls '), dict(wait=500)], [';ls '], abc),
            ('ukrainian-word', abc, [dict(text='ghbdsn '), dict(wait=500)], ['привіт '], ukr),
            ('english-contraction', ukr, [dict(text="it's not so good for the english "), dict(wait=500)], ["it's not so good for the english "], abc),
            ('mixed-language', abc, [dict(text="it's not nfr lj,ht zr [jnskjcz , "), dict(wait=600)], ["it's not так добре як хотілося б "], ukr),
            ('punctuation', abc, [dict(text='hjpevs. '), dict(wait=500)], ['розумію '], ukr),
            ('slash-prefix', ukr, [dict(text='/c'), dict(wait=500)], ['/c'], abc),
            ('slash-command', ukr, [dict(text='/command'), dict(wait=500)], ['/command'], abc),
            ('undo-word', abc, [dict(text='ghbdsn '), dict(wait=500), dict(undo=True)], ['ghbdsn '], abc),
            ('undo-punctuation', abc, [dict(text='lj,ht '), dict(wait=500), dict(undo=True)], ['lj,ht '], abc),
            ('undo-slash', ukr, [dict(text='/c'), dict(wait=500), dict(undo=True)], ['.с'], ukr),
            ('undo-after-input', abc, [dict(text='ghbdsn '), dict(wait=500), dict(text='a'), dict(undo=True)], ['привіт ф'], ukr),
            ('undo-after-navigation', abc, [dict(text='ghbdsn '), dict(wait=500), dict(key=123), dict(undo=True)], ['привіт '], ukr),
            ('fast-following-key', abc, [dict(text='ghbdsn ', gap=15), dict(text='a', gap=0), dict(wait=500), dict(undo=True)], ['ghbdsn a', 'привіт ф'], None),
        ]
        scenarios += [(f'english-repeated-{i}', ukr, [dict(text="it's not so good for the english "), dict(wait=500)], ["it's not so good for the english "], abc) for i in range(3)]
        for name, source, actions, expected, destination in scenarios:
            nonce = uuid.uuid4().hex
            write_json(root / 'control.json', dict(nonce=nonce))
            deadline = time.monotonic() + 2
            while state()['nonce'] != nonce and time.monotonic() < deadline:
                time.sleep(.03)
            assert state()['nonce'] == nonce
            time.sleep(.25)  # Allow the PTY renderer to paint the new marker.
            keys([dict(key=53), dict(key=36)], source)  # Establish a fresh line in the sink.
            write_json(root / 'control.json', dict(nonce=nonce + '-start'))
            deadline = time.monotonic() + 2
            while state()['nonce'] != nonce + '-start' and time.monotonic() < deadline:
                time.sleep(.03)
            assert state()['nonce'] == nonce + '-start'
            time.sleep(.15)
            observed = keys(actions)
            current = state()
            passed = (current['text'] in expected and current['enters'] == 0 and observed['clipboardUnchanged']
                      and (destination is None or observed['source'] == destination))
            result = dict(case=name, passed=passed, observed=current['text'], source=observed['source'],
                          enters=current['enters'], clipboardUnchanged=observed['clipboardUnchanged'])
            results.append(result)
            write_json(root / 'results.json', dict(results=results))
            print(json.dumps(result, ensure_ascii=False), flush=True)
            if not passed:
                print(run(['superset','terminals','read','--local','--workspace',workspace,'--terminal',created['terminalId'],'--json']), flush=True)
                print(json.dumps(current), flush=True)
                raise RuntimeError(f'{name}: unexpected text or input source')
        nonce = uuid.uuid4().hex
        write_json(root / 'control.json', dict(nonce=nonce))
        time.sleep(.3)
        keys([dict(key=53), dict(key=36)], abc)
        write_json(root / 'control.json', dict(nonce=nonce + '-focus'))
        time.sleep(.3)
        keys([dict(text='ghbdsn '), dict(wait=500)])
        assert state()['text'] == 'привіт '
        outcome = json.loads(run([str(helper), 'focus-undo', str(upyr_pid)]))
        run(['cua-driver', 'call', 'bring_to_front', json.dumps(dict(pid=window['pid'], window_id=window['window_id']))])
        observed = keys([dict(undo=True)])
        passed = outcome['focusUndoIgnored'] and state()['text'] == 'привіт ' and observed['source'] == ukr and observed['clipboardUnchanged']
        result = dict(case='undo-after-window-change', passed=passed, observed=state()['text'], source=observed['source'])
        results.append(result)
        write_json(root / 'results.json', dict(results=results))
        print(json.dumps(result, ensure_ascii=False), flush=True)
        assert passed, 'undo edited a field after focus changed'
    finally:
        write_json(root / 'control.json', dict(nonce='stop', stop=True))
        layout = cli('panes', 'list', '--workspace', workspace)
        owned_active = any(t['active'] and t['activePaneId'] == pane for t in layout['tabs'])
        cli('panes', 'close', '--workspace', workspace, '--pane', pane)
        if owned_active:
            cli('panes', 'focus', '--workspace', workspace, '--pane', original_pane)
        run([str(helper), 'select', before['source']])
        subprocess.run(['cua-driver', 'call', 'end_session', json.dumps(dict(session=session))], capture_output=True, text=True)
        if config.read_bytes() != config_before:
            raise RuntimeError('Upyr preferences changed during the test; preserved the new user preferences')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixture', type=Path, help=argparse.SUPPRESS)
    parser.add_argument('--upyr-pid', type=int, help='PID of an explicitly launched Upyr preview')
    parser.add_argument('--workspace', default=os.environ.get('SUPERSET_WORKSPACE_ID', ''))
    options = parser.parse_args()
    if options.fixture:
        fixture(options.fixture)
    else:
        main(options.workspace, options.upyr_pid)
