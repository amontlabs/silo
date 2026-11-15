#!/usr/bin/env python3
"""Ratchet on the silo-engine module dependency knot.

Nodes are the engine's top-level modules (resolved from lib.rs, including
`#[path]` files). An edge A -> B exists when production code of A names
`crate::B`, `super::B` that reaches the crate root, or `silo_engine::B`.
Code under `cfg(test)` is ignored. The knot is the largest strongly connected
component of that graph. The committed baseline lists the knot's modules and
the edges between them; the check fails when the knot gains a module or an
edge, and asks for the baseline to be lowered when it shrinks.

    engine_cycle_ratchet.py            check; print the change's reach
    engine_cycle_ratchet.py --update   rewrite the baseline (only ever smaller)
    engine_cycle_ratchet.py --base REF measure reach against REF (default: merge base with main)
"""
import argparse
import json
import os
import re
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
REPO = SCRIPTS.parents[2]
ENGINE_SRC = Path('app/SiloUI/src-tauri/engine/src')
BASELINE = SCRIPTS / 'engine-cycle-baseline.json'
TOKEN = re.compile(r'''
    (?P<line>//[^\n]*)|(?P<block>/\*)|(?P<raw>b?r(?P<hashes>\#*)")|(?P<str>b?")
  | (?P<char>'(?:\\[^']*|[^\\'])')|(?P<life>'[A-Za-z_]\w*)|(?P<id>[A-Za-z_]\w*)
  | (?P<num>[0-9][\w.]*)|(?P<colons>::)|(?P<nl>\n)|(?P<ws>\s+)|(?P<p>.)''', re.X)


def tokenize(text):
    """Return (kind, text, line) tokens; comments, chars and lifetimes are dropped."""
    out, i, line, n = [], 0, 1, len(text)
    while i < n:
        m = TOKEN.match(text, i)
        kind = m.lastgroup
        if kind == 'hashes':
            kind = 'raw'
        if kind == 'line':
            i = m.end()
        elif kind == 'block':
            depth, i = 1, m.end()
            while i < n and depth:
                if text.startswith('/*', i):
                    depth, i = depth + 1, i + 2
                elif text.startswith('*/', i):
                    depth, i = depth - 1, i + 2
                else:
                    line += text[i] == '\n'
                    i += 1
        elif kind in ('raw', 'str'):
            if kind == 'raw':
                end, start = '"' + m.group('hashes'), m.end()
                j = text.find(end, start)
                j = n if j < 0 else j
                nxt = j + len(end)
            else:
                start, j = m.end(), m.end()
                while j < n and text[j] != '"':
                    j += 2 if text[j] == '\\' else 1
                nxt = j + 1
            out.append(('str', text[start:j], line))
            line += text.count('\n', i, j)
            i = nxt
        elif kind == 'nl':
            line += 1
            i = m.end()
        elif kind in ('char', 'life', 'num', 'ws'):
            i = m.end()
        else:
            out.append(('id' if kind == 'id' else 'p', m.group(), line))
            i = m.end()
    return out


def attribute(toks, i):
    """toks[i] is '#': return (index after the attribute, its tokens) or None."""
    j = i + 1
    if j < len(toks) and toks[j][1] == '!':
        j += 1
    if j >= len(toks) or toks[j][1] != '[':
        return None
    depth, k = 0, j
    while k < len(toks):
        depth += (toks[k][1] == '[') - (toks[k][1] == ']')
        if depth == 0:
            break
        k += 1
    return k + 1, toks[j + 1:k]


def is_test_cfg(attr):
    ids = [t[1] for t in attr]
    if not ids or ids[0] != 'cfg':
        return False
    return any(t == 'test' and not (k >= 2 and ids[k - 1] == '(' and ids[k - 2] == 'not')
               for k, t in enumerate(ids))


def path_attribute(attr):
    if len(attr) >= 3 and attr[0][1] == 'path' and attr[2][0] == 'str':
        return attr[2][1]
    return None


