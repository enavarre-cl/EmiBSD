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
 * difftest: system call probes for `cargo xtask diff-openbsd`.
 *
 * EmiBSD's own test program, not OpenBSD code. The same static binary runs
 * on EmiBSD and on a real OpenBSD, and the outputs are compared line by
 * line, so everything it prints must be deterministic: return values,
 * errno names, file types and modes, sizes, wait statuses. Inode numbers
 * are printed as labels (#1, #2, ...) in order of first appearance within
 * one run; addresses and times are never printed, except times the program
 * set itself.
 *
 * usage: difftest syscalls [sect]   probes (one section of them) in the
 *                                   current directory
 *        difftest stat path ...     lstat(2) fields of each path
 *        difftest dirents dir       readdir(3) order, types and inodes
 *        difftest truncate path len truncate(2)
 *        difftest utimes path sec   utimes(2), then the times read back
 */

#include <sys/types.h>
#include <sys/event.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/sysctl.h>
#include <sys/time.h>
#include <sys/un.h>
#include <sys/wait.h>

#include <netinet/in.h>

#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static const char *const errnames[] = {
	[0] = "0",
	[EPERM] = "EPERM", [ENOENT] = "ENOENT", [ESRCH] = "ESRCH",
	[EINTR] = "EINTR", [EIO] = "EIO", [ENXIO] = "ENXIO",
	[E2BIG] = "E2BIG", [ENOEXEC] = "ENOEXEC", [EBADF] = "EBADF",
	[ECHILD] = "ECHILD", [EDEADLK] = "EDEADLK", [ENOMEM] = "ENOMEM",
	[EACCES] = "EACCES", [EFAULT] = "EFAULT", [ENOTBLK] = "ENOTBLK",
	[EBUSY] = "EBUSY", [EEXIST] = "EEXIST", [EXDEV] = "EXDEV",
	[ENODEV] = "ENODEV", [ENOTDIR] = "ENOTDIR", [EISDIR] = "EISDIR",
	[EINVAL] = "EINVAL", [ENFILE] = "ENFILE", [EMFILE] = "EMFILE",
	[ENOTTY] = "ENOTTY", [ETXTBSY] = "ETXTBSY", [EFBIG] = "EFBIG",
	[ENOSPC] = "ENOSPC", [ESPIPE] = "ESPIPE", [EROFS] = "EROFS",
	[EMLINK] = "EMLINK", [EPIPE] = "EPIPE", [EDOM] = "EDOM",
	[ERANGE] = "ERANGE", [EAGAIN] = "EAGAIN",
	[EINPROGRESS] = "EINPROGRESS", [EALREADY] = "EALREADY",
	[ENOTSOCK] = "ENOTSOCK", [EDESTADDRREQ] = "EDESTADDRREQ",
	[EMSGSIZE] = "EMSGSIZE", [EPROTOTYPE] = "EPROTOTYPE",
	[ENOPROTOOPT] = "ENOPROTOOPT", [EPROTONOSUPPORT] = "EPROTONOSUPPORT",
	[ESOCKTNOSUPPORT] = "ESOCKTNOSUPPORT", [EOPNOTSUPP] = "EOPNOTSUPP",
	[EPFNOSUPPORT] = "EPFNOSUPPORT", [EAFNOSUPPORT] = "EAFNOSUPPORT",
	[EADDRINUSE] = "EADDRINUSE", [EADDRNOTAVAIL] = "EADDRNOTAVAIL",
	[ENETDOWN] = "ENETDOWN", [ENETUNREACH] = "ENETUNREACH",
	[ENETRESET] = "ENETRESET", [ECONNABORTED] = "ECONNABORTED",
	[ECONNRESET] = "ECONNRESET", [ENOBUFS] = "ENOBUFS",
	[EISCONN] = "EISCONN", [ENOTCONN] = "ENOTCONN",
	[ESHUTDOWN] = "ESHUTDOWN", [ETOOMANYREFS] = "ETOOMANYREFS",
	[ETIMEDOUT] = "ETIMEDOUT", [ECONNREFUSED] = "ECONNREFUSED",
	[ELOOP] = "ELOOP", [ENAMETOOLONG] = "ENAMETOOLONG",
	[EHOSTDOWN] = "EHOSTDOWN", [EHOSTUNREACH] = "EHOSTUNREACH",
	[ENOTEMPTY] = "ENOTEMPTY", [EPROCLIM] = "EPROCLIM",
	[EUSERS] = "EUSERS", [EDQUOT] = "EDQUOT", [ESTALE] = "ESTALE",
	[EREMOTE] = "EREMOTE", [EBADRPC] = "EBADRPC",
	[ERPCMISMATCH] = "ERPCMISMATCH", [EPROGUNAVAIL] = "EPROGUNAVAIL",
	[EPROGMISMATCH] = "EPROGMISMATCH", [EPROCUNAVAIL] = "EPROCUNAVAIL",
	[ENOLCK] = "ENOLCK", [ENOSYS] = "ENOSYS", [EFTYPE] = "EFTYPE",
	[EAUTH] = "EAUTH", [ENEEDAUTH] = "ENEEDAUTH", [EIPSEC] = "EIPSEC",
	[ENOATTR] = "ENOATTR", [EILSEQ] = "EILSEQ", [ENOMEDIUM] = "ENOMEDIUM",
	[EMEDIUMTYPE] = "EMEDIUMTYPE", [EOVERFLOW] = "EOVERFLOW",
	[ECANCELED] = "ECANCELED", [EIDRM] = "EIDRM", [ENOMSG] = "ENOMSG",
	[ENOTSUP] = "ENOTSUP", [EBADMSG] = "EBADMSG",
	[ENOTRECOVERABLE] = "ENOTRECOVERABLE", [EOWNERDEAD] = "EOWNERDEAD",
	[EPROTO] = "EPROTO",
};

