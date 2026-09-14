#!/usr/bin/env perl
# Digest fixture files without spawning one hashing process per file.
#
# The shell cache helpers pass paths as argv entries, so spaces (and other
# characters that are safe in an argv entry) never go through a text parser.
# Digest::SHA is part of the standard Perl distribution on the supported
# macOS/Linux environments.
use strict;
use warnings;
use Digest::SHA ();
use Fcntl qw(:mode);

sub fail {
    my ($message) = @_;
    print STDERR "fixture digest: $message\n";
    exit 1;
}

sub hash_file {
    my ($path) = @_;
    open my $fh, '<:raw', $path
        or fail("cannot read file '$path': $!");

    my $sha = Digest::SHA->new(256);
    while (1) {
        my $bytes = read($fh, my $buffer, 131_072);
        defined $bytes or fail("cannot read file '$path': $!");
        last if $bytes == 0;
        $sha->add($buffer);
    }
    close $fh or fail("cannot close file '$path': $!");
    return $sha->hexdigest;
}

sub relative_path {
    my ($base, $path) = @_;
    my $prefix = "$base/";
    return substr($path, length($prefix)) if index($path, $prefix) == 0;
    return $path;
}

sub normalize_generated_path {
    my ($path) = @_;
    # This is the equivalent of zsh's:
    #   [[ $rel == */generated/* ]] && rel="generated/${rel##*/generated/}"
    my $marker = '/generated/';
    my $position = rindex($path, $marker);
    return $position >= 0
        ? 'generated/' . substr($path, $position + length($marker))
        : $path;
}

sub digest_records {
    my ($records) = @_;
    my $sha = Digest::SHA->new(256);
    for my $record (sort { $a cmp $b } @$records) {
        $sha->add($record, "\n");
    }
    return $sha->hexdigest;
}

sub collect_tree {
    my ($root, $base, $normalize_generated, $skip_generated_markers, $only_scala) = @_;
    my @root_stat = stat($root);
    @root_stat && S_ISDIR($root_stat[2])
        or fail("generated/output directory is missing or unreadable: $root");

    my @directories = ($root);
    my @records;
    while (@directories) {
        my $directory = pop @directories;
        opendir my $dh, $directory
            or fail("cannot read directory '$directory': $!");
        my @names;
        $! = 0;
        while (1) {
            my $name = readdir($dh);
            last unless defined $name;
            next if $name eq '.' || $name eq '..';
            push @names, $name;
        }
        $! and fail("cannot read directory '$directory': $!");
        closedir $dh or fail("cannot close directory '$directory': $!");

        for my $name (@names) {
            my $path = "$directory/$name";
            my @stat = lstat($path)
                or fail("cannot inspect path '$path': $!");
            if (S_ISDIR($stat[2])) {
                push @directories, $path;
                next;
            }
            # Match find -type f: do not follow symlinks found below a root.
            next unless S_ISREG($stat[2]);
            next if $only_scala && $name !~ /\.scala\z/;
            next if $skip_generated_markers
                && ($name eq '.fixture-generated'
                    || $name =~ /\A\.fixture-generated\./);

            my $relative = relative_path($base, $path);
            $relative = normalize_generated_path($relative)
                if $normalize_generated;
            push @records, "$relative " . hash_file($path);
        }
    }
    return @records;
}

sub path_tail {
    my ($path) = @_;
    # Match zsh's ${path##*/}, including its empty result for a trailing '/'.
    $path =~ s{.*\/}{}s;
    return $path;
}

my $mode = shift @ARGV // fail('missing digest mode');

if ($mode eq 'source') {
    my $base = shift @ARGV // fail('source digest requires a base path');
    my @records;
    for my $path (@ARGV) {
        my @stat = stat($path);
        @stat && S_ISREG($stat[2])
            or fail("source file is missing or unreadable: $path");
        my $relative = relative_path($base, $path);
        $relative = normalize_generated_path($relative);
        push @records, "$relative " . hash_file($path);
    }
    print digest_records(\@records), "\n";
    exit 0;
}

if ($mode eq 'generated') {
    my $base = shift @ARGV // fail('generated digest requires a base path');
    my @records;
    for my $directory (@ARGV) {
        push @records, collect_tree($directory, $base, 1, 1, 1);
    }
    print digest_records(\@records), "\n";
    exit 0;
}

if ($mode eq 'output') {
    my $directory = shift @ARGV // fail('output digest requires a directory');
    my @records = collect_tree($directory, $directory, 0, 0);
    print digest_records(\@records), "\n";
    exit 0;
}

if ($mode eq 'classpath') {
    my @lines;
    push @lines, 'classpath_entries=' . join(':', map { path_tail($_) } @ARGV);
    for my $entry (@ARGV) {
        my @stat = lstat($entry)
            or fail("classpath entry is missing or unreadable: $entry");
        if (S_ISREG($stat[2])) {
            push @lines, 'file=' . path_tail($entry)
                . ' hash=' . hash_file($entry);
        } elsif (S_ISDIR($stat[2])) {
            my @records = collect_tree($entry, $entry, 0, 0);
            push @lines, 'dir=' . path_tail($entry)
                . ' hash=' . digest_records(\@records);
        } else {
            fail("classpath entry is not a regular file or directory: $entry");
        }
    }
    # The shell implementation hashes these lines in insertion order; unlike
    # file records, classpath entries are intentionally ordered.
    my $sha = Digest::SHA->new(256);
    $sha->add($_, "\n") for @lines;
    print $sha->hexdigest, "\n";
    exit 0;
}

fail("unknown digest mode '$mode'");
