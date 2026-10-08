#!/usr/bin/env python3
"""Generate the deflate test vectors of sys/lib/libz/deflate.rs.

Run from anywhere with the macOS system Python (zlib 1.2.12):

    /usr/bin/python3 sys/lib/libz/testdata/gen_deflate.py

It drives the system libz (the same 1.2.12 that Python's zlib module is built on) through
ctypes, with exactly the calls the Rust tests make: deflateInit2_, an optional
deflateSetDictionary, deflate() over a schedule of flushes with output buffers of a fixed
size, an optional deflateParams, deflateEnd. ctypes is used instead of zlib.compressobj
because compressobj chooses its own output buffer sizes (which matter at level 0) and has
no deflateParams; for every case at level 1..9 with a large output buffer the script checks
that zlib.compressobj produces the same bytes.

Outputs, next to this script:
- deflate_vectors.txt: one line per case, `name|input|level|wbits|memlevel|strategy|
  schedule|chunk|dict|out_len|out_crc32|total_in|adler`, and `input|name|len|crc32` lines
  describing the inputs (which the tests rebuild, see make_inputs()).
- deflate_ipcomp_text.bin, deflate_zlib_text_l9.bin, deflate_raw_text_l1.bin: complete
  outputs of three cases, compared byte for byte.
"""

import ctypes
import os
import sys
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))

Z_NO_FLUSH, Z_PARTIAL_FLUSH, Z_SYNC_FLUSH, Z_FULL_FLUSH, Z_FINISH, Z_BLOCK = 0, 1, 2, 3, 4, 5
Z_OK, Z_STREAM_END, Z_BUF_ERROR = 0, 1, -5
FLUSHES = {"partial": Z_PARTIAL_FLUSH, "sync": Z_SYNC_FLUSH, "full": Z_FULL_FLUSH,
           "block": Z_BLOCK}


class ZStream(ctypes.Structure):
    _fields_ = [
        ("next_in", ctypes.c_void_p), ("avail_in", ctypes.c_uint), ("total_in", ctypes.c_ulong),
        ("next_out", ctypes.c_void_p), ("avail_out", ctypes.c_uint),
        ("total_out", ctypes.c_ulong), ("msg", ctypes.c_char_p), ("state", ctypes.c_void_p),
        ("zalloc", ctypes.c_void_p), ("zfree", ctypes.c_void_p), ("opaque", ctypes.c_void_p),
        ("data_type", ctypes.c_int), ("adler", ctypes.c_ulong), ("reserved", ctypes.c_ulong),
    ]


libz = ctypes.CDLL("libz.dylib")
libz.zlibVersion.restype = ctypes.c_char_p
VERSION = libz.zlibVersion()
assert VERSION == b"1.2.12", VERSION


def lcg(seed):
    """The tests' pseudo-random generator: x = x * 1103515245 + 12345 mod 2^31."""
    x = seed
    while True:
        x = (x * 1103515245 + 12345) & 0x7FFFFFFF
        yield x


def make_inputs():
    """The inputs, built exactly as deflate.rs builds them."""
    text = open(os.path.join(HERE, "deflate_text.txt"), "rb").read()
    rep = b"0123456789abcdef" * 1500 + b"z" * 7000 + b"hello, world. " * 900
    g = lcg(1)
    rand = bytes((next(g) >> 16) & 0xFF for _ in range(30000))
    mixed = bytearray()
    g = lcg(7)
    i = 0
    while len(mixed) < 102400:
        kind = i % 4
        if kind == 0:
            start = (i * 37) % len(text)
            mixed += text[start:start + 3000]
        elif kind == 1:
            mixed += bytes((next(g) >> 16) & 0xFF for _ in range(2000))
        elif kind == 2:
            mixed += bytes([(i * 7) & 0xFF]) * (500 + (i * 13) % 3000)
        else:
            mixed += bytes(ord("a") + ((next(g) >> 16) % 4) for _ in range(4000))
        i += 1
    mixed = bytes(mixed[:102400])
    return {"empty": b"", "one": b"a", "text": text, "rep": rep, "rand": rand,
            "mixed": mixed}


def run(data, level, wbits, memlevel, strategy, schedule, chunk, dictionary):
    """Compress `data` with the system libz; returns (output, total_in, adler)."""
    strm = ZStream()
    ret = libz.deflateInit2_(ctypes.byref(strm), level, 8, wbits, memlevel, strategy,
                             VERSION, ctypes.sizeof(ZStream))
    assert ret == Z_OK, ret
    if dictionary is not None:
        dbuf = ctypes.create_string_buffer(dictionary, len(dictionary))
        ret = libz.deflateSetDictionary(ctypes.byref(strm), dbuf, len(dictionary))
        assert ret == Z_OK, ret
    inbuf = ctypes.create_string_buffer(data, len(data) + 1)
    base = ctypes.addressof(inbuf)
    out = bytearray()
    obuf = ctypes.create_string_buffer(chunk)

    def feed(lo, hi, flush):
        strm.next_in = base + lo
        strm.avail_in = hi - lo
        while True:
            strm.next_out = ctypes.addressof(obuf)
            strm.avail_out = chunk
            ret = libz.deflate(ctypes.byref(strm), flush)
            assert ret in (Z_OK, Z_STREAM_END, Z_BUF_ERROR), ret
            out.extend(obuf.raw[:chunk - strm.avail_out])
            if strm.avail_out != 0:
                break
        if flush == Z_FINISH:
            assert ret == Z_STREAM_END, ret
        assert strm.avail_in == 0

    parts = schedule.split(":")
    if parts[0] == "finish":
        feed(0, len(data), Z_FINISH)
    elif parts[0] == "params":
        at, lvl, strat = int(parts[1]), int(parts[2]), int(parts[3])
        feed(0, at, Z_NO_FLUSH)
        strm.next_out = ctypes.addressof(obuf)
        strm.avail_out = chunk
        ret = libz.deflateParams(ctypes.byref(strm), lvl, strat)
        assert ret == Z_OK, ret
        out.extend(obuf.raw[:chunk - strm.avail_out])
        feed(at, len(data), Z_FINISH)
    else:
        at = int(parts[1])
        feed(0, at, FLUSHES[parts[0]])
        feed(at, len(data), Z_FINISH)
    total_in, adler = strm.total_in, strm.adler
    ret = libz.deflateEnd(ctypes.byref(strm))
    assert ret == Z_OK, ret
    return bytes(out), total_in, adler


