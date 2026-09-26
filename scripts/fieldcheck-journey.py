#!/usr/bin/env python3
"""Exercise Fieldcheck through its public CLI; no store inspection."""
import json
import os
from pathlib import Path
import queue
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
DEADLINE = time.monotonic() + 70
clients = []
TARGET = Path(os.environ.get('CARGO_TARGET_DIR', ROOT / 'examples/fieldcheck/target')).resolve()


def check(name, condition):
    if not condition:
        raise AssertionError(name)


class Client:
    def __init__(self, store, writer, network=()):
        self.lines = queue.Queue()
        self.printed = set()
        self.pid = None
        self.checklist = None
        self.process = subprocess.Popen(
            [sys.executable, str(ROOT / 'examples/fieldcheck/fieldcheck.py'),
             str(store), '--service', str(TARGET / 'debug/fieldcheck'),
             '--writer', str(writer), *network],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            text=True, start_new_session=True)
        clients.append(self)
        threading.Thread(target=self.read, daemon=True).start()
        self.wait('service-ready', lambda line: line.startswith('Local service PID '))
        check('compiled-checklist-visible', self.checklist == 'Does the latch hold when pulled?')

    def read(self):
        for line in self.process.stdout:
            self.lines.put(line.rstrip())
        self.lines.put(None)

    def wait(self, name, predicate):
        end = min(DEADLINE, time.monotonic() + 10)
        while True:
            try:
                line = self.lines.get(timeout=max(0, end - time.monotonic()))
            except queue.Empty:
                raise AssertionError(name + ': output timeout')
            check(name + ': CLI exited', line is not None)
            if line not in self.printed:
                print(line, flush=True)
                self.printed.add(line)
            if line == 'Does the latch hold when pulled?':
                self.checklist = line
            if line.startswith('Local service PID '):
                self.pid = int(line.rsplit(' ', 1)[1])
            if predicate(line):
                return line

    def command(self, command):
        self.process.stdin.write(command + '\n')
        self.process.stdin.flush()

    def snapshot(self):
        self.command('show')
        line = self.wait('checklist-snapshot', lambda line: line.startswith('{"records":'))
        state = json.loads(line)
        review = None
        if len(state['records']) >= 2:
            review = self.wait('review-projection', lambda line: line.startswith(('Needs review:', 'Resolved:')))
        return state, review

    def save(self, answer, inspector):
        self.command(f'draft {answer} "{inspector}" "offline observation"')
        self.wait('draft-created', lambda line: line.startswith('Draft '))
        self.command('save')
        self.wait('durable-save', lambda line: line == 'Saved on this device')
        line = self.wait('saved-record', lambda line: line.startswith('{'))
        return json.loads(line)

    def close(self):
        if self.process.poll() is None:
            self.command('quit')
            self.process.wait(timeout=5)


def interrupted(signum, frame):
    raise AssertionError('interrupted-' + signal.Signals(signum).name)


def journey(directory):
    a_store, b_store = directory / 'a', directory / 'b'
    # The documented way to make opposing offline observations is to omit TCP flags.
    b = Client(b_store, 1)
    a = Client(a_store, 0)
    a_record = a.save('Pass', 'Journey A')
    b_record = b.save('Fail', 'Journey B')
    os.kill(b.pid, signal.SIGKILL)
    b.wait('SIGKILL-observed', lambda line: line.startswith('Local service stopped'))
    b.command('reopen')
    b.wait('restart-ready', lambda line: line.startswith('Local service PID '))
    recovered, _ = b.snapshot()
    check('saved-observation-survives-SIGKILL', recovered['records'] == [b_record])
    a.close()
    b.close()
    with socket.socket() as reservation:
        reservation.bind(('127.0.0.1', 0))
        address = '127.0.0.1:' + str(reservation.getsockname()[1])
    print('TCP endpoint ' + address, flush=True)
    b = Client(b_store, 1, ('--listen', address))
    a = Client(a_store, 0, ('--connect', address))
    # Each show is a fresh observable snapshot, not a timing sleep or a journey retry.
    states = []
    exchange_deadline = min(DEADLINE, time.monotonic() + 10)
    for client in (a, b):
        while True:
            state, review = client.snapshot()
            if state['peer_confirmed'] == 2 and len(state['records']) == 2:
                states.append((state, review))
                break
            check('reconnect-exchange-timeout', time.monotonic() < exchange_deadline)
    expected = sorted([a_record, b_record], key=lambda record: json.dumps(record, sort_keys=True))
    for name, (state, review) in zip(('a', 'b'), states):
        actual = sorted(state['records'], key=lambda record: json.dumps(record, sort_keys=True))
        check(name + '-both-original-observations', actual == expected)
        check(name + '-opposing-answers-need-review', review == 'Needs review: north-yard/gate-3/latch')
    check('same-checklist-state', states[0] == states[1])


def main():
    for signum in (signal.SIGINT, signal.SIGTERM):
        signal.signal(signum, interrupted)
    try:
        # Unique stores for concurrent jobs; all CLI/service descendants share a tracked group.
        with tempfile.TemporaryDirectory(prefix='fieldcheck-journey-', dir=ROOT / 'examples/fieldcheck') as directory:
            try:
                journey(Path(directory))
            finally:
                for client in reversed(clients):
                    try:
                        client.close()
                    except (OSError, subprocess.SubprocessError):
                        pass
                    try:
                        os.killpg(client.process.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    client.process.wait()
        print('PASS fieldcheck-disconnect-SIGKILL-restart-reconnect', flush=True)
        return 0
    except (AssertionError, OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        print('FAIL fieldcheck-journey: ' + str(error), file=sys.stderr, flush=True)
        return 1


if __name__ == '__main__':
    sys.exit(main())
