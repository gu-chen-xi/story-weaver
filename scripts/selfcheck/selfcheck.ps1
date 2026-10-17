# ============================================================
#  织梦机 · 启动自检（selfcheck.ps1）
#
#  只报告**观测到的事实**，不猜"应该没问题"。
#  由 自检.cmd 用 -ExecutionPolicy Bypass 拉起。
#
#  设计原则（每一条都是踩过的坑换来的）：
#   1. "可写"必须是**真写了一个文件**，不是看目录在不在
#   2. "窗口正常"必须是**枚举到窗口且尺寸合理**，不是看进程活不活
#   3. 检查项自己失败时要明说"没测成"（SKIP），**不能默认算通过**
#   4. 只查**静默失败**类问题；通用网络/端口占用不做（应用不监听端口）
# ============================================================

param(
    # 可选：指定要检查的 exe。不给就用项目的 target\release\app.exe。
    # 用途一：自证（把同一个 exe 放到安全路径再测一次，应该从 FAIL 变 PASS）
    # 用途二：检查别处的安装
    [string]$ExePath = ''
)

$ErrorActionPreference = 'Continue'

$script:Results  = New-Object System.Collections.ArrayList
$script:ProjRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$RELEASE  = Join-Path $script:ProjRoot 'src-tauri\target\release\app.exe'
$script:ExplicitMissing = $false
if ($ExePath) {
    # 显式指定时优先；**给错了就明说**，不要偷偷回退到别的 exe
    if (-not (Test-Path -LiteralPath $ExePath)) { $script:ExplicitMissing = $true }
    $RELEASE = $ExePath
}
$DEBUGEXE = Join-Path $script:ProjRoot 'src-tauri\target\debug\app.exe'
$DATA_DIR = Join-Path $script:ProjRoot 'data'
$OUT_DIR  = Join-Path $script:ProjRoot 'debug-diagnostics'
$REPORT   = Join-Path $OUT_DIR ('selfcheck-{0}.txt' -f (Get-Date -Format 'yyyyMMdd-HHmmss'))

# 已知危险的目录名：实测目录名恰好是 "ai生成" 时 Tauri 无法创建主窗口，
# 而 "ai生成x" / "a生成" / "生成" / "中文" / 纯英文 全部正常。
$DANGEROUS_DIR_NAMES = @('ai生成')

function Write-Line { param([string]$Text, [string]$Color = 'Gray')
    Write-Host $Text -ForegroundColor $Color
    if ($script:Buf) { [void]$script:Buf.Add($Text) }
}

function Add-Result {
    param(
        [string]$Name,
        [ValidateSet('PASS','FAIL','WARN','SKIP')][string]$Status,
        [string]$Detail = '',
        [string]$Why = ''
    )
    [void]$script:Results.Add([pscustomobject]@{ Name=$Name; Status=$Status; Detail=$Detail; Why=$Why })
    $icon = switch ($Status) { 'PASS' { '[ OK ]' } 'FAIL' { '[FAIL]' } 'WARN' { '[WARN]' } 'SKIP' { '[SKIP]' } }
    $color = switch ($Status) { 'PASS' { 'Green' } 'FAIL' { 'Red' } 'WARN' { 'Yellow' } 'SKIP' { 'DarkGray' } }
    Write-Host ("  {0} {1}" -f $icon, $Name) -ForegroundColor $color
    if ($script:Buf) { [void]$script:Buf.Add(("  {0} {1}" -f $icon, $Name)) }
    if ($Detail) {
        Write-Host ("         {0}" -f $Detail) -ForegroundColor DarkGray
        if ($script:Buf) { [void]$script:Buf.Add(("         {0}" -f $Detail)) }
    }
    if ($Why) {
        Write-Host ("         为什么查：{0}" -f $Why) -ForegroundColor DarkYellow
        if ($script:Buf) { [void]$script:Buf.Add(("         为什么查：{0}" -f $Why)) }
    }
}

function Section {
    param([string]$Title)
    Write-Host ""
    Write-Host ("=== {0} ===" -f $Title) -ForegroundColor Cyan
    if ($script:Buf) { [void]$script:Buf.Add(''); [void]$script:Buf.Add(("=== {0} ===" -f $Title)) }
}

