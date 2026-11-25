package forwarder

// Fuzz test for the deny list of gvproxy-silo.patch (#380), with addresses a computer chooses.
// Property: with "private" denied, an address is denied when it is, or carries inside a standard
// IPv6 form, an address of a private IPv4 network: IPv4-mapped (::ffff:a.b.c.d), NAT64
// (64:ff9b::/96, RFC 6052), local NAT64 (64:ff9b:1::/48, RFC 8215) or 6to4 (2002:aabb:ccdd::/16).

import (
	"net"
	"net/netip"
	"testing"
)

func embeddedV4(a netip.Addr) (netip.Addr, string) {
	if a.Is4() || a.Is4In6() {
		return a.Unmap(), "ipv4"
	}
	b := a.As16()
	switch {
	case b[0] == 0x00 && b[1] == 0x64 && b[2] == 0xff && b[3] == 0x9b && [8]byte(b[4:12]) == [8]byte{}:
		return netip.AddrFrom4([4]byte(b[12:16])), "nat64"
	case b[0] == 0x00 && b[1] == 0x64 && b[2] == 0xff && b[3] == 0x9b && b[4] == 0 && b[5] == 1:
		return netip.AddrFrom4([4]byte(b[12:16])), "local-nat64"
	case b[0] == 0x20 && b[1] == 0x02:
		return netip.AddrFrom4([4]byte(b[2:6])), "6to4"
	}
	return netip.Addr{}, ""
}

func FuzzDenyPrivate(f *testing.F) {
	d, err := ParseDenyList([]string{"private"})
	if err != nil {
		f.Fatal(err)
	}
	for _, s := range []string{"192.168.1.1", "::ffff:192.168.1.1", "64:ff9b::c0a8:101", "2002:c0a8:101::1", "8.8.8.8", "64:ff9b:1::a00:1"} {
		f.Add(netip.MustParseAddr(s).AsSlice())
	}
	f.Fuzz(func(t *testing.T, raw []byte) {
		a, ok := netip.AddrFromSlice(raw)
		if !ok {
			return
		}
		v4, how := embeddedV4(a)
		if !v4.IsValid() {
			return
		}
		private := v4.IsPrivate() || v4.IsLoopback() || v4.IsLinkLocalUnicast() || v4.IsMulticast() || v4.IsUnspecified() ||
			netip.MustParsePrefix("100.64.0.0/10").Contains(v4)
		if private && !d.Contains(net.IP(a.AsSlice())) {
			t.Fatalf("%s carries %s (%s), a private address, and is not denied", a, v4, how)
		}
	})
}
