"""Shared application plumbing; installed separately beside each entry point."""
import argparse
import base64
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import sqlite3
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.request import Request, urlopen

import safemesh_python as sm


class Inventory:
    def __init__(self, directory, coordinate, name):
        directory.mkdir(parents=True, exist_ok=True)
        self.coordinate = coordinate
        self.name = name
        self.online = True
        self.db = sqlite3.connect(directory / 'inventory.sqlite')
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
            self.replica.merge_record_bytes(wire)

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
                self.replica.merge_record_bytes(wire)
                self.db.execute('INSERT OR IGNORE INTO records(wire) VALUES (?)', (wire,))
            accepted = self.db.total_changes - before
            return {'accepted': accepted, 'duplicates': len(encoded) - accepted}
        return self.mutate(operation)

    def status(self):
        # The public log encoder preserves arrival order. Replay a sorted set of
        # opaque records to compare complete histories without decoding internals.
        normalized = sm.GCounterReplica(self.coordinate, 2)
        for wire in sorted(self.records()):
            normalized.merge_record_bytes(wire)
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
                self.reply(200, inventory.status())
            else:
                self.reply(404, {'error': 'unknown endpoint'})

        def do_POST(self):
            try:
                size = int(self.headers.get('Content-Length', '0'))
                if not 0 < size <= 4 * 1024 * 1024:
                    raise ValueError('request must be between 1 byte and 4 MiB')
                body = json.loads(self.rfile.read(size))
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
                        with urlopen(request, timeout=30) as response:
                            result = json.load(response)
                        result.update(sent=len(records), dropped=dropped,
                                      reversed=bool(body.get('reverse')))
                else:
                    self.reply(404, {'error': 'unknown endpoint'})
                    return
                self.reply(200, result)
            except Exception as error:
                self.reply(400, {'error': str(error)})

    # One request at a time protects the replica/transaction boundary.
    server = HTTPServer(('127.0.0.1', args.port), Handler)
    print(json.dumps({'listening': server.server_address, **inventory.status()}), flush=True)
    server.serve_forever()