class Source:
    def __init__(self, src):
        self.src = src
        self.files = {}  # path -> (module path, is test file)
        self.walk(src / 'lib.rs', [], False)

    def walk(self, path, modpath, is_test):
        if path in self.files or not path.exists():
            return
        self.files[path] = (modpath, is_test)
        toks = tokenize(path.read_text(encoding='utf-8', errors='replace'))
        childdir = path.parent if path.stem in ('mod', 'lib', 'main') else path.parent / path.stem
        inline, brace, pend_path, pend_test, i = [], 0, None, False, 0
        while i < len(toks):
            kind, t, _ = toks[i]
            if t == '#' and (r := attribute(toks, i)):
                pend_path = path_attribute(r[1]) or pend_path
                pend_test = pend_test or is_test_cfg(r[1])
                i = r[0]
                continue
            if t == '{':
                brace, pend_path, pend_test = brace + 1, None, False
            elif t == '}':
                brace -= 1
                while inline and inline[-1][1] > brace:
                    inline.pop()
            elif t == ';':
                pend_path, pend_test = None, False
            elif t == 'mod' and kind == 'id' and i + 2 < len(toks) and toks[i + 1][0] == 'id':
                name, nxt = toks[i + 1][1], toks[i + 2][1]
                names = [x[0] for x in inline]
                if nxt == ';':
                    if pend_path:
                        base = path.parent if not inline else childdir
                        child = Path(os.path.normpath(base / pend_path))
                    else:
                        sub = childdir.joinpath(*names)
                        child = sub / f'{name}.rs'
                        if not child.exists():
                            child = sub / name / 'mod.rs'
                    self.walk(child, modpath + names + [name], is_test or pend_test)
                    pend_path, pend_test = None, False
                    i += 3
                    continue
                if nxt == '{':
                    inline.append((name, brace + 1))
                    brace += 1
                    pend_path, pend_test = None, False
                    i += 3
                    continue
            i += 1


def use_paths(toks, i, prefix):
    """Parse the use tree starting at toks[i]; return (paths, next index)."""
    pre, res = list(prefix), []
    while i < len(toks):
        kind, t, _ = toks[i]
        if kind == 'id' and t != 'as':
            pre.append(t)
            i += 1
            if i < len(toks) and toks[i][1] == '::':
                i += 1
                continue
            return [pre], i
        if t == '*':
            return [pre + ['*']], i + 1
        if t == '{':
            i += 1
            while i < len(toks) and toks[i][1] != '}':
                sub, i = use_paths(toks, i, pre)
                res.extend(sub)
                depth = 0
                while i < len(toks) and not (depth == 0 and toks[i][1] in (',', '}')):
                    depth += (toks[i][1] == '{') - (toks[i][1] == '}')
                    i += 1
                if i < len(toks) and toks[i][1] == ',':
                    i += 1
            return res, i + 1
        break
    return ([pre] if pre != list(prefix) else []), i


def file_references(toks, modpath, is_test_file):
    """Absolute module paths named by production code of one file, as (path, line)."""
    refs, inline, blocks, brace, pend_test, i = [], [], [], 0, False, 0
    n = len(toks)
    while i < n:
        kind, t, line = toks[i]
        if t == '#' and (r := attribute(toks, i)):
            pend_test = pend_test or is_test_cfg(r[1])
            i = r[0]
            continue
        if t == '{':
            brace += 1
            if pend_test:
                blocks.append(brace)
            pend_test = False
            i += 1
            continue
        if t == '}':
            while blocks and blocks[-1] >= brace:
                blocks.pop()
            while inline and inline[-1][1] >= brace:
                inline.pop()
            brace -= 1
            i += 1
            continue
        if t == ';':
            pend_test = False
        if t == 'mod' and kind == 'id' and i + 2 < n and toks[i + 1][0] == 'id' and toks[i + 2][1] == '{':
            inline.append((toks[i + 1][1], brace + 1, pend_test))
            pend_test = False
            brace += 1
            i += 3
            continue
        after_path = i > 0 and toks[i - 1][1] == '::'
        start, j = None, i
        if kind == 'id' and t in ('crate', 'silo_engine') and i + 1 < n and toks[i + 1][1] == '::' and not after_path:
            start, j = [], i + 2
        elif kind == 'id' and t == 'super' and not after_path:
            ups = 0
            while j + 1 < n and toks[j][1] == 'super' and toks[j + 1][1] == '::':
                ups, j = ups + 1, j + 2
            here = modpath + [x[0] for x in inline]
            if ups <= len(here):
                start = here[:len(here) - ups]
        if start is None:
            i += 1
            continue
        paths, nxt = use_paths(toks, j, [])
        testing = is_test_file or any(x[2] for x in inline) or bool(blocks)
        if not testing:
            refs.extend((start + p, line) for p in paths if start + p)
        i = max(nxt, i + 1)
    return refs