static const char *
ename(int e)
{
	static char buf[32];

	if (e >= 0 && (size_t)e < sizeof(errnames) / sizeof(errnames[0]) &&
	    errnames[e] != NULL)
		return errnames[e];
	snprintf(buf, sizeof(buf), "errno %d", e);
	return buf;
}

/* Prints `name: value`, or `name: -1 ENAME` for a failed call. */
static void
report(const char *name, long r)
{
	if (r == -1)
		printf("%s: -1 %s\n", name, ename(errno));
	else
		printf("%s: %ld\n", name, r);
}

#define R(name, expr) do {					\
	errno = 0;						\
	report((name), (long)(expr));				\
} while (0)

/* Like R, for calls whose success value is not deterministic (an fd, an address). */
static void
report_ok(const char *name, int ok)
{
	if (ok)
		printf("%s: ok\n", name);
	else
		printf("%s: -1 %s\n", name, ename(errno));
}

#define OK(name, expr) do {					\
	errno = 0;						\
	report_ok((name), (expr));				\
} while (0)

/* For mmap(2): 0 when it mapped, -1 (errno kept) when it failed. */
#define MAPPED(p) ((p) == MAP_FAILED ? -1 : 0)

static void
section(const char *name)
{
	printf("# %s\n", name);
}

/* Inode labels: #1, #2, ... in order of first appearance. */
static ino_t inos[1024];
static int ninos;

static int
ino_label(ino_t ino)
{
	int i;

	for (i = 0; i < ninos; i++)
		if (inos[i] == ino)
			return i + 1;
	if (ninos < (int)(sizeof(inos) / sizeof(inos[0])))
		inos[ninos++] = ino;
	return ninos;
}

static const char *
ftype(mode_t m)
{
	if (S_ISREG(m))
		return "reg";
	if (S_ISDIR(m))
		return "dir";
	if (S_ISLNK(m))
		return "lnk";
	if (S_ISFIFO(m))
		return "fifo";
	if (S_ISCHR(m))
		return "chr";
	if (S_ISBLK(m))
		return "blk";
	if (S_ISSOCK(m))
		return "sock";
	return "?";
}

static void
print_stat(const char *name, const struct stat *st)
{
	printf("%s: type=%s mode=%04o nlink=%u uid=%u gid=%u size=%lld "
	    "blocks=%lld flags=0x%x ino=#%d\n", name, ftype(st->st_mode),
	    (unsigned)(st->st_mode & 07777), (unsigned)st->st_nlink,
	    (unsigned)st->st_uid, (unsigned)st->st_gid,
	    (long long)st->st_size, (long long)st->st_blocks,
	    (unsigned)st->st_flags, ino_label(st->st_ino));
}

static void
show(const char *path)
{
	struct stat st;

	if (lstat(path, &st) == -1)
		printf("%s: -1 %s\n", path, ename(errno));
	else
		print_stat(path, &st);
}

static void
show_fd(const char *name, int fd)
{
	struct stat st;

	if (fstat(fd, &st) == -1)
		printf("%s: -1 %s\n", name, ename(errno));
	else
		print_stat(name, &st);
}

/* Runs `fn` in a child and prints how it ended. */
static void
in_child(const char *name, void (*fn)(void))
{
	pid_t pid;
	int status;

	fflush(stdout);
	pid = fork();
	if (pid == -1) {
		printf("%s: fork: -1 %s\n", name, ename(errno));
		return;
	}
	if (pid == 0) {
		fn();
		fflush(stdout);
		_exit(0);
	}
	if (waitpid(pid, &status, 0) == -1)
		printf("%s: waitpid: -1 %s\n", name, ename(errno));
	else if (WIFEXITED(status))
		printf("%s: exit %d\n", name, WEXITSTATUS(status));
	else if (WIFSIGNALED(status))
		printf("%s: signal %d\n", name, WTERMSIG(status));
	else
		printf("%s: status 0x%x\n", name, status);
}

static char *
repeat(char c, size_t n)
{
	char *s;

	if ((s = malloc(n + 1)) == NULL) {
		printf("difftest: out of memory\n");
		exit(1);
	}
	memset(s, c, n);
	s[n] = '\0';
	return s;
}

