"""Run with the installed safemesh-python wheel: python test_sync.py.

A barrier in two forwarding peers makes both /sync handlers wait on outgoing
HTTP before either delivery reaches the opposite application.
"""
from concurrent.futures import ThreadPoolExecutor
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


def rpc(port, route, body=None, timeout=40):
    data = None if body is None else json.dumps(body).encode()
    request = Request(f'http://127.0.0.1:{port}/{route}', data,
                      {'Content-Type': 'application/json'})
    with urlopen(request, timeout=timeout) as response:
        return json.load(response)


class SimultaneousSync(unittest.TestCase):
    def test_simultaneous_sync_serves_records(self):
        with tempfile.TemporaryDirectory(prefix='inventory-sync-') as directory:
            self.exercise(Path(directory))

    def exercise(self, root):
        barrier = threading.Barrier(2)
        outgoing = [threading.Event(), threading.Event()]
        reservations = [socket.socket(), socket.socket()]
        for sock in reservations:
            sock.bind(('127.0.0.1', 0))
        ports = [sock.getsockname()[1] for sock in reservations]
        processes, logs, proxies = [], [], []

        def forwarding_handler(index):
            class Forward(BaseHTTPRequestHandler):
                def log_message(self, *_):
                    pass

                def do_POST(self):
                    body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                    outgoing[index].set()
                    barrier.wait(timeout=10)
                    try:
                        result = rpc(ports[1-index], 'records', body)
                        code = 200
                    except HTTPError as error:
                        result, code = json.load(error), error.code
                    data = json.dumps(result).encode()
                    self.send_response(code)
                    self.send_header('Content-Length', str(len(data)))
                    self.end_headers()
                    try:
                        self.wfile.write(data)
                    except BrokenPipeError:
                        pass  # Expected when reproducing the old 30-second timeout.
            return Forward

        try:
            for index, name in enumerate(('clinic', 'warehouse')):
                proxy = ThreadingHTTPServer(('127.0.0.1', 0), forwarding_handler(index))
                proxies.append(proxy)
                threading.Thread(target=proxy.serve_forever, daemon=True).start()
                log = (root / f'{name}.log').open('w')
                logs.append(log)
                reservations[index].close()
                process = subprocess.Popen([
                    sys.executable, str(Path(__file__).with_name(f'{name}.py')),
                    '--store', str(root / name), '--port', str(ports[index]),
                    '--peer', f'http://127.0.0.1:{proxy.server_port}',
                ], stdout=log, stderr=log)
                processes.append(process)
                for attempt in range(100):
                    self.assertIsNone(process.poll(), f'{name} exited')
                    try:
                        rpc(ports[index], 'status', timeout=1)
                        break
                    except (URLError, ConnectionError):
                        time.sleep(0.05)
                else:
                    self.fail(f'{name} did not start')
                self.assertEqual(rpc(ports[index], 'add', {}), {'tally': 1})

            def timed_rpc(port, route, body):
                start = time.monotonic()
                try:
                    result = rpc(port, route, body)
                except HTTPError as error:
                    result = {'http_error': error.code, **json.load(error)}
                return time.monotonic() - start, result

            with ThreadPoolExecutor(max_workers=4) as pool:
                syncs = [pool.submit(timed_rpc, port, 'sync', {}) for port in ports]
                for event in outgoing:
                    self.assertTrue(event.wait(10), 'sync did not reach outgoing HTTP')
                probes = [pool.submit(timed_rpc, port, 'records', {'records': []})
                          for port in ports]
                probe_results = [future.result(timeout=40) for future in probes]
                sync_results = [future.result(timeout=40) for future in syncs]
            print(json.dumps({'records': probe_results, 'sync': sync_results}), flush=True)
            for elapsed, result in probe_results:
                self.assertLess(elapsed, 2, 'ordinary request blocked behind outgoing sync')
                self.assertEqual(result, {'accepted': 0, 'duplicates': 0})
            for elapsed, result in sync_results:
                self.assertLess(elapsed, 2, 'simultaneous sync stalled')
                self.assertEqual(result, {'accepted': 1, 'duplicates': 0,
                                          'sent': 1, 'dropped': 0, 'reversed': False})
            for port in ports:
                self.assertEqual(rpc(port, 'status')['state'], [1, 1])
            # Concurrent local writes must retain the read/append/commit boundary.
            with ThreadPoolExecutor(max_workers=8) as pool:
                adds = list(pool.map(lambda _: rpc(ports[0], 'add', {})['tally'], range(20)))
            self.assertEqual(sorted(adds), list(range(2, 22)))
            self.assertEqual(rpc(ports[0], 'status')['state'], [21, 1])
            self.assertEqual(rpc(ports[0], 'status')['records'], 22)
        finally:
            for process in processes:
                process.terminate()
                process.wait(timeout=5)
            for proxy in proxies:
                proxy.shutdown()
                proxy.server_close()
            for sock in reservations:
                sock.close()
            for log in logs:
                log.close()


if __name__ == '__main__':
    unittest.main()
