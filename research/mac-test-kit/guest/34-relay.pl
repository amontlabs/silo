#!/usr/bin/perl
# Runs as root inside a macOS guest (test 34): hands every connection made to 127.0.0.1:443 and
# [::1]:443 to 127.0.0.1:PORT, the end of the SSH tunnel to the boundary on the Mac. Perl, because
# a computer without the Command Line Tools has no python3.
#   34-relay.pl PORT [LISTEN_PORT]    (LISTEN_PORT is 443 unless given)
use strict;
use warnings;
use IO::Socket::IP;
use IO::Select;
use POSIX ":sys_wait_h";

$SIG{CHLD} = sub { 1 while waitpid(-1, WNOHANG) > 0 };
my $to = shift || 18443;
my $port = shift || 443;
my @listen;
for my $addr ('127.0.0.1', '::1') {
  my $s = IO::Socket::IP->new(LocalHost => $addr, LocalPort => $port, Listen => 128, ReuseAddr => 1, Proto => 'tcp');
  push @listen, $s if $s;
}
die "could not listen on port $port\n" unless @listen;
my $sel = IO::Select->new(@listen);
while (1) {
  for my $ls ($sel->can_read) {
    my $c = $ls->accept or next;
    my $pid = fork;
    if (!defined $pid || $pid) { close $c; next }
    close $_ for @listen;
    my $u = IO::Socket::IP->new(PeerHost => '127.0.0.1', PeerPort => $to, Proto => 'tcp') or exit 0;
    pump($c, $u);
    exit 0;
  }
}

sub pump {
  my ($x, $y) = @_;
  my %peer = ("$x" => $y, "$y" => $x);
  my $s = IO::Select->new($x, $y);
  my $open = 2;
  while ($open > 0) {
    for my $h ($s->can_read) {
      my $buf;
      my $n = sysread($h, $buf, 65536);
      if (!$n) { shutdown($peer{"$h"}, 1); $s->remove($h); $open--; next }
      my $off = 0;
      while ($off < $n) {
        my $w = syswrite($peer{"$h"}, $buf, $n - $off, $off);
        return unless defined $w;
        $off += $w;
      }
    }
  }
}