static void
t_files(void)
{
	char buf[64];
	struct iovec iov[2];
	char *longname, *longpath;
	int fd, fd2, fd3;
	size_t i;

	section("files");
	R("open missing", open("missing", O_RDONLY));
	fd = open("f", O_CREAT | O_EXCL | O_RDWR, 0644);
	R("open creat excl (lowest fd)", fd);
	R("open creat excl again", open("f", O_CREAT | O_EXCL | O_RDWR, 0644));
	R("write", write(fd, "hello\n", 6));
	R("lseek cur", lseek(fd, 0, SEEK_CUR));
	R("lseek end", lseek(fd, 0, SEEK_END));
	R("lseek bad whence", lseek(fd, 0, 42));
	R("lseek negative", lseek(fd, -100, SEEK_SET));
	R("pread", pread(fd, buf, sizeof(buf), 1));
	R("pread negative offset", pread(fd, buf, 1, -1));
	R("pwrite past eof", pwrite(fd, "x", 1, 70000));
	show_fd("after hole", fd);
	R("ftruncate shrink", ftruncate(fd, 3));
	R("ftruncate negative", ftruncate(fd, -1));
	show_fd("after ftruncate", fd);
	R("lseek past eof", lseek(fd, 100, SEEK_SET));
	R("read at eof", read(fd, buf, sizeof(buf)));
	iov[0].iov_base = "ab";
	iov[0].iov_len = 2;
	iov[1].iov_base = "cde";
	iov[1].iov_len = 3;
	R("writev", writev(fd, iov, 2));
	R("lseek set", lseek(fd, 100, SEEK_SET));
	memset(buf, 0, sizeof(buf));
	iov[0].iov_base = buf;
	iov[0].iov_len = 1;
	iov[1].iov_base = buf + 10;
	iov[1].iov_len = 10;
	R("readv", readv(fd, iov, 2));
	printf("readv data: %c %.4s\n", buf[0], buf + 10);
	R("writev too many", writev(fd, iov, IOV_MAX + 1));

	fd2 = open("f", O_RDONLY);
	OK("open rdonly", fd2 >= 0);
	R("write on rdonly", write(fd2, "x", 1));
	R("ftruncate on rdonly", ftruncate(fd2, 0));
	fd3 = open("f", O_WRONLY | O_APPEND);
	OK("open append", fd3 >= 0);
	R("lseek append to 0", lseek(fd3, 0, SEEK_SET));
	R("write append", write(fd3, "zz", 2));
	R("append offset", lseek(fd3, 0, SEEK_CUR));
	R("read on wronly", read(fd3, buf, 1));
	R("fcntl getfl append", fcntl(fd3, F_GETFL) & (O_ACCMODE | O_APPEND));
	close(fd3);
	close(fd2);

	R("open dir wronly", open(".", O_WRONLY));
	R("open dir rdwr", open(".", O_RDWR));
	R("open O_DIRECTORY on file", open("f", O_RDONLY | O_DIRECTORY));
	R("open trailing slash on file", open("f/", O_RDONLY));
	R("open through a file", open("f/x", O_RDONLY));
	R("open O_CREAT on dir", open(".", O_CREAT | O_RDONLY, 0644));
	R("open bad accmode", open("f", O_ACCMODE));
	R("open empty path", open("", O_RDONLY));
	longname = repeat('n', NAME_MAX + 1);
	R("open name too long", open(longname, O_RDONLY | O_CREAT, 0644));
	longname[NAME_MAX] = '\0';
	fd2 = open(longname, O_RDONLY | O_CREAT, 0644);
	OK("open name NAME_MAX", fd2 >= 0);
	close(fd2);
	R("unlink name NAME_MAX", unlink(longname));
	free(longname);
	longpath = repeat('p', PATH_MAX + 10);
	for (i = 1; i < PATH_MAX + 10; i += 2)
		longpath[i] = '/';
	R("open path too long", open(longpath, O_RDONLY));
	free(longpath);

	R("close -1", close(-1));
	R("close unopened", close(200));
	R("dup (lowest fd)", dup(fd));
	R("dup2 same", dup2(fd, fd));
	R("dup2 huge", dup2(fd, 1000000));
	R("dup2 to 40", dup2(fd, 40));
	R("fcntl getfd", fcntl(fd, F_GETFD));
	R("fcntl setfd", fcntl(fd, F_SETFD, FD_CLOEXEC));
	R("fcntl getfd after", fcntl(fd, F_GETFD));
	R("fcntl getfl", fcntl(fd, F_GETFL) & (O_ACCMODE | O_APPEND | O_NONBLOCK));
	R("fcntl setfl nonblock", fcntl(fd, F_SETFL, O_NONBLOCK));
	R("fcntl getfl after", fcntl(fd, F_GETFL) & (O_ACCMODE | O_APPEND | O_NONBLOCK));
	R("fcntl bad cmd", fcntl(fd, 9999));
	R("fcntl bad fd", fcntl(500, F_GETFD));
	R("fcntl dupfd 20", fcntl(fd, F_DUPFD, 20));
	R("fcntl dupfd 20 again", fcntl(fd, F_DUPFD, 20));
	R("fcntl dupfd_cloexec 30", fcntl(fd, F_DUPFD_CLOEXEC, 30));
	R("fcntl getfd of 30", fcntl(30, F_GETFD));
	R("fcntl getfd of 20", fcntl(20, F_GETFD));
	R("fcntl dupfd negative", fcntl(fd, F_DUPFD, -1));
	for (i = 3; i < 64; i++)
		close(i);
}

static void
t_dirs(void)
{
	char buf[PATH_MAX];
	int fd;

	section("directories");
	R("umask", umask(022));
	R("umask again", umask(022));
	R("mkdir d", mkdir("d", 0777));
	show("d");
	R("mkdir d again", mkdir("d", 0777));
	R("mkdir trailing slash", mkdir("e/", 0700));
	show("e");
	R("mkdir under a file", mkdir("f/x", 0777));
	R("mkdir missing parent", mkdir("nope/x", 0777));
	R("mkdir d/sub", mkdir("d/sub", 0755));
	show("d");
	R("rmdir non-empty", rmdir("d"));
	R("rmdir a file", rmdir("f"));
	R("rmdir dot", rmdir("."));
	R("rmdir dotdot", rmdir("d/sub/.."));
	R("rmdir missing", rmdir("missing"));
	R("unlink a dir", unlink("d/sub"));
	R("unlink missing", unlink("missing"));
	R("chdir a file", chdir("f"));
	R("chdir missing", chdir("missing"));
	R("chdir d", chdir("d"));
	OK("getcwd", getcwd(buf, sizeof(buf)) != NULL);
	printf("cwd basename: %s\n", strrchr(buf, '/') ? strrchr(buf, '/') + 1 : buf);
	OK("getcwd 1 byte", getcwd(buf, 1) != NULL);
	OK("getcwd 0 bytes", getcwd(buf, 0) != NULL);
	R("chdir ..", chdir(".."));
	fd = open("d", O_RDONLY | O_DIRECTORY);
	OK("open O_DIRECTORY", fd >= 0);
	R("fchdir", fchdir(fd));
	R("chdir back", chdir(".."));
	R("fchdir on a tty", fchdir(0));
	close(fd);
	R("rmdir d/sub", rmdir("d/sub"));
	R("rmdir d", rmdir("d"));
	R("rmdir e", rmdir("e"));
}

