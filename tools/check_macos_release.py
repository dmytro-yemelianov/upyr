#!/usr/bin/env python3
"""Validate a macOS bundle and simulate upgrades in an isolated directory.

Does not launch a background app, register shortcuts, edit the login item,
change input sources, or touch the installed app/user configuration.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
BINARIES = ('Contents/MacOS/upyr', 'Contents/MacOS/upyr-background',
            'Contents/Helpers/Upyr Settings.app/Contents/MacOS/upyr-settings')


def run(args: list[str], *, config: Path | None = None, required: bool = True) -> subprocess.CompletedProcess:
    env = os.environ.copy()
    if config is not None:
        env['UPYR_CONFIG'] = str(config)
    result = subprocess.run(args, capture_output=True, text=True, env=env, timeout=45)
    if required and result.returncode:
        raise RuntimeError(f'{args[0]} failed: {result.stderr}{result.stdout}')
    return result


def doctor(app: Path, config: Path) -> str:
    return run([str(app / BINARIES[0]), 'doctor'], config=config).stdout


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path, required=True)
    parser.add_argument('--previous-app', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    app, previous = args.app.resolve(), args.previous_app.resolve()
    version = run(['sh', str(ROOT / 'packaging/version.sh')]).stdout.strip()
    run(['codesign', '--verify', '--deep', '--strict', str(app)])
    previous_version = run([str(previous / BINARIES[0]), '--version']).stdout.strip()
    assert run([str(app / BINARIES[0]), '--version']).stdout.strip() == 'upyr ' + version
    for relative in BINARIES:
        architectures = run(['lipo', '-archs', str(app / relative)]).stdout.split()
        assert set(architectures) == {'arm64', 'x86_64'}, relative
    for relative, identifier in [('Contents/Info.plist', 'dev.Upyr.Upyr'),
                                  ('Contents/Helpers/Upyr Settings.app/Contents/Info.plist', 'dev.Upyr.Upyr.Settings')]:
        with (app / relative).open('rb') as handle:
            info = plistlib.load(handle)
        assert info['CFBundleIdentifier'] == identifier
        assert info['CFBundleVersion'] == info['CFBundleShortVersionString'] == version
    assert run([str(app / BINARIES[0]), 'convert', 'ghbdsn']).stdout.strip() == 'привіт'
    assert run([str(app / BINARIES[0]), 'convert', '--direction', 'ukrainian-to-english', 'руддщ']).stdout.strip() == 'hello'
    signature = run(['codesign', '-dv', '--verbose=4', str(app)]).stderr
    assessment = run(['spctl', '--assess', '--type', 'execute', str(app)], required=False)
    stapling = run(['xcrun', 'stapler', 'validate', str(app)], required=False)
    report = dict(version=version, previous_version=previous_version, universal=True,
                  code_signature_valid=True, developer_id='Authority=Developer ID Application:' in signature,
                  ad_hoc='Signature=adhoc' in signature, notarization_ticket_valid=stapling.returncode == 0,
                  gatekeeper_accepted_locally=assessment.returncode == 0, clean_install=False, configuration_loads=[])
    with tempfile.TemporaryDirectory(prefix='upyr-release-upgrade-') as scratch:
        root = Path(scratch)
        clean = root / 'clean' / 'Upyr.app'
        run(['ditto', str(app), str(clean)])
        clean_config = root / 'clean-config.toml'
        run([str(clean / BINARIES[0]), 'init'], config=clean_config)
        clean_report = doctor(clean, clean_config)
        assert 'Config schema: 12' in clean_report and 'Automatic correction: disabled' in clean_report
        assert 'Globe-key switching: false' in clean_report
        assert clean_config.stat().st_mode & 0o777 == 0o600
        report['clean_install'] = True
        fixture = ROOT / 'tests/fixtures/config/published-v6.toml'
        install = root / 'upgrade' / 'Upyr.app'
        config = root / 'upgrade-config.toml'
        shutil.copyfile(fixture, config)
        config.chmod(0o600)
        original = config.read_bytes()
        run(['ditto', str(previous), str(install)])
        assert 'Config schema: 6' in doctor(install, config)
        staged = root / 'Upyr-next.app'
        run(['ditto', str(app), str(staged)])
        install.rename(root / 'Upyr-previous.app')
        staged.rename(install)
        upgraded = doctor(install, config)
        assert 'Upyr ' + version in upgraded and 'Config schema: 12' in upgraded
        assert 'Automatic correction: enabled (Aggressive, minimum 5 characters)' in upgraded
        assert config.read_bytes() == original
        report['published_bundle_upgrade'] = True
        for fixture in sorted((ROOT / 'tests/fixtures/config').glob('*.toml')):
            path = root / fixture.name
            shutil.copyfile(fixture, path)
            original = path.read_bytes()
            assert 'Config schema: 12' in doctor(app, path)
            assert path.read_bytes() == original
            report['configuration_loads'].append(dict(fixture=fixture.name, migrated_in_memory=True, file_unchanged=True))
    report['public_signing_ready'] = report['developer_id'] and report['notarization_ticket_valid'] and report['gatekeeper_accepted_locally']
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
