# EmiBSD's own test program (see difftest.c): an OpenBSD-style Makefile that
# tools/xtask/src/userland.rs evaluates like those of the reference tree.
# `cargo xtask diff-openbsd` runs the same static binary on EmiBSD (from the
# ramdisk) and on a real OpenBSD VM (fetched over HTTP).

PROG=	difftest
BINDIR=	/usr/bin
LDSTATIC=	-static
NOMAN=	yes

.include <bsd.prog.mk>