# ---------- 探针：真写一个文件才算可写 ----------
function Test-DirWritable {
    param([string]$Path)
    try {
        if (-not (Test-Path -LiteralPath $Path)) {
            New-Item -ItemType Directory -Path $Path -Force -ErrorAction Stop | Out-Null
        }
        $probe = Join-Path $Path ('.selfcheck-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
        Set-Content -LiteralPath $probe -Value 'ok' -ErrorAction Stop
        Remove-Item -LiteralPath $probe -Force -ErrorAction SilentlyContinue
        return $true
    } catch {
        return $false
    }
}

# ---------- 枚举进程的顶层窗口（关键检查）----------
$script:WinEnumReady = $false
function Initialize-WinEnum {
    if ($script:WinEnumReady) { return $true }
    $code = @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public class DwWinEnum {
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc lpEnumFunc, IntPtr lParam);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr hWnd, out RECT r);
    // 关键：必须声明 CharSet.Unicode，否则 StringBuilder 按 ANSI 编组，
    // 宽字符被截成第一个字节 —— 窗口类名 "Tauri Window" 会变成 "T"（实测踩到）
    [DllImport("user32.dll", CharSet = CharSet.Unicode, EntryPoint = "GetClassNameW")]
    static extern int GetClassNameW(IntPtr hWnd, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, EntryPoint = "GetWindowTextW")]
    static extern int GetWindowTextW(IntPtr hWnd, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hWnd);
    public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    public class Info { public string Class; public string Title; public int Width; public int Height; public bool Visible; }
    public static List<Info> Of(uint target) {
        var list = new List<Info>();
        EnumWindows((h, l) => {
            uint pid; GetWindowThreadProcessId(h, out pid);
            if (pid != target) return true;
            var cls = new StringBuilder(256); GetClassNameW(h, cls, 256);
            var txt = new StringBuilder(512); GetWindowTextW(h, txt, 512);
            RECT r; GetWindowRect(h, out r);
            list.Add(new Info { Class = cls.ToString(), Title = txt.ToString(),
                                Width = r.Right - r.Left, Height = r.Bottom - r.Top,
                                Visible = IsWindowVisible(h) });
            return true;
        }, IntPtr.Zero);
        return list;
    }
}
'@
    try {
        Add-Type -TypeDefinition $code -ErrorAction Stop
        $script:WinEnumReady = $true
        return $true
    } catch {
        return $false
    }
}

function Get-ExeWindows {
    param([int]$ProcessId)
    if (-not (Initialize-WinEnum)) { return $null }
    return @([DwWinEnum]::Of([uint32]$ProcessId))
}

# ---------- 假监听器：数应用有没有去连 dev 服务器 ----------
function Start-TcpProbe {
    param([int]$Port)
    try {
        $listener = New-Object System.Net.Sockets.TcpListener([System.Net.IPAddress]::Loopback, $Port)
        $listener.Start()
        return $listener
    } catch {
        return $null
    }
}

# ---------- 读日志尾部 ----------
function Get-LogKeyLines {
    param([string]$Path, [int]$Tail = 60)
    if (-not (Test-Path -LiteralPath $Path)) { return @() }
    return @(Get-Content -LiteralPath $Path -Encoding UTF8 -Tail $Tail -ErrorAction SilentlyContinue)
}

$script:Buf = $null
New-Item -ItemType Directory -Path $OUT_DIR -Force | Out-Null