def root_aliases(lib_toks, decls):
    """Names usable at the crate root that stand for a top-level module."""
    alias = {d: d for d in decls}
    i = 0
    while i < len(lib_toks):
        if lib_toks[i][1] == 'use' and (i == 0 or lib_toks[i - 1][1] in ('pub', ';', '}', ']', ')')):
            j = i + 1
            while lib_toks[j][1] != ';':
                j += 1
            seg = [t[1] for t in lib_toks[i + 1:j]]
            if 'as' in seg and '{' not in seg and seg[0] in alias:
                alias[seg[-1]] = alias[seg[0]]
            i = j
        i += 1
    return alias


def scan(root):
    """Return {'edges': {a: {b: [site]}}, 'lines': {node: n}, 'files': {relpath: node}} for the engine under root."""
    src = root / ENGINE_SRC
    tree = Source(src)
    lib_toks = tokenize((src / 'lib.rs').read_text(encoding='utf-8'))
    node_of_decl = {mp[0]: mp[0] for p, (mp, _) in tree.files.items() if len(mp) == 1}
    # a `#[path]`-renamed declaration is named by its file, as the engine's own layout is
    for p, (mp, _) in tree.files.items():
        if len(mp) == 1:
            node_of_decl[mp[0]] = p.parent.name if p.stem == 'mod' else p.stem
    alias = {k: node_of_decl[v] for k, v in root_aliases(lib_toks, list(node_of_decl)).items()}
    alias.update(node_of_decl)
    edges = defaultdict(lambda: defaultdict(list))
    lines, files = defaultdict(int), {}
    for path, (modpath, is_test_file) in tree.files.items():
        if not modpath:
            continue
        node = alias[modpath[0]]
        text = path.read_text(encoding='utf-8', errors='replace')
        rel = str(path.relative_to(root))
        files[rel] = node
        lines[node] += text.count('\n') + 1
        for full, line in file_references(tokenize(text), modpath, is_test_file):
            target = alias.get(full[0])
            if target and target != node:
                sites = edges[node][target]
                if len(sites) < 3:
                    sites.append(f'{rel}:{line}')
    return {'edges': {a: dict(b) for a, b in edges.items()}, 'lines': dict(lines), 'files': files}


def largest_scc(nodes, succ):
    index, low, stack, on, comps, counter = {}, {}, [], set(), [], [0]

    def visit(v):
        index[v] = low[v] = counter[0]
        counter[0] += 1
        stack.append(v)
        on.add(v)
        for w in sorted(succ.get(v, ())):
            if w not in index:
                visit(w)
                low[v] = min(low[v], low[w])
            elif w in on:
                low[v] = min(low[v], index[w])
        if low[v] == index[v]:
            comp = []
            while True:
                w = stack.pop()
                on.discard(w)
                comp.append(w)
                if w == v:
                    break
            comps.append(comp)

    sys.setrecursionlimit(10000)
    for v in sorted(nodes):
        if v not in index:
            visit(v)
    return sorted(max(comps, key=len)) if comps else []


