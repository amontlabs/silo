"""Test 07's outcome from the values the test collected.

    07-decide.py SUMMARY_FILE

SUMMARY_FILE has one "key=value" per line (the test appends them as it goes; a later line wins).
Prints the outcome key on the first line and its sentence on the second.
"""
import sys


def load(path):
    v = {}
    for line in open(path):
        line = line.rstrip("\n")
        if "=" in line:
            k, x = line.split("=", 1)
            v[k] = x
    return v


def num(v, k):
    try:
        return float(v[k])
    except (KeyError, ValueError):
        return None


def ratio(a, b):
    return a / b if a is not None and b else None


def fmt(x, nd=1):
    return "?" if x is None else ("%." + str(nd) + "f") % x


def main():
    v = load(sys.argv[1])
    prefix = "Linux analogue, not a Mac result: " if v.get("platform") == "linux" else ""
    facts = []
    if v.get("gh_lease"):
        facts.append("DHCP gave %s in %s s after the start, MTU inside %s (DHCP option 26: %s)" % (
            v["gh_lease"], v.get("gh_dhcp_s", "?"), v.get("gh_mtu", "?"), v.get("gh_dhcp_mtu", "?")))
    if v.get("gh_ssh") == "ok":
        facts.append("ssh through the Unix socket forward ready %s s after the start, %s ms per `ssh true` (NAT %s)" % (
            v.get("gh_ssh_ready_s", "?"), v.get("gh_ssh_ms", "?"), v.get("nat_ssh_ms", "-")))
    for net in ("gh", "nat"):
        if v.get(net + "_c1_count"):
            facts.append("%s: %s sequential connections in %s s, %s failed, %s over 5 s; %s at 8 in parallel, %s failed, %s over 5 s" % (
                "gvproxy" if net == "gh" else "NAT", v[net + "_c1_count"], v.get(net + "_c1_s"), v.get(net + "_c1_failed"),
                v.get(net + "_c1_over5"), v.get(net + "_c8_count"), v.get(net + "_c8_failed"), v.get(net + "_c8_over5")))
        if v.get(net + "_out_count"):
            facts.append("%s: %s connections out of the computer, %s failed, %s over 1 s, longest %s s" % (
                "gvproxy" if net == "gh" else "NAT", v[net + "_out_count"], v.get(net + "_out_failed"),
                v.get(net + "_out_over1"), v.get(net + "_out_max")))
    up_g, up_n = num(v, "gh_up_gbit"), num(v, "nat_up_gbit")
    if up_g is not None:
        facts.append("SSH copy Mac to computer %s Gbit/s on gvproxy (NAT %s), back %s (NAT %s)" % (
            fmt(up_g, 2), fmt(up_n, 2), fmt(num(v, "gh_down_gbit"), 2), fmt(num(v, "nat_down_gbit"), 2)))
    npm_g, npm_n = num(v, "gh_npm_s"), num(v, "nat_npm_s")
    git_g, git_n = num(v, "gh_git_s"), num(v, "nat_git_s")
    if npm_g is not None or git_g is not None:
        facts.append("npm install %s s on gvproxy, %s s on NAT; git clone %s s and %s s" % (
            fmt(npm_g), fmt(npm_n), fmt(git_g), fmt(git_n)))
    if v.get("gh_cpu_idle_pct"):
        facts.append("gvproxy %s%% of a core idle, %s%% during npm install, %s MB" % (
            v.get("gh_cpu_idle_pct"), v.get("gh_cpu_npm_pct", "?"), v.get("gh_rss_mb", "?")))
    if v.get("gh_cpu_after_sleep_pct"):
        facts.append("%s%% after the Mac slept" % v["gh_cpu_after_sleep_pct"])
    reach = []
    if v.get("gd_alias_loopback"):
        reach.append("gvproxy as it comes: 192.168.127.254 reached the Mac's 127.0.0.1 listener: %s; the computer used gvproxy's API with no token: %s" % (
            v["gd_alias_loopback"], v.get("gd_api_expose", "?")))
    if v.get("gh_alias_loopback"):
        reach.append("hardened: 192.168.127.254 reached it: %s, API from inside answered %s" % (
            v["gh_alias_loopback"], v.get("gh_api_code", "?")))
    if v.get("gh_lan_any"):
        reach.append("the Mac's LAN address reached the 0.0.0.0 listener through gvproxy: %s, its 127.0.0.1 listener: %s" % (
            v["gh_lan_any"], v.get("gh_lan_loopback", "?")))
    if v.get("nat_bridge_any"):
        reach.append("on NAT the bridge address reached the 0.0.0.0 listener: %s, the LAN address: %s, 127.0.0.1: %s" % (
            v["nat_bridge_any"], v.get("nat_lan_any", "?"), v.get("nat_bridge_loopback", "?")))
    text = "; ".join(facts + reach) + "."

    if not v.get("gh_lease") or v.get("gh_ssh") != "ok":
        key = "gvproxy-fails"
        why = "No %s on gvproxy (%s). " % ("DHCP lease" if not v.get("gh_lease") else "ssh through the forward",
                                          v.get("gh_fail", "see the logs"))
    elif v.get("gh_alias_loopback") == "reached" or v.get("gh_api_expose") == "opened":
        key = "hardening-fails"
        why = "The hardened configuration still lets the computer reach the Mac. "
    else:
        stall = False
        for c in ("gh_c1_over5", "gh_c8_over5"):
            if (num(v, c) or 0) > 0:
                stall = True
        for c in ("c1", "c8"):
            g, n = num(v, "gh_%s_failed" % c), num(v, "nat_%s_failed" % c)
            if g is not None and g > (n or 0) + 2:
                stall = True
        g, n = num(v, "gh_out_failed"), num(v, "nat_out_failed")
        if g is not None and g > (n or 0) + 2:
            stall = True
        g, n = num(v, "gh_out_over1"), num(v, "nat_out_over1")
        if g is not None and g > 2 * (n or 0) + 5:
            stall = True
        slow = any(r is not None and r > 1.5 for r in (ratio(npm_g, npm_n), ratio(git_g, git_n)))
        if stall:
            key, why = "gvproxy-stalls", "Connections stall or fail on gvproxy and not on NAT. "
        elif slow:
            key, why = "gvproxy-slow", "gvproxy is more than 1.5 times slower than NAT for npm install or git clone. "
        elif v.get("platform") != "linux" and npm_n is None and git_n is None:
            key, why = "inconclusive", "gvproxy worked, but there is no NAT comparison (see the notes). "
        else:
            key, why = "gvproxy-now", "gvproxy works for the computer and its hardened configuration closes the Mac's loopback. "
    print(key)
    print(prefix + why + text)


if __name__ == "__main__":
    main()
