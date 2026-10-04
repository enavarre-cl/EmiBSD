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
 * sr6create: create a softraid(4) RAID 6 volume.
 *
 * EmiBSD's own test program, not OpenBSD code. bioctl(8) refuses `-c 6`
 * ("unsupported RAID level") although the RAID 6 discipline is in the kernel
 * and reachable through BIOCCREATERAID; the OpenBSD userland is built
 * unmodified, so this does what bioctl's bio_createraid() does for a
 * non-crypto level, for level 6. (The interface has no strip size: the
 * discipline chooses it, so there is no -s.)
 *
 * usage: sr6create [-C] -l chunk[,chunk...] softraid0
 */

#include <sys/param.h>	/* NODEV */
#include <sys/ioctl.h>
#include <sys/stat.h>

#include <dev/biovar.h>

#include <err.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define LEVEL		6
#define MIN_DISKS	4

static void __dead
usage(void)
{
	extern char *__progname;

	fprintf(stderr, "usage: %s [-C] -l chunk[,chunk...] softraid0\n",
	    __progname);
	exit(1);
}

/* Print the kernel's messages as bioctl's bio_status() does. */
static void
bio_status(struct bio_status *bs)
{
	extern char *__progname;
	const char *prefix = strlen(bs->bs_controller) ? bs->bs_controller :
	    __progname;
	int i;

	for (i = 0; i < bs->bs_msg_count; i++)
		fprintf(bs->bs_msgs[i].bm_type == BIO_MSG_INFO ?
		    stdout : stderr, "%s: %s\n", prefix, bs->bs_msgs[i].bm_msg);
	if (bs->bs_status == BIO_STATUS_ERROR) {
		if (bs->bs_msg_count == 0)
			errx(1, "unknown error");
		exit(1);
	}
}

int
main(int argc, char *argv[])
{
	struct bio_locate bl;
	struct bioc_createraid create;
	struct stat sb;
	char *dev_list = NULL, *chunk, *p;
	dev_t *dt;
	int ch, i, n = 0, bioh, force = 0;

	while ((ch = getopt(argc, argv, "Cl:")) != -1) {
		switch (ch) {
		case 'C':
			force = 1;
			break;
		case 'l':
			dev_list = optarg;
			break;
		default:
			usage();
		}
	}
	argc -= optind;
	argv += optind;
	if (argc != 1 || dev_list == NULL)
		usage();

	if ((dt = calloc(1, BIOC_CRMAXLEN)) == NULL)
		err(1, "not enough memory for dev_t list");
	if ((dev_list = strdup(dev_list)) == NULL)
		err(1, "strdup");
	for (p = dev_list; (chunk = strsep(&p, ",")) != NULL;) {
		if (*chunk == '\0')
			errx(1, "invalid device list");
		if (stat(chunk, &sb) == -1)
			err(1, "could not stat %s", chunk);
		if (n >= (int)(BIOC_CRMAXLEN / sizeof(dev_t)))
			errx(1, "too many devices on device list");
		for (i = 0; i < n; i++)
			if (dt[i] == sb.st_rdev)
				errx(1, "duplicate device in list");
		dt[n++] = sb.st_rdev;
	}
	if (n < MIN_DISKS)
		errx(1, "not enough disks");

	if ((bioh = open("/dev/bio", O_RDWR)) == -1)
		err(1, "can't open /dev/bio");
	memset(&bl, 0, sizeof(bl));
	bl.bl_name = argv[0];
	if (ioctl(bioh, BIOCLOCATE, &bl) == -1)
		errx(1, "can't locate %s device via /dev/bio", bl.bl_name);

	memset(&create, 0, sizeof(create));
	create.bc_bio.bio_cookie = bl.bl_bio.bio_cookie;
	create.bc_level = LEVEL;
	create.bc_dev_list_len = n * sizeof(dev_t);
	create.bc_dev_list = dt;
	create.bc_flags = BIOC_SCDEVT | (force ? BIOC_SCFORCE : 0);
	create.bc_key_disk = NODEV;
	if (ioctl(bioh, BIOCCREATERAID, &create) == -1)
		err(1, "BIOCCREATERAID");
	bio_status(&create.bc_bio.bio_status);

	free(dt);
	return 0;
}
