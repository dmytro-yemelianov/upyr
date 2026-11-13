"""Offline checks for the live probe's gating, config isolation and reporting."""
import argparse
import json
from pathlib import Path
import tempfile
import tomllib
import unittest
from unittest.mock import patch

from tools import run_typing_latency as probe


class ReportingTests(unittest.TestCase):
    def test_percentiles_use_nearest_rank_and_show_only_successful_timed_results(self):
        values = list(range(1, 21))
        self.assertEqual(probe.percentile(values, .5), 10)
        self.assertEqual(probe.percentile(values, .95), 19)
        self.assertIsNone(probe.percentile([], .95))
        results = [dict(surface='native', case='space', passed=True, checkpoints=[
            dict(passed=True, timed=True, settled_ms=value),
            dict(passed=True, timed=False, settled_ms=9999)]) for value in values]
        results.append(dict(surface='native', case='space', passed=False,
                            checkpoints=[dict(passed=True,timed=True,settled_ms=9999)]))
        summary = probe.aggregate(results)['native/space']
        self.assertEqual(summary, dict(n=20,p50_ms=10,p95_ms=19,p99_ms=None,min_ms=1,max_ms=20))
        larger = [dict(surface='native',case='series',passed=True,checkpoints=[
            dict(passed=True,timed=True,settled_ms=value) for value in range(1,101)])]
        self.assertEqual(probe.aggregate(larger)['native/series']['p99_ms'], 99)

    def test_private_config_preserves_sections_and_only_disables_learning(self):
        for root in ['', 'auto_correct_learning = true\n']:
            original = (root + 'copy_delay_ms = 90\n[globe_switch]\nenabled = true\n').encode()
            before = tomllib.loads(original.decode())
            after = tomllib.loads(probe.private_config(original).decode())
            self.assertEqual(after, {**before, 'auto_correct_learning': False})

    def test_silent_experiment_does_not_disable_other_sections(self):
        original = b'[sounds]\nenabled = true\nvolume_percent = 44\n[globe_switch]\nenabled = true\n'
        after = tomllib.loads(probe.private_config(original, silent=True).decode())
        self.assertEqual(after, {'auto_correct_learning': False, 'sounds': {'enabled': False, 'volume_percent': 44},
                                'globe_switch': {'enabled': True}})

    def test_no_correction_control_changes_only_private_root_switch(self):
        original = b'auto_correct = true\n[sounds]\nenabled = true\nauto_correct = true\n'
        after = tomllib.loads(probe.private_config(original, auto_off=True).decode())
        self.assertEqual(after, {'auto_correct': False, 'auto_correct_learning': False,
                                'sounds': {'enabled': True, 'auto_correct': True}})

    def test_regression_fixtures_cover_reported_words_and_four_typing_speeds(self):
        cases = json.loads((probe.ROOT/'tests/fixtures/typing-latency-cases.json').read_text())['cases']
        names = {case['name'] for case in cases}
        self.assertEqual(len(names), len(cases))
        self.assertTrue({'context-space','context-manual','context-enter','context-undo',
                         'context-backspace','globe-pending-context','mixed-languages'} <= names)
        for gap in [10,20,40,80]:
            self.assertIn(f'fast-uk-{gap}ms', names)
            self.assertIn(f'fast-en-{gap}ms', names)
        for case in cases:
            self.assertIn('expect', case['actions'][-1])
            self.assertEqual(case['source'].split('.')[-1] in ['ABC','Ukrainian-PC'], True)

    def test_timed_trigger_is_followed_immediately_by_observation(self):
        cases = json.loads((probe.ROOT/'tests/fixtures/typing-latency-cases.json').read_text())['cases']
        for case in cases:
            for trigger, following in zip(case['actions'], case['actions'][1:]):
                if 'mark' not in trigger:
                    continue
                self.assertEqual(following.get('since'), trigger['mark'], case['name'])
                self.assertIn('expect', following, case['name'])
                if 'text' in trigger or 'key' in trigger:
                    self.assertEqual(trigger.get('gap'), 0, case['name'])
                if trigger.get('undo'):
                    self.assertEqual(trigger.get('keyWait'), 0, case['name'])
                    self.assertEqual(trigger.get('settle'), 0, case['name'])


class GateTests(unittest.TestCase):
    def test_waits_for_fullscreen_space_transition_without_targeting_hidden_windows(self):
        hidden = dict(app_name='Paradev',pid=10,window_id=20,is_on_screen=False,bounds={'height':1000})
        visible = {**hidden,'is_on_screen':True}
        with patch.object(probe,'driver',side_effect=[{'windows':[hidden]},{'windows':[visible]}]), patch.object(probe.time,'sleep'):
            self.assertEqual(probe.terminal_window(None),dict(pid=10,window_id=20))
        with patch.object(probe,'driver',return_value={'windows':[hidden]}):
            with self.assertRaisesRegex(RuntimeError,'exact visible'):
                probe.terminal_window(None,timeout=0)

    def test_explicit_window_is_snapshotted_before_returning_from_another_space(self):
        hidden = dict(app_name='Paradev',pid=10,window_id=20,is_on_screen=False,bounds={'height':1000})
        visible = {**hidden,'is_on_screen':True}
        with patch.object(probe,'driver',side_effect=[{'windows':[hidden]}, {}, {}, {'windows':[visible]}]) as driver, patch.object(probe.time,'sleep'):
            self.assertEqual(probe.terminal_window(20),dict(pid=10,window_id=20))
            self.assertEqual([c.args[0] for c in driver.call_args_list],
                             ['list_windows','get_window_state','bring_to_front','list_windows'])

    def test_locked_mac_never_reaches_gui_or_process_changes(self):
        for locked, bundle in [(True,'com.apple.finder'),(False,'com.apple.loginwindow')]:
            with self.subTest(locked=locked,bundle=bundle), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                config = root/'config.toml'; config.write_text('auto_correct = true\nswitch_layout = true\n')
                binary = root/'upyr'; binary.write_bytes(b'test')
                status = dict(postAccess=True,screenLocked=locked,bundle=bundle)
                def run(command, **kwargs):
                    if command[0] == 'swiftc': return ''
                    if command[0] == 'cua-driver': return 'Accessibility: granted\nScreen Recording: granted\n'
                    return json.dumps(status)
                args = argparse.Namespace(output=root/'output',case=None,run=True,surface='native',repetitions=1,workspace='',window_id=None)
                with patch.object(probe,'CONFIG',config), patch.object(probe,'BACKGROUND',binary), \
                     patch.object(probe.sys,'platform','darwin'), patch.object(probe.probe,'run',side_effect=run), \
                     patch.object(probe,'driver') as driver, patch.object(probe,'stop_upyr') as stop, \
                     patch.object(probe.subprocess,'Popen') as launch:
                    self.assertEqual(probe.main(args), 2)
                    driver.assert_not_called(); stop.assert_not_called(); launch.assert_not_called()
                report = json.loads((root/'output/report.json').read_text())
                self.assertEqual(report['status'],'blocked')
                self.assertEqual(report['latency'],{})
                self.assertFalse((root/'output/private-config.toml').exists())


if __name__ == '__main__':
    unittest.main()
