# Silo guest process supervisor. Arguments: TOKEN CWD PROGRAM [ARGUMENT...].
# Runs PROGRAM in its own process group so the engine can stop the whole tree,
# records the group in /tmp/silo-exec-TOKEN.pid and reports a signal death as
# 128+N after saving the signal name in /tmp/silo-exec-TOKEN.sig.
use strict;
use warnings;
use POSIX ();
use Config;

my ($token, $cwd, @argv) = @ARGV;
my $state = "/tmp/silo-exec-$token";
if (!@argv) { print STDERR "silo: no command\n"; exit 126; }
if (getpgrp(0) != $$) {
    setpgrp(0, 0) or POSIX::setsid();
}
{
    my $old = umask(077);
    if (open(my $handle, '>', "$state.pid")) {
        print $handle "$$\n";
        close $handle;
    }
    umask($old);
}
if (length $cwd) {
    unless (chdir $cwd) {
        print STDERR "silo: cannot enter $cwd: $!\n";
        unlink "$state.pid";
        exit 126;
    }
}
$SIG{TERM} = $SIG{INT} = $SIG{HUP} = 'IGNORE';
my $pid = fork();
if (!defined $pid) {
    print STDERR "silo: cannot start $argv[0]: $!\n";
    unlink "$state.pid";
    exit 126;
}
if ($pid == 0) {
    $SIG{TERM} = $SIG{INT} = $SIG{HUP} = 'DEFAULT';
    { no warnings; exec { $argv[0] } @argv; }
    my $code = $!{ENOENT} ? 127 : 126;
    print STDERR "silo: cannot run $argv[0]: $!\n";
    POSIX::_exit($code);
}
waitpid($pid, 0);
my $status = $?;
unlink "$state.pid";
if ($status & 127) {
    my $number = $status & 127;
    my @names = split ' ', $Config{sig_name};
    my $name = $names[$number] // "SIG$number";
    my $old = umask(077);
    if (open(my $handle, '>', "$state.sig")) {
        print $handle "$name\n";
        close $handle;
    }
    umask($old);
    exit 128 + $number;
}
exit($status >> 8);
