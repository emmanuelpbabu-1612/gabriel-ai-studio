#Requires -Version 5.1
<#
.SYNOPSIS
  Kill stale Gabriel processes that block `pnpm tauri dev`.

.DESCRIPTION
  A previous `pnpm tauri dev` run can leave a stale gabriel.exe (or a
  leftover cargo/vite process) holding the WebView2 user-data folder lock
  and the REST/Vite ports. Two processes cannot share one WebView2
  user-data folder ("The requested resource is in use"), and the second
  REST server cannot bind 8080 (os error 10048).

  This script kills:
    - any process named `gabriel` (the Tauri binary, case-insensitive), and
    - any process listening on the app ports: 5173/5174 (Vite) and 8080 (REST).

  Safe to run when nothing is stale (no-ops silently).
  Run manually with:  pnpm run kill:stale
  It also runs automatically via `beforeDevCommand` in
  src-tauri/tauri.conf.json (once, before Vite/gabriel start — never as a
  `predev` hook, so plain `pnpm dev` can't kill a running session).
#>

$ErrorActionPreference = 'SilentlyContinue'

# 1. Kill any existing Gabriel binary from a previous `tauri dev` run.
Get-Process -Name gabriel -ErrorAction SilentlyContinue | Stop-Process -Force

# 2. Kill anything still listening on the app ports (Vite 5173/5174, REST 8080).
#    Get-NetTCPConnection may require elevation for some entries; failures are ignored.
$StalePorts = @(5173, 5174, 8080)
$Listeners = Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue |
    Where-Object { $_.LocalPort -in $StalePorts }
$Listeners |
    Select-Object -ExpandProperty OwningProcess -Unique |
    ForEach-Object { Stop-Process -Id $_ -Force -ErrorAction SilentlyContinue }
