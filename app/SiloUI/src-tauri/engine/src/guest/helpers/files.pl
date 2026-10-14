# Silo guest file access. Subcommands: read, write, list, remove, watch.
# Failures print "silo-files: MESSAGE" on stderr and exit with 64 not_found,
# 65 permission_denied, 66 not_a_file, 67 exists, 68 changed, 69 failed, 70 too_large.
use strict;
use warnings;
use Errno ();
use Fcntl qw(:DEFAULT :flock :mode SEEK_SET);
use Digest::SHA ();
use Time::HiRes ();
use POSIX ();
use IO::Handle;
use Cwd ();

binmode(STDIN);
binmode(STDOUT);
my ($command, @args) = @ARGV;

sub fail {
    my ($code, $message) = @_;
    print STDERR "silo-files: $message\n";
    exit $code;
}

sub fail_errno {
    my ($what) = @_;
    my $error = $!;
    if ($!{ENOENT} || $!{ENOTDIR}) { fail(64, "$what: not found"); }
    if ($!{EACCES} || $!{EPERM} || $!{EROFS}) { fail(65, "$what: permission denied"); }
    if ($!{EISDIR}) { fail(66, "$what: not a file"); }
    if ($!{EEXIST}) { fail(67, "$what: exists"); }
    fail(69, "$what: $error");
}

sub millis {
    my ($seconds) = @_;
    return int($seconds * 1000);
}

if ($command eq 'read') {
    my ($path, $offset, $length, $maximum) = @args;
    $offset = $offset // 0;
    open(my $handle, '<:raw', $path) or fail_errno($path);
    my @stat = Time::HiRes::stat($handle);
    @stat or fail_errno($path);
    if (-d $handle) { fail(66, "$path: not a file"); }
    my $size = $stat[7];
    my $wanted = ($length eq '') ? ($size > $offset ? $size - $offset : 0) : $length;
    fail(70, "$path: the range is too large to return in one read") if $wanted > $maximum;
    my $data = '';
    if ($wanted > 0 && $offset < $size) {
        sysseek($handle, $offset, SEEK_SET) or fail_errno($path);
        while (length($data) < $wanted) {
            my $count = sysread($handle, my $chunk, $wanted - length($data));
            fail_errno($path) unless defined $count;
            last if $count == 0;
            $data .= $chunk;
        }
    }
    printf STDOUT "%d %d %d %d %d\n", $size, millis($stat[9]), $stat[1], $stat[2] & 07777, length($data);
    print STDOUT $data;
    exit 0;
}

if ($command eq 'list') {
    my ($path) = @args;
    opendir(my $directory, $path) or fail_errno($path);
    my @names = grep { $_ ne '.' && $_ ne '..' } readdir($directory);
    closedir($directory);
    for my $name (sort @names) {
        my @stat = Time::HiRes::lstat("$path/$name");
        next unless @stat;
        my $mode = $stat[2];
        my $type = S_ISLNK($mode) ? 'link' : S_ISDIR($mode) ? 'dir' : S_ISREG($mode) ? 'file' : 'other';
        print STDOUT join("\0", $type, $stat[7], millis($stat[9]), $name), "\0";
    }
    exit 0;
}

if ($command eq 'remove') {
    for my $path (@args) {
        my @stat = lstat($path);
        if (!@stat) {
            next if $!{ENOENT};
            fail_errno($path);
        }
        fail(66, "$path: not a file") if S_ISDIR($stat[2]);
        unlink($path) or ($!{ENOENT} ? next : fail_errno($path));
    }
    exit 0;
}

