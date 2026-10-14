# Silo guest process supervisor. Arguments: TOKEN CWD PROGRAM [ARGUMENT...].
# Runs PROGRAM as the leader of its own process group so the engine can stop the whole
# tree, and records that group in /tmp/silo-exec-TOKEN.pid. The supervisor itself stays
# outside the group, so once PROGRAM exits the group id still names exactly its remaining
# descendants. The record is kept while any descendant lives, so a later cancellation can
# still end them, and removed once none remain. A signal death is reported as 128+N after
# saving the signal name in /tmp/silo-exec-TOKEN.sig; an exit code of 255, which SSH also
# uses for its own failures, is saved in /tmp/silo-exec-TOKEN.exit so the engine can tell
# the two apart.
use strict;
use warnings;
use POSIX ();
use Config;

my ($token, $cwd, @argv) = @ARGV;
my $state = "/tmp/silo-exec-$token";
if (!@argv) { print STDERR "silo: no command\n"; exit 126; }

sub record {
    my ($file, $text) = @_;
    my $old = umask(077);
    if (open(my $handle, '>', $file)) {
        print $handle $text;
        close $handle;
    }
    umask($old);
}

if (length $cwd) {
    unless (chdir $cwd) {
        print STDERR "silo: cannot enter $cwd: $!\n";
        exit 126;
    }
}
my $child;
$SIG{TERM} = $SIG{INT} = $SIG{HUP} = sub {
    my ($name) = @_;
    kill($name, -$child) if defined $child && $child > 0;
};
my $pid = fork();
if (!defined $pid) {
    print STDERR "silo: cannot start $argv[0]: $!\n";
    exit 126;
}
if ($pid == 0) {
    $SIG{TERM} = $SIG{INT} = $SIG{HUP} = 'DEFAULT';
    setpgrp(0, 0) or POSIX::setsid();
    { no warnings; exec { $argv[0] } @argv; }
    my $code = $!{ENOENT} ? 127 : 126;
    print STDERR "silo: cannot run $argv[0]: $!\n";
    POSIX::_exit($code);
}
# Both sides set the group, so it exists before either continues.
setpgrp($pid, $pid);
$child = $pid;
record("$state.pid", "$pid\n");
waitpid($pid, 0);
my $status = $?;
# Keep the record while descendants of the command live: they are still in its group.
if (!(kill(0, -$pid) || $!{EPERM})) {
    unlink "$state.pid";
}
if ($status & 127) {
    my $number = $status & 127;
    my @names = split ' ', $Config{sig_name};
    my $name = $names[$number] // "SIG$number";
    record("$state.sig", "$name\n");
    exit 128 + $number;
}
my $code = $status >> 8;
record("$state.exit", "$code\n") if $code == 255;
exit($code);
