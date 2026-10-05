#!/usr/bin/env python3
"""Make libsa's test fixtures: a small FFS1, FFS2 and ISO 9660 image of one staged tree,
made by OpenBSD's makefs (the host build `cargo xtask userland` leaves), zlib-compressed.

Run from anywhere: python3 sys/lib/libsa/testdata/gen_fixtures.py (after `just userland`).
The tests (`sys/lib/libsa/ufs/tests.rs`) read the images; regenerate them only when the
staged tree below changes.
"""
import gzip, os, subprocess, tempfile, zlib

OUT = os.path.dirname(os.path.abspath(__file__))
W = os.path.abspath(os.path.join(OUT, "../../../.."))
MAKEFS = W + "/target/userland/amd64/host/bin/makefs"
S = tempfile.mkdtemp(prefix="libsa-fixture-")

root = S + "/root"
os.makedirs(root + "/etc")
os.makedirs(root + "/dir/deep")
open(root + "/etc/boot.conf", "w").write("set timeout 0\necho fixture\n")
open(root + "/hello.txt", "w").write("hello from ffs\n")
open(root + "/dir/deep/sub.txt", "w").write("deep file\n")
open(root + "/big.dat", "wb").write(b"".join(b"%06d\n" % i for i in range(10000)))
open(root + "/hello.gz", "wb").write(gzip.compress(b"compressed hello\n" * 50, mtime=0))
os.symlink("dir/deep/sub.txt", root + "/link")
os.symlink("/hello.txt", root + "/dir/abs")
owners = []
for d, dirs, files in os.walk(root):
    for n in dirs + files:
        p = os.path.join(d, n)[len(root):]
        owners.append(("0755" if n in dirs else "0644") + " 0 0 " + p)
open(S + "/owners.txt", "w").write("\n".join(owners) + "\n")
env = dict(os.environ, EMIBSD_OWNERS=S + "/owners.txt", EMIBSD_STAGING=root)
for name, args in [
    ("ffs1", ["-t", "ffs", "-s", "1m", "-o", "version=1,bsize=4096,fsize=512,density=4096"]),
    ("ffs2", ["-t", "ffs", "-s", "1m", "-o", "version=2,bsize=4096,fsize=512,density=4096"]),
    ("cd9660", ["-t", "cd9660"]),
]:
    img = S + "/" + name + ".img"
    subprocess.run([MAKEFS, "-T", "1790985600"] + args + [img, root], env=env, check=True)
    raw = open(img, "rb").read()
    if name == "ffs2":
        # makefs writes FFS2's super-block at SBLOCK_UFS1 (mkfs.c: "makefs is used for small
        # filesystems"); libsa's ufs2 reads it at SBLOCK_UFS2 only, where newfs(8) puts it.
        # Copy it there, over inodes this small tree does not use, with fs_sblockloc fixed.
        assert raw[65536:65536 + 8192] == bytes(8192), "SBLOCK_UFS2 area in use"
        sb = bytearray(raw[8192:8192 + 8192])
        sb[1000:1008] = (65536).to_bytes(8, "little")
        raw = raw[:65536] + bytes(sb) + raw[65536 + 8192:]
    open(OUT + "/" + name + ".img.z", "wb").write(zlib.compress(raw, 9))
    print(name, len(raw), "->", os.path.getsize(OUT + "/" + name + ".img.z"))
