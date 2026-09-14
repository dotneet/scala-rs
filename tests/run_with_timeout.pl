#!/usr/bin/env perl
# Run one test binary with a wall-clock limit and kill its process group.
#
# This is deliberately a small Perl helper instead of a dependency on the
# platform-specific `timeout` command. The workspace runner uses it on macOS
# and Linux, where Perl and POSIX process groups are available. The child is
# made the leader of a fresh process group before exec, so a timed-out test
# cannot leave a scala-rs/java descendant running after the worker exits.

use strict;
use warnings;
use Errno qw(EINTR);
use POSIX qw(_exit WEXITSTATUS WIFEXITED WIFSIGNALED WTERMSIG setpgid);
use Time::HiRes qw(alarm sleep);

sub usage {
    die "usage: run_with_timeout.pl SECONDS COMMAND [ARG ...]\n";
}

@ARGV >= 2 or usage();
my ($seconds, @command) = @ARGV;
$seconds =~ /\A(?:0|[1-9][0-9]*)\z/
    or die "run_with_timeout: SECONDS must be a non-negative integer\n";

my $pid = fork();
defined $pid or die "run_with_timeout: fork failed: $!\n";

if ($pid == 0) {
    # Do this in the child as well as the parent to close the small fork/exec
    # race in which the alarm could arrive before the parent setpgid call.
    setpgid(0, 0) or _exit(125);
    exec @command or _exit(127);
}

# The parent call is harmless if the child already did it, and handles the
# usual case before the first timer tick. A failure is non-fatal: the direct
# child is still waited for and killed on timeout; the child-side call above
# is the process-group guarantee.
setpgid($pid, $pid);

my ($timed_out, $forced) = (0, 0);
$SIG{ALRM} = sub {
    if (!$timed_out) {
        $timed_out = 1;
        kill 'TERM', -$pid;
        kill 'TERM', $pid;
        # Give a well-behaved test a short grace period, then force-kill the
        # group. The second alarm also makes an ignore-TERM test bounded.
        alarm(1);
    } else {
        $forced = 1;
        kill 'KILL', -$pid;
        kill 'KILL', $pid;
    }
};

alarm(0 + $seconds) if $seconds != 0;
while (1) {
    my $waited = waitpid($pid, 0);
    last if $waited == $pid;
    next if $waited == -1 && $! == EINTR;
    die "run_with_timeout: waitpid failed: $!\n";
}
alarm(0);

if ($timed_out) {
    # Do not cancel the grace-period alarm merely because the direct child
    # exited on TERM. A test can leave a TERM-ignoring compiler/JVM child in
    # the process group, so keep the parent alive until the second alarm has
    # had a chance to KILL the whole group. The explicit KILL also covers the
    # case where waitpid returned just before that alarm fired.
    sleep(1) unless $forced;
    kill 'KILL', -$pid;
    kill 'KILL', $pid;
    # The direct child has been reaped by the wait above. Its process group
    # was signalled before that wait, including any compiler/JVM descendants.
    # Keep this marker deterministic so workspace logs remain diff-friendly.
    print STDERR "workspace_tests: TIMEOUT limit=${seconds}s\n";
    exit 124;
}

if (WIFEXITED($?)) {
    exit WEXITSTATUS($?);
}
if (WIFSIGNALED($?)) {
    exit 128 + WTERMSIG($?);
}
exit 125;
