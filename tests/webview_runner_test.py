"""Launcher tests use fake processes, not a browser or WebKit acceptance result."""
import importlib.util
from contextlib import chdir
import os
from pathlib import Path
import tempfile
import subprocess
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "webview_runner", Path(__file__).parents[1] / "scripts/verify-webview-x11.py"
)
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)


class RunnerTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.authority = self.root / "authority"
        self.authority.write_text("fake authorization for process tests only")
        self.binary = self.root / "app"
        self.wmctrl = self.root / "wmctrl"
        self.pidfile = self.root / "pid"
        self.log = self.root / "run.log"
        self.environment = patch.dict(os.environ, {
            "DISPLAY": ":test", "XAUTHORITY": str(self.authority),
            "PATH": str(self.root) + os.pathsep + os.environ["PATH"],
            "FIXTURE_PID": str(self.pidfile),
        })
        self.environment.start()
        for key in runner.FORBIDDEN_ENV:
            os.environ.pop(key, None)
        self.write(self.wmctrl, '''import os, signal, sys
from pathlib import Path
p = Path(os.environ["FIXTURE_PID"])
if sys.argv[1:] == ["-lp"]:
    if p.exists():
        print("0x00000001 0 " + p.read_text() + " fakehost fixture")
elif sys.argv[1:] == ["-ic", "0x00000001"]:
    os.kill(int(p.read_text()), signal.SIGUSR1)
else:
    print("unexpected wmctrl arguments", file=sys.stderr)
    raise SystemExit(2)
''')

    def tearDown(self):
        self.environment.stop()
        self.tmp.cleanup()

    def write(self, path, source):
        path.write_text("#!/usr/bin/env python3\n" + source)
        path.chmod(0o755)

    def write_success_app(self):
        self.write(self.binary, '''import os, signal, time
from pathlib import Path
p = Path(os.environ["FIXTURE_PID"])
p.write_text(str(os.getpid()))
def close(*_):
    p.unlink()
    print("LUMENDESK_WEB_ACCEPTANCE_PASS", flush=True)
    raise SystemExit(0)
signal.signal(signal.SIGUSR1, close)
print("LUMENDESK_WEB_READY_TO_CLOSE", flush=True)
while True: time.sleep(0.02)
''')

    def test_complete_handshake_and_normal_close(self):
        self.write_success_app()
        runner.run(self.binary, 2, 1, self.log)
        self.assertIn(runner.PASS, self.log.read_text())

    def test_shell_metacharacters_in_binary_path_are_literal(self):
        self.binary = self.root / "app 'quoted'; touch SHELL_EXECUTED; # $(echo boom)"
        self.write_success_app()
        with chdir(self.root):
            runner.run(self.binary, 2, 1, self.log)
        self.assertIn(runner.PASS, self.log.read_text())
        self.assertFalse((self.root / "SHELL_EXECUTED").exists())

    def test_fake_wmctrl_rejects_wrong_close_option_or_id(self):
        # No PID file exists: invalid argv must fail before attempting any signal.
        for args in (["-c", "0x00000001"], ["-ic", "0x00000002"],
                     ["-ic", "0x00000001", "extra"], ["-lp", "extra"], []):
            with self.subTest(args=args):
                result = subprocess.run([str(self.wmctrl), *args], shell=False,
                                        capture_output=True, text=True, timeout=1)
                self.assertEqual(result.returncode, 2)
                self.assertIn("unexpected wmctrl arguments", result.stderr)

    def test_malformed_or_option_like_window_ids_are_rejected(self):
        for window in ("-1", "+1", "-0x1", "1", "0x", "0xGG", "0x100000000",
                       "-ic", "0x1;touch", "$(touch)"):
            with self.subTest(window=window):
                result = subprocess.CompletedProcess([], 0, f"{window} 0 123 host title\n", "")
                with patch.object(runner.subprocess, "run", return_value=result):
                    with self.assertRaisesRegex(runner.AcceptanceError, "invalid window ID"):
                        runner.windows_for_pid(str(self.wmctrl), 123)

    def test_unrelated_pid_is_not_selected(self):
        result = subprocess.CompletedProcess([], 0,
            "0x00000002 0 456 host unrelated\n0x00000001 0 123 host owned\n", "")
        with patch.object(runner.subprocess, "run", return_value=result) as execute:
            self.assertEqual(runner.windows_for_pid(str(self.wmctrl), 123), ["0x00000001"])
            self.assertEqual(runner.windows_for_pid(str(self.wmctrl), 789), [])
            self.assertFalse(execute.call_args.kwargs["shell"])
            self.assertEqual(execute.call_args.args[0], [str(self.wmctrl), "-lp"])

    def test_timeout_is_failure(self):
        self.write(self.binary, "import time\ntime.sleep(60)\n")
        with self.assertRaisesRegex(runner.AcceptanceError, "timed out"):
            runner.run(self.binary, 0.1, 0.1, self.log)

    def test_normal_close_timeout_is_failure(self):
        self.write(self.binary, """import os, signal, time
from pathlib import Path
Path(os.environ["FIXTURE_PID"]).write_text(str(os.getpid()))
signal.signal(signal.SIGUSR1, signal.SIG_IGN)
print("LUMENDESK_WEB_READY_TO_CLOSE", flush=True)
while True: time.sleep(0.02)
""")
        with self.assertRaisesRegex(runner.AcceptanceError, "normal window close"):
            runner.run(self.binary, 2, 0.1, self.log)

    def test_missing_wmctrl_fails(self):
        self.write(self.binary, "pass\n")
        with patch.object(runner.shutil, "which", return_value=None):
            with self.assertRaisesRegex(runner.AcceptanceError, "wmctrl is missing"):
                runner.preflight(self.binary)

    def test_clean_exit_without_assertions_is_failure(self):
        self.write(self.binary, "print('nothing tested')\n")
        with self.assertRaisesRegex(runner.AcceptanceError, "handshake"):
            runner.run(self.binary, 2, 1, self.log)

    def test_premature_pass_is_failure(self):
        self.write(self.binary, "print('LUMENDESK_WEB_ACCEPTANCE_PASS', flush=True)\n")
        with self.assertRaisesRegex(runner.AcceptanceError, "before normal close"):
            runner.run(self.binary, 2, 1, self.log)

    def test_stderr_cannot_supply_pass(self):
        self.write(self.binary, "import sys\nprint('LUMENDESK_WEB_ACCEPTANCE_PASS', file=sys.stderr)\n")
        with self.assertRaisesRegex(runner.AcceptanceError, "handshake"):
            runner.run(self.binary, 2, 1, self.log)

    def test_sandbox_override_refused(self):
        self.write(self.binary, "pass\n")
        os.environ["WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS"] = "1"
        with self.assertRaisesRegex(runner.AcceptanceError, "refusing"):
            runner.preflight(self.binary)

    def test_smoke_timer_refused(self):
        self.write(self.binary, "pass\n")
        os.environ["LUMENDESK_SMOKE_MS"] = "500"
        with self.assertRaisesRegex(runner.AcceptanceError, "refusing"):
            runner.preflight(self.binary)

    def test_missing_display_fails(self):
        os.environ.pop("DISPLAY")
        with self.assertRaisesRegex(runner.AcceptanceError, "DISPLAY"):
            runner.preflight(self.binary)


if __name__ == "__main__":
    unittest.main()
