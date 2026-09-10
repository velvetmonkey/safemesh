#!/usr/bin/env python3
"""Read-only bootstrap identity probe. Usage: identity_probe.py FILE...

Container kind is selected explicitly by suffix: .transaction, .log, .fence,
.state. A suffix selects parsing, not a version identity. No package/schema
identity is inferred from a filename, known tag, source version or sidecar.
"""
import pathlib
import struct
import sys
import zlib


def inspect(path):
    data = path.read_bytes()
    wrapper = "ABSENT"
    frame = "ABSENT"
    schema = "ABSENT"
    detail = ""
    if path.suffix == ".transaction":
        if len(data) < 24:
            raise ValueError("truncated transaction prefix")
        detail = "words=" + str(struct.unpack_from("<QQQ", data))
        wrapper = "ABSENT (unversioned three-field prefix)"
        data = data[24:]
    elif path.suffix == ".fence":
        if len(data) != 24:
            raise ValueError("invalid fence length")
        return [str(path), "ABSENT", frame, schema, wrapper,
                "ownership sidecar words=" + str(struct.unpack("<QQQ", data))]
    elif path.suffix == ".state":
        if not data:
            raise ValueError("empty state")
        return [str(path), "ABSENT", frame, schema, wrapper,
                f"state tag=0x{data[0]:02x}; schema identity not embedded"]
    elif path.suffix != ".log":
        raise ValueError("select container explicitly with .transaction/.log/.state/.fence suffix")
    if not data:
        raise ValueError("missing log tag")
    frame = f"0x{data[0]:02x}"
    if data[0] != 3:
        return [str(path), "ABSENT", frame, schema, wrapper, detail + "; unparsed frame"]
    if len(data) < 13:
        raise ValueError("truncated frame")
    length, complement = struct.unpack_from("<II", data, 1)
    if complement != length ^ 0xffffffff or len(data) != length + 13:
        raise ValueError("frame length disagreed")
    if zlib.crc32(data[1:-4]) != struct.unpack_from("<I", data, len(data)-4)[0]:
        raise ValueError("frame integrity disagreed")
    if length < 8:
        raise ValueError("truncated shape")
    marker, schema_len = struct.unpack_from("<II", data, 9)
    if marker != 0xffffffff or schema_len > length - 8:
        raise ValueError("invalid shape/schema length")
    schema = data[17:17+schema_len].decode("utf-8")
    return [str(path), "ABSENT", frame, schema, wrapper, detail]


if __name__ == "__main__":
    print("file | package version | event-log frame version | payload schema version | durable wrapper version | details")
    failed = False
    for arg in sys.argv[1:]:
        try:
            print(" | ".join(inspect(pathlib.Path(arg))))
        except (OSError, ValueError, struct.error) as exc:
            print(f"{arg} | ERROR: {exc}")
            failed = True
    sys.exit(1 if failed else 0)
