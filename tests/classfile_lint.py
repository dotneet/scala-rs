#!/usr/bin/env python3
"""Check emitted class files for jumps and code lengths that cannot be right.

Why this exists: the checks we already run overlap more than they look.
`Class.forName(initialize=false)` and `javap` both stop after parsing the
constant pool, so a method body is never looked at; only `slick_run.sh` and
`tests/conform/` execute anything, and they only reach the classes a program
actually calls. Between those, a whole class of defect -- an offset that
wrapped on its way into the file -- reaches nobody until someone runs the
method. Two `CONSTANT_Utf8` constants once wrapped past 65535 that way, and a
branch offset cast to `i16` shipped `ifeq -7611` in a 57 KB method.

This reads `javap -c -p`, which prints every branch target as an absolute
offset, and reports:

  * a branch whose target is outside its own method (a wrapped offset shows up
    as a negative or absurd target, which is exactly what the bug produced);
  * a method whose code is 65536 bytes or longer, which JVMS 4.7.3 forbids and
    a class loader rejects while parsing.

It is a *structural* check, not a verifier: it says nothing about types, stack
depth, or whether a branch target carries a stack map frame.

    tests/classfile_lint.py <dir-of-class-files>...
"""

import os
import re
import subprocess
import sys

# JVMS 6.5: every branch but `goto_w`/`jsr_w` carries a signed 16-bit offset.
BRANCHES = {
    "ifeq", "ifne", "iflt", "ifge", "ifgt", "ifle",
    "if_icmpeq", "if_icmpne", "if_icmplt", "if_icmpge", "if_icmpgt", "if_icmple",
    "if_acmpeq", "if_acmpne", "ifnull", "ifnonnull",
    "goto", "goto_w", "jsr", "jsr_w",
}

# JVMS 4.7.3: `code_length` "must be less than 65536".
MAX_CODE_LENGTH = 65535

INSN = re.compile(r"^\s+(\d+): (\S+)(?:\s+(.*))?$")
SWITCH_CASE = re.compile(r"^\s+(?:default|-?\d+): (-?\d+)\s*$")
MEMBER = re.compile(r"^  \S.*;$")
DECL = re.compile(r"^\S.*\b(?:class|interface)\s+(\S+)")
SWITCHES = {"tableswitch", "lookupswitch"}


def classes_under(root):
    for dirpath, _, names in os.walk(root):
        for n in names:
            if n.endswith(".class"):
                yield os.path.join(dirpath, n)


def javap(paths):
    out = subprocess.run(
        ["javap", "-c", "-p"] + paths,
        capture_output=True, text=True,
    )
    return out.stdout


def check(text, problems):
    """Collect every method's instructions, then check its branch targets."""
    owner = "?"
    member = "?"
    insns = []          # (pc, mnemonic, target or None)
    in_switch = False

    def flush():
        if not insns:
            return
        end = max(pc for pc, _, _ in insns)
        if end >= MAX_CODE_LENGTH:
            problems.append(
                f"{owner}.{member}: code is at least {end + 1} bytes, "
                f"over the {MAX_CODE_LENGTH}-byte limit (JVMS 4.7.3)"
            )
        for pc, op, target in insns:
            if target is None:
                continue
            if not 0 <= target <= end:
                problems.append(
                    f"{owner}.{member}: `{op}` at {pc} jumps to {target}, "
                    f"outside the method (0..{end})"
                )

    switch_pc = 0
    for line in text.splitlines():
        # A switch spans several lines; its cases are read below, and nothing
        # else in the block is an instruction. The `}` that closes it is at the
        # start of its own (indented) line -- but so is the `}` that closes the
        # class, so leave the block on any line that is not a case.
        if in_switch:
            m = SWITCH_CASE.match(line)
            if m:
                insns.append((switch_pc, "switch case", int(m.group(1))))
                continue
            in_switch = False
            if line.strip() == "}":
                continue
        decl = DECL.match(line)
        if decl:
            flush()
            insns = []
            owner, member = decl.group(1), "?"
            continue
        if MEMBER.match(line):
            flush()
            insns = []
            member = line.strip().rstrip(";")
            continue
        m = INSN.match(line)
        if not m:
            continue
        pc, op, rest = int(m.group(1)), m.group(2), (m.group(3) or "")
        insns.append((pc, op, None))
        if op in SWITCHES:
            # Not "the line ends in `{`": `ldc_w // String \{` does too.
            in_switch, switch_pc = True, pc
        elif op in BRANCHES:
            word = rest.split()[0] if rest.split() else ""
            try:
                insns[-1] = (pc, op, int(word))
            except ValueError:
                problems.append(f"{owner}.{member}: cannot read `{op} {rest}` at {pc}")
    flush()


def main(argv):
    if len(argv) < 2:
        print(__doc__.strip().splitlines()[-1], file=sys.stderr)
        return 2
    paths = []
    for root in argv[1:]:
        paths.extend(sorted(classes_under(root)) if os.path.isdir(root) else [root])
    if not paths:
        print("classfile_lint: no class files", file=sys.stderr)
        return 2
    problems = []
    # `javap` takes many files at once; chunk so the argument list stays sane.
    for i in range(0, len(paths), 200):
        check(javap(paths[i:i + 200]), problems)
    for p in problems:
        print("BAD " + p)
    print(f"lint_classes={len(paths)} lint_problems={len(problems)}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
