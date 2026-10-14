# Silo guest process control. Arguments: kill TOKEN GRACE_MS WAIT_MS | status TOKEN.
# `kill` stops the supervised group (TERM, a grace period, then KILL) and exits 0
# once no member remains, or 1 if some survive. A run whose supervisor has not yet
# recorded its group is waited for up to WAIT_MS, because the caller only asks to
# kill runs that are still going. `status` prints and clears the signal name a
# supervised run left behind.
use strict;
use warnings;
use Time::HiRes ();

my ($command, $token, $grace, $wait) = @ARGV;
$wait = 0 unless defined $wait;
my $state = "/tmp/silo-exec-$token";
if ($command eq 'status') {
    if (open(my $handle, '<', "$state.sig")) {
        my $name = <$handle>;
        close $handle;
        print $name if defined $name;
    }
    unlink "$state.sig", "$state.pid";
    exit 0;
}
my $group;
my $deadline_start = Time::HiRes::time() + $wait / 1000;
while (1) {
    if (open(my $handle, '<', "$state.pid")) {
        $group = <$handle>;
        close $handle;
    }
    last if defined $group && $group =~ /^(\d+)$/;
    last if Time::HiRes::time() >= $deadline_start;
    Time::HiRes::sleep(0.05);
}
if (!defined $group || $group !~ /^(\d+)$/) { exit 0; }
$group = $1;
sub alive { return kill(0, -$group) || $!{EPERM}; }
sub wait_gone {
    my ($milliseconds) = @_;
    my $deadline = Time::HiRes::time() + $milliseconds / 1000;
    while (alive()) {
        return 0 if Time::HiRes::time() >= $deadline;
        Time::HiRes::sleep(0.05);
    }
    return 1;
}
if (alive()) {
    kill 'TERM', -$group;
    if (!wait_gone($grace)) {
        kill 'KILL', -$group;
        wait_gone(3000);
    }
}
if (alive()) { exit 1; }
unlink "$state.pid";
exit 0;
