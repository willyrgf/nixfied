/* Decode private libproc records through the selected SDK, never guessed Rust
 * offsets or a host PCB stream. Only this flat internal result crosses FFI. */
#include <arpa/inet.h>
#include <errno.h>
#include <libproc.h>
#include <stdint.h>
#include <string.h>
#include <sys/proc_info.h>
#include <sys/socket.h>

struct nixfied_socket {
    uint8_t address[16];
    uint64_t socket_handle;
    uint64_t inpcb_generation;
    uint32_t family;
    uint32_t port;
};
_Static_assert(sizeof(struct nixfied_socket) == 40, "unsupported bridge layout");

/* 1 = TCP listener, 0 = other socket, negative errno = inspection failure. */
int nixfied_inspect_socket(int pid, int fd, struct nixfied_socket *out, size_t size) {
    if (size != sizeof(*out)) return -EINVAL;
    struct socket_fdinfo info = {0};
    errno = 0;
    int bytes = proc_pidfdinfo(pid, fd, PROC_PIDFDSOCKETINFO, &info, sizeof(info));
    if (bytes != (int)sizeof(info)) return -(bytes <= 0 && errno ? errno : EPROTO);
    const struct socket_info *socket = &info.psi;
    if (socket->soi_kind != SOCKINFO_TCP) return 0;
    if (socket->soi_type != SOCK_STREAM || socket->soi_protocol != IPPROTO_TCP) return -EPROTO;
    const struct tcp_sockinfo *tcp = &socket->soi_proto.pri_tcp;
    if (tcp->tcpsi_state != TSI_S_LISTEN) return 0;
    const struct in_sockinfo *in = &tcp->tcpsi_ini;
    memset(out, 0, sizeof(*out));
    if (socket->soi_family == AF_INET && (in->insi_vflag & INI_IPV4)) {
        out->family = 4;
        memcpy(out->address, &in->insi_laddr.ina_46.i46a_addr4, 4);
    } else if (socket->soi_family == AF_INET6 && (in->insi_vflag & INI_IPV6)) {
        out->family = 6;
        memcpy(out->address, &in->insi_laddr.ina_6, 16);
    } else return -EPROTO;
    out->port = ntohs((uint16_t)in->insi_lport);
    out->socket_handle = socket->soi_so;
    out->inpcb_generation = in->insi_gencnt;
    if (!out->socket_handle || !out->port) return -EPROTO;
    return 1;
}
