# Silo guest process control.
# Arguments: kill TOKEN GRACE_MS WAIT_MS | status TOKEN | exit TOKEN.
# `kill` stops the supervised group (TERM, a grace period, then KILL) and exits 0 once no
# member remains, or 1 if some survive. A run whose supervisor has not yet recorded its
# group is waited for up to WAIT_MS, because the caller only asks to kill runs that are
# still going. The supervisor keeps the record while any descendant of the command lives,
# so a command that exited first does not hide them. `status` prints and clears the signal
# name a supervised run left behind. `exit` prints and clears the exit code the supervisor
# saved for a command that exited with 255.
use strict;
use warnings;
use Time::HiRes ();

my ($command, $token, $grace, $wait) = @ARGV;
$wait = 0 unless defined $wait;
my $state = "/tmp/silo-exec-$token";
sub group_alive {
    my ($group) = @_;
    return kill(0, -$group) || $!{EPERM};
}
sub read_first_line {
    my ($file) = @_;
    return undef unless open(my $handle, '<', $file);
    my $line = <$handle>;
    close $handle;
    return $line;
}
if ($command eq 'status') {
    my $name = read_first_line("$state.sig");
    print $name if defined $name;
    unlink "$state.sig", "$state.exit";
    # The group record only stays while a descendant of the command lives.
    my $group = read_first_line("$state.pid");
    if (defined $group && $group =~ /^(\d+)$/ && !group_alive($1)) { unlink "$state.pid"; }
    exit 0;
}
if ($command eq 'exit') {
    my $code = read_first_line("$state.exit");
    print $code if defined $code;
    unlink "$state.exit", "$state.sig";
    exit 0;
}
my $group;
my $deadline_start = Time::HiRes::time() + $wait / 1000;
while (1) {
    $group = read_first_line("$state.pid");
    last if defined $group && $group =~ /^(\d+)$/;
    last if Time::HiRes::time() >= $deadline_start;
    Time::HiRes::sleep(0.05);
}
if (!defined $group || $group !~ /^(\d+)$/) { exit 0; }
$group = $1;
sub wait_gone {
    my ($milliseconds) = @_;
    my $deadline = Time::HiRes::time() + $milliseconds / 1000;
    while (group_alive($group)) {
        return 0 if Time::HiRes::time() >= $deadline;
        Time::HiRes::sleep(0.05);
    }
    return 1;
}
if (group_alive($group)) {
    kill 'TERM', -$group;
    if (!wait_gone($grace)) {
        kill 'KILL', -$group;
        wait_gone(3000);
    }
}
if (group_alive($group)) { exit 1; }
unlink "$state.pid";
exit 0;
