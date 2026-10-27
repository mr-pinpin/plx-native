#!/usr/bin/env python3
"""Hermetic actual lease/timeout/termination coverage, with no TV or network."""
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]


class PreflightTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.repo = self.base / 'repo'
        (self.repo / 'tools').mkdir(parents=True)
        for name in ('tv-lock.sh', 'tv-lock-note.py'):
            shutil.copy2(ROOT / 'tools' / name, self.repo / 'tools' / name)
        (self.repo / 'Makefile').write_text('print-appdir:\n\t@echo /fixture/$(FLAVOR)\n')
        fake = self.repo / 'tools/tv-ssh'
        fake.write_text('''#!/usr/bin/env bash
if [ "${@: -1}" = true ]; then exit 0; fi
printf '%s' "$$" > "$NOTE_STARTED"
if [ "$NOTE_MODE" = hang ]; then sleep 60; else printf '123 NONE\\n2\\n'; fi
''')
        fake.chmod(0o755)
        self.remote = self.base / 'remote-lock'
        self.env = dict(os.environ, TV='fixture', PLX_TV_LOCK_LOCAL=str(self.base),
                        PLX_TV_LOCK_PATH=str(self.remote), PLX_TV_LOCK_STATE=str(self.base / 'state'),
                        PLX_TV_LOCK_LANE='fixture-A', NOTE_STARTED=str(self.base / 'note-started'),
                        NOTE_MODE='normal')
        self.script = self.repo / 'tools/tv-lock.sh'

    def invoke(self, *args, **kwargs):
        return subprocess.run([str(self.script), *args], env=self.env,
                              capture_output=True, text=True, timeout=10, **kwargs)

    def test_successful_notes_still_report_occupancy_and_command_releases_lease(self):
        result = self.invoke('with', '--ttl', '1', '--', 'sh', '-c',
                             'test -r "$PLX_TV_LOCK_PATH/owner" && grep -q "^lane=fixture-A$" "$PLX_TV_LOCK_PATH/owner" && echo PROTECTED_COMMAND')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('123 NONE', result.stdout)
        self.assertIn('2 ssh sessions', result.stderr)
        self.assertIn('PROTECTED_COMMAND', result.stdout)
        self.assertFalse(self.remote.exists())

    def test_hung_advisory_is_bounded_then_runs_command_and_releases_same_lease(self):
        self.env['NOTE_MODE'] = 'hang'
        started = time.monotonic()
        result = self.invoke('with', '--ttl', '1', '--', 'sh', '-c',
                             'test -r "$PLX_TV_LOCK_PATH/owner" && grep -q "^lane=fixture-A$" "$PLX_TV_LOCK_PATH/owner" && echo PROTECTED_COMMAND')
        self.assertLess(time.monotonic() - started, 8)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('5-second budget', result.stderr)
        self.assertIn('PROTECTED_COMMAND', result.stdout)
        self.assertFalse(self.remote.exists())
        pid = int((self.base / 'note-started').read_text())
        with self.assertRaises(ProcessLookupError):
            os.kill(pid, 0)

    def test_termination_after_acquire_during_notes_does_not_orphan_lease_or_run_command(self):
        self.env['NOTE_MODE'] = 'hang'
        with tempfile.TemporaryFile(mode='w+') as output:
            process = subprocess.Popen([str(self.script), 'with', '--ttl', '1', '--',
                                        'sh', '-c', 'echo MUST_NOT_RUN'], env=self.env,
                                       stdout=output, stderr=output)
            deadline = time.monotonic() + 5
            while not (self.base / 'note-started').exists():
                self.assertLess(time.monotonic(), deadline)
                time.sleep(.02)
            self.assertTrue(self.remote.exists())
            process.terminate()
            process.wait(timeout=8)
            self.assertEqual(process.returncode, 143)
            self.assertFalse(self.remote.exists())
            output.seek(0)
            self.assertNotIn('MUST_NOT_RUN', output.read())

    def test_an_existing_owner_keeps_its_lease_after_nested_with_returns(self):
        self.assertEqual(self.invoke('acquire', '--ttl', '1').returncode, 0)
        owner = (self.remote / 'owner').read_text()
        self.assertEqual(self.invoke('with', '--', 'true').returncode, 0)
        self.assertEqual((self.remote / 'owner').read_text(), owner)
        self.assertEqual(self.invoke('release').returncode, 0)

    def test_the_same_budget_covers_a_stalled_make_before_any_ssh_note(self):
        (self.repo / 'Makefile').write_text('print-appdir:\n\t@sleep 60\n\t@echo /fixture/$(FLAVOR)\n')
        started = time.monotonic()
        result = self.invoke('with', '--ttl', '1', '--', 'true')
        self.assertLess(time.monotonic() - started, 8)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('5-second budget', result.stderr)
        self.assertFalse(self.remote.exists())
        self.assertFalse((self.base / 'note-started').exists())

    def test_foreign_lane_cannot_release_lease_when_acquisition_is_refused(self):
        result = self.invoke('acquire', '--ttl', '1')
        self.assertEqual(result.returncode, 0, result.stderr)
        owner = (self.remote / 'owner').read_text()
        self.env['PLX_TV_LOCK_LANE'] = 'fixture-B'
        result = self.invoke('with', '--', 'sh', '-c', 'echo MUST_NOT_RUN')
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual((self.remote / 'owner').read_text(), owner)
        self.assertNotIn('MUST_NOT_RUN', result.stdout)
        self.env['PLX_TV_LOCK_LANE'] = 'fixture-A'
        self.assertEqual(self.invoke('release').returncode, 0)


if __name__ == '__main__':
    unittest.main()
