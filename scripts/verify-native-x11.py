#!/usr/bin/env python3
"""Exercise native_form with real X11 input; run under Xvfb and an EWMH window manager.

Build first: cargo build -p lumendesk --features native --example native_form --locked
Requires xdotool, wmctrl, xprop, ImageMagick import, an 800x600 viewport at 96 DPI.
Uses ASCII input only; this does not verify IME, accessibility or GPU hardware.
"""
import argparse
import datetime
import hashlib
import json
import os
import pathlib
import re
import shutil
import subprocess
import time

SCENARIOS = ('disabled', 'repeated', 'close', 'minimized-close')
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('scenario', nargs='?', choices=('all',) + SCENARIOS, default='all')
parser.add_argument('--repo', type=pathlib.Path, default=pathlib.Path(__file__).resolve().parents[1])
parser.add_argument('--binary', type=pathlib.Path)
parser.add_argument('--output', type=pathlib.Path)
args = parser.parse_args()
repo = args.repo.resolve()
binary = (args.binary or repo / 'target/debug/examples/native_form').resolve()
out = (args.output or repo / 'target/native-x11-evidence').resolve()
out.mkdir(parents=True, exist_ok=True)
(out / 'cache').mkdir(exist_ok=True)


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def run_case(scenario):
    steps = []

    def command(argv, allow_failure=False):
        started = time.monotonic()
        cp = subprocess.run(argv, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10)
        steps.append({'command': argv, 'exit_code': cp.returncode,
                      'duration_seconds': round(time.monotonic() - started, 3),
                      'stdout': cp.stdout, 'stderr': cp.stderr})
        require(allow_failure or cp.returncode == 0, f'{argv!r}: {cp.stderr}')
        return cp

    def exists(window):
        return command(['xdotool', 'getwindowname', str(window)], True).returncode == 0

    def screenshot(window, name):
        path = out / (name + '.png')
        command(['import', '-window', str(window), str(path)])
        if shutil.which('tesseract'):
            command(['tesseract', str(path), str(out / name), '--psm', '6'])

    def key(value):
        command(['xdotool', 'key', '--clearmodifiers', value])
        time.sleep(0.18)

    def click(window, x, y):
        command(['xdotool', 'mousemove', '--window', str(window), str(x), str(y), 'click', '1'])
        time.sleep(0.2)

    def type_text(value):
        command(['xdotool', 'type', '--clearmodifiers', '--delay', '65', value])
        time.sleep(0.2)

    env = dict(os.environ)
    env['XDG_CACHE_HOME'] = str(out / 'cache')
    env.pop('LUMENDESK_SMOKE_MS', None)
    env.pop('LUMENDESK_CANCEL_FIRST_CLOSE', None)
    closing = scenario in ('close', 'minimized-close')
    if closing:
        env['LUMENDESK_CANCEL_FIRST_CLOSE'] = '1'
    else:
        env['LUMENDESK_SMOKE_MS'] = '9000' if scenario == 'disabled' else '15000'
    diff = command(['git', '-C', str(repo), 'diff', '--', 'crates/lumendesk/src/native.rs']).stdout
    result = {
        'scenario': scenario, 'started_at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'git_head': command(['git', '-C', str(repo), 'rev-parse', 'HEAD']).stdout.strip(),
        'git_status': command(['git', '-C', str(repo), 'status', '--short']).stdout,
        'native_source_sha256': sha256(repo / 'crates/lumendesk/src/native.rs'),
        'cargo_lock_sha256': sha256(repo / 'Cargo.lock'),
        'native_diff_sha256': hashlib.sha256(diff.encode()).hexdigest(),
        'binary': str(binary), 'display': env.get('DISPLAY'),
        'smoke_ms': env.get('LUMENDESK_SMOKE_MS'),
        'input_method': 'xdotool X11 pointer/key events; EWMH WM close requests', 'steps': steps,
    }
    proc = None
    logpath = out / (scenario + '.log')
    try:
        with logpath.open('w') as log:
            proc = subprocess.Popen([str(binary)], cwd=repo, env=env, stdout=log, stderr=log)
            result['pid'] = proc.pid
            result['binary_sha256'] = sha256(pathlib.Path(f'/proc/{proc.pid}/exe'))
            window = None
            for _ in range(100):
                require(proc.poll() is None, 'native_form exited before its window appeared')
                found = command(['xdotool', 'search', '--onlyvisible', '--pid', str(proc.pid), '--name', 'LumenDesk'], True)
                if found.returncode == 0 and found.stdout.strip():
                    window = int(found.stdout.strip().splitlines()[-1])
                    break
                time.sleep(0.1)
            require(window is not None, 'native_form window did not appear within 10 seconds')
            result['window'] = window
            command(['xdotool', 'windowactivate', '--sync', str(window)])
            geometry = command(['xdotool', 'getwindowgeometry', '--shell', str(window)]).stdout
            require('WIDTH=800\n' in geometry and 'HEIGHT=600\n' in geometry,
                    'This coordinate-based test requires the default 800x600 viewport at 96 DPI')
            time.sleep(0.5)
            screenshot(window, scenario + '-initial')
            if scenario == 'disabled':
                click(window, 43, 120)
                click(window, 155, 78)
                type_text('   ')
                click(window, 43, 120)
                screenshot(window, 'disabled-whitespace')
            elif scenario == 'repeated':
                click(window, 155, 78)
                type_text('Orion')
                click(window, 43, 120)
                click(window, 43, 120)
                screenshot(window, 'repeated-two-submits')
                click(window, 155, 78)
                key('ctrl+a')
                type_text('  Luna  ')
                for value in ('Tab', 'space', 'Tab', 'space'):
                    key(value)
                screenshot(window, 'repeated-keyboard-third')
            else:
                if scenario == 'minimized-close':
                    command(['xdotool', 'windowminimize', str(window)])
                    time.sleep(0.5)
                    state = command(['xprop', '-id', str(window), '_NET_WM_STATE']).stdout
                    require('_NET_WM_STATE_HIDDEN' in state, 'Window manager did not minimize the app')
                command(['wmctrl', '-ic', hex(window)])
                time.sleep(0.8)
                result['alive_after_first_close'] = proc.poll() is None
                result['window_exists_after_first_close'] = exists(window)
                require(result['alive_after_first_close'] and result['window_exists_after_first_close'],
                        'First close request was not cancelled')
                require('lumendesk: first close cancelled' in logpath.read_text(), 'Missing close cancellation log')
                if scenario == 'close':
                    screenshot(window, 'close-first-cancelled')
                else:
                    command(['xprop', '-id', str(window), '_NET_WM_STATE'])
                command(['wmctrl', '-ic', hex(window)])
            result['exit_code'] = proc.wait(timeout=3 if closing else 22)
            result['window_exists_after_exit'] = exists(window)
            require(result['exit_code'] == 0, 'App did not exit successfully')
            require(not result['window_exists_after_exit'], 'App window remained after process exit')
        result['log'] = logpath.read_text()
        if not closing:
            states = re.findall(r'lumendesk: native smoke complete; state=(\{.*\})', result['log'])
            require(bool(states), 'Missing final model state from actual app')
            result['final_state'] = json.loads(states[-1])
            expected = ({'name': '   ', 'enthusiastic': False, 'greeting': '', 'submissions': 0}
                        if scenario == 'disabled' else
                        {'name': '  Luna  ', 'enthusiastic': True, 'greeting': 'Hello, Luna!', 'submissions': 3})
            require(result['final_state'] == expected, f'Unexpected model state: {result["final_state"]!r}')
        result['status'] = 'PASS'
    except Exception as error:
        result.update(status='FAIL', error=repr(error))
        if proc is not None and proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=3)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
        if logpath.exists():
            result['log'] = logpath.read_text()
    result['observed_process_return_code'] = proc.poll() if proc else None
    result['finished_at'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    (out / (scenario + '-result.json')).write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps({k: v for k, v in result.items() if k != 'steps'}, ensure_ascii=False, indent=2))
    return result['status'] == 'PASS'


cases = SCENARIOS if args.scenario == 'all' else (args.scenario,)
results = [run_case(case) for case in cases]
raise SystemExit(0 if all(results) else 1)
