#!/usr/bin/perl
# Test 36, inside the computer: #347's checks 6 to 9 against py/36-server.py on the Mac, through
# the computer's network stack, and the probe that watches a restart of the stack (10). Plain perl
# with core modules only: a macOS computer has no python3 until the Command Line Tools are in it.
#
#   36-checks.pl bytes HOST PORT N          6: N bodies of 1 KB to 2 MB echoed, byte for byte
#   36-checks.pl first HOST PORT R          7: R streamed replies: when the first event arrives
#   36-checks.pl slow HOST PORT SECONDS     8: a reply silent for SECONDS, then its end
#   36-checks.pl duplex HOST PORT SECONDS   9: lines both ways on one connection for SECONDS
#   36-checks.pl probe HOST PORT SECONDS FILE
#                                          10: a request every 0.2 s, "time ok|fail" lines to FILE
# Each check prints one JSON object.
use strict;
use warnings;
use IO::Socket::INET;
use IO::Select;
use Time::HiRes qw(time sleep);
use Digest::SHA qw(sha256_hex);

my ($cmd, $host, $port, $arg, $file) = @ARGV;
die "usage: 36-checks.pl bytes|first|slow|duplex|probe HOST PORT ARG [FILE]\n" unless $cmd && $host && $port;

sub conn {
    my $s = IO::Socket::INET->new(PeerAddr => $host, PeerPort => $port, Proto => 'tcp', Timeout => 10);
    return $s;
}

sub read_all {    # the whole reply; returns (headers, body, error)
    my ($s, $limit) = @_;
    my $buf = '';
    my $sel = IO::Select->new($s);
    while (1) {
        my @r = $sel->can_read($limit);
        return (undef, undef, 'timeout') unless @r;
        my $n = sysread($s, my $d, 1 << 20);
        return (undef, undef, "read: $!") unless defined $n;
        last if $n == 0;
        $buf .= $d;
    }
    my $i = index($buf, "\r\n\r\n");
    return (undef, undef, 'no headers') if $i < 0;
    return (substr($buf, 0, $i), substr($buf, $i + 4), undef);
}

sub dechunk {
    my ($b) = @_;
    my $out = '';
    while ($b =~ s/^([0-9a-fA-F]+)\r\n//) {
        my $n = hex($1);
        last if $n == 0;
        $out .= substr($b, 0, $n);
        $b = substr($b, $n + 2);
    }
    return $out;
}

