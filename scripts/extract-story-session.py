import io, sys, os, re, json
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8')
base = r'D:\project\ai生成'
src = os.path.join(base, 'src')
app_path = os.path.join(src, 'App.tsx')
lines = open(app_path, encoding='utf-8').read().split('\n')
meta = json.load(open(os.path.join(base, 'temp', 'hook_members.json'), encoding='utf-8'))
members = meta['members']
decls = {k: tuple(v) for k, v in meta['decls'].items()}

member_set = set(members)
ranges = sorted({(decls[n][1], decls[n][2], n) for n in members if n in decls})
# 合并重叠/相邻（相邻的变量声明一起搬更整齐）
merged = []
for a, b, n in ranges:
    if merged and a <= merged[-1][1]:
        merged[-1] = (merged[-1][0], max(merged[-1][1], b), merged[-1][2] + ',' + n)
    else:
        merged.append((a, b, n))

# 校验：合并后的区段里不能有非成员代码
covered_members = set()
for a, b, ns in merged:
    for n in ns.split(','):
        covered_members.add(n)
bad = []
for a, b, ns in merged:
    for ln in range(a, b + 1):
        # **严格**：这一行必须落在某个成员的 [s,e] 区间内
        in_member = any(decls[m][1] <= ln <= decls[m][2] for m in member_set if m in decls)
        if not in_member:
            bad.append((ln + 1, lines[ln].strip()[:70], ns[:30]))
if bad:
    print('  BLOCKED 区段里含非成员代码：')
    for ln, t, own in bad[:8]:
        print('     %d: %s (%s)' % (ln, t, own))
    sys.exit(1)
print('  区段 %d 段 / %d 行' % (len(merged), sum(b - a + 1 for a, b, _ in merged)))

body_text = '\n'.join('\n'.join(lines[a:b + 1]) for a, b, _ in merged)

# ── 外部依赖 ──
def refs(t):
    return {n for n in decls if re.search(r'\b' + re.escape(n) + r'\b', t)}
ext = sorted(refs(body_text) - member_set)
print('  外部依赖 %d 个：%s' % (len(ext), ', '.join(ext)))

# ── 常量导入（模块级）──
raw = open(app_path, encoding='utf-8').read()
imap = {}
for m in re.finditer(r'import\s+(?:type\s+)?\{([\s\S]*?)\}\s+from\s+"([^"]+)"', raw):
    for nm in m.group(1).split(','):
        nm = nm.strip().split(' as ')[0].strip()
        if nm:
            imap[nm] = m.group(2)
mods_used = {}
for nm, mod in imap.items():
    if nm in member_set or nm in ext:
        continue
    if re.search(r'\b' + re.escape(nm) + r'\b', body_text):
        mods_used.setdefault(mod.replace('./', '../'), []).append(nm)


# 模块级 const（在 App 文件顶层、不在 App() 内）→ 直接搬进 hook 文件
inline_consts = []
for nm in list(ext):
    m = re.search(r'(?m)^const ' + re.escape(nm) + r'\b[^\n]*', raw)
    if m:
        inline_consts.append(m.group(0))
        ext.remove(nm)
        print('  模块级常量随迁：' + nm)

# 外部依赖的类型（手工表）
EXT_TYPES = {
    'setMsg': 'Dispatch<SetStateAction<string>>',
    'setView': 'Dispatch<SetStateAction<"chars" | "world" | "relations" | "story" | "proposals" | "temp" | "timeline" | "stories" | "plugins" | "portrait" | "compliance" | "params" | "home">>',
    'setCharGraphRootId': 'Dispatch<SetStateAction<number | null>>',
    'setFocusCharId': 'Dispatch<SetStateAction<number | null>>',
    'setPluginFlag': '(id: string, patch: Partial<{ enabled: boolean; panelOpen: boolean }>) => void',
    'buildBoardContent': '(board: "characters" | "world") => string',
}
missing = [n for n in ext if n not in EXT_TYPES]
if missing:
    print('  MISSING_TYPES ' + ', '.join(missing))
    sys.exit(2)
deps_type = '\n'.join('  %s: %s;' % (n, EXT_TYPES[n]) for n in ext)

imp = ['import { useRef, useState, type Dispatch, type SetStateAction } from "react";']
for mod, nms in sorted(mods_used.items()):
    if mod == '__inline__':
        continue
    imp.append('import { %s } from "%s";' % (', '.join(sorted(set(nms))), mod))

hook = (('// 故事会话：数据加载 + 分支/节点操作（从 App.tsx 抽出，2026-10-17）。\n'
         '//\n'
         '// 为什么抽这一刀：这些 state 与加载器是 App 里最大的一坨（29 个 state + 20 多个异步加载器），\n'
         '// 而且彼此强耦合（加载时间线要带分支、切分支要重载全部）—— 留在 App 里会一直长。\n'
         '// 页面组件只接收它们需要的值/回调，不再直接碰这些内部状态。\n'
         '//\n'
         '// 参数（deps）只保留 3~5 个跨域回调：全局提示、视图跳转、少数几个页面的局部 setter。\n\n')
        + '\n'.join(imp) + '\n\n'
        + ('\n'.join(inline_consts) + '\n\n' if inline_consts else '')
        + 'export type StorySessionDeps = {\n' + deps_type + '\n};\n\n'
        + 'export function useStorySession(deps: StorySessionDeps) {\n'
        + '  const { ' + ', '.join(ext) + ' } = deps;\n\n'
        + body_text + '\n\n'
        + '  return {\n' + '\n'.join('    %s,' % n for n in members) + '\n  };\n}\n')

os.makedirs(os.path.join(src, 'hooks'), exist_ok=True)
open(os.path.join(src, 'hooks', 'useStorySession.ts'), 'w', encoding='utf-8', newline='\n').write(hook)
print('  OK 写 src/hooks/useStorySession.ts（%d 行）' % hook.count('\n'))

# ── App：删除成员区段，插入 hook 调用 + 解构 ──
for a, b, _ in sorted(merged, reverse=True):
    del lines[a:b + 1]
text = '\n'.join(lines)

call = ('  const session = useStorySession({ %s });\n'
        '  const { %s } = session;\n' % (', '.join(ext), ', '.join(members)))
anchor = next(k for k, l in enumerate(lines) if l.strip() == '// ===== 故事库（M1）=====')
# 插在声明区开头（紧跟函数开始后的第一段）
ins = next(k for k, l in enumerate(lines) if l.startswith('function App()')) + 1
while ins < len(lines) and lines[ins].strip() in ('', '{'):
    ins += 1
lines.insert(ins, call.rstrip('\n'))
text = '\n'.join(lines)
imp_line = 'import { useStorySession } from "./hooks/useStorySession";'
if imp_line not in text:
    ls = text.split('\n')
    at = next(k for k, l in enumerate(ls) if l.startswith('function App()'))
    while at > 0 and ls[at - 1].strip() == '':
        at -= 1
    ls.insert(at, imp_line)
    text = '\n'.join(ls)
open(app_path, 'w', encoding='utf-8', newline='\n').write(text)
print('  OK App 现在 %d 行' % (text.count('\n') + 1))
