/*
 * Standalone, unprivileged macOS feasibility check. Build and run with:
 *   cc -Wall -Wextra -Werror -o /tmp/prove_macos_fd_listener \
 *     runtime/crates/nixfied-runtime/tests/fixtures/prove_macos_fd_listener.c
 *   /tmp/prove_macos_fd_listener
 *
 * This deliberately uses only the managed child's FD information. It never
 * reads a host PCB list or other processes' socket FDs.
 */
#include <arpa/inet.h>
#include <errno.h>
#include <libproc.h>
#include <netinet/in.h>
#include <stdbool.h>
#include <stdint.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/proc_info.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <unistd.h>

struct ports {
    uint16_t ipv4;
    uint16_t ipv6;
    uint16_t ipv6_dual;
    uint16_t wildcard;
    uint16_t bound_only;
    uint16_t group_member;
    pid_t group_member_pid;
};

struct member_report {
    pid_t pid;
    uint16_t port;
};

static pid_t orphan_to_clean;

static void clean_orphan(void) {
    if (orphan_to_clean > 0) kill(orphan_to_clean, SIGTERM);
}

struct witness {
    uint64_t socket_id;
    uint64_t generation;
    int fd;
};

static void fail(const char *message) {
    perror(message);
    exit(1);
}

static void transfer(int fd, void *buffer, size_t size, bool writing) {
    size_t done = 0;
    while (done < size) {
        ssize_t n = writing ? write(fd, (char *)buffer + done, size - done)
                            : read(fd, (char *)buffer + done, size - done);
        if (n <= 0) fail(writing ? "write pipe" : "read pipe");
        done += (size_t)n;
    }
}

static int listen_on(int family, bool wildcard, bool ipv6_only, uint16_t port,
                     uint16_t *selected) {
    int fd = socket(family, SOCK_STREAM, IPPROTO_TCP);
    if (fd < 0) fail("socket");
    int one = 1;
    if (setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &one, sizeof(one)) < 0)
        fail("SO_REUSEADDR");
    one = ipv6_only ? 1 : 0;
    if (family == AF_INET6 &&
        setsockopt(fd, IPPROTO_IPV6, IPV6_V6ONLY, &one, sizeof(one)) < 0)
        fail("IPV6_V6ONLY");
    if (family == AF_INET) {
        struct sockaddr_in address = {.sin_family = AF_INET, .sin_port = htons(port)};
        address.sin_addr.s_addr = htonl(wildcard ? INADDR_ANY : INADDR_LOOPBACK);
        if (bind(fd, (struct sockaddr *)&address, sizeof(address)) < 0) fail("bind IPv4");
        socklen_t length = sizeof(address);
        if (getsockname(fd, (struct sockaddr *)&address, &length) < 0) fail("getsockname IPv4");
        *selected = ntohs(address.sin_port);
    } else {
        struct sockaddr_in6 address = {.sin6_family = AF_INET6, .sin6_port = htons(port),
                                        .sin6_addr = IN6ADDR_LOOPBACK_INIT};
        if (bind(fd, (struct sockaddr *)&address, sizeof(address)) < 0) fail("bind IPv6");
        socklen_t length = sizeof(address);
        if (getsockname(fd, (struct sockaddr *)&address, &length) < 0) fail("getsockname IPv6");
        *selected = ntohs(address.sin6_port);
    }
    if (listen(fd, 4) < 0) fail("listen");
    return fd;
}

static int bind_without_listen(uint16_t *selected) {
    int fd = socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
    if (fd < 0) fail("bound-only socket");
    struct sockaddr_in address = {.sin_family = AF_INET, .sin_port = 0};
    address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    if (bind(fd, (struct sockaddr *)&address, sizeof(address)) < 0) fail("bound-only bind");
    socklen_t length = sizeof(address);
    if (getsockname(fd, (struct sockaddr *)&address, &length) < 0)
        fail("bound-only getsockname");
    *selected = ntohs(address.sin_port);
    return fd;
}

static bool exact(const struct socket_fdinfo *info, int family, uint16_t port, bool wildcard) {
    const struct socket_info *socket = &info->psi;
    const struct tcp_sockinfo *tcp = &socket->soi_proto.pri_tcp;
    const struct in_sockinfo *in = &tcp->tcpsi_ini;
    if (socket->soi_kind != SOCKINFO_TCP || socket->soi_type != SOCK_STREAM ||
        socket->soi_family != family || tcp->tcpsi_state != TSI_S_LISTEN ||
        ntohs((uint16_t)in->insi_lport) != port)
        return false;
    if (family == AF_INET) {
        if (!(in->insi_vflag & INI_IPV4)) return false;
        uint32_t wanted = htonl(wildcard ? INADDR_ANY : INADDR_LOOPBACK);
        return in->insi_laddr.ina_46.i46a_addr4.s_addr == wanted;
    }
    if (!(in->insi_vflag & INI_IPV6)) return false;
    return memcmp(&in->insi_laddr.ina_6, &in6addr_loopback, sizeof(in6addr_loopback)) == 0;
}

