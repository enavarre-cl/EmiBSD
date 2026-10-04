#!/usr/bin/env python3
"""Generate the inflate test vectors, sys/lib/libz/testdata/inflate_*, with Python's zlib.

    /usr/bin/python3 sys/lib/libz/testdata/gen_inflate.py

The files in the tree were made by macOS's /usr/bin/python3 (zlib 1.2.12). Another zlib
makes valid streams too, but not necessarily the same bytes: rerun the tests after
regenerating. Everything is deterministic (fixed seeds, no time stamps).

Files (all relative to this directory):
- inflate_corpus.bin, inflate_small.bin: the uncompressed data: ~36 KiB (more than a 32 KiB
  window: text, random bytes, runs) and its first 4 KiB-ish slice.
- inflate_level<N>.z: zlib streams of the small corpus at levels 0..9.
- inflate_big<N>.z: the corpus at levels 1 and 6 (level 9 is inflate_wbits15.z).
- inflate_<strategy>.z: the corpus at level 6 with Z_FILTERED, Z_HUFFMAN_ONLY, Z_RLE, Z_FIXED.
- inflate_wbits<N>.z: the corpus at level 9 with windowBits 9..15 (zlib wrapper).
- inflate_raw15.z, inflate_raw12.z: raw deflate (windowBits -15, -12) of the corpus.
- inflate_dict.bin: a preset dictionary; inflate_dict.z (zlib, FDICT set) and inflate_dict_raw.z
  (raw) compress the small corpus with it.
- inflate_fullflush.z: the corpus in three pieces separated by Z_FULL_FLUSH, and
  inflate_fullflush.txt: "<stream offset just past each flush> <corpus offset of each piece>".
- inflate_syncflush.z: the small corpus in two pieces separated by Z_SYNC_FLUSH, and
  inflate_syncflush.txt: "<stream offset of the 00 00 ff ff marker> <corpus offset of piece 2>".
"""

import os
import random
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))


def put(name, data):
    with open(os.path.join(HERE, "inflate_" + name), "wb") as f:
        f.write(data)


def corpus():
    rnd = random.Random(1951)
    words = (
        b"the quick brown fox jumps over a lazy dog while inflate reads a window of "
        b"huffman codes lengths distances literals and stored fixed dynamic blocks"
    ).split()
    text = b" ".join(rnd.choice(words) for _ in range(5200))
    noise = bytes(rnd.getrandbits(8) for _ in range(3000))
    runs = b"\0" * 2000 + b"ab" * 700
    return text[:16000] + noise + runs + text[16000:]


def compress(data, level=6, wbits=15, strategy=zlib.Z_DEFAULT_STRATEGY, zdict=None):
    if zdict is None:
        c = zlib.compressobj(level, zlib.DEFLATED, wbits, 8, strategy)
    else:
        c = zlib.compressobj(level, zlib.DEFLATED, wbits, 8, strategy, zdict)
    return c.compress(data) + c.flush(zlib.Z_FINISH)


def main():
    big = corpus()
    small = big[:3000] + big[16000:16500] + big[19000:19600]
    put("corpus.bin", big)
    put("small.bin", small)
    for level in range(10):
        put("level%d.z" % level, compress(small, level))
    for level in (1, 6):
        put("big%d.z" % level, compress(big, level))
    for name, strategy in (
        ("filtered", zlib.Z_FILTERED),
        ("huffman", zlib.Z_HUFFMAN_ONLY),
        ("rle", zlib.Z_RLE),
        ("fixed", zlib.Z_FIXED),
    ):
        put(name + ".z", compress(big, 6, strategy=strategy))
    for wbits in range(9, 16):
        put("wbits%d.z" % wbits, compress(big, 9, wbits))
    put("raw15.z", compress(big, 6, -15))
    put("raw12.z", compress(big, 6, -12))

    zdict = big[4000:4400] + small[:200]
    put("dict.bin", zdict)
    put("dict.z", compress(small, 6, zdict=zdict))
    put("dict_raw.z", compress(small, 6, -15, zdict=zdict))

    c = zlib.compressobj(6, zlib.DEFLATED, 15)
    out = b""
    flushes = []
    pieces = []
    for start, end in ((0, 12000), (12000, 25000), (25000, len(big))):
        pieces.append(start)
        out += c.compress(big[start:end])
        if end != len(big):
            out += c.flush(zlib.Z_FULL_FLUSH)
            assert out.endswith(b"\0\0\xff\xff")
            flushes.append(len(out))
    out += c.flush(zlib.Z_FINISH)
    put("fullflush.z", out)
    put("fullflush.txt", (" ".join(map(str, flushes + pieces[1:])) + "\n").encode())

    c = zlib.compressobj(6, zlib.DEFLATED, 15)
    out = c.compress(small[:2000]) + c.flush(zlib.Z_SYNC_FLUSH)
    assert out.endswith(b"\0\0\xff\xff")
    marker = len(out) - 4
    out += c.compress(small[2000:]) + c.flush(zlib.Z_FINISH)
    put("syncflush.z", out)
    put("syncflush.txt", ("%d %d\n" % (marker, 2000)).encode())


if __name__ == "__main__":
    main()