if ($command eq 'write') {
    my ($path, $mode, $parents, $replace, $expected) = @args;
    my $data = do { local $/; my $text = <STDIN>; defined $text ? $text : '' };
    my ($requested) = $path =~ m{^(.*)/[^/]*\z}s;
    $requested = '/' if !defined $requested || $requested eq '';
    if ($parents eq '1') {
        my $built = '';
        for my $part (grep { length } split m{/}, $requested) {
            $built .= "/$part";
            next if -d $built;
            my $old = umask(0);
            mkdir($built, 0700) or ($!{EEXIST} ? 1 : fail_errno($built));
            umask($old);
        }
    }
    my $name = ($path =~ m{([^/]*)\z}s)[0];
    fail(66, "$path: not a file") if $name eq '' || $name eq '.' || $name eq '..';
    # The destination is identified by its resolved parent folder and its name, so every
    # alias of one file (`/a/./f`, `/a/../a/f`, a symlinked folder) shares one identity.
    my $directory = Cwd::abs_path($requested);
    defined $directory or fail_errno($requested);
    my $folder;
    open($folder, '<', $directory) or fail_errno($directory);
    my @pinned = stat($folder);
    # Writers that use this helper take turns on the folder. The lock is held until exit.
    flock($folder, LOCK_EX) or fail(69, "$path: could not lock the folder: $!");
    my $target = ($directory eq '/' ? '' : $directory) . "/$name";
    fail(66, "$path: not a file") if -d $target;
    my $same_folder = sub {
        my @now = stat($directory);
        return @now && @pinned && $now[0] == $pinned[0] && $now[1] == $pinned[1];
    };
    my $temporary;
    my $handle;
    for (1 .. 16) {
        $temporary = sprintf("%s/.%s.silo-%d-%d", $directory eq '/' ? '' : $directory, $name, $$, int(rand(1e9)));
        my $old = umask(0);
        my $ok = sysopen($handle, $temporary, O_WRONLY | O_CREAT | O_EXCL, 0600);
        umask($old);
        last if $ok;
        fail_errno($directory) unless $!{EEXIST};
        undef $handle;
    }
    fail(69, "$path: could not create a temporary file") unless $handle;
    my $cleanup = sub { unlink($temporary); };
    binmode($handle);
    my $written = 0;
    while ($written < length($data)) {
        my $count = syswrite($handle, $data, length($data) - $written, $written);
        if (!defined $count) { my $saved = $!; $cleanup->(); $! = $saved; fail_errno($path); }
        $written += $count;
    }
    chmod($mode + 0, $handle) or do { my $saved = $!; $cleanup->(); $! = $saved; fail_errno($path); };
    my @stat = Time::HiRes::stat($handle);
    $handle->sync if $handle->can('sync');
    close($handle);
    if (!$same_folder->()) {
        $cleanup->();
        fail(68, "$path: changed");
    }
    if ($replace eq 'always') {
        rename($temporary, $target) or do { my $saved = $!; $cleanup->(); $! = $saved; fail_errno($path); };
    } elsif ($replace eq 'never' || ($replace eq 'ifSha256' && $expected eq '')) {
        if (!link($temporary, $target)) {
            my $saved = $!;
            $cleanup->();
            $! = $saved;
            if ($saved == Errno::EEXIST) {
                fail($replace eq 'never' ? 67 : 68, "$path: " . ($replace eq 'never' ? 'exists' : 'changed'));
            }
            fail_errno($path);
        }
        $cleanup->();
    } elsif ($replace eq 'ifSha256') {
        my $current;
        my @before;
        if (open(my $existing, '<:raw', $target)) {
            @before = Time::HiRes::stat($existing);
            my $digest = Digest::SHA->new(256);
            $digest->addfile($existing);
            $current = $digest->hexdigest;
            close($existing);
        }
        if (!defined $current || lc($current) ne lc($expected)) {
            $cleanup->();
            fail(68, "$path: changed");
        }
        my @after = Time::HiRes::stat($target);
        if (!@after || $after[1] != $before[1] || $after[0] != $before[0] || $after[7] != $before[7] || $after[9] != $before[9]) {
            $cleanup->();
            fail(68, "$path: changed");
        }
        rename($temporary, $target) or do { my $saved = $!; $cleanup->(); $! = $saved; fail_errno($path); };
    } else {
        $cleanup->();
        fail(69, "unknown replace mode");
    }
    printf STDOUT "%d %d %d\n", $stat[7], millis($stat[9]), $stat[1];
    exit 0;
}

if ($command eq 'watch') {
    # Polls every folder, but at bounded cost: at most MAX_ENTRIES names are read per folder
    # and each folder is kept as one digest, so memory does not grow with the folder. The
    # pause between looks grows with the time a look takes. A folder with more entries is
    # watched by its own metadata alone (additions and removals; not in-place rewrites).
    # The first record, a lone NUL, says the baseline is recorded and later changes count.
    $| = 1;
    use constant MAX_ENTRIES => 4096;
    my %last;
    my $armed = 0;
    while (1) {
        my $started = Time::HiRes::time();
        for my $path (@args) {
            my $digest = Digest::SHA->new(1);
            my @own = Time::HiRes::stat($path);
            if (!@own) {
                $digest->add('missing');
            } else {
                $digest->add("d:$own[1]:$own[9]");
                if (opendir(my $directory, $path)) {
                    my @names;
                    my $overflow = 0;
                    while (defined(my $name = readdir($directory))) {
                        next if $name eq '.' || $name eq '..';
                        if (@names >= MAX_ENTRIES) { $overflow = 1; last; }
                        push @names, $name;
                    }
                    closedir($directory);
                    if ($overflow) {
                        $digest->add('overflow');
                    } else {
                        for my $name (sort @names) {
                            my @stat = Time::HiRes::lstat("$path/$name");
                            $digest->add(@stat ? "$name:$stat[1]:$stat[7]:$stat[9]\n" : "$name:gone\n");
                        }
                    }
                }
            }
            my $signature = $digest->hexdigest;
            if ($armed && (!defined $last{$path} || $last{$path} ne $signature)) {
                print STDOUT "$path\0";
            }
            $last{$path} = $signature;
        }
        if (!$armed) {
            $armed = 1;
            print STDOUT "\0";
        }
        my $took = Time::HiRes::time() - $started;
        my $pause = $took * 5;
        $pause = 0.2 if $pause < 0.2;
        $pause = 5 if $pause > 5;
        Time::HiRes::sleep($pause);
    }
}

fail(69, "unknown command");