static struct witness observe(pid_t pid, int family, uint16_t port, bool wildcard) {
    int required = proc_pidinfo(pid, PROC_PIDLISTFDS, 0, NULL, 0);
    if (required <= 0) fail("proc_pidinfo size");
    size_t capacity = (size_t)required + 32 * sizeof(struct proc_fdinfo);
    for (int attempt = 0; attempt < 4; ++attempt) {
        struct proc_fdinfo *list = calloc(1, capacity);
        if (!list) fail("calloc");
        int bytes = proc_pidinfo(pid, PROC_PIDLISTFDS, 0, list, (int)capacity);
        if (bytes <= 0) fail("proc_pidinfo list");
        if ((size_t)bytes >= capacity) {
            free(list);
            capacity *= 2;
            continue;
        }
        if (bytes % (int)sizeof(struct proc_fdinfo)) {
            fprintf(stderr, "malformed FD list: %d bytes\n", bytes);
            exit(1);
        }
        for (int i = 0; i < bytes / (int)sizeof(struct proc_fdinfo); ++i) {
            if (list[i].proc_fdtype != PROX_FDTYPE_SOCKET) continue;
            struct socket_fdinfo info = {0};
            int size = proc_pidfdinfo(pid, list[i].proc_fd, PROC_PIDFDSOCKETINFO,
                                      &info, sizeof(info));
            if (size != (int)sizeof(info)) {
                fprintf(stderr, "socket FD %d returned %d bytes, errno %d (wanted %zu)\n",
                        list[i].proc_fd, size, errno, sizeof(info));
                exit(1);
            }
            if (exact(&info, family, port, wildcard)) {
                struct witness found = {.socket_id = info.psi.soi_so,
                                         .generation = info.psi.soi_proto.pri_tcp.tcpsi_ini.insi_gencnt,
                                         .fd = list[i].proc_fd};
                free(list);
                return found;
            }
        }
        free(list);
        return (struct witness){0};
    }
    fprintf(stderr, "FD list kept growing\n");
    exit(1);
}

static bool same_socket(struct witness first, struct witness second) {
    return first.socket_id && first.socket_id == second.socket_id &&
           first.generation == second.generation;
}