static void
t_links(void)
{
	struct stat st;
	char buf[PATH_MAX + 2];
	char *longtarget;
	int fd;

	section("links and renames");
	R("link f g", link("f", "g"));
	show("f");
	show("g");
	R("link f g again", link("f", "g"));
	R("link missing", link("missing", "x"));
	R("mkdir dl", mkdir("dl", 0755));
	R("link a dir", link("dl", "dlink"));
	R("link into missing dir", link("f", "nope/x"));
	R("symlink f s", symlink("f", "s"));
	R("symlink again", symlink("f", "s"));
	show("s");
	memset(buf, 0, sizeof(buf));
	R("readlink s", readlink("s", buf, sizeof(buf)));
	printf("readlink s data: %s\n", buf);
	R("readlink short buffer", readlink("s", buf, 0));
	R("readlink a file", readlink("f", buf, sizeof(buf)));
	R("readlink missing", readlink("missing", buf, sizeof(buf)));
	R("symlink loop1", symlink("loop2", "loop1"));
	R("symlink loop2", symlink("loop1", "loop2"));
	R("open loop", open("loop1", O_RDONLY));
	R("open O_NOFOLLOW on symlink", open("s", O_RDONLY | O_NOFOLLOW));
	R("symlink dangling", symlink("target-missing", "dang"));
	R("open dangling", open("dang", O_RDONLY));
	fd = open("dang", O_RDWR | O_CREAT, 0600);
	OK("open dangling O_CREAT", fd >= 0);
	close(fd);
	show("target-missing");
	R("open dangling O_CREAT|O_EXCL", open("dang", O_RDWR | O_CREAT | O_EXCL, 0600));
	longtarget = repeat('t', PATH_MAX);
	R("symlink target PATH_MAX", symlink(longtarget, "longlink"));
	free(longtarget);
	R("symlink empty target", symlink("", "emptylink"));
	R("open empty symlink", open("emptylink", O_RDONLY));

	R("rename f h", rename("f", "h"));
	R("rename h h", rename("h", "h"));
	R("rename missing", rename("missing", "x"));
	R("rename h g (same inode)", rename("h", "g"));
	show("h");
	show("g");
	R("mkdir r1", mkdir("r1", 0755));
	R("mkdir r1/in", mkdir("r1/in", 0755));
	R("mkdir r2", mkdir("r2", 0755));
	R("rename file over dir", rename("g", "r2"));
	R("rename dir over file", rename("r2", "g"));
	R("rename dir into itself", rename("r1", "r1/in/x"));
	R("rename dir over non-empty dir", rename("r2", "r1"));
	R("rename dir over empty dir", rename("r1", "r2"));
	show("r2");
	R("rename dot", rename(".", "x"));
	R("rename to dotdot", rename("r2", "r2/in/.."));
	R("rename trailing slash file", rename("g/", "g2"));
	R("rename dir trailing slash", rename("r2/", "r3/"));
	R("rename symlink", rename("s", "s2"));
	show("s2");

	section("at-calls");
	fd = open(".", O_RDONLY | O_DIRECTORY);
	if (fstatat(fd, "s2", &st, AT_SYMLINK_NOFOLLOW) == 0)
		print_stat("fstatat s2 nofollow", &st);
	if (fstatat(fd, "s2", &st, 0) == -1)
		printf("fstatat s2 follow: -1 %s\n", ename(errno));
	else
		print_stat("fstatat s2 follow", &st);
	close(fd);
	R("fstatat bad flag", fstatat(AT_FDCWD, "g", &st, 0x4000));
	R("openat closed fd", openat(77, "g", O_RDONLY));
	R("openat tty as dirfd", openat(0, "g", O_RDONLY));
	fd = openat(77, "/", O_RDONLY);
	OK("openat absolute ignores fd", fd >= 0);
	close(fd);
	R("unlinkat removedir on file", unlinkat(AT_FDCWD, "g", AT_REMOVEDIR));
	R("unlinkat bad flag", unlinkat(AT_FDCWD, "g", 0x4000));
	R("linkat follow", linkat(AT_FDCWD, "s2", AT_FDCWD, "viafollow", AT_SYMLINK_FOLLOW));
	R("linkat nofollow", linkat(AT_FDCWD, "s2", AT_FDCWD, "vianofollow", 0));
	show("viafollow");
	show("vianofollow");
	R("faccessat F_OK", faccessat(AT_FDCWD, "g", F_OK, 0));
	R("faccessat bad mode", faccessat(AT_FDCWD, "g", 0x100, 0));
	R("mkdirat", mkdirat(AT_FDCWD, "ma", 0700));
	R("renameat", renameat(AT_FDCWD, "ma", AT_FDCWD, "mb"));
	R("unlinkat removedir", unlinkat(AT_FDCWD, "mb", AT_REMOVEDIR));
	R("mkfifo p", mkfifo("p", 0644));
	show("p");
	R("open fifo wronly nonblock no reader", open("p", O_WRONLY | O_NONBLOCK));
	fd = open("p", O_RDONLY | O_NONBLOCK);
	OK("open fifo rdonly nonblock", fd >= 0);
	close(fd);
	R("mkfifo existing", mkfifo("p", 0644));
}

