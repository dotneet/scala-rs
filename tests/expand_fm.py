#!/usr/bin/env python3
"""Expand slick's FreeMarker templates into the Scala sources sbt generates.

Slick keeps `GetResult`, `SetParameter`, `TupleSupport` and four others as
`.fm` templates that its build expands; the checked-out tree has no `.scala`
for them, so a measurement over the checked-in sources alone reports errors
scalac would report too. This covers the directives those seven files use --
numeric and list `<#list>`, `<#if>`, `${...}` and one `<#assign>` -- and
nothing else: an unknown directive is an error, never silently dropped.
"""

import re
import sys
from pathlib import Path

DIRECTIVE = re.compile(r"<#(list|if|assign)\b|</#(list|if)>|<#else>")


def value(expr, env):
    expr = expr.strip()
    if expr.endswith("?lower_case"):
        return str(value(expr[: -len("?lower_case")], env)).lower()
    m = re.fullmatch(r"(\w+)\s*-\s*(\d+)", expr)
    if m:
        return int(value(m.group(1), env)) - int(m.group(2))
    if expr.isdigit():
        return int(expr)
    if expr in env:
        return env[expr]
    raise SystemExit(f"expand_fm: cannot evaluate {expr!r}")


def truth(cond, env):
    m = re.fullmatch(r"(.+?)\s*!=\s*(.+)", cond.strip())
    if not m:
        raise SystemExit(f"expand_fm: cannot evaluate condition {cond!r}")
    return value(m.group(1), env) != value(m.group(2), env)


def interpolate(text, env):
    return re.sub(r"\$\{([^}]*)\}", lambda m: str(value(m.group(1), env)), text)


def find_close(text, start, open_tag, close_tag):
    """Offset of the `close_tag` matching the tag that opens at `start`."""
    depth = 0
    i = start
    while i < len(text):
        if text.startswith(open_tag, i):
            depth += 1
            i += len(open_tag)
        elif text.startswith(close_tag, i):
            depth -= 1
            if depth == 0:
                return i
            i += len(close_tag)
        else:
            i += 1
    raise SystemExit(f"expand_fm: unclosed {open_tag}")


def expand(text, env):
    out = []
    i = 0
    while True:
        m = DIRECTIVE.search(text, i)
        if not m:
            out.append(interpolate(text[i:], env))
            return "".join(out)
        out.append(interpolate(text[i : m.start()], env))
        tag_end = text.index(">", m.start())
        head = text[m.start() + 2 : tag_end]
        if head.startswith("assign"):
            name, _, rhs = head[len("assign") :].partition("=")
            env[name.strip()] = eval(rhs.strip())  # a list literal, nothing else
            i = tag_end + 1
        elif head.startswith("list"):
            close = find_close(text, m.start(), "<#list", "</#list>")
            body = text[tag_end + 1 : close]
            spec = head[len("list") :].strip()
            var = spec.rsplit(" as ", 1)[1].strip()
            src = spec.rsplit(" as ", 1)[0].strip()
            if ".." in src:
                lo, hi = src.split("..", 1)
                items = range(value(lo, env), value(hi, env) + 1)
            else:
                items = value(src, env)
            for it in items:
                out.append(expand(body, {**env, var: it}))
            i = close + len("</#list>")
        else:  # if
            close = find_close(text, m.start(), "<#if", "</#if>")
            body = text[tag_end + 1 : close]
            then, sep, other = body.partition("<#else>")
            chosen = then if truth(head[len("if") :], env) else (other if sep else "")
            out.append(expand(chosen, env))
            i = close + len("</#if>")


def main():
    if len(sys.argv) != 3:
        raise SystemExit("usage: expand_fm.py <slick-src-dir> <out-dir>")
    src, out = Path(sys.argv[1]), Path(sys.argv[2])
    n = 0
    for fm in sorted(src.rglob("*.fm")):
        rel = fm.relative_to(src).with_suffix(".scala")
        dest = out / rel
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_text(expand(fm.read_text(), {}))
        n += 1
    print(f"expanded {n} templates into {out}")


if __name__ == "__main__":
    main()
