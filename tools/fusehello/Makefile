# EmiBSD's own test program (see fusehello.c): an OpenBSD-style Makefile that
# tools/xtask/src/userland.rs evaluates like those of the reference tree.
# libfuse installs its headers in /usr/include/fuse; fuse.pc's Cflags name it.

PROG=	fusehello
BINDIR=	/usr/sbin
LDSTATIC=	-static
NOMAN=	yes
CFLAGS+=	-I${DESTDIR}/usr/include/fuse
DPADD+=	${LIBFUSE}
LDADD+=	-lfuse

.include <bsd.prog.mk>
