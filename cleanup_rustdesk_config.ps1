# 手动彻底清理 RustDesk 配置（以管理员身份运行 PowerShell 后执行）
# 此脚本会删除所有用户配置文件、服务账户配置文件以及注册表中的旧授权信息。

$ErrorActionPreference = "SilentlyContinue"

Write-Host "=== 清理 RustDesk 用户配置 ===" -ForegroundColor Cyan
$paths = @(
    "$env:APPDATA\RustDesk",
    "C:\Windows\System32\config\systemprofile\AppData\Roaming\RustDesk",
    "C:\Windows\ServiceProfiles\LocalService\AppData\Roaming\RustDesk"
)
foreach ($p in $paths) {
    if (Test-Path $p) {
        Remove-Item -Recurse -Force $p
        Write-Host "已删除: $p" -ForegroundColor Green
    } else {
        Write-Host "不存在: $p" -ForegroundColor Gray
    }
}

Write-Host "`n=== 清理旧版自定义客户端注册表遗留（Key/Host/Api） ===" -ForegroundColor Cyan
$regBases = @(
    "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{54E86BC2-6C85-41F3-A9EB-1A94AC9B1F93}_is1",
    "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\RustDesk",
    "HKLM:\Software\Wow6432Node\Microsoft\Windows\CurrentVersion\Uninstall\RustDesk"
)
foreach ($rb in $regBases) {
    if (Test-Path $rb) {
        foreach ($val in @("Key", "Host", "Api")) {
            Remove-ItemProperty -Path $rb -Name $val -Force
            Write-Host "已删除注册表值: $rb\$val" -ForegroundColor Green
        }
    } else {
        Write-Host "不存在: $rb" -ForegroundColor Gray
    }
}

Write-Host "`n=== 清理安装目录 ===" -ForegroundColor Cyan
$installDirs = @(
    "C:\Program Files\RustDesk",
    "C:\Program Files (x86)\RustDesk"
)
foreach ($d in $installDirs) {
    if (Test-Path $d) {
        Remove-Item -Recurse -Force $d
        Write-Host "已删除: $d" -ForegroundColor Green
    } else {
        Write-Host "不存在: $d" -ForegroundColor Gray
    }
}

Write-Host "`n清理完成。请重新安装最新安装包。" -ForegroundColor Cyan
