"""Exercise the live probe's raw sink in a PTY, without desktop input."""
from __future__ import annotations

import json
import os
from pathlib import Path
import re
import select
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import call, patch

from tools import superset_input_probe as probe


@unittest.skipUnless(os.name == 'posix', 'the raw fixture requires a POSIX PTY')
class RawFixtureTests(unittest.TestCase):
    def setUp(self):
        import fcntl
        import pty
        import struct
        import termios

        self.scratch = tempfile.TemporaryDirectory(prefix='upyr-fixture-test-')
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.control = self.root / 'control.json'
        probe.write_json(self.control, {'nonce': 'a5a5a5a5-test'})
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 24, 80, 0, 0))
        self.output = bytearray()
        self.child = subprocess.Popen(
            [sys.executable, str(Path(probe.__file__)), '--fixture', str(self.root)],
            stdin=slave, stdout=slave, stderr=subprocess.PIPE)
        os.close(slave)
        self.addCleanup(self.stop_fixture)
        self.wait_for(lambda s: s['nonce'] == 'a5a5a5a5-test' and s['ready'])

    def stop_fixture(self):
        probe.write_json(self.control, {'nonce': 'stop', 'stop': True})
        try:
            # Drain the final screen while waiting; a PTY has a small output
            # buffer and the fixture must finish its last repaint to exit.
            deadline = time.monotonic() + 3
            while self.child.poll() is None and time.monotonic() < deadline:
                if select.select([self.master], [], [], .025)[0]:
                    try:
                        self.output.extend(os.read(self.master, 65536))
                    except OSError:
                        break  # PTY EOF after a normal exit.
            self.child.wait(timeout=3)
        except subprocess.TimeoutExpired:
            self.child.kill()
            self.child.wait(timeout=3)
        finally:
            self.child.stderr.close()
            os.close(self.master)
        self.assertEqual(self.child.returncode, 0)

    def wait_for(self, predicate):
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if select.select([self.master], [], [], .025)[0]:
                self.output.extend(os.read(self.master, 65536))
            state = self.root / 'state.json'
            if state.exists():
                current = json.loads(state.read_text())
                if predicate(current):
                    return current
            if self.child.poll() is not None:
                self.fail('raw fixture exited before producing the expected state')
        self.fail('raw fixture did not reach the expected state')

    def test_nonce_marker_fits_an_80_column_terminal(self):
        bars = [line for line in self.output.split(b'\r\n') if b'\x1b[48;2;' in line]
        self.assertTrue(bars)
        ansi = re.compile(rb'\x1b\[[0-?]*[ -/]*[@-~]')
        for line in bars:
            self.assertEqual(line.count(b'\x1b[48;2;'), 34)
            self.assertLessEqual(len(ansi.sub(b'', line)), 80)

    def test_split_utf8_and_cursor_edits_are_observed_as_text(self):
        text = 'привіт '.encode('utf-8')
        os.write(self.master, text[:1])
        os.write(self.master, text[1:] + b'\x7f\x1b[D\x7f')
        current = self.wait_for(lambda s: s['text'] == 'привт')
        self.assertEqual(current['cursor'], 4)
        os.write(self.master, 'є'.encode('utf-8'))
        self.wait_for(lambda s: s['text'] == 'привєт' and s['cursor'] == 5)

    def test_submission_is_stored_and_reset_never_executes_it(self):
        sentinel = self.root / 'must-not-exist'
        text = 'touch ' + str(sentinel)
        os.write(self.master, text.encode() + b'\r')
        current = self.wait_for(lambda s: s['text'] == text + '\n')
        self.assertEqual(current['enters'], 1)
        self.assertFalse(sentinel.exists())
        probe.write_json(self.control, {'nonce': 'b6b6b6b6-reset', 'seed': 'untouched'})
        current = self.wait_for(lambda s: s['nonce'] == 'b6b6b6b6-reset')
        self.assertEqual(current['text'], 'untouched')
        self.assertEqual(current['cursor'], len('untouched'))
        self.assertEqual(current['enters'], 0)
        self.assertFalse(sentinel.exists())


class CleanupTests(unittest.TestCase):
    def test_full_disk_still_closes_owned_pane_and_restores_input_source(self):
        layout = {'tabs': [{'active': True, 'activePaneId': 'owned'}]}
        with patch.object(probe, 'write_json', side_effect=OSError(28, 'full disk')), \
                patch.object(probe, 'cli', side_effect=[layout, {}, {}]) as cli, \
                patch.object(probe, 'run') as run, \
                patch.object(probe.subprocess, 'run') as end:
            with self.assertRaises(OSError):
                probe.cleanup_probe(Path('scratch'), Path('helper'), 'abc', 'session',
                                    'workspace', 'owned', 'original')
        self.assertIn(call('panes', 'close', '--workspace', 'workspace', '--pane', 'owned'), cli.call_args_list)
        self.assertIn(call('panes', 'focus', '--workspace', 'workspace', '--pane', 'original'), cli.call_args_list)
        run.assert_called_once_with(['helper', 'select', 'abc'])
        self.assertEqual(end.call_args.args[0][:3], ['cua-driver', 'call', 'end_session'])

    def test_layout_read_failure_still_closes_owned_pane_without_moving_focus(self):
        with patch.object(probe, 'write_json'), \
                patch.object(probe, 'cli', side_effect=[RuntimeError('UI unavailable'), {}]) as cli, \
                patch.object(probe, 'run') as run, \
                patch.object(probe.subprocess, 'run') as end:
            with self.assertRaises(RuntimeError):
                probe.cleanup_probe(Path('scratch'), Path('helper'), 'abc', 'session',
                                    'workspace', 'owned', 'original')
        self.assertEqual(cli.call_args_list[-1], call('panes', 'close', '--workspace', 'workspace', '--pane', 'owned'))
        run.assert_called_once_with(['helper', 'select', 'abc'])
        self.assertEqual(end.call_args.args[0][:3], ['cua-driver', 'call', 'end_session'])


if __name__ == '__main__':
    unittest.main()
