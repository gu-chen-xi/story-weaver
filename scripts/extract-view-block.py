"""精简版块提取器：把一个 `{view === "X" && (...)}` 块搬成组件。

与旧版的区别：**只搬视图**，state 与处理函数留在 App 当 props。
（旧版的"闭包 + 可搬判定"在搬页面时更有用；对纯 UI 块反而多余。）

用法：python extract_block.py <viewKey> <OutFile> <CompName> [blockLine] [typesJson]
"""
import io, sys, os, re, json, subprocess

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8')
BASE = r'D:\project\ai生成'
SRC = os.path.join(BASE, 'src')
APP = os.path.join(SRC, 'App.tsx')


def main():
    view_key, out_name, comp_name = sys.argv[1], sys.argv[2], sys.argv[3]
    block_line = int(sys.argv[4]) if len(sys.argv) > 4 and sys.argv[4].isdigit() else None
    TYPES = json.loads(sys.argv[5]) if len(sys.argv) > 5 else {}

    text = open(APP, encoding='utf-8').read()
    lines = text.split('\n')
    app_start = next(i for i, l in enumerate(lines) if l.startswith('function App()'))

    # ── 声明表 ──
    decls = {}
    i = app_start
    while i < len(lines):
        l = lines[i]
        m = re.match(r'^  const \[(\w+)?, (\w+)\] = useState(<(.+)>)?\(', l) \
            or re.match(r'^  const \[(\w+)?, (\w+)\] = useState\(', l)
        if m:
            if m.group(1):
                decls[m.group(1)] = ('state', i, m.group(3))
            decls[m.group(2)] = ('setter', i, None)
            i += 1
            continue
        m = re.match(r'^  const (\w+)\s*[:=]', l)
        if m:
            d = l.count('{') - l.count('}')
            par = l.count('(') - l.count(')')
            j = i
            while j + 1 < len(lines) and (d > 0 or par > 0):
                j += 1
                d += lines[j].count('{') - lines[j].count('}')
                par += lines[j].count('(') - lines[j].count(')')
            decls[m.group(1)] = ('const', i, None)
            i = j + 1
            continue
        m = re.match(r'^  (?:async )?function (\w+)', l)
        if m:
            d = l.count('{') - l.count('}')
            j = i
            while j + 1 < len(lines) and d > 0:
                j += 1
                d += lines[j].count('{') - lines[j].count('}')
            decls[m.group(1)] = ('func', i, None)
            i = j + 1
            continue
        i += 1

    # ── 找块 ──
    pat = re.compile(r'\{\s*\(?\s*view === "' + re.escape(view_key) + r'"')
    starts = [k for k, l in enumerate(lines) if pat.search(l)]
    if block_line:
        want = block_line - 1
        if want not in starts:
            print('  BLOCK_NOT_FOUND 可选：%s' % [s + 1 for s in starts])
            sys.exit(1)
        start = want
    else:
        start = starts[-1]
    depth = 0
    started = False
    end = None
    for k in range(start, len(lines)):
        depth += lines[k].count('{') - lines[k].count('}')
        if '{' in lines[k]:
            started = True
        if started and depth == 0:
            end = k
            break
    block = lines[start:end + 1]
    print('  块：行 %d~%d（%d 行）' % (start + 1, end + 1, len(block)))

    body_text = '\n'.join(block)
    no_tags = re.sub(r'</?(?:input|textarea|select|option|label|button|div|span|p|h[1-6]|img|svg|table|tr|td|th|ul|li|section|form|a|b|i|pre|code|hr|br|strong|em|small|tbody|thead)\b', '', body_text)

    # ── props ──
    def state_type(n):
        d = lines[decls[n][1]]
        m = re.search(r'useState<(.+)>\(', d)
        if m:
            return m.group(1)
        if re.search(r'useState\(false\)|useState\(true\)', d):
            return 'boolean'
        if re.search(r'useState\(-?\d', d):
            return 'number'
        if re.search(r'useState\("', d):
            return 'string'
        if 'new Set()' in d:
            return 'Set<unknown>'
        mo = re.search(r'useState\(\{(.*)\}\)', d)
        if mo:
            parts = []
            for f in [x.strip() for x in mo.group(1).split(',') if x.strip()]:
                if ':' not in f:
                    return 'Record<string, unknown>'
                kk, vv = f.split(':', 1)
                vv = vv.strip()
                ty = 'string' if vv[:1] in ('"', "'") else ('number' if re.fullmatch(r'-?\d+(\.\d+)?', vv) else 'unknown')
                parts.append('%s: %s' % (kk.strip(), ty))
            return '{ ' + '; '.join(parts) + ' }'
        return 'unknown'

    props, unknown = [], []
    for n in sorted(decls):
        if not re.search(r'\b' + re.escape(n) + r'\b', no_tags):
            continue
        kind = decls[n][0]
        if n in TYPES:
            props.append((n, TYPES[n]))
        elif kind == 'state':
            ty = state_type(n)
            props.append((n, ty))
            if ty == 'unknown':
                unknown.append(n)
        elif kind == 'setter':
            cand = n[3].lower() + n[4:]
            ty = state_type(cand) if cand in decls and decls[cand][0] == 'state' else 'unknown'
            props.append((n, 'Dispatch<SetStateAction<%s>>' % ty))
            if ty == 'unknown':
                unknown.append(n)
        else:
            props.append((n, 'unknown'))
            unknown.append(n)
    print('  props %d 个%s' % (len(props), ('；类型未知：%s' % unknown) if unknown else ''))

    # ── imports ──
    imap = {}
    for m in re.finditer(r'import\s+(?:type\s+)?\{([\s\S]*?)\}\s+from\s+"([^"]+)"', text):
        for nm in m.group(1).split(','):
            nm = nm.strip().split(' as ')[0].strip()
            if nm:
                imap[nm] = m.group(2)
    used = {}
    for nm, mod in imap.items():
        if re.search(r'\b' + re.escape(nm) + r'\b', no_tags):
            used.setdefault(mod.replace('./', '../'), []).append(nm)
    imp = ['import type { Dispatch, SetStateAction } from "react";']
    for mod, nms in sorted(used.items()):
        imp.append('import { %s } from "%s";' % (', '.join(sorted(set(nms))), mod))

    first = re.sub(r'^\s*\{\s*\(?\s*view === "' + re.escape(view_key) + r'"?\)?\s*&&\s*\(?', '(', block[0].rstrip())
    last = block[-1].rstrip()[:-1]
    inner = [l[2:] if l.startswith('  ') else l for l in ([first] + block[1:-1] + [last])]

    comp = ('// 「%s」（从 App.tsx 拆分，2026-10-17）。\n'
            '// 只搬视图：state 与处理函数仍在 App，通过 props 传入。\n\n' % comp_name) + \
           '\n'.join(imp) + '\n\n' + \
           'export type %sProps = {\n' % comp_name + \
           '\n'.join('  %s: %s;' % (n, t) for n, t in props) + '\n};\n\n' + \
           'export function %s(props: %sProps) {\n  const { %s } = props;\n  return (\n' % (
               comp_name, comp_name, ', '.join(n for n, _ in props)) + \
           '\n'.join(inner) + '\n  );\n}\n'
    open(os.path.join(SRC, out_name), 'w', encoding='utf-8', newline='\n').write(comp)
    print('  OK 写 src/%s（%d 行）' % (out_name, comp.count('\n')))

    # ── 原文精确替换 ──
    original = '\n'.join(block)
    args = '\n'.join('            %s={%s}' % (n, n) for n, _ in props)
    repl = ('        {view === "%s" && (\n          <%s\n' % (view_key, comp_name)) + args + '\n          />\n        )}'
    assert original in text, 'block not found after compute'
    text = text.replace(original, repl, 1)
    imp_line = 'import { %s } from "./%s";' % (comp_name, out_name.replace('.tsx', ''))
    if imp_line not in text:
        ls = text.split('\n')
        at = next(k for k, l in enumerate(ls) if l.startswith('function App()'))
        while at > 0 and ls[at - 1].strip() == '':
            at -= 1
        ls.insert(at, imp_line)
        text = '\n'.join(ls)
    open(APP, 'w', encoding='utf-8', newline='\n').write(text)
    print('  OK App 现在 %d 行' % (text.count('\n') + 1))


main()