Write-Host ""
Write-Host "============================================================" -ForegroundColor White
Write-Host "  织梦机 · 启动自检" -ForegroundColor White
Write-Host ("  项目：{0}" -f $script:ProjRoot) -ForegroundColor Gray
Write-Host ("  时间：{0}" -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss')) -ForegroundColor Gray
Write-Host "============================================================" -ForegroundColor White

# ============================================================
Section '一、产物与版本'
# ============================================================

# 1. exe 是否存在
$usingExe = $null
$usingKind = $null
if ($script:ExplicitMissing) {
    Add-Result '指定的 exe 存在' 'FAIL' ('没这个文件：' + $RELEASE) '-ExePath 指错了；不指定的话默认用项目的 target\release\app.exe'
    $usingExe = $null
} elseif (Test-Path -LiteralPath $RELEASE) {
    $fi = Get-Item -LiteralPath $RELEASE
    Add-Result '打包版（release）app.exe 存在' 'PASS' ("{0}  {1:N1} MB  构建于 {2}" -f $fi.Name, ($fi.Length / 1MB), $fi.LastWriteTime.ToString('MM-dd HH:mm'))

    # **模拟 start.cmd 的真实启动方式**：把 exe 复制到安全路径再启动。
    #
    # 为什么必须这样测：实测（本机复现）同一 exe
    #   · 从 D:\project\ai生成\... 启动 → 只出现 Tao 事件窗口，**Tauri 主窗口建不出来**
    #   · 复制到 D:\DreamWeaverRun 启动 → 主窗口正常 1295x837
    # 用户实际用的是 start.cmd（它会复制），所以自检也应该测"复制后"的那份，
    # 否则会一直误报 FAIL。而"直接启动会失败"这件事单独作为风险提示（见第二节）。
    $script:RunDir = $null
    $copyTo = $null
    if (-not $ExePath) {
        if ($env:LOCALAPPDATA) { $copyTo = Join-Path $env:LOCALAPPDATA 'DreamWeaverRun' }
        else { $copyTo = 'D:\DreamWeaverRun' }
    }
    if ($copyTo) {
        try {
            New-Item -ItemType Directory -Path $copyTo -Force -ErrorAction Stop | Out-Null
            $copied = Join-Path $copyTo 'app.exe'
            # 目标可能被占用 → 先杀掉在跑的实例
            $busy = Get-Process -Name app -ErrorAction SilentlyContinue
            foreach ($b in $busy) { try { Stop-Process -Id $b.Id -Force -ErrorAction SilentlyContinue } catch { } }
            if ($busy) { Start-Sleep -Milliseconds 900 }
            Copy-Item -LiteralPath $RELEASE -Destination $copied -Force -ErrorAction Stop
            $usingExe = $copied
            $script:RunDir = $copyTo
            Add-Result '按 start.cmd 的方式复制到安全路径' 'PASS' $copied
        } catch {
            $usingExe = $RELEASE
            Add-Result '按 start.cmd 的方式复制到安全路径' 'WARN' ('复制失败，改为直接测项目内那份：' + $_.Exception.Message)
        }
    } else {
        $usingExe = $RELEASE
    }
    $usingKind = 'release'
} elseif (Test-Path -LiteralPath $DEBUGEXE) {
    $usingExe = $DEBUGEXE; $usingKind = 'debug'
    $fi = Get-Item -LiteralPath $DEBUGEXE
    Add-Result '打包版（release）app.exe 存在' 'FAIL' '只有 debug 版，没有 release 版' 'debug 版会去 localhost:5173 加载界面，没起 vite 时窗口会开了立刻关'
} else {
    Add-Result '打包版（release）app.exe 存在' 'FAIL' 'release 与 debug 版都不存在' '需要先构建'
}

# 2. 前端产物
$distIndex = Join-Path $script:ProjRoot 'dist\index.html'
if (Test-Path -LiteralPath $distIndex) {
    $di = Get-Item -LiteralPath $distIndex
    Add-Result '前端产物 dist\index.html 存在' 'PASS' ("构建于 {0}" -f $di.LastWriteTime.ToString('MM-dd HH:mm'))
} else {
    Add-Result '前端产物 dist\index.html 存在' 'FAIL' '没有 dist 目录 → release 版内嵌的是空前端' '需要 npm run build'
}

# 3. 版本号一致性（npm 包 vs Rust crate 不一致会让 CLI 直接拒绝启动）
$verPkg = $null; $verTauri = $null; $verCargo = $null
try { $verPkg = (Get-Content -LiteralPath (Join-Path $script:ProjRoot 'package.json') -Raw -Encoding UTF8 | ConvertFrom-Json).version } catch { }
try { $verTauri = (Get-Content -LiteralPath (Join-Path $script:ProjRoot 'src-tauri\tauri.conf.json') -Raw -Encoding UTF8 | ConvertFrom-Json).version } catch { }
try {
    $cargoTxt = Get-Content -LiteralPath (Join-Path $script:ProjRoot 'src-tauri\Cargo.toml') -Raw -Encoding UTF8
    if ($cargoTxt -match '(?m)^version\s*=\s*"([^"]+)"') { $verCargo = $Matches[1] }
} catch { }

if ($verPkg -and $verTauri -and $verCargo) {
    if ($verPkg -eq $verTauri -and $verTauri -eq $verCargo) {
        Add-Result '三处版本号一致' 'PASS' ("package.json / tauri.conf.json / Cargo.toml 都是 {0}" -f $verPkg)
    } else {
        Add-Result '三处版本号一致' 'WARN' ("package.json={0}  tauri.conf.json={1}  Cargo.toml={2}" -f $verPkg, $verTauri, $verCargo) '不一致本身不至于起不来，但会让"我现在跑的是哪版"说不清'
    }
} else {
    Add-Result '三处版本号一致' 'SKIP' '有文件读不出来（没测成，不代表没问题）'
}

# 4. Tauri 包版本匹配（npm 包与 Rust crate 必须同 major/minor）
$tauriVer = $null; $apiVer = $null
$m = [regex]::Match($cargoTxt, 'tauri\s*=\s*\{?[^\r\n]*?version\s*=\s*"([^"]+)"')
if ($m.Success) { $tauriVer = $m.Groups[1].Value }
# 优先读 node_modules 里真实安装的版本；读不到再退回 package-lock
    $apiPkg = Join-Path (Join-Path (Join-Path $script:ProjRoot 'node_modules') '@tauri-apps') 'api'
    $apiPkg = Join-Path $apiPkg 'package.json'
if (Test-Path -LiteralPath $apiPkg) {
    try { $apiVer = (Get-Content -LiteralPath $apiPkg -Raw -Encoding UTF8 | ConvertFrom-Json).version } catch { }
}
if (-not $apiVer) {
    try {
        $lockPath = Join-Path $script:ProjRoot 'package-lock.json'
        if (Test-Path -LiteralPath $lockPath) {
            $lock = Get-Content -LiteralPath $lockPath -Raw -Encoding UTF8 | ConvertFrom-Json
            $node = $lock.packages.PSObject.Properties | Where-Object { $_.Name -eq 'node_modules/@tauri-apps/api' } | Select-Object -First 1
            if ($node) { $apiVer = $node.Value.version }
        }
    } catch { }
}
if ($tauriVer -and $apiVer) {
    $mmRust = ($tauriVer -split '\.')[0..1] -join '.'
    $mmApi  = ($apiVer -split '\.')[0..1] -join '.'
    if ($mmRust -eq $mmApi) {
        Add-Result 'Tauri npm 包与 Rust crate 版本匹配' 'PASS' ("tauri={0}  @tauri-apps/api={1}" -f $tauriVer, $apiVer)
    } else {
        Add-Result 'Tauri npm 包与 Rust crate 版本匹配' 'FAIL' ("tauri={0}  @tauri-apps/api={1}" -f $tauriVer, $apiVer) 'major/minor 不一致时 Tauri CLI 会报 "version mismatched" 并拒绝启动'
    }
} else {
    Add-Result 'Tauri npm 包与 Rust crate 版本匹配' 'SKIP' '版本读不出来（没测成）'
}

# 5. 构建产物是否"打包版"（含内嵌前端）
if ($usingExe) {
    # 全文扫描（HTML 可能落在 exe 很靠后的位置 —— 实测在 15.4 MB 处，
    # 而我只扫前 6 MB 时误报过"没内嵌前端"）
    $bytes = [System.IO.File]::ReadAllBytes($usingExe)
    $hasHtml = $false
    $htmlAt = -1
    $needle = [System.Text.Encoding]::ASCII.GetBytes('<!doctype html')
    for ($i = 0; $i -le $bytes.Length - $needle.Length; $i++) {
        if ($bytes[$i] -eq $needle[0]) {
            $match = $true
            for ($j = 1; $j -lt $needle.Length; $j++) {
                if ($bytes[$i + $j] -ne $needle[$j]) { $match = $false; break }
            }
            if ($match) { $hasHtml = $true; $htmlAt = $i; break }
        }
    }
    if ($hasHtml) {
        Add-Result 'exe 内嵌了前端（custom-protocol 已生效）' 'PASS' ("在 exe 里找到 HTML 内容（偏移 {0:N1} MB）" -f ($htmlAt / 1MB))
    } else {
        Add-Result 'exe 内嵌了前端（custom-protocol 已生效）' 'FAIL' 'exe 里没找到 HTML → 它仍会去 localhost:5173 取界面' '构建时必须带 --features custom-protocol，否则打包版也依赖 dev 服务器'
    }
}

# ============================================================
Section '二、路径安全'
# ============================================================

if ($usingExe) {
    # 6. exe 所在目录名是否命中已知危险名单
    $exeDir = Split-Path -Parent (Split-Path -Parent $usingExe)   # target/release → target
    $leafNames = @()
    $d = (Get-Item -LiteralPath $usingExe).Directory
    $guard = 0
    while ($d -and $guard -lt 12) {
        $leafNames += $d.Name
        $d = $d.Parent
        $guard++
    }
    $hit = @()
    foreach ($bad in $DANGEROUS_DIR_NAMES) {
        if ($leafNames -contains $bad) { $hit += $bad }
    }
    $chain = ($leafNames | Select-Object -First 5) -join ' 内 '
    if ($hit.Count -eq 0) {
        Add-Result 'exe 路径不在已知危险目录下' 'PASS' ("祖先链：" + $chain)
    } elseif ($script:RunDir) {
        # 已经在用复制后的安全路径 → 这条只是"说明为什么必须复制"，不算问题
        Add-Result '源 exe 在风险目录下（已通过复制规避）' 'WARN' ("源祖先链：{0}    实际启动用的是：{1}" -f $chain, $script:RunDir) '直接双击 target\release\app.exe 会没有窗口（静默）；start.cmd 会先复制到安全路径，所以它能正常用'
    } else {
        # 判定为 WARN 而不是 FAIL：实测有个例外 —— 直接把 exe 从危险目录里启动会失败，
        # 但把 exe **复制到安全路径**再启动是成功的（这也就是 start.cmd 的做法）。
        # 所以这是"存在风险"，不是"必然失败"，最终以第三节的真实启动结果为准。
        Add-Result 'exe 路径含已知风险目录名' 'WARN' (("祖先链里有：{0}    完整链：{1}" -f ($hit -join ', '), $chain)) '实测：同一 exe 在 ai生成 目录下启动时 Tauri 建不出主窗口（静默）；复制到 D:\DreamWeaverRun 后正常。所以建议用 start.cmd（它会先复制），而不是直接双击 target\release\app.exe'
    }
}

# 7. 数据根候选 & 可写性
$appData = Join-Path $env:APPDATA 'com.guchenxi.story-weaver'
$envOverride = [Environment]::GetEnvironmentVariable('DREAMWEAVER_DATA_DIR')
$candidates = @()
if ($envOverride) { $candidates += [pscustomobject]@{ Label='DREAMWEAVER_DATA_DIR'; Path=$envOverride } }
$candidates += [pscustomobject]@{ Label='%APPDATA%\com.guchenxi.story-weaver'; Path=$appData }
$candidates += [pscustomobject]@{ Label='%LOCALAPPDATA%\com.guchenxi.story-weaver'; Path=(Join-Path $env:LOCALAPPDATA 'com.guchenxi.story-weaver') }
if ($usingExe) { $candidates += [pscustomobject]@{ Label='<exe 同级>\data'; Path=(Join-Path (Split-Path -Parent $usingExe) 'data') } }
$candidates += [pscustomobject]@{ Label='<项目>\data'; Path=$DATA_DIR }

$writableAny = @()
foreach ($c in $candidates) {
    $ok = Test-DirWritable $c.Path
    if ($ok) { $writableAny += $c }
}
# start.cmd 会**强制**把数据根设成 <项目>\data；不设环境变量时才走下面的探测顺序。
# 这两种情况要说清楚，否则用户会疑惑"到底用哪个"
$projDataWritable = Test-DirWritable $DATA_DIR
if ($projDataWritable) {
    Add-Result 'start.cmd 用的数据目录可写' 'PASS' ("<项目>\data → " + $DATA_DIR)
} else {
    Add-Result 'start.cmd 用的数据目录可写' 'FAIL' ("写不进去：" + $DATA_DIR) 'start.cmd 会把 DREAMWEAVER_DATA_DIR 指到这里，写不了就起不来'
}
if ($writableAny.Count -gt 0) {
    $first = $writableAny[0]
    Add-Result '数据根候选（不设环境变量时）至少一个可写' 'PASS' ("探测顺序里第一个可写的是：{0} → {1}" -f $first.Label, $first.Path)
} else {
    Add-Result '至少有一个数据根候选可写' 'FAIL' '所有候选都写不进去' '程序会在初始化数据库时报 attempt to write a readonly database 并退出'
}
$unwritable = $candidates | Where-Object { $writableAny.Path -notcontains $_.Path }
foreach ($u in $unwritable) {
    Write-Host ("         · 不可写：{0}  ({1})" -f $u.Label, $u.Path) -ForegroundColor DarkGray
    if ($script:Buf) { [void]$script:Buf.Add(("         · 不可写：{0}  ({1})" -f $u.Label, $u.Path)) }
}

# 8. 数据是否在有故事的位置
$storyDbFound = $null
foreach ($base in @($appData, (Join-Path $usingExe 'data'), $DATA_DIR)) {
    if (-not $base) { continue }
    $db = Join-Path $base 'library\default\library.db'
    if (Test-Path -LiteralPath $db) {
        $size = (Get-Item -LiteralPath $db).Length
        if ($size -gt 10KB) { $storyDbFound = $db; break }
    }
}
if ($storyDbFound) {
    Add-Result '找到含故事的数据库' 'PASS' $storyDbFound
} else {
    Add-Result '找到含故事的数据库' 'WARN' '没找到明显的数据库（程序会以空库启动）' '如果期待看到已有故事，说明数据目录对不上'
}

# ============================================================
Section '三、真实启动测试'
# ============================================================

if (-not $usingExe) {
    Add-Result '真实启动测试' 'SKIP' '没有可用的 exe'
} else {
    # 清场：先杀掉可能在跑的实例
    $old = Get-Process -Name app -ErrorAction SilentlyContinue
    if ($old) {
        foreach ($p in $old) { try { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue } catch { } }
        Start-Sleep -Milliseconds 800
        Write-Host "         （先结束了已有的 app 进程，避免互相干扰）" -ForegroundColor DarkGray
    }

    # 假监听器：如果应用去连 5173，我们能数到
    $probePort = 5173
    $portBusy = $false
    try {
        $existing = Get-NetTCPConnection -LocalPort $probePort -State Listen -ErrorAction SilentlyContinue
        if ($existing) { $portBusy = $true }
    } catch { }
    $listener = $null
    $connHits = New-Object System.Collections.ArrayList
    if (-not $portBusy) {
        $listener = Start-TcpProbe -Port $probePort
        if ($listener) {
            Write-Host ("         已在 127.0.0.1:{0} 起假监听器（不返回内容），用来探测应用是否在找 dev 服务器" -f $probePort) -ForegroundColor DarkGray
        }
    } else {
        Write-Host ("         端口 {0} 已被占用 -- 跳过「假监听器」探测（可能你正跑着 npm run dev）" -f $probePort) -ForegroundColor DarkGray
    }

    # 环境：用 <项目>\data 作为数据根（和 start.cmd 一致）
    $env:DREAMWEAVER_DATA_DIR = $DATA_DIR

    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $proc = $null
    try {
        $ps = New-Object System.Diagnostics.ProcessStartInfo
        $ps.FileName = $usingExe
        $ps.WorkingDirectory = Split-Path -Parent $usingExe
        $ps.UseShellExecute = $false
        $ps.RedirectStandardError = $true
        $ps.RedirectStandardOutput = $true
        # **启动前**先等上一轮遗留的 app 进程退出。
        # 不清理的话：stale 进程会让窗口枚举张冠李戴，WebView2 的用户数据目录也可能被占用，
        # 表现就是"主窗口已创建"偶发 FAIL（实测踩到过一次）。
        try {
            $stale = @(Get-Process -Name 'app' -ErrorAction SilentlyContinue | Where-Object { $_.Id -ne $PID })
            if ($stale.Count -gt 0) {
                Add-Result '启动前清理遗留进程' 'WARN' ("发现 {0} 个残留 app 进程，先结束掉" -f $stale.Count) '自检需要干净的起点'
                foreach ($sp in $stale) { try { $sp.Kill() } catch { } }
                $winSw = [System.Diagnostics.Stopwatch]::StartNew()
                for ($i = 0; $i -lt 20; $i++) {
                    Start-Sleep -Milliseconds 250
                    $still = @(Get-Process -Name 'app' -ErrorAction SilentlyContinue | Where-Object { $_.Id -ne $PID })
                    if ($still.Count -eq 0) { break }
                }
            }
        } catch { }
        $proc = [System.Diagnostics.Process]::Start($ps)
    } catch {
        Add-Result '进程能否启动' 'FAIL' ("启动失败：" + $_.Exception.Message)
    }

    if ($proc) {
        # 轮询窗口：Tauri 创建窗口有延迟，单次采样会误判（我踩过）。
        # 最多等约 18 秒（原来 11 秒，实测偶发超时 —— 主要是上一轮 WebView2 还在退出）。
        $appWin = $null
        $attempts = 0
        $maxAttempts = 20
        $winSw = [System.Diagnostics.Stopwatch]::StartNew()
        while ($attempts -lt $maxAttempts) {
            Start-Sleep -Milliseconds 900
            $attempts++
            if ($proc.HasExited) { break }
            $wins = Get-ExeWindows -ProcessId $proc.Id
            if ($null -ne $wins) {
                $cand = $wins | Where-Object { $_.Class -eq 'Tauri Window' -and $_.Width -gt 500 -and $_.Height -gt 400 }
                if ($cand) { $appWin = $cand[0]; break }
            }
            # 顺便收一下监听器连接
            if ($listener) {
                try {
                    while ($listener.Pending()) {
                        $cl = $listener.AcceptTcpClient()
                        [void]$connHits.Add($sw.Elapsed.TotalSeconds)
                        $cl.Close()
                    }
                } catch { }
            }
        }
        $sw.Stop()

        # 结论 1：进程是否退出
        if ($proc.HasExited) {
            Add-Result '进程能启动并保持运行' 'FAIL' ("进程在 {0:N1} 秒内退出，退出码 {1}" -f $sw.Elapsed.TotalSeconds, $proc.ExitCode) '进程一起就退，通常是启动期报错（数据库/日志目录/依赖）'
        } else {
            Add-Result '进程能启动并保持运行' 'PASS' ("已运行 {0:N1} 秒" -f $sw.Elapsed.TotalSeconds)
        }

        # 结论 2：**主窗口真的创建了吗**（最关键）
        if ($proc.HasExited) {
            Add-Result '主窗口已创建' 'SKIP' '进程已退出，无法判断窗口'
        } else {
            # **补一轮重试**：紧跟 release 构建之后跑自检时，杀软扫描 / 磁盘繁忙会让窗口
            # 晚于 18 秒出现 —— 实测偶发 FAIL（重跑就过）。这里再等最多 9 秒（10 × 900ms）。
            if (-not $appWin) {
                for ($rt = 0; $rt -lt 10; $rt++) {
                    Start-Sleep -Milliseconds 900
                    if ($proc.HasExited) { break }
                    $w = Get-ExeWindows -ProcessId $proc.Id
                    if ($null -ne $w) {
                        $c2 = $w | Where-Object { $_.Class -eq 'Tauri Window' -and $_.Width -gt 500 -and $_.Height -gt 400 }
                        if ($c2) { $appWin = $c2[0]; break }
                    }
                }
            }
            $winSw.Stop()
            $waited = $winSw.Elapsed.TotalSeconds

            if ($appWin) {
                # 超过首轮 18 秒才拿到 → 标注"是重试拿到的"，便于观察机器到底多慢
                $mark = ''
                if ($waited -gt 18) { $mark = '（第二轮重试才出现）' }
                Add-Result '主窗口已创建' 'PASS' ("class=Tauri Window  标题=""{0}""  尺寸={1}x{2}  等待 {3:N1}s{4}" -f $appWin.Title, $appWin.Width, $appWin.Height, $waited, $mark)
            } else {
                # 窗口真没建出来 → 列出实际枚举到的窗口，便于判断
                $wins = Get-ExeWindows -ProcessId $proc.Id
                $desc = ''
                if ($null -eq $wins) {
                    $desc = '窗口枚举功能不可用（没测成）'
                    Add-Result '主窗口已创建' 'SKIP' $desc '这一项没测成，不代表没问题'
                } else {
                    $parts = New-Object System.Collections.ArrayList
                    foreach ($w in $wins) {
                        $one = $w.Class
                        $one = $one + '[' + $w.Width + 'x' + $w.Height + ']'
                        [void]$parts.Add($one)
                    }
                    $desc = $parts -join '  '
                    if (-not $desc) { $desc = '（一个窗口都没有）' }
                    # 附一句 WebView2 情况：窗口建不出来常见于 WebView2 起不来
                    try {
                        $wv = @(Get-Process -Name 'msedgewebview2' -ErrorAction SilentlyContinue).Count
                        $desc = $desc + ("  （msedgewebview2 进程 {0} 个）" -f $wv)
                    } catch { }
                    $msg = ("等了 {0:N1} 秒仍没枚举到 Tauri 主窗口。实际窗口：" -f $waited) + $desc
                    Add-Result '主窗口已创建' 'FAIL' $msg 'Tauri 窗口创建失败时完全静默（进程活、无报错），只能靠枚举窗口发现'
                }
            }
        }

        # 结论 3：有没有去连 dev 服务器
        if ($listener) {
            # 再收一次
            try {
                while ($listener.Pending()) {
                    $cl = $listener.AcceptTcpClient()
                    [void]$connHits.Add($sw.Elapsed.TotalSeconds)
                    $cl.Close()
                }
            } catch { }
            if ($connHits.Count -gt 0) {
                Add-Result '没有去连 dev 服务器（5173）' 'FAIL' ("收到 {0} 次连接 -- 它在等 vite，页面会显示「拒绝连接」" -f $connHits.Count) '缺 custom-protocol 特性时，打包版也会去 localhost:5173 取界面'
            } else {
                Add-Result '没有去连 dev 服务器（5173）' 'PASS' '假监听器 0 次连接 → 用的是内嵌前端'
            }
        } else {
            Add-Result '没有去连 dev 服务器（5173）' 'SKIP' '5173 已被占用，无法做假监听器探测'
        }

        # 收尾：关掉应用
        if (-not $proc.HasExited) {
            try { $proc.Kill(); $proc.WaitForExit(3000) | Out-Null } catch { }
            Write-Host "         （已关闭测试用的应用进程）" -ForegroundColor DarkGray
        }
        if ($listener) { try { $listener.Stop() } catch { } }
        Remove-Item Env:\DREAMWEAVER_DATA_DIR -ErrorAction SilentlyContinue
    }
}

# ============================================================
Section '四、启动日志（关键行）'
# ============================================================

$logDirs = @()
if (Test-Path -LiteralPath $DATA_DIR) { $logDirs += (Join-Path $DATA_DIR 'logs') }
$logDirs += (Join-Path $script:ProjRoot 'debug-diagnostics')
foreach ($ld in $logDirs) {
    foreach ($name in @('startup-trace.log', 'startup-error.log')) {
        $f = Join-Path $ld $name
        if (Test-Path -LiteralPath $f) {
            $all = Get-LogKeyLines -Path $f -Tail 200
            # 这个 trace 是**追加**写的，跑过几次就有几段。只看最后一次：
            # 从最后一个 "=== setup 开始 ===" 到结尾
            $lines = $all
            for ($k = $all.Count - 1; $k -ge 0; $k--) {
                if ($all[$k] -match 'setup 开始') { $lines = $all[$k..($all.Count - 1)]; break }
            }
            if ($lines.Count -gt 0) {
                Write-Host ("  --- {0} ---" -f $f) -ForegroundColor DarkGray
                if ($script:Buf) { [void]$script:Buf.Add(("  --- {0} ---" -f $f)) }
                foreach ($l in $lines) {
                    Write-Host ("    {0}" -f $l) -ForegroundColor DarkGray
                    if ($script:Buf) { [void]$script:Buf.Add(("    {0}" -f $l)) }
                }
            }
        }
    }
}
$hitLog = $false
foreach ($ld in $logDirs) { if ((Test-Path -LiteralPath (Join-Path $ld 'startup-error.log'))) { $hitLog = $true } }
if ($hitLog) {
    Add-Result '没有启动失败日志' 'WARN' '发现 startup-error.log -- 之前有过启动失败' '可以打开看看当时的具体原因'
} else {
    Add-Result '没有启动失败日志' 'PASS' '没找到 startup-error.log'
}

# ============================================================
Section '五、结论'
# ============================================================

$fail = @($script:Results | Where-Object { $_.Status -eq 'FAIL' })
$warn = @($script:Results | Where-Object { $_.Status -eq 'WARN' })
$skip = @($script:Results | Where-Object { $_.Status -eq 'SKIP' })
$pass = @($script:Results | Where-Object { $_.Status -eq 'PASS' })

Write-Host ""
if ($fail.Count -eq 0) {
    Write-Host ("  结论：可用  （通过 {0} 项，警告 {1} 项，未测 {2} 项）" -f $pass.Count, $warn.Count, $skip.Count) -ForegroundColor Green
    if ($script:Buf) { [void]$script:Buf.Add(("  结论：可用  （通过 {0} 项，警告 {1} 项，未测 {2} 项）" -f $pass.Count, $warn.Count, $skip.Count)) }
} else {
    Write-Host ("  结论：**有问题**  （失败 {0} 项，警告 {1} 项）" -f $fail.Count, $warn.Count) -ForegroundColor Red
    if ($script:Buf) { [void]$script:Buf.Add(("  结论：有问题  （失败 {0} 项，警告 {1} 项）" -f $fail.Count, $warn.Count)) }
    foreach ($f in $fail) {
        Write-Host ("    × {0}" -f $f.Name) -ForegroundColor Red
        if ($f.Detail) { Write-Host ("      {0}" -f $f.Detail) -ForegroundColor DarkGray }
        if ($script:Buf) { [void]$script:Buf.Add(("    × {0}" -f $f.Name)); if ($f.Detail) { [void]$script:Buf.Add(("      {0}" -f $f.Detail)) } }
    }
}
if ($skip.Count -gt 0) {
    Write-Host ("  注意：有 {0} 项**没测成**（不代表通过）" -f $skip.Count) -ForegroundColor Yellow
    foreach ($s in $skip) { Write-Host ("    ? {0} - {1}" -f $s.Name, $s.Detail) -ForegroundColor DarkGray }
    if ($script:Buf) {
        [void]$script:Buf.Add(("  注意：有 {0} 项没测成（不代表通过）" -f $skip.Count))
        foreach ($s in $skip) { [void]$script:Buf.Add(("    ? {0} - {1}" -f $s.Name, $s.Detail)) }
    }
}

Write-Host ""
Write-Host ("  报告已保存：{0}" -f $REPORT) -ForegroundColor Gray
if ($fail.Count -gt 0) { $script:ExitCode = 1 } else { $script:ExitCode = 0 }
if ($script:Buf) {
    [void]$script:Buf.Add('')
    [void]$script:Buf.Add(("  报告保存：{0}" -f $REPORT))
    $script:Buf | Out-File -LiteralPath $REPORT -Encoding UTF8
}
Write-Host ""

exit $script:ExitCode
