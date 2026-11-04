#!/usr/bin/env python3
"""Bounded real-WebView acceptance launcher; requires an existing X11 session.

This does not create a display, alter WebKit sandbox policy, or emulate IPC.
The application must report completion of its assertions before a normal
window-manager close request is sent. A timeout never counts as acceptance.

Trust boundary: --binary and wmctrl resolved from PATH are trusted executables,
not sandboxed inputs. All three subprocess sites use argv lists with shell=False;
paths are passed literally, not shell-escaped. Window IDs from wmctrl are validated
as unsigned X11 hex IDs and selected only for the launched process PID.
"""
from __future__ import annotations

import argparse
import os
import re
from pathlib import Path
import selectors
import shutil
import signal
import subprocess
import sys
import time

READY = "LUMENDESK_WEB_READY_TO_CLOSE"
PASS = "LUMENDESK_WEB_ACCEPTANCE_PASS"
MAX_LOG_BYTES = 4 * 1024 * 1024
EXIT_DRAIN_SECONDS = 0.2
FORBIDDEN_ENV = (
    "LUMENDESK_SMOKE_MS",
    "WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS",
    "WEBKIT_DISABLE_SANDBOX",
)


class AcceptanceError(RuntimeError):
    pass


def preflight(binary: Path) -> str:
    if not os.environ.get("DISPLAY"):
        raise AcceptanceError("DISPLAY is missing; run under an authenticated X11 session")
    authority = os.environ.get("XAUTHORITY")
    if not authority or not Path(authority).is_file():
        raise AcceptanceError("XAUTHORITY must name the existing X11 authorization file")
    for key in FORBIDDEN_ENV:
        if key in os.environ:
            raise AcceptanceError(f"refusing {key}: acceptance requires normal close and sandbox policy")
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise AcceptanceError(f"executable not found: {binary}; build web_acceptance first")
    wmctrl = shutil.which("wmctrl")
    if wmctrl is None:
        raise AcceptanceError("wmctrl is missing; an EWMH window manager such as Openbox is required")
    return wmctrl


def windows_for_pid(wmctrl: str, pid: int) -> list[str]:
    result = subprocess.run(
        [wmctrl, "-lp"], capture_output=True, text=True, timeout=1, check=False, shell=False
    )
    if result.returncode:
        raise AcceptanceError(f"wmctrl cannot enumerate windows: {result.stderr.strip()}")
    windows = []
    for line in result.stdout.splitlines():
        fields = line.split(None, 4)
        if len(fields) >= 4 and fields[2] == str(pid):
            window = fields[0]
            if re.fullmatch(r"0x[0-9a-fA-F]{1,8}", window) is None:
                raise AcceptanceError("wmctrl returned an invalid window ID")
            windows.append(window)
    return windows


def stop_owned_group(process: subprocess.Popen[bytes]) -> None:
    # start_new_session=True gives this launch its own group. Never kill by name.
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        process.wait(timeout=1)
    except subprocess.TimeoutExpired:
        pass
    # The leader may have exited while a child still owns inherited pipes.
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    process.wait(timeout=1)


def run(binary: Path, timeout: float, close_timeout: float, output: Path | None) -> None:
    wmctrl = preflight(binary)
    log: list[str] = []
    log_size = 0
    process = subprocess.Popen(
        [str(binary.resolve())], stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        start_new_session=True, shell=False,
    )
    selector = selectors.DefaultSelector()
    buffers: dict[str, bytes] = {"stdout": b"", "stderr": b""}
    ready = False
    passed = False
    closed = False
    deadline = time.monotonic() + timeout
    exit_observed = None
    assert process.stdout is not None and process.stderr is not None
    for name, pipe in (("stdout", process.stdout), ("stderr", process.stderr)):
        os.set_blocking(pipe.fileno(), False)
        selector.register(pipe, selectors.EVENT_READ, name)
    try:
        while True:
            now = time.monotonic()
            if process.poll() is not None and exit_observed is None:
                exit_observed = now
            if exit_observed is None:
                if now >= deadline:
                    phase = "normal window close" if closed else "WebView assertion readiness"
                    raise AcceptanceError(f"timed out waiting for {phase}")
                read_deadline = deadline
            else:
                # WebKit helpers can inherit both pipes after the app has exited.
                # Drain final buffered markers briefly, but do not require helper EOF.
                read_deadline = exit_observed + EXIT_DRAIN_SECONDS
                if not selector.get_map() or now >= read_deadline:
                    if process.returncode != 0:
                        raise AcceptanceError(f"application exited {process.returncode}")
                    if not (ready and closed and passed):
                        raise AcceptanceError("application exited without the complete acceptance handshake")
                    break
            for key, _ in selector.select(min(0.05, read_deadline - now)):
                chunk = os.read(key.fileobj.fileno(), 65536)
                if not chunk:
                    selector.unregister(key.fileobj)
                    continue
                log_size += len(chunk)
                if log_size > MAX_LOG_BYTES:
                    raise AcceptanceError("application output exceeded 4 MiB")
                name = key.data
                log.append(f"[{name}] {chunk.decode('utf-8', errors='replace')}")
                buffers[name] += chunk
                while b"\n" in buffers[name]:
                    line, buffers[name] = buffers[name].split(b"\n", 1)
                    if name == "stdout":
                        text = line.decode("utf-8", errors="replace").rstrip("\r")
                        if text == READY:
                            ready = True
                        elif text == PASS:
                            if not closed:
                                raise AcceptanceError("PASS was reported before normal close")
                            passed = True
            if ready and not closed and process.poll() is None:
                windows = windows_for_pid(wmctrl, process.pid)
                if len(windows) > 1:
                    raise AcceptanceError("expected exactly one application window")
                if windows:
                    result = subprocess.run(
                        [wmctrl, "-ic", windows[0]], capture_output=True, text=True,
                        timeout=1, check=False, shell=False,
                    )
                    if result.returncode:
                        raise AcceptanceError(f"window-manager close failed: {result.stderr.strip()}")
                    closed = True
                    deadline = time.monotonic() + close_timeout
        if windows_for_pid(wmctrl, process.pid):
            raise AcceptanceError("application window remains after process exit")
    finally:
        selector.close()
        stop_owned_group(process)
        process.stdout.close()
        process.stderr.close()
        if output is not None:
            output.parent.mkdir(parents=True, exist_ok=True)
            output.write_text("".join(log), encoding="utf-8")
        elif log:
            print("".join(log), end="")


def bounded_seconds(maximum: float):
    def parse(value: str) -> float:
        number = float(value)
        if not 0 < number <= maximum:
            raise argparse.ArgumentTypeError(f"must be positive and at most {maximum} seconds")
        return number
    return parse


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/debug/examples/web_acceptance"))
    parser.add_argument("--timeout", type=bounded_seconds(45), default=45)
    parser.add_argument("--close-timeout", type=bounded_seconds(5), default=5)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        run(args.binary, args.timeout, args.close_timeout, args.output)
    except (AcceptanceError, OSError, subprocess.SubprocessError) as error:
        print(f"WebView acceptance FAILED: {error}", file=sys.stderr)
        return 1
    print("WebView acceptance passed: real application handshake and normal close")
    return 0


if __name__ == "__main__":
    sys.exit(main())