int main(void) {
    int report[2], command[2];
    if (pipe(report) || pipe(command)) fail("pipe");
    pid_t child = fork();
    if (child < 0) fail("fork");
    if (child == 0) {
        if (atexit(clean_orphan)) fail("atexit");
        close(report[0]);
        close(command[1]);
        if (setpgid(0, 0)) fail("setpgid");
        struct ports ports = {0};
        int v4 = listen_on(AF_INET, false, true, 0, &ports.ipv4);
        int v6 = listen_on(AF_INET6, false, true, 0, &ports.ipv6);
        int v6dual = listen_on(AF_INET6, false, false, 0, &ports.ipv6_dual);
        int any = listen_on(AF_INET, true, true, 0, &ports.wildcard);
        int bound = bind_without_listen(&ports.bound_only);
        int member_pipe[2];
        if (pipe(member_pipe)) fail("member pipe");
        pid_t intermediate = fork();
        if (intermediate < 0) fail("intermediate fork");
        if (intermediate == 0) {
            close(member_pipe[0]);
            pid_t member = fork();
            if (member < 0) fail("member fork");
            if (member == 0) {
                close(v4);
                close(v6);
                close(v6dual);
                close(any);
                close(bound);
                close(report[1]);
                close(command[0]);
                struct member_report item = {.pid = getpid(), .port = 0};
                int held = listen_on(AF_INET, false, true, 0, &item.port);
                transfer(member_pipe[1], &item, sizeof(item), true);
                close(member_pipe[1]);
                while (true) pause();
                close(held);
            }
            _exit(0);
        }
        close(member_pipe[1]);
        struct member_report item;
        transfer(member_pipe[0], &item, sizeof(item), false);
        close(member_pipe[0]);
        if (waitpid(intermediate, NULL, 0) != intermediate) fail("wait intermediate");
        orphan_to_clean = item.pid;
        ports.group_member = item.port;
        ports.group_member_pid = item.pid;
        transfer(report[1], &ports, sizeof(ports), true);
        char instruction;
        transfer(command[0], &instruction, 1, false);
        close(v4);
        v4 = listen_on(AF_INET, false, true, ports.ipv4, &ports.ipv4);
        transfer(report[1], &instruction, 1, true);
        transfer(command[0], &instruction, 1, false);
        close(v4);
        close(v6);
        close(v6dual);
        close(any);
        close(bound);
        clean_orphan();
        _exit(0);
    }
    close(report[1]);
    close(command[0]);
    struct ports ports;
    transfer(report[0], &ports, sizeof(ports), false);
    int group_members[64] = {0};
    if (proc_listpgrppids(child, group_members, sizeof(group_members)) <= 0)
        fail("proc_listpgrppids");
    bool group_contains_child = false;
    bool group_contains_orphan = false;
    for (size_t i = 0; i < sizeof(group_members) / sizeof(group_members[0]); ++i)
    {
        group_contains_child |= group_members[i] == child;
        group_contains_orphan |= group_members[i] == ports.group_member_pid;
    }
    if (!group_contains_child || !group_contains_orphan) {
        fprintf(stderr, "process group omitted leader or reparented member\n");
        return 1;
    }
    struct witness v4 = observe(child, AF_INET, ports.ipv4, false);
    struct witness v6 = observe(child, AF_INET6, ports.ipv6, false);
    struct witness v6dual = observe(child, AF_INET6, ports.ipv6_dual, false);
    struct witness wildcard = observe(child, AF_INET, ports.wildcard, true);
    struct witness orphan = observe(ports.group_member_pid, AF_INET, ports.group_member, false);
    struct witness false_exact = observe(child, AF_INET, ports.wildcard, false);
    struct witness false_bound = observe(child, AF_INET, ports.bound_only, false);
    if (!v4.socket_id || !v6.socket_id || !v6dual.socket_id || !wildcard.socket_id ||
        !orphan.socket_id ||
        false_exact.socket_id || false_bound.socket_id) {
        fprintf(stderr, "listener classification failed: v4=%llu v6=%llu dual=%llu wildcard=%llu orphan=%llu false-exact=%llu false-bound=%llu\n",
                v4.socket_id, v6.socket_id, v6dual.socket_id, wildcard.socket_id, orphan.socket_id,
                false_exact.socket_id, false_bound.socket_id);
        return 1;
    }
    if (!same_socket(v4, observe(child, AF_INET, ports.ipv4, false)) ||
        !same_socket(v6, observe(child, AF_INET6, ports.ipv6, false)) ||
        !same_socket(v6dual, observe(child, AF_INET6, ports.ipv6_dual, false)) ||
        !same_socket(wildcard, observe(child, AF_INET, ports.wildcard, true)) ||
        !same_socket(orphan, observe(ports.group_member_pid, AF_INET, ports.group_member, false))) {
        fprintf(stderr, "unchanged listener identity was unstable\n");
        return 1;
    }
    printf("child=%d v4=%u(fd=%d socket=%llu generation=%llu) "
           "v6=%u(fd=%d socket=%llu generation=%llu) "
           "v6-dual=%u(fd=%d socket=%llu generation=%llu) "
           "wildcard=%u(fd=%d socket=%llu generation=%llu) "
           "group-member=%d:%u(fd=%d socket=%llu generation=%llu) "
           "exact-wildcard=false bound-only=false reparented-group-member=true stable=true\n",
           child, ports.ipv4, v4.fd, v4.socket_id, v4.generation,
           ports.ipv6, v6.fd, v6.socket_id, v6.generation,
           ports.ipv6_dual, v6dual.fd, v6dual.socket_id, v6dual.generation,
           ports.wildcard, wildcard.fd, wildcard.socket_id, wildcard.generation,
           ports.group_member_pid, ports.group_member, orphan.fd, orphan.socket_id, orphan.generation);
    char instruction = 'R';
    transfer(command[1], &instruction, 1, true);
    transfer(report[0], &instruction, 1, false);
    struct witness replaced = observe(child, AF_INET, ports.ipv4, false);
    if (!replaced.socket_id || replaced.socket_id == v4.socket_id) {
        fprintf(stderr, "socket replacement did not change identity\n");
        return 1;
    }
    printf("replacement: old=%llu new=%llu\n", v4.socket_id, replaced.socket_id);
    transfer(command[1], &instruction, 1, true);
    int status;
    if (waitpid(child, &status, 0) != child || !WIFEXITED(status) || WEXITSTATUS(status))
        fail("child exit");
    return 0;
}
