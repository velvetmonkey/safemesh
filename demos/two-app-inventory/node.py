"""Shared application plumbing; installed separately beside each entry point."""
import argparse
import base64
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import sqlite3
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.request import Request, urlopen

import safemesh_python as sm


def merge_record(replica, wire):
    # The binding reports a record-ID collision as a verdict. Refuse it here, so a
    # collision still fails the request, startup replay and status replay.
    verdict = replica.merge_record_bytes(wire)
    if verdict == 'collision':
        raise ValueError('record ID collision')
    return verdict


class Inventory:
    def __init__(self, directory, coordinate, name):
        directory.mkdir(parents=True, exist_ok=True)
        self.coordinate = coordinate
        self.name = name
        self.online = True
        # Request handlers share this connection only while holding the state lock.
        self.lock = threading.Lock()
        self.db = sqlite3.connect(directory / 'inventory.sqlite', check_same_thread=False)
        self.db.execute('PRAGMA journal_mode=WAL')
        self.db.execute('PRAGMA synchronous=FULL')
        self.db.execute('CREATE TABLE IF NOT EXISTS identity (name TEXT, coordinate INTEGER)')
        identity = self.db.execute('SELECT name, coordinate FROM identity').fetchall()
        if not identity:
            self.db.execute('INSERT INTO identity VALUES (?, ?)', (name, coordinate))
            self.db.commit()
        elif identity != [(name, coordinate)]:
            raise ValueError('store belongs to another application')
        self.db.execute('CREATE TABLE IF NOT EXISTS records (position INTEGER PRIMARY KEY, wire BLOB UNIQUE NOT NULL)')
        self.db.commit()
        self.recover()

    def recover(self):
        self.replica = sm.GCounterReplica(self.coordinate, 2)
        for wire in self.records():
            merge_record(self.replica, wire)

    def records(self):
        return [row[0] for row in self.db.execute('SELECT wire FROM records ORDER BY position')]

    def mutate(self, operation):
        # Serialize requests; acknowledge only after FULL-synchronous commit.
        # On any failure, discard tentative in-memory state and replay the store.
        try:
            with self.db:
                result = operation()
            return result
        except Exception:
            self.recover()
            raise

    def add(self):
        def operation():
            tally = self.replica.state()[self.coordinate] + 1
            wire = self.replica.append_bump(self.coordinate, tally)
            self.db.execute('INSERT INTO records(wire) VALUES (?)', (wire,))
            return {'tally': tally}
        return self.mutate(operation)

    def receive(self, encoded):
        def operation():
            before = self.db.total_changes
            for item in encoded:
                wire = base64.b64decode(item, validate=True)
                merge_record(self.replica, wire)
                self.db.execute('INSERT OR IGNORE INTO records(wire) VALUES (?)', (wire,))
            accepted = self.db.total_changes - before
            return {'accepted': accepted, 'duplicates': len(encoded) - accepted}
        return self.mutate(operation)

    def status(self):
        # The public log encoder preserves arrival order. Replay a sorted set of
        # opaque records to compare complete histories without decoding internals.
        normalized = sm.GCounterReplica(self.coordinate, 2)
        for wire in sorted(self.records()):
            merge_record(normalized, wire)
        return {
            'application': self.name, 'pid': os.getpid(), 'online': self.online,
            'value': self.replica.value(), 'state': self.replica.state(),
            'records': self.db.execute('SELECT count(*) FROM records').fetchone()[0],
            'log_sha256': hashlib.sha256(normalized.log_bytes()).hexdigest(),
            'raw_log_sha256': hashlib.sha256(self.replica.log_bytes()).hexdigest(),
            'package_version': importlib.metadata.version('safemesh-python'),
            'package_file': sm.__file__,
        }


def serve(name, coordinate):
    parser = argparse.ArgumentParser()
    parser.add_argument('--store', type=Path, required=True)
    parser.add_argument('--port', type=int, required=True)
    parser.add_argument('--peer', required=True)
    args = parser.parse_args()
    inventory = Inventory(args.store, coordinate, name)

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def reply(self, code, result):
            data = json.dumps(result).encode()
            self.send_response(code)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def do_GET(self):
            if self.path == '/status':
                with inventory.lock:
                    result = inventory.status()
                self.reply(200, result)
            else:
                self.reply(404, {'error': 'unknown endpoint'})

        def do_POST(self):
            try:
                # Check browser origin and rebinding authority before reading or
                # applying any body, including local controls and peer delivery.
                hosts = self.headers.get_all('Host', [])
                allowed_hosts = {f'127.0.0.1:{self.server.server_port}',
                                 f'localhost:{self.server.server_port}'}
                if len(hosts) != 1 or hosts[0] not in allowed_hosts:
                    raise ValueError('Host must name this loopback node and port')
                origins = self.headers.get_all('Origin', [])
                if origins and origins != ['http://' + hosts[0]]:
                    raise ValueError('Origin must match this node')
                content_types = self.headers.get_all('Content-Type', [])
                if (len(content_types) != 1 or
                        self.headers.get_content_type() != 'application/json'):
                    raise ValueError('Content-Type must be application/json')
                size = int(self.headers.get('Content-Length', '0'))
                if not 0 < size <= 4 * 1024 * 1024:
                    raise ValueError('request must be between 1 byte and 4 MiB')
                body = json.loads(self.rfile.read(size))
                with inventory.lock:
                    if self.path == '/link':
                        if type(body.get('online')) is not bool:
                            raise ValueError('online must be boolean')
                        inventory.online = body['online']
                        result = {'online': inventory.online}
                    elif self.path == '/add':
                        result = inventory.add()
                    elif self.path in ('/records', '/sync'):
                        if not inventory.online:
                            self.reply(503, {'error': 'replication link disconnected'})
                            return
                        if self.path == '/records':
                            result = inventory.receive(body['records'])
                        else:
                            records = inventory.records()
                            # Deliberate fault injection, exposed only in this local demo.
                            dropped = int(bool(body.get('drop_newest')))
                            if dropped:
                                records = records[:-1]
                            if body.get('reverse'):
                                records.reverse()
                            if body.get('duplicate'):
                                records = records + records
                            payload = {'records': [base64.b64encode(w).decode() for w in records]}
                            request = Request(args.peer + '/records', json.dumps(payload).encode(),
                                              {'Content-Type': 'application/json'})
                    else:
                        self.reply(404, {'error': 'unknown endpoint'})
                        return
                # Snapshot under the lock, but never hold it across peer I/O:
                # the peer may be synchronizing back to us at the same time.
                if self.path == '/sync':
                    with urlopen(request, timeout=30) as response:
                        result = json.load(response)
                    result.update(sent=len(records), dropped=dropped,
                                  reversed=bool(body.get('reverse')))
                self.reply(200, result)
            except Exception as error:
                self.reply(400, {'error': str(error)})

    # Threads keep incoming requests moving while a sync waits for its peer.
    # The state lock protects every replica/SQLite access after startup.
    server = ThreadingHTTPServer(('127.0.0.1', args.port), Handler)
    print(json.dumps({'listening': server.server_address, **inventory.status()}), flush=True)
    server.serve_forever()
