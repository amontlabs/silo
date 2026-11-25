#!/usr/bin/perl
# Test 35, inside the computer: one DNS question for NAME (type A) to SERVER:53, and how it was
# answered: "rcode N" (0 an answer, 3 no such name, 5 refused) or "none" after 4 s. A "no such
# name" for a made-up name means the question left gvproxy for a resolver beyond it; "refused" or
# "none" means it stopped there. Plain perl with core modules only.
#   35-dns.pl SERVER NAME
use strict;
use warnings;
use IO::Socket::INET;
use IO::Select;

my ($server, $name) = @ARGV;
die "usage: 35-dns.pl SERVER NAME\n" unless $server && $name;
my $id = int(rand(65536));
my $q = pack('n n n n n n', $id, 0x0100, 1, 0, 0, 0);
$q .= join('', map { chr(length $_) . $_ } split /\./, $name) . "\0";
$q .= pack('n n', 1, 1);
my $s = IO::Socket::INET->new(PeerAddr => $server, PeerPort => 53, Proto => 'udp') or do { print "none\n"; exit 0 };
$s->send($q);
my $sel = IO::Select->new($s);
my $end = time + 4;
while (time < $end) {
    last unless $sel->can_read($end - time);
    my $r = '';
    $s->recv($r, 4096);
    next if length $r < 12;
    my ($rid, $flags) = unpack('n n', $r);
    next unless $rid == $id;
    printf "rcode %d\n", $flags & 0x0f;
    exit 0;
}
print "none\n";
