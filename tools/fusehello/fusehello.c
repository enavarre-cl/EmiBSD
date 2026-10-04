/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */

/*
 * fusehello: a small read-only FUSE file system with a fixed tree.
 *
 * EmiBSD's own test program, not OpenBSD code. It is linked to OpenBSD's
 * libfuse, which opens /dev/fuse0, mounts the kernel's fusefs on the mount
 * point and answers the kernel's requests with the operations below:
 *
 *	/		a directory
 *	/hello.txt	"m10d-fuse-42\n"
 *	/sub/		a directory
 *	/sub/deep.txt	"m10d-fuse-sub-42\n"
 *
 * Every write is refused (EROFS for opens for writing; the other operations
 * are left out, so libfuse answers ENOSYS for them).
 *
 * usage: fusehello [-d] [-f] mountpoint	(fuse_main(3)'s options)
 */

#include <sys/types.h>
#include <sys/stat.h>
#include <sys/statvfs.h>

#include <errno.h>
#include <fcntl.h>
#include <fuse.h>
#include <string.h>

struct node {
	const char	*path;		/* absolute, as libfuse passes it */
	const char	*name;		/* the last component */
	const char	*parent;	/* the parent's path */
	mode_t		 type;		/* S_IFDIR or S_IFREG */
	const char	*data;		/* a file's contents */
};

static const struct node nodes[] = {
	{ "/",			"",		"",	S_IFDIR, NULL },
	{ "/hello.txt",		"hello.txt",	"/",	S_IFREG, "m10d-fuse-42\n" },
	{ "/sub",		"sub",		"/",	S_IFDIR, NULL },
	{ "/sub/deep.txt",	"deep.txt",	"/sub",	S_IFREG,
	    "m10d-fuse-sub-42\n" },
};

#define NNODES	(sizeof(nodes) / sizeof(nodes[0]))

static const struct node *
lookup(const char *path)
{
	size_t i;

	for (i = 0; i < NNODES; i++)
		if (strcmp(nodes[i].path, path) == 0)
			return (&nodes[i]);
	return (NULL);
}

static void
fill_stat(const struct node *n, struct stat *st)
{
	size_t i;

	memset(st, 0, sizeof(*st));
	st->st_uid = 0;
	st->st_gid = 0;
	if (n->type == S_IFDIR) {
		st->st_mode = S_IFDIR | 0555;
		/* ".", the entry in the parent, and each subdirectory's "..". */
		st->st_nlink = 2;
		for (i = 0; i < NNODES; i++)
			if (nodes[i].type == S_IFDIR &&
			    strcmp(nodes[i].parent, n->path) == 0)
				st->st_nlink++;
	} else {
		st->st_mode = S_IFREG | 0444;
		st->st_nlink = 1;
		st->st_size = strlen(n->data);
	}
	st->st_blksize = 512;
	st->st_blocks = (st->st_size + 511) / 512;
}

static int
hello_getattr(const char *path, struct stat *st)
{
	const struct node *n;

	if ((n = lookup(path)) == NULL)
		return (-ENOENT);
	fill_stat(n, st);
	return (0);
}

static int
hello_readdir(const char *path, void *buf, fuse_fill_dir_t filler,
    off_t offset, struct fuse_file_info *fi)
{
	const struct node *n;
	struct stat st;
	size_t i;

	(void)offset;
	(void)fi;
	if ((n = lookup(path)) == NULL)
		return (-ENOENT);
	if (n->type != S_IFDIR)
		return (-ENOTDIR);
	fill_stat(n, &st);
	filler(buf, ".", &st, 0);
	filler(buf, "..", NULL, 0);
	for (i = 0; i < NNODES; i++) {
		if (strcmp(nodes[i].parent, path) != 0)
			continue;
		fill_stat(&nodes[i], &st);
		filler(buf, nodes[i].name, &st, 0);
	}
	return (0);
}

static int
hello_open(const char *path, struct fuse_file_info *fi)
{
	const struct node *n;

	if ((n = lookup(path)) == NULL)
		return (-ENOENT);
	if (n->type == S_IFDIR)
		return (-EISDIR);
	if ((fi->flags & O_ACCMODE) != O_RDONLY)
		return (-EROFS);
	return (0);
}

static int
hello_read(const char *path, char *buf, size_t size, off_t offset,
    struct fuse_file_info *fi)
{
	const struct node *n;
	size_t len;

	(void)fi;
	if ((n = lookup(path)) == NULL)
		return (-ENOENT);
	if (n->type == S_IFDIR)
		return (-EISDIR);
	len = strlen(n->data);
	if (offset < 0)
		return (-EINVAL);
	if ((size_t)offset >= len)
		return (0);
	if (size > len - (size_t)offset)
		size = len - (size_t)offset;
	memcpy(buf, n->data + offset, size);
	return ((int)size);
}

static int
hello_statfs(const char *path, struct statvfs *sv)
{
	(void)path;
	memset(sv, 0, sizeof(*sv));
	sv->f_bsize = 512;
	sv->f_frsize = 512;
	sv->f_blocks = 1;
	sv->f_files = NNODES;
	sv->f_namemax = 255;
	sv->f_flag = ST_RDONLY;
	return (0);
}

static const struct fuse_operations hello_ops = {
	.getattr	= hello_getattr,
	.readdir	= hello_readdir,
	.open		= hello_open,
	.read		= hello_read,
	.statfs		= hello_statfs,
};

int
main(int argc, char *argv[])
{
	return (fuse_main(argc, argv, &hello_ops, NULL));
}
