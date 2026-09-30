#include <errno.h>
#include <stdio.h>

#ifdef __APPLE__
#include <sys/attr.h>
#else
#include <fcntl.h>
#include <linux/fs.h>
#include <sys/syscall.h>
#include <unistd.h>
#endif

int main(int argc, char **argv) {
    if (argc != 3) {
        fputs("atomic exchange requires two paths\n", stderr);
        return 2;
    }
#ifdef __APPLE__
    int result = renamex_np(argv[1], argv[2], RENAME_SWAP);
#else
    int result = syscall(SYS_renameat2, AT_FDCWD, argv[1], AT_FDCWD, argv[2], RENAME_EXCHANGE);
#endif
    if (result != 0) {
        perror("atomic exchange");
        return 1;
    }
    return 0;
}