sub jstr { my $s = shift; $s =~ s/(["\\])/\\$1/g; $s =~ s/[\x00-\x1f]/ /g; return "\"$s\""; }

if ($cmd eq 'bytes') {
    my $n = $arg || 100;
    srand(36);
    my ($same, @bad) = (0);
    my $t0 = time;
    for my $i (0 .. $n - 1) {
        my $size = int(1024 * exp(log(2048) * $i / ($n > 1 ? $n - 1 : 1)));
        my $body = join('', map { chr(int(rand(256))) } 1 .. $size);
        my $want = sha256_hex($body);
        my $s = conn() or do { push @bad, "$size: connect $!"; next };
        print $s "POST /echo HTTP/1.1\r\nHost: $host\r\nContent-Type: application/octet-stream\r\nContent-Length: $size\r\nConnection: close\r\n\r\n$body";
        my ($h, $b, $err) = read_all($s, 60);
        close $s;
        if ($err) { push @bad, "$size: $err"; next }
        my ($got) = $h =~ /X-Sha256: ([0-9a-f]+)/i;
        if (($got // '') eq $want && sha256_hex($b) eq $want) { $same++ }
        else { push @bad, "$size: " . (($got // '') eq $want ? 'reply differs' : 'request differs') }
    }
    printf "{\"check\":6,\"n\":%d,\"same\":%d,\"seconds\":%.2f,\"bad\":[%s]}\n", $n, $same, time - $t0,
        join(',', map { jstr($_) } @bad[0 .. ($#bad < 4 ? $#bad : 4)]);
}
elsif ($cmd eq 'first') {
    my $r = $arg || 5;
    my (@first, @second, @bad);
    for (1 .. $r) {
        my $t0 = time;
        my $s = conn() or do { push @bad, "connect $!"; next };
        print $s "GET /stream HTTP/1.1\r\nHost: $host\r\nAccept: text/event-stream\r\nConnection: close\r\n\r\n";
        my ($buf, $f, $g) = ('', undef, undef);
        my $sel = IO::Select->new($s);
        while ($sel->can_read(10)) {
            my $n = sysread($s, my $d, 65536);
            last unless $n;
            $buf .= $d;
            $f //= time - $t0 if index($buf, 'data: first') >= 0;
            if (index($buf, 'data: second') >= 0) { $g = time - $t0; last }
        }
        close $s;
        if (defined $f && defined $g) { push @first, $f * 1000; push @second, $g * 1000 }
        else { push @bad, 'stream did not arrive whole' }
    }
    my @fs = sort { $a <=> $b } @first;
    printf "{\"check\":7,\"n\":%d,\"whole\":%d,\"first_ms_median\":%.1f,\"first_ms_max\":%.1f,\"second_ms_median\":%.1f,\"bad\":[%s]}\n",
        $r, scalar(@first), (@fs ? $fs[int($#fs / 2)] : -1), (@fs ? $fs[-1] : -1),
        (@second ? (sort { $a <=> $b } @second)[int($#second / 2)] : -1), join(',', map { jstr($_) } @bad);
}
elsif ($cmd eq 'slow') {
    my $idle = $arg || 60;
    my $t0 = time;
    my $s = conn() or do { printf "{\"check\":8,\"idle\":%s,\"whole\":0,\"error\":%s}\n", $idle, jstr("connect $!"); exit 0 };
    print $s "GET /slow/$idle HTTP/1.1\r\nHost: $host\r\nConnection: close\r\n\r\n";
    my ($h, $b, $err) = read_all($s, $idle + 60);
    close $s;
    my $whole = (!$err && dechunk($b) eq "start\nend\n") ? 1 : 0;
    printf "{\"check\":8,\"idle\":%s,\"whole\":%d,\"seconds\":%.1f,\"error\":%s}\n", $idle, $whole, time - $t0, jstr($err // '');
}
elsif ($cmd eq 'duplex') {
    my $secs = $arg || 30;
    my $s = conn() or do { printf "{\"check\":9,\"sent\":0,\"error\":%s}\n", jstr("connect $!"); exit 0 };
    $s->blocking(0);
    my $sel = IO::Select->new($s);
    my ($i, $got, $ooo, $max, $next, $last, $buf) = (0, 0, 0, 0, 0, -1, '');
    my %sent;
    my $t0 = time;
    my $err = '';
    while (time - $t0 < $secs || ($got < $i && time - $t0 < $secs + 10)) {
        if (time - $t0 < $secs && time >= $next) {
            $sent{$i} = time;
            my $line = "$i " . ('x' x 200) . "\n";
            my $w = syswrite($s, $line);
            unless (defined $w && $w == length $line) { $err = "write: $!"; last }
            $i++;
            $next = time + 0.05;
        }
        if ($sel->can_read(0.01)) {
            my $n = sysread($s, my $d, 65536);
            if (!defined $n) { next if $!{EAGAIN}; $err = "read: $!"; last }
            if ($n == 0) { $err = 'closed by the other end'; last }
            $buf .= $d;
            while ($buf =~ s/^(\d+) x*\n//) {
                my $k = $1;
                $ooo++ if $k != $last + 1;
                $last = $k;
                my $rtt = (time - $sent{$k}) * 1000;
                $max = $rtt if $rtt > $max;
                $got++;
            }
        }
    }
    close $s;
    printf "{\"check\":9,\"seconds\":%d,\"sent\":%d,\"echoed\":%d,\"out_of_order\":%d,\"max_rtt_ms\":%.1f,\"error\":%s}\n",
        $secs, $i, $got, $ooo, $max, jstr($err);
}
elsif ($cmd eq 'probe') {
    my $secs = $arg || 120;
    open(my $out, '>>', $file) or die "$file: $!";
    $out->autoflush(1);
    my $t0 = time;
    while (time - $t0 < $secs) {
        my $t = time;
        my $s = IO::Socket::INET->new(PeerAddr => $host, PeerPort => $port, Proto => 'tcp', Timeout => 1);
        my $ok = 0;
        if ($s) {
            print $s "GET /ok HTTP/1.1\r\nHost: $host\r\nConnection: close\r\n\r\n";
            my ($h, $b, $err) = read_all($s, 2);
            $ok = (!$err && $b eq "ok\n") ? 1 : 0;
            close $s;
        }
        printf $out "%.3f %s\n", $t, $ok ? 'ok' : 'fail';
        my $rest = 0.2 - (time - $t);
        sleep($rest) if $rest > 0;
    }
    print "{\"check\":10,\"probe\":\"done\"}\n";
}
else {
    die "unknown check $cmd\n";
}
