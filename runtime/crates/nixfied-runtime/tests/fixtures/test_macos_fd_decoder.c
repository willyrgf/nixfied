/* Compile the production SDK decoder against controlled libproc returns. */
#include <assert.h>
#include <libproc.h>
#include <sys/proc_info.h>
#include <string.h>
#include <errno.h>

static int fixture_pidfdinfo(int, int, int, void *, int);
#define proc_pidfdinfo fixture_pidfdinfo
#include "../../src/service/endpoint/macos_fd.c"
#undef proc_pidfdinfo

static struct socket_fdinfo fixture;
static int returned, fault;

static int fixture_pidfdinfo(int pid, int fd, int flavor, void *buffer, int size) {
    assert(pid == 41 && fd == 7 && flavor == PROC_PIDFDSOCKETINFO);
    assert(size == (int)sizeof(fixture));
    memcpy(buffer, &fixture, sizeof(fixture));
    errno = fault;
    return returned;
}

int main(void) {
    struct nixfied_socket out;
    for (int ipv6 = 0; ipv6 <= 1; ++ipv6) {
        memset(&fixture, 0, sizeof(fixture));
        fixture.psi.soi_kind = SOCKINFO_TCP;
        fixture.psi.soi_type = SOCK_STREAM;
        fixture.psi.soi_protocol = IPPROTO_TCP;
        fixture.psi.soi_family = ipv6 ? AF_INET6 : AF_INET;
        fixture.psi.soi_so = UINT64_C(0x123456789abcdef0);
        struct tcp_sockinfo *tcp = &fixture.psi.soi_proto.pri_tcp;
        tcp->tcpsi_state = TSI_S_LISTEN;
        tcp->tcpsi_ini.insi_vflag = ipv6 ? INI_IPV6 : INI_IPV4;
        tcp->tcpsi_ini.insi_lport = htons(23080);
        tcp->tcpsi_ini.insi_gencnt = UINT64_C(0xfedcba9876543210);
        unsigned char *address = ipv6
            ? (void *)&tcp->tcpsi_ini.insi_laddr.ina_6
            : (void *)&tcp->tcpsi_ini.insi_laddr.ina_46.i46a_addr4;
        address[ipv6 ? 15 : 0] = ipv6 ? 1 : 127;
        if (!ipv6) address[3] = 1;
        returned = sizeof(fixture);
        fault = 0;
        assert(nixfied_inspect_socket(41, 7, &out, sizeof(out)) == 1);
        assert(out.family == (ipv6 ? 6u : 4u) && out.port == 23080);
        assert(out.socket_handle == UINT64_C(0x123456789abcdef0));
        assert(out.inpcb_generation == UINT64_C(0xfedcba9876543210));
        assert(out.address[ipv6 ? 15 : 0] == (ipv6 ? 1 : 127));
        assert(nixfied_inspect_socket(41, 7, &out, sizeof(out) - 1) == -EINVAL);
        returned--;
        assert(nixfied_inspect_socket(41, 7, &out, sizeof(out)) == -EPROTO);
        returned = 0;
        for (int i = 0; i < 4; ++i) {
            const int errors[] = {EPERM, EBADF, ESRCH, 0};
            fault = errors[i];
            assert(nixfied_inspect_socket(41, 7, &out, sizeof(out)) == -(fault ? fault : EPROTO));
        }
        returned = sizeof(fixture);
        fault = 0;
        fixture.psi.soi_so = 0;
        assert(nixfied_inspect_socket(41, 7, &out, sizeof(out)) == -EPROTO);
        fixture.psi.soi_so = 1;
        fixture.psi.soi_family = AF_UNIX;
        assert(nixfied_inspect_socket(41, 7, &out, sizeof(out)) == -EPROTO);
        fixture.psi.soi_family = ipv6 ? AF_INET6 : AF_INET;
        tcp->tcpsi_state = 0;
        assert(nixfied_inspect_socket(41, 7, &out, sizeof(out)) == 0);
    }
    return 0;
}
