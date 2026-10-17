import io, sys, os, re, json
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8')
base = r'D:\project\ai生成'
src = os.path.join(base, 'src')
app_path = os.path.join(src, 'App.tsx')
lines = open(app_path, encoding='utf-8').read().split('\n')
app_start = next(i for i, l in enumerate(lines) if l.startswith('function App()'))

# ── 声明表 ──
decls, pairs = {}, {}
i = app_start
while i < len(lines):
    l = lines[i]
    m = re.match(r'^  const \[(\w+)?, (\w+)\] = useState(<(.+)>)?\(', l) \
        or re.match(r'^  const \[(\w+)?, (\w+)\] = useState\(', l)
    if m:
        # **必须按括号配平算结束行**：useState 的初始值可能是多行对象/数组，
        # 只搬第一行会把后半截留在原地 → 两侧语法都坏（这个错已经踩过）。
        d = l.count('{') - l.count('}')
        par = l.count('(') - l.count(')')
        j = i
        while j + 1 < len(lines) and (d > 0 or par > 0):
            j += 1
            d += lines[j].count('{') - lines[j].count('}')
            par += lines[j].count('(') - lines[j].count(')')
        if m.group(1):
            decls[m.group(1)] = ('state', i, j)
            pairs[m.group(1)] = m.group(2)
        decls[m.group(2)] = ('setter', i, j)
        i = j + 1
        continue
    m = re.match(r'^  const (\w+)\s*[:=]', l)
    if m:
        d = l.count('{') - l.count('}'); par = l.count('(') - l.count(')')
        j = i
        while j + 1 < len(lines) and (d > 0 or par > 0):
            j += 1; d += lines[j].count('{') - lines[j].count('}'); par += lines[j].count('(') - lines[j].count(')')
        decls[m.group(1)] = ('const', i, j); i = j + 1; continue
    m = re.match(r'^  (?:async )?function (\w+)', l)
    if m:
        d = l.count('{') - l.count('}')
        j = i
        while j + 1 < len(lines) and d > 0:
            j += 1; d += lines[j].count('{') - lines[j].count('}')
        decls[m.group(1)] = ('func', i, j); i = j + 1; continue
    i += 1

GROUP_STATE = [
    'stories', 'currentStory', 'branches', 'currentBranchId',
    'chars', 'charUsage', 'charsLoading', 'relations', 'worlds', 'worldLoading',
    'timelineNodes', 'node', 'messages', 'proposals', 'requireApproval',
    'snapshots', 'auditLogs', 'eventLogs', 'debugList', 'selectedDebug',
    'evals', 'evalBusy', 'genList', 'genSel', 'cloudForm', 'pluginState',
    'recallDays', 'recallDay',
    'branchDialog', 'branchEdit', 'newTitle', 'newOrigin', 'showNew',
]
GROUP_FN = [
    'loadStories', 'loadChars', 'loadRelations', 'loadWorlds',
    'loadTimelineAndMessages', 'loadBranches', 'loadSnapshots', 'loadAudit',
    'loadEventLogs', 'loadProposals', 'loadDebug', 'loadEvals', 'loadGenerators',
    'loadCloudConfig', 'loadRequireApproval', 'loadRecallDays',
    'switchBranch', 'createBranch', 'removeBranch', 'openBranchDialog',
    'addNextDay', 'addInterlude', 'makeDaySummary', 'backfillSummaries',
    'autoSummarizeToday', 'switchNode', 'removeNode', 'ensureEditBackup',
    'refreshTimelineNodes', 'backupTakenRef', 'SUMMARY_MIN_REPLIES',
]
group = [n for n in GROUP_STATE + GROUP_FN if n in decls]
# 带上 setter
allnames = []
for n in GROUP_STATE:
    if n in decls:
        allnames.append(n)
        if n in pairs and pairs[n] in decls:
            allnames.append(pairs[n])
for n in GROUP_FN:
    if n in decls:
        allnames.append(n)

# 复杂成员的 setter 也被外部用（branchDialog/branchEdit）→ 一起搬
for extra in ['setBranchDialog', 'setBranchEdit', 'setNewTitle', 'setNewOrigin', 'setShowNew']:
    if extra in decls and extra not in allnames:
        allnames.append(extra)

# 加它们引用的常量的 setter 依赖（如 setChars/setRelations 等已被 state 覆盖）
allnames = sorted(set(allnames), key=lambda n: decls[n][1])

print('  搬移成员 %d 个' % len(allnames))
print('  ' + ', '.join(allnames))
json.dump({'members': allnames, 'decls': {k: list(v) for k, v in decls.items()}},
          open(os.path.join(base, 'temp', 'hook_members.json'), 'w', encoding='utf-8'), ensure_ascii=False)
