#!/usr/bin/perl
# Runs inside a guest computer: the write-wait probe (tests 15 and 17). Every INTERVAL seconds it
# writes 4 KiB to FILE and flushes it (MODE full: F_FULLFSYNC; fsync: fsync), and appends
# "time seconds" to LOG for every write that took longer than THRESHOLD seconds. LOG.max holds the
# longest wait so far. Time is the guest's clock (Time::HiRes, which macOS's perl ships).
#   writewait.pl FILE LOG MODE [INTERVAL] [THRESHOLD]
use strict;
use warnings;
use Fcntl qw(O_WRONLY O_CREAT SEEK_SET);
use IO::Handle;
use Time::HiRes qw(time sleep);
my ($file, $logf, $mode, $interval, $thr) = @ARGV;
$interval = 0.1 unless defined $interval;
$thr = 0.05 unless defined $thr;
sysopen(my $fh, $file, O_WRONLY | O_CREAT) or die "$!\n";
open(my $log, '>>', $logf) or die "$!\n";
$log->autoflush(1);
my $blk = 'w' x 4096;
my $max = 0;
while (1) {
  my $t = time;
  sysseek($fh, 0, SEEK_SET);
  syswrite($fh, $blk) == 4096 or die "write: $!\n";
  if ($mode eq 'full') { defined fcntl($fh, 51, 0) or $fh->sync; } else { $fh->sync; }
  my $d = time - $t;
  if ($d > $thr) { printf $log "%.3f %.4f\n", $t, $d; }
  if ($d > $max) {
    $max = $d;
    if (open(my $m, '>', "$logf.max")) { printf $m "%.4f\n", $max; close $m; }
  }
  sleep($interval);
}