static void
print_times(const char *path)
{
	struct stat st;

	if (stat(path, &st) == -1)
		printf("%s times: -1 %s\n", path, ename(errno));
	else
		printf("%s times: atime %lld.%09ld mtime %lld.%09ld\n", path,
		    (long long)st.st_atim.tv_sec, st.st_atim.tv_nsec,
		    (long long)st.st_mtim.tv_sec, st.st_mtim.tv_nsec);
}

static void
t_perms(void)
{
	struct timeval tv[2];
	struct timespec ts[2];
	int fd;

	section("permissions and times");
	fd = open("pm", O_CREAT | O_RDWR, 0666);
	show("pm");
	R("chmod 0", chmod("pm", 0));
	show("pm");
	R("access R_OK as root", access("pm", R_OK));
	R("access X_OK no x bit", access("pm", X_OK));
	R("chmod 0755", chmod("pm", 0755));
	R("access X_OK", access("pm", X_OK));
	R("access bad mode", access("pm", 0x100));
	R("chmod sticky+setgid", chmod("pm", 03755));
	show("pm");
	R("fchmod 0600", fchmod(fd, 0600));
	R("chown 1000 1000", chown("pm", 1000, 1000));
	show("pm");
	R("fchown -1 -1", fchown(fd, -1, -1));
	R("chmod 04755 after chown", chmod("pm", 04755));
	show("pm");
	R("lchown symlink", lchown("s2", 5, 5));
	show("s2");
	R("chmod missing", chmod("missing", 0644));
	R("chflags nodump", chflags("pm", UF_NODUMP));
	show("pm");
	R("chflags 0", chflags("pm", 0));

	R("truncate 100", truncate("pm", 100));
	show("pm");
	R("truncate negative", truncate("pm", -1));
	R("truncate a dir", truncate(".", 0));
	R("truncate missing", truncate("missing", 0));

	tv[0].tv_sec = 1000000000;
	tv[0].tv_usec = 0;
	tv[1].tv_sec = 1000000001;
	tv[1].tv_usec = 500000;
	R("utimes", utimes("pm", tv));
	print_times("pm");
	ts[0].tv_sec = 1200000000;
	ts[0].tv_nsec = 123456789;
	ts[1].tv_sec = 0;
	ts[1].tv_nsec = UTIME_OMIT;
	R("futimens omit mtime", futimens(fd, ts));
	print_times("pm");
	ts[0].tv_nsec = 1000000000;
	R("futimens bad nsec", futimens(fd, ts));
	R("utimensat bad flag", utimensat(AT_FDCWD, "pm", NULL, 0x4000));
	close(fd);
}

static void
child_setuid(void)
{
	int fd;

	if (setuid(1000) == -1) {
		printf("setuid: -1 %s\n", ename(errno));
		return;
	}
	R("child setuid back to 0", setuid(0));
	R("child open root's 0600 file", open("secret", O_RDONLY));
	R("child chmod not owner", chmod("secret", 0777));
	R("child unlink in root's 0755 dir", unlink("secret"));
	R("child mkdir in root's dir", mkdir("x", 0755));
	fd = open("pm", O_RDONLY);
	OK("child open its own file", fd >= 0);
	R("child chown to root", chown("pm", 0, 0));
	R("child kill pid 1", kill(1, 0));
}

static void
child_term(void)
{
	raise(SIGTERM);
}

static void
child_exit7(void)
{
	fflush(stdout);
	_exit(7);
}

static void
child_pledge(void)
{
	R("child pledge stdio", pledge("stdio", NULL));
	fflush(stdout);
	open("pm", O_RDONLY);
	printf("child survived open under pledge\n");
}

static void
child_pledge_bad(void)
{
	R("child pledge bogus promise", pledge("stdio bogus", NULL));
}

static void
child_unveil(void)
{
	int fd;

	R("child unveil . r", unveil(".", "r"));
	R("child unveil lock", unveil(NULL, NULL));
	R("child open outside unveil", open("/etc/passwd", O_RDONLY));
	fd = open("pm", O_RDONLY);
	OK("child open inside unveil", fd >= 0);
	R("child open inside for write", open("pm", O_WRONLY));
	R("child unveil after lock", unveil("/", "r"));
}

static void
child_segv(void)
{
	volatile char *p;

	p = mmap(NULL, 4096, PROT_READ, MAP_ANON | MAP_PRIVATE, -1, 0);
	if (p == MAP_FAILED)
		return;
	p[0] = 1;
}

static void
child_sigbus(void)
{
	volatile char *p;
	int fd;

	fd = open("small", O_CREAT | O_RDWR | O_TRUNC, 0644);
	if (write(fd, "x", 1) != 1)
		return;
	p = mmap(NULL, 3 * 4096, PROT_READ, MAP_SHARED, fd, 0);
	if (p == MAP_FAILED) {
		printf("mmap small: -1 %s\n", ename(errno));
		return;
	}
	printf("byte 0: %c\n", p[0]);
	fflush(stdout);
	printf("past eof page byte: %d\n", p[2 * 4096]);
}

static volatile sig_atomic_t got_usr1;

static void
on_usr1(int sig)
{
	(void)sig;
	got_usr1++;
}

static void
write_file(const char *path, mode_t mode, const char *text)
{
	int fd;

	if ((fd = open(path, O_CREAT | O_WRONLY | O_TRUNC, mode)) == -1)
		return;
	if (write(fd, text, strlen(text)) == -1)
		printf("write %s: -1 %s\n", path, ename(errno));
	close(fd);
}

