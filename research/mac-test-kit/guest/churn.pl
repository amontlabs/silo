#!/usr/bin/perl
# Runs inside a guest computer: file-system churn, many small transactions, as a build or a
# package install makes (tests 17 and 18). Never stops on its own.
#   churn.pl DIR [FILES_PER_ROUND]
# Each round makes a folder of FILES_PER_ROUND files of 1 to 16 KiB, flushes every tenth, renames
# a few, appends a line to a log and flushes it, and deletes the folder of three rounds before.
use strict;
use warnings;
use File::Path qw(make_path remove_tree);
use IO::Handle;
my ($dir, $per) = @ARGV;
$per ||= 200;
make_path($dir);
open(my $log, '>>', "$dir/log.txt") or die "$!\n";
my $buf = 'z' x 16384;
for (my $r = 0; ; $r++) {
  my $d = "$dir/r$r";
  make_path($d);
  for my $i (1 .. $per) {
    open(my $f, '>', "$d/f$i") or die "$!\n";
    print $f substr($buf, 0, 1024 * (1 + int(rand(16))));
    $f->sync if $i % 10 == 0;
    close $f;
  }
  rename("$d/f$_", "$d/g$_") for (1 .. 10);
  print $log "round $r\n";
  $log->flush; $log->sync;
  remove_tree("$dir/r" . ($r - 3)) if $r >= 3;
}