def cases(inputs):
    big = 1 << 18
    out = []

    def add(name, inp, level, wbits, memlevel, strategy, schedule="finish", chunk=big,
            dictionary="-"):
        out.append((name, inp, level, wbits, memlevel, strategy, schedule, chunk, dictionary))

    names = list(inputs)
    # every level, zlib wrapper, defaults
    for inp in names:
        for level in range(10):
            add("level", inp, level, 15, 8, 0)
    # the IPComp stream: raw, 4 KiB window, output space running out mid-stream
    for inp in names:
        for chunk in (big, 64, 7):
            add("ipcomp", inp, -1, -12, 8, 0, chunk=chunk)
    # strategies: filtered, huffman only, rle, fixed
    for inp in names:
        for strategy in (1, 2, 3, 4):
            for level in (1, 6, 9):
                add("strategy", inp, level, 15, 8, strategy)
    # window sizes and memory levels, zlib and raw
    for inp in ("text", "rep", "mixed"):
        for wbits in (15, 9, -12, -15, 8):
            for memlevel in (1, 8, 9):
                for level in (1, 6, 9):
                    add("window", inp, level, wbits, memlevel, 0)
    # stored blocks with small and large windows and pending buffers, small outputs
    for inp in ("rand", "mixed", "text"):
        for wbits in (9, -15):
            for memlevel in (1, 9):
                for chunk in (big, 300, 7):
                    add("stored", inp, 0, wbits, memlevel, 0, chunk=chunk)
    # flushes in the middle
    for inp in ("text", "rep", "mixed"):
        at = len(inputs[inp]) // 3
        for flush in ("partial", "sync", "full", "block"):
            for level in (0, 1, 6):
                for chunk in (big, 37):
                    add("flush", inp, level, 15, 8, 0, f"{flush}:{at}", chunk)
    # preset dictionaries (the last one longer than the window)
    for inp in ("text", "mixed"):
        for wbits in (15, -15):
            for level in (0, 1, 6, 9):
                add("dict", inp, level, wbits, 8, 0, dictionary="text:1000")
    for wbits in (9, -9):
        add("dict", "mixed", 6, wbits, 8, 0, dictionary="text:1000")
    # deflateParams in the middle
    for inp in ("text", "mixed"):
        at = len(inputs[inp]) // 2
        for (l1, l2, s2) in ((1, 9, 0), (9, 1, 0), (6, 0, 0), (0, 6, 0), (0, 1, 0),
                             (6, 6, 2), (6, 6, 3), (6, 6, 4), (6, 4, 1), (3, 3, 0)):
            add("params", inp, l1, 15, 8, 0, f"params:{at}:{l2}:{s2}")
    return out


def main():
    inputs = make_inputs()
    text = inputs["text"]
    lines = []
    for name, data in inputs.items():
        lines.append(f"input|{name}|{len(data)}|{zlib.crc32(data):08x}")
    full = {}
    for (name, inp, level, wbits, memlevel, strategy, schedule, chunk, dictionary) in cases(inputs):
        data = inputs[inp]
        dbytes = None
        if dictionary != "-":
            dbytes = text[:int(dictionary.split(":")[1])]
        got, total_in, adler = run(data, level, wbits, memlevel, strategy, schedule, chunk,
                                   dbytes)
        if level != 0 and schedule == "finish" and chunk == 1 << 18:
            c = zlib.compressobj(level, zlib.DEFLATED, wbits, memlevel, strategy,
                                 **({"zdict": dbytes} if dbytes else {}))
            assert c.compress(data) + c.flush() == got, (name, inp, level, wbits)
        lines.append(f"{name}|{inp}|{level}|{wbits}|{memlevel}|{strategy}|{schedule}|{chunk}|"
                     f"{dictionary}|{len(got)}|{zlib.crc32(got):08x}|{total_in}|{adler:08x}")
        key = (name, inp, level, wbits, memlevel, strategy, schedule, chunk)
        full[key] = got
    with open(os.path.join(HERE, "deflate_vectors.txt"), "w") as f:
        f.write("# generated by gen_deflate.py from zlib %s; do not edit\n" % VERSION.decode())
        f.write("\n".join(lines) + "\n")
    picks = {
        "deflate_ipcomp_text.bin": ("ipcomp", "text", -1, -12, 8, 0, "finish", 1 << 18),
        "deflate_zlib_text_l9.bin": ("level", "text", 9, 15, 8, 0, "finish", 1 << 18),
        "deflate_raw_text_l1.bin": ("window", "text", 1, -15, 8, 0, "finish", 1 << 18),
    }
    for fname, key in picks.items():
        with open(os.path.join(HERE, fname), "wb") as f:
            f.write(full[key])
    print(f"{len(lines)} lines", file=sys.stderr)


if __name__ == "__main__":
    main()