static void
t_procs(void)
{
	struct sigaction sa;
	sigset_t set, old, pend;
	struct timespec ts;
	struct rlimit rl;
	struct itimerval itv;
	char *argv[] = { "x", NULL };
	char *envp[] = { NULL };
	char e[300];
	pid_t pid;
	int status, p[2];

	section("processes and signals");
	write_file("secret", 0600, "");
	chown("pm", 1000, 1000);
	in_child("setuid child", child_setuid);
	in_child("raise SIGTERM", child_term);
	in_child("exit 7", child_exit7);
	in_child("pledge stdio then open", child_pledge);
	in_child("pledge bogus", child_pledge_bad);
	in_child("unveil", child_unveil);
	in_child("write to a PROT_READ page", child_segv);
	in_child("read past eof of a mapping", child_sigbus);
	R("waitpid no children", waitpid(-1, &status, 0));
	R("waitpid WNOHANG no children", waitpid(-1, &status, WNOHANG));
	if (pipe(p) == 0) {
		fflush(stdout);
		if ((pid = fork()) == 0) {
			char c;

			close(p[1]);
			(void)read(p[0], &c, 1);
			_exit(3);
		}
		close(p[0]);
		R("waitpid WNOHANG running child", waitpid(pid, &status, WNOHANG));
		close(p[1]);
		R("waitpid child", waitpid(pid, &status, 0) == pid ? 0 : -1);
		printf("child status: exit %d\n", WEXITSTATUS(status));
	}
	R("waitpid bad options", waitpid(-1, &status, 0x10000));
	R("kill unused pid", kill(99998, 0));
	R("kill bad signal", kill(getpid(), 999));
	R("kill self 0", kill(getpid(), 0));
	memset(&sa, 0, sizeof(sa));
	sa.sa_handler = on_usr1;
	R("sigaction SIGKILL", sigaction(SIGKILL, &sa, NULL));
	R("sigaction SIGSTOP", sigaction(SIGSTOP, &sa, NULL));
	R("sigaction 0", sigaction(0, &sa, NULL));
	R("sigaction 999", sigaction(999, &sa, NULL));
	R("sigaction SIGUSR1", sigaction(SIGUSR1, &sa, NULL));
	sigemptyset(&set);
	sigaddset(&set, SIGUSR1);
	R("sigprocmask block", sigprocmask(SIG_BLOCK, &set, &old));
	R("raise SIGUSR1", raise(SIGUSR1));
	printf("handled while blocked: %d\n", (int)got_usr1);
	sigpending(&pend);
	printf("pending: %d\n", sigismember(&pend, SIGUSR1));
	R("sigprocmask unblock", sigprocmask(SIG_SETMASK, &old, NULL));
	printf("handled after unblock: %d\n", (int)got_usr1);
	R("sigprocmask bad how", sigprocmask(99, &set, NULL));

	R("execve missing", execve("/nonexistent/x", argv, envp));
	write_file("noexec", 0644, "#!/bin/sh\n");
	R("execve without x bit", execve("noexec", argv, envp));
	R("execve a dir", execve(".", argv, envp));
	write_file("garbage", 0755, "\177ELF garbage garbage garbage");
	R("execve garbage", execve("garbage", argv, envp));
	write_file("badinterp", 0755, "#!/nonexistent/sh\n");
	R("execve missing interpreter", execve("badinterp", argv, envp));

	ts.tv_sec = -1;
	ts.tv_nsec = 0;
	R("nanosleep negative", nanosleep(&ts, NULL));
	ts.tv_sec = 0;
	ts.tv_nsec = 1000000000;
	R("nanosleep bad nsec", nanosleep(&ts, NULL));
	R("clock_gettime bad clock", clock_gettime(999, &ts));
	R("clock_gettime monotonic", clock_gettime(CLOCK_MONOTONIC, &ts));
	memset(&itv, 0, sizeof(itv));
	R("setitimer bad which", setitimer(99, &itv, NULL));
	R("getrlimit bad", getrlimit(999, &rl));
	if (getrlimit(RLIMIT_NOFILE, &rl) == 0) {
		rl.rlim_cur = rl.rlim_max + 1;
		R("setrlimit soft over hard", setrlimit(RLIMIT_NOFILE, &rl));
	}
	R("issetugid", issetugid());
	R("getentropy 256 bytes", getentropy(e, 256));
	R("getentropy 257 bytes", getentropy(e, 257));
}