def knot(graph):
    succ = {a: set(b) for a, b in graph['edges'].items()}
    nodes = set(graph['lines'])
    members = largest_scc(nodes, succ)
    inside = set(members)
    pairs = sorted(f'{a} -> {b}' for a in members for b in succ.get(a, ()) if b in inside)
    return members, pairs


def dependents(graph, changed):
    """Modules that transitively depend on any of `changed`, including them."""
    rev = defaultdict(set)
    for a, targets in graph['edges'].items():
        for b in targets:
            rev[b].add(a)
    seen, todo = set(changed), list(changed)
    while todo:
        for d in rev.get(todo.pop(), ()):
            if d not in seen:
                seen.add(d)
                todo.append(d)
    return seen


def changed_files(base):
    if base is None:
        merged = subprocess.run(['git', 'merge-base', 'HEAD', 'main'], cwd=REPO, capture_output=True, text=True)
        base = merged.stdout.strip() or 'HEAD'
    result = subprocess.run(['git', 'diff', '--name-only', base], cwd=REPO, capture_output=True, text=True)
    return [line for line in result.stdout.splitlines() if line]


def reach_report(graph, files):
    changed = {graph['files'][f] for f in files if f in graph['files']}
    if not changed:
        return 'reach: the change touches no engine module'
    affected = dependents(graph, changed)
    total = sum(graph['lines'].values())
    lines = sum(graph['lines'].get(n, 0) for n in affected)
    count = len(graph['lines'])
    return (f'reach: changed {", ".join(sorted(changed))}; {len(affected)}/{count} modules '
            f'({100 * len(affected) / count:.0f}%) and {lines}/{total} lines ({100 * lines / total:.0f}%) depend on them')


def compare(baseline, members, pairs, graph):
    problems = []
    for name in sorted(set(members) - set(baseline['modules'])):
        problems.append(f'module `{name}` joined the knot')
    for pair in sorted(set(pairs) - set(baseline['edges'])):
        a, b = pair.split(' -> ')
        sites = ', '.join(graph['edges'][a][b][:3])
        problems.append(f'new edge inside the knot: {pair} ({sites})')
    return problems


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--update', action='store_true', help='rewrite the baseline from the current tree')
    parser.add_argument('--base', help='git ref for the reach report')
    parser.add_argument('--root', type=Path, default=REPO)
    parser.add_argument('--baseline', type=Path, default=BASELINE)
    args = parser.parse_args(argv)
    graph = scan(args.root)
    members, pairs = knot(graph)
    current = {'modules': members, 'edges': pairs}
    if args.update:
        old = json.loads(args.baseline.read_text()) if args.baseline.exists() else None
        if old and compare(old, members, pairs, graph):
            print('refusing to raise the baseline: the knot grew', file=sys.stderr)
            for p in compare(old, members, pairs, graph):
                print(f'  {p}', file=sys.stderr)
            return 1
        args.baseline.write_text(json.dumps(current, indent=1) + '\n')
        print(f'baseline written: {len(members)} modules, {len(pairs)} edges')
        return 0
    baseline = json.loads(args.baseline.read_text())
    print(f'engine knot: {len(members)} modules, {len(pairs)} edges (baseline {len(baseline["modules"])}/{len(baseline["edges"])})')
    print(reach_report(graph, changed_files(args.base) if args.root == REPO else []))
    problems = compare(baseline, members, pairs, graph)
    if problems:
        print('The engine dependency knot grew:', file=sys.stderr)
        for p in problems:
            print(f'  {p}', file=sys.stderr)
        print('Depend on the other module through a smaller module or a trait instead of closing the cycle; '
              'the baseline only ever shrinks.', file=sys.stderr)
        return 1
    if set(baseline['modules']) - set(members) or set(baseline['edges']) - set(pairs):
        print('The knot shrank. Lower the baseline: python3 app/SiloUI/scripts/engine_cycle_ratchet.py --update '
              'and commit app/SiloUI/scripts/engine-cycle-baseline.json')
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
