#!/usr/bin/perl
# Runs inside a guest computer: the cost of a small flushed write (test 32). N writes of 4 KiB at
# random places of a 64 MiB FILE, each flushed (MODE full: F_FULLFSYNC; fsync: fsync), timed with
# the guest's clock. Prints one line: {"n":..,"p50_ms":..,"p99_ms":..,"max_ms":..,"mean_ms":..}.
#   wlat.pl FILE N MODE
use strict;
use warnings;
use Fcntl qw(O_RDWR O_CREAT SEEK_SET);
use IO::Handle;
use Time::HiRes qw(time);
my ($file, $n, $mode) = @ARGV;
$n = 1000 unless defined $n;
$mode = 'full' unless defined $mode;
my $size = 64 * 1024 * 1024;
sysopen(my $fh, $file, O_RDWR | O_CREAT) or die "$file: $!\n";
if (-s $file < $size) {
  my $mib = 'f' x (1024 * 1024);
  sysseek($fh, 0, SEEK_SET);
  for (1 .. 64) { syswrite($fh, $mib) == length($mib) or die "fill: $!\n"; }
  $fh->sync;
}
my $blk = 'w' x 4096;
my @t;
srand(32);
for (1 .. $n) {
  my $off = int(rand($size / 4096)) * 4096;
  my $t0 = time;
  sysseek($fh, $off, SEEK_SET);
  syswrite($fh, $blk) == 4096 or die "write: $!\n";
  if ($mode eq 'full') { defined fcntl($fh, 51, 0) or $fh->sync; } else { $fh->sync; }
  push @t, time - $t0;
}
my @s = sort { $a <=> $b } @t;
my $sum = 0;
$sum += $_ for @s;
printf "{\"n\":%d,\"p50_ms\":%.3f,\"p99_ms\":%.3f,\"max_ms\":%.3f,\"mean_ms\":%.3f}\n",
  scalar(@s), 1000 * $s[int(@s / 2)], 1000 * $s[int(@s * 0.99)], 1000 * $s[-1], 1000 * $sum / @s;
