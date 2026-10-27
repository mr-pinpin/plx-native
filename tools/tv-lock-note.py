#!/usr/bin/env python3
"""Best-effort TV occupancy notes: one five-second host budget, no lease decisions.

Use the existing make identities and tv-ssh transport. Output goes through temporary files,
so an SSH multiplexer's inherited stdout cannot keep a capture pipe open past the deadline.
Only processes started here receive cancellation; existing SSH masters are not touched.
"""
import os
from pathlib import Path
import shlex
import signal
import subprocess
import sys
import tempfile
import time

BUDGET_SECONDS = 5.0
MAX_OUTPUT = 16384


def run(argv, deadline, env=None):
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        raise TimeoutError
    with tempfile.TemporaryFile() as output:
        process = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=output,
                                   stderr=subprocess.DEVNULL, env=env, start_new_session=True)
        try:
            process.wait(timeout=remaining)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait(timeout=.25)
            raise TimeoutError from None
        finally:
            if process.poll() is None:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                process.wait(timeout=.25)
        if process.returncode:
            raise RuntimeError('advisory command failed')
        output.seek(0)
        result = output.read(MAX_OUTPUT + 1)
        if len(result) > MAX_OUTPUT:
            raise RuntimeError('advisory output exceeded its budget')
        return result.decode('utf-8', errors='replace').strip()


def main():
    def interrupted(signum, _frame):
        raise SystemExit(128 + signum)
    for signum in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(signum, interrupted)
    repo, host = Path(sys.argv[1]), sys.argv[2]
    deadline = time.monotonic() + BUDGET_SECONDS
    try:
        directories = [run(['make', '-s', '-C', str(repo), 'FLAVOR=' + flavor,
                            'print-appdir'], deadline) for flavor in ('stable', 'debug')]
        if not all(directories):
            return 1
        # Remote timeout also ends a proc scan after the SSH peer has disconnected.
        scan = ('for d in ' + ' '.join(map(shlex.quote, directories)) + '; do '
                'printf "%s " "$(fuser "$d/plxnative" 2>/dev/null || echo NONE)"; '
                'done; echo; netstat -an 2>/dev/null | '
                "grep -c 'ESTABLISHED.*:22 \\|:22 .*ESTABLISHED'; true")
        command = 'timeout 3 sh -c ' + shlex.quote(scan)
        env = dict(os.environ, PLX_TV_ADDR=host)
        print(run([str(repo / 'tools/tv-ssh'), 'ssh', 'tv', command], deadline, env), flush=True)
        return 0
    except (TimeoutError, subprocess.TimeoutExpired):
        return 124
    except (OSError, RuntimeError):
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