static void
t_ipc(void)
{
	struct sockaddr_un sun;
	struct kevent kev, out;
	struct timespec zero = { 0, 0 };
	struct pollfd pfd;
	char buf[16];
	int p[2], sv[2], s, s2, kq, n, type;
	socklen_t len;

	section("pipes, sockets, kqueue");
	signal(SIGPIPE, SIG_IGN);
	R("pipe", pipe(p));
	R("write pipe", write(p[1], "12345", 5));
	R("FIONREAD", ioctl(p[0], FIONREAD, &n) == 0 ? n : -1);
	show_fd("pipe fstat", p[0]);
	R("lseek pipe", lseek(p[0], 0, SEEK_SET));
	pfd.fd = p[0];
	pfd.events = POLLIN | POLLOUT;
	R("poll pipe", poll(&pfd, 1, 0));
	printf("poll revents: 0x%x\n", pfd.revents);
	R("read pipe", read(p[0], buf, sizeof(buf)));
	R("set nonblock", fcntl(p[0], F_SETFL, O_NONBLOCK));
	R("read empty nonblock", read(p[0], buf, sizeof(buf)));
	R("write to read end", write(p[0], "x", 1));
	close(p[0]);
	R("write no reader", write(p[1], "x", 1));
	close(p[1]);
	R("pipe2 cloexec", pipe2(p, O_CLOEXEC));
	R("pipe2 getfd", fcntl(p[0], F_GETFD));
	close(p[0]);
	close(p[1]);
	R("pipe2 bad flag", pipe2(p, 0x40000000));

	R("socketpair", socketpair(AF_UNIX, SOCK_STREAM, 0, sv));
	R("send", write(sv[0], "hi", 2));
	R("recv", read(sv[1], buf, sizeof(buf)));
	R("shutdown wr", shutdown(sv[0], SHUT_WR));
	R("read after peer shutdown", read(sv[1], buf, sizeof(buf)));
	R("write after shutdown", write(sv[0], "x", 1));
	R("shutdown bad how", shutdown(sv[0], 7));
	len = sizeof(type);
	R("getsockopt SO_TYPE",
	    getsockopt(sv[1], SOL_SOCKET, SO_TYPE, &type, &len) == 0 ? type : -1);
	R("getsockopt bad level", getsockopt(sv[1], 9999, SO_TYPE, &type, &len));
	R("getsockopt on a tty", getsockopt(0, SOL_SOCKET, SO_TYPE, &type, &len));
	close(sv[0]);
	close(sv[1]);
	R("socket bad domain", socket(9999, SOCK_STREAM, 0));
	R("socket bad type", socket(AF_INET, 99, 0));
	R("socket stream udp", socket(AF_INET, SOCK_STREAM, IPPROTO_UDP));
	R("socket unix proto 5", socket(AF_UNIX, SOCK_STREAM, 5));
	s = socket(AF_UNIX, SOCK_STREAM, 0);
	memset(&sun, 0, sizeof(sun));
	sun.sun_family = AF_UNIX;
	strlcpy(sun.sun_path, "nosock", sizeof(sun.sun_path));
	R("connect missing", connect(s, (struct sockaddr *)&sun, sizeof(sun)));
	strlcpy(sun.sun_path, "g", sizeof(sun.sun_path));
	R("connect to a file", connect(s, (struct sockaddr *)&sun, sizeof(sun)));
	strlcpy(sun.sun_path, "sock", sizeof(sun.sun_path));
	R("bind", bind(s, (struct sockaddr *)&sun, sizeof(sun)));
	show("sock");
	s2 = socket(AF_UNIX, SOCK_STREAM, 0);
	R("bind in use", bind(s2, (struct sockaddr *)&sun, sizeof(sun)));
	R("connect not listening", connect(s2, (struct sockaddr *)&sun, sizeof(sun)));
	R("listen", listen(s, 5));
	R("connect", connect(s2, (struct sockaddr *)&sun, sizeof(sun)));
	R("connect again", connect(s2, (struct sockaddr *)&sun, sizeof(sun)));
	close(s2);
	close(s);
	s = socket(AF_UNIX, SOCK_DGRAM, 0);
	R("listen on dgram", listen(s, 5));
	R("accept on dgram", accept(s, NULL, NULL));
	close(s);
	R("accept on a tty", accept(0, NULL, NULL));

	kq = kqueue();
	OK("kqueue", kq >= 0);
	R("pipe for kqueue", pipe(p));
	R("write 3", write(p[1], "abc", 3));
	EV_SET(&kev, p[0], EVFILT_READ, EV_ADD, 0, 0, NULL);
	R("kevent add", kevent(kq, &kev, 1, NULL, 0, NULL));
	R("kevent poll", kevent(kq, NULL, 0, &out, 1, &zero));
	printf("kevent data: %lld filter %d\n", (long long)out.data, out.filter);
	EV_SET(&kev, 300, EVFILT_READ, EV_ADD, 0, 0, NULL);
	R("kevent bad fd", kevent(kq, &kev, 1, NULL, 0, NULL));
	memset(&out, 0, sizeof(out));
	R("kevent bad fd with eventlist", kevent(kq, &kev, 1, &out, 1, &zero));
	printf("kevent error: flags 0x%x data %s\n", out.flags & EV_ERROR,
	    ename((int)out.data));
	EV_SET(&kev, p[0], 999, EV_ADD, 0, 0, NULL);
	R("kevent bad filter", kevent(kq, &kev, 1, NULL, 0, NULL));
	EV_SET(&kev, p[0], EVFILT_READ, EV_DELETE, 0, 0, NULL);
	R("kevent delete", kevent(kq, &kev, 1, NULL, 0, NULL));
	R("kevent delete again", kevent(kq, &kev, 1, NULL, 0, NULL));
	close(p[0]);
	close(p[1]);
	close(kq);
}

