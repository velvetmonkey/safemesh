"""Executable acceptance journey. Control RPCs only; apps deliver their own bytes."""
import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import sys
import time
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


def rpc(port, route, body=None):
    data = None if body is None else json.dumps(body).encode()
    request = Request(f'http://127.0.0.1:{port}/{route}', data,
                      {'Content-Type': 'application/json'})
    with urlopen(request, timeout=40) as response:
        return json.load(response)


def emit(event, **details):
    print(json.dumps({'event': event, **details}, sort_keys=True), flush=True)


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def convergence(ports, expected):
    states = [rpc(port, 'status') for port in ports]
    require(all(s['state'] == expected for s in states), f'divergent state: {states}')
    require(all(s['value'] == sum(expected) for s in states), 'wrong total')
    require(all(s['records'] == sum(expected) for s in states), 'missing retained history')
    require(states[0]['log_sha256'] == states[1]['log_sha256'], 'different canonical logs')
    return states


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--wheels', type=Path, required=True)
    parser.add_argument('--run-dir', type=Path, required=True)
    parser.add_argument('--rounds', type=int, default=1000,
                        help='edits per application per phase (three phases)')
    parser.add_argument('--interval', type=float, default=0.025,
                        help='seconds between pairs of local edits')
    parser.add_argument('--leave-divergent', action='store_true',
                        help='negative control: exit nonzero at the planted dropped delivery')
    args = parser.parse_args()
    require(args.rounds > 0 and args.interval >= 0, 'invalid workload')
    root = args.run_dir.resolve()
    root.mkdir(parents=True, exist_ok=False)
    source = Path(__file__).resolve().parent
    env = dict(os.environ)
    env.pop('PYTHONPATH', None)
    env.pop('PYTHONHOME', None)
    env['PYTHONDONTWRITEBYTECODE'] = '1'
    names = ['clinic', 'warehouse']
    # Reserve distinct ephemeral ports until both numbers have been selected.
    reservations = [socket.socket(), socket.socket()]
    for reservation in reservations:
        reservation.bind(('127.0.0.1', 0))
    ports = [s.getsockname()[1] for s in reservations]
    processes = [None, None]
    logs = []
    expected = [0, 0]

    def launch(index):
        name = names[index]
        app = root / name
        logfile = (app / 'process.log').open('a')
        logs.append(logfile)
        process = subprocess.Popen([
            str(app / 'venv/bin/python'), '-E', '-s', str(app / f'{name}.py'),
            '--store', str(app / 'store'), '--port', str(ports[index]),
            '--peer', f'http://127.0.0.1:{ports[1-index]}',
        ], cwd=app, env=env, stdout=logfile, stderr=subprocess.STDOUT)
        processes[index] = process
        for _ in range(200):
            require(process.poll() is None, f'{name} exited; inspect {logfile.name}')
            try:
                status = rpc(ports[index], 'status')
                require(status['pid'] == process.pid, 'unexpected listener')
                require(status['package_version'] == '0.1.0', 'wrong package version')
                require(app / 'venv' in Path(status['package_file']).parents,
                        'package did not load from private installed environment')
                emit('started', **status)
                return status
            except (URLError, ConnectionError):
                time.sleep(0.05)
        raise TimeoutError(f'{name} failed to start')

    def kill(index):
        process = processes[index]
        before = rpc(ports[index], 'status')
        process.kill()
        require(process.wait(timeout=10) == -signal.SIGKILL, 'not killed with SIGKILL')
        emit('sigkill', application=names[index], pid=process.pid, persisted=before)
        return before

    def sync(index, **faults):
        result = rpc(ports[index], 'sync', faults)
        emit('delivery', sender=names[index], **result)
        return result

    def edits(phase):
        for _ in range(args.rounds):
            for index in range(2):
                result = rpc(ports[index], 'add', {})
                expected[index] += 1
                require(result['tally'] == expected[index], 'local tally/sequence failed after replay')
            time.sleep(args.interval)
        emit('edits', phase=phase, expected=expected[:],
             states=[rpc(p, 'status') for p in ports])

    try:
        for index, name in enumerate(names):
            app = root / name
            app.mkdir()
            for filename in (f'{name}.py', 'node.py', 'requirements.txt'):
                shutil.copy2(source / filename, app / filename)
            subprocess.run([sys.executable, '-m', 'venv', str(app / 'venv')], check=True, env=env)
            subprocess.run([str(app / 'venv/bin/pip'), 'install', '--no-index',
                            '--find-links', str(args.wheels.resolve()),
                            '-r', str(app / 'requirements.txt')], check=True, env=env)
            reservations[index].close()
            launch(index)
        require(processes[0].pid != processes[1].pid, 'applications share a process')
        started = time.monotonic()
        edits('connected')
        sync(0)
        sync(1)
        emit('initial_convergence', states=convergence(ports, expected))
        for port in ports:
            rpc(port, 'link', {'online': False})
        for index in range(2):
            try:
                rpc(ports[index], 'sync', {})
            except HTTPError as error:
                require(error.code == 503, 'wrong partition error')
            else:
                raise AssertionError('partition allowed delivery')
        edits('partitioned')
        before = kill(0)
        recovered = launch(0)
        for key in ('state', 'value', 'records', 'log_sha256', 'raw_log_sha256'):
            require(before[key] == recovered[key], f'restart lost {key}')
        rpc(ports[0], 'link', {'online': False})
        edits('partitioned_after_restart')
        for port in ports:
            rpc(port, 'link', {'online': True})
        for index in range(2):
            delivery = sync(index, reverse=True, duplicate=True)
            require(delivery['duplicates'] > 0 and delivery['accepted'] > 0,
                    'did not exercise both duplicate and new admission')
        emit('reconnected', states=convergence(ports, expected))

        # Drop the only new, maximal tally: dropping an older G-Counter bump
        # would not necessarily change the value and would be a weak control.
        rpc(ports[0], 'add', {})
        expected[0] += 1
        sync(0, reverse=True, duplicate=True, drop_newest=True)
        if args.leave_divergent:
            convergence(ports, expected)  # Must raise, producing a nonzero exit.
            raise AssertionError('negative control unexpectedly converged')
        try:
            convergence(ports, expected)
        except AssertionError as error:
            emit('planted_divergence_detected', detail=str(error))
        else:
            raise AssertionError('acceptance failed to detect dropped final record')
        sync(0, reverse=True, duplicate=True)
        sync(1, reverse=True, duplicate=True)
        convergence(ports, expected)
        # Also prove each application's complete final store survives SIGKILL.
        for index in range(2):
            kill(index)
            launch(index)
        states = convergence(ports, expected)
        emit('PASS', states=states, unique_records=sum(expected),
             elapsed_seconds=round(time.monotonic() - started, 3),
             run_dir=str(root))
    finally:
        for reservation in reservations:
            reservation.close()
        for process in processes:
            if process is not None and process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
        for logfile in logs:
            logfile.close()


if __name__ == '__main__':
    main()
