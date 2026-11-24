#!/usr/bin/perl
# Runs inside a guest computer (macOS ships perl; no Command Line Tools needed). Writes a sequence
# that a copy of the disk can be checked against: what was acknowledged before the copy must be in
# it, and nothing after a missing write may be (tests 04, 15, 17).
#
#   seqwriter.pl raw DEVICE MAGIC FLUSH COUNTFILE FIRSTBLOCK NBLOCKS [MAX]
#       4 KiB blocks numbered 0, 1, 2, ... to DEVICE (a raw disk, /dev/rdiskN) at
#       FIRSTBLOCK + ((i * 2654435761 + MAGIC) mod NBLOCKS); NBLOCKS a power of two, so no place is
#       written twice in NBLOCKS writes. Each block starts with MAGIC and i (two little-endian u64).
#   seqwriter.pl file PATH MAGIC FLUSH COUNTFILE [MAX]
#       64-byte records appended to PATH: "SEQ <magic> <i> <checksum> ...\n".
#
# FLUSH 1: each write is flushed before it counts (F_FULLFSYNC, else DKIOCSYNCHRONIZECACHE for a
# raw disk, else fsync). After each write, COUNTFILE holds the number of writes acknowledged.
# COUNTFILE.how says which flush call worked. Stops after MAX writes (default: never).
use strict;
use warnings;
use Fcntl qw(O_WRONLY O_CREAT O_APPEND O_RDWR SEEK_SET);
use IO::Handle;

my $mode = shift @ARGV or die "usage: see the top of the file\n";
my ($target, $magic_hex, $flush, $countfile) = @ARGV[0 .. 3];
my $magic = hex($magic_hex);
my ($first, $nblk, $max);
if ($mode eq 'raw') { ($first, $nblk, $max) = @ARGV[4 .. 6]; }
else { $max = $ARGV[4]; }
$max = 0 unless defined $max;

my $F_FULLFSYNC = 51;
my $DKIOCSYNCHRONIZECACHE = 0x20006416;   # _IO('d', 22), <sys/disk.h>
my $how = '';
open(my $cf, '+>', $countfile) or die "count file: $!\n";

sub flush_fh {
  my ($fh) = @_;
  if ($how eq '' || $how eq 'F_FULLFSYNC') {
    if (defined fcntl($fh, $F_FULLFSYNC, 0)) { $how = 'F_FULLFSYNC'; return; }
  }
  if ($mode eq 'raw' && ($how eq '' || $how eq 'DKIOCSYNCHRONIZECACHE')) {
    if (defined ioctl($fh, $DKIOCSYNCHRONIZECACHE, 0)) { $how = 'DKIOCSYNCHRONIZECACHE'; return; }
  }
  $fh->sync or die "no flush works: $!\n";
  $how = 'fsync';
}

sub say_how {
  open(my $h, '>', "$countfile.how") or return;
  print $h ($how eq '' ? 'none' : $how), "\n";
  close $h;
}

my $fh;
if ($mode eq 'raw') {
  sysopen($fh, $target, O_RDWR) or die "open $target: $!\n";
  die "NBLOCKS must be a power of two\n" if $nblk & ($nblk - 1);
} elsif ($mode eq 'file') {
  sysopen($fh, $target, O_WRONLY | O_CREAT | O_APPEND) or die "open $target: $!\n";
} else { die "mode raw or file\n"; }
binmode $fh;

my $pad = 'x' x 4096;
for (my $i = 0; $max == 0 || $i < $max; $i++) {
  if ($mode eq 'raw') {
    my $pos = $first + (($i * 2654435761 + $magic) % $nblk);
    my $blk = pack('Q<Q<', $magic, $i) . substr($pad, 16);
    sysseek($fh, $pos * 4096, SEEK_SET) or die "seek: $!\n";
    my $n = syswrite($fh, $blk);
    die "write: $!\n" unless defined $n && $n == 4096;
  } else {
    my $body = sprintf('SEQ %08x %012d', $magic, $i);
    my $sum = unpack('%32C*', $body);
    my $rec = sprintf('%s %08x', $body, $sum);
    $rec .= '.' x (63 - length($rec)) . "\n";
    my $n = syswrite($fh, $rec);
    die "write: $!\n" unless defined $n && $n == 64;
  }
  flush_fh($fh) if $flush;
  say_how() if $i == 0;
  sysseek($cf, 0, SEEK_SET);
  syswrite($cf, sprintf('%12d', $i + 1));
}