static void
t_memory(void)
{
	char *p;
	int fd;

	section("memory");
	R("mmap len 0", MAPPED(mmap(NULL, 0, PROT_READ, MAP_ANON | MAP_PRIVATE, -1, 0)));
	p = mmap(NULL, 4 * 4096, PROT_READ | PROT_WRITE, MAP_ANON | MAP_PRIVATE, -1, 0);
	OK("mmap anon", p != MAP_FAILED);
	if (p == MAP_FAILED)
		return;
	p[4096] = 'z';
	R("mprotect read", mprotect(p, 4096, PROT_READ));
	R("mprotect misaligned", mprotect(p + 1, 4096, PROT_READ));
	R("munmap misaligned", munmap(p + 1, 4096));
	R("madvise bad", madvise(p, 4096, 999));
	R("minherit bad", minherit(p, 4096, 999));
	R("munmap", munmap(p, 4 * 4096));
	R("mmap bad fd", MAPPED(mmap(NULL, 4096, PROT_READ, MAP_PRIVATE, 77, 0)));
	R("mmap shared+private", MAPPED(mmap(NULL, 4096, PROT_READ,
	    MAP_SHARED | MAP_PRIVATE | MAP_ANON, -1, 0)));
	R("mmap write+exec", MAPPED(mmap(NULL, 4096, PROT_WRITE | PROT_EXEC,
	    MAP_ANON | MAP_PRIVATE, -1, 0)));
	fd = open("pm", O_RDONLY);
	R("mmap shared write on rdonly fd",
	    MAPPED(mmap(NULL, 4096, PROT_WRITE, MAP_SHARED, fd, 0)));
	R("mmap misaligned offset",
	    MAPPED(mmap(NULL, 4096, PROT_READ, MAP_PRIVATE, fd, 1)));
	close(fd);
	fd = open(".", O_RDONLY);
	R("mmap a dir", MAPPED(mmap(NULL, 4096, PROT_READ, MAP_PRIVATE, fd, 0)));
	close(fd);
}

static void
t_sysctl(void)
{
	int mib[2], v;
	char s[64];
	size_t len;

	section("sysctl");
	mib[0] = CTL_HW;
	mib[1] = HW_PAGESIZE;
	len = sizeof(v);
	R("hw.pagesize", sysctl(mib, 2, &v, &len, NULL, 0) == 0 ? v : -1);
	mib[0] = CTL_KERN;
	mib[1] = KERN_ARGMAX;
	len = sizeof(v);
	R("kern.argmax", sysctl(mib, 2, &v, &len, NULL, 0) == 0 ? v : -1);
	mib[1] = KERN_MAXPARTITIONS;
	len = sizeof(v);
	R("kern.maxpartitions", sysctl(mib, 2, &v, &len, NULL, 0) == 0 ? v : -1);
	mib[1] = 99999;
	R("kern.99999", sysctl(mib, 2, &v, &len, NULL, 0));
	R("empty name", sysctl(mib, 0, &v, &len, NULL, 0));
	mib[1] = KERN_OSTYPE;
	len = 2;
	R("kern.ostype short buffer", sysctl(mib, 2, s, &len, NULL, 0));
	R("set kern.ostype", sysctl(mib, 2, NULL, NULL, "x", 1));
}

/* The system's name: the one line EmiBSD's branding changes (kept apart). */
static void
t_ostype(void)
{
	int mib[2];
	char s[64];
	size_t len;

	section("ostype");
	mib[0] = CTL_KERN;
	mib[1] = KERN_OSTYPE;
	len = sizeof(s);
	R("kern.ostype", sysctl(mib, 2, s, &len, NULL, 0));
	printf("kern.ostype value: %s\n", s);
}

static const struct {
	const char *name;
	void (*fn)(void);
} sections[] = {
	{ "files", t_files },
	{ "dirs", t_dirs },
	{ "links", t_links },
	{ "perms", t_perms },
	{ "procs", t_procs },
	{ "ipc", t_ipc },
	{ "memory", t_memory },
	{ "sysctl", t_sysctl },
	{ "ostype", t_ostype },
};

/*
 * The probes of section `name` (all of them, in order, for NULL), in the
 * current directory, which must be empty at the start: each section uses
 * the files the ones before it left.
 */
static void
syscalls(const char *name)
{
	size_t i;
	int found = 0;

	for (i = 0; i < sizeof(sections) / sizeof(sections[0]); i++) {
		if (name == NULL || strcmp(name, sections[i].name) == 0) {
			sections[i].fn();
			found = 1;
		}
	}
	if (!found) {
		printf("difftest: no section %s\n", name);
		exit(2);
	}
}

static void
dirents(const char *path)
{
	struct dirent *d;
	DIR *dir;

	if ((dir = opendir(path)) == NULL) {
		printf("%s: -1 %s\n", path, ename(errno));
		return;
	}
	while ((d = readdir(dir)) != NULL)
		printf("%s type=%d ino=#%d\n", d->d_name, d->d_type,
		    ino_label(d->d_fileno));
	closedir(dir);
}

static void
usage(void)
{
	fprintf(stderr, "usage: difftest syscalls [section] | stat path ... | dirents dir |"
	    " truncate path len | utimes path sec\n");
	exit(2);
}

int
main(int argc, char *argv[])
{
	struct timeval tv[2];
	struct stat st;
	int i;

	setvbuf(stdout, NULL, _IOLBF, 0);
	if (argc < 2)
		usage();
	if (strcmp(argv[1], "syscalls") == 0 && argc <= 3)
		syscalls(argc == 3 ? argv[2] : NULL);
	else if (strcmp(argv[1], "stat") == 0 && argc > 2) {
		for (i = 2; i < argc; i++)
			show(argv[i]);
	} else if (strcmp(argv[1], "dirents") == 0 && argc == 3)
		dirents(argv[2]);
	else if (strcmp(argv[1], "truncate") == 0 && argc == 4)
		R(argv[2], truncate(argv[2], strtoll(argv[3], NULL, 10)));
	else if (strcmp(argv[1], "utimes") == 0 && argc == 4) {
		tv[0].tv_sec = tv[1].tv_sec = strtoll(argv[3], NULL, 10);
		tv[0].tv_usec = tv[1].tv_usec = 0;
		R(argv[2], utimes(argv[2], tv));
		if (stat(argv[2], &st) == 0)
			printf("%s: atime %lld mtime %lld\n", argv[2],
			    (long long)st.st_atime, (long long)st.st_mtime);
	} else
		usage();
	return 0;
}
