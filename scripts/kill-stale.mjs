#!/usr/bin/env node
/**
 * Cross-platform stale-process cleanup for `pnpm tauri dev`.
 *
 * Kills:
 *   - any `gabriel` / `gabriel.exe` process left over from a previous run
 *     (it holds the WebView2 user-data folder lock -> "resource is in use"), and
 *   - any process listening on 5173/5174 (Vite) or 8080 (REST API)
 *     (second bind fails with os error 10048 / EADDRINUSE).
 *
 * Windows: delegates to scripts/kill-stale.ps1 (the documented
 *   Get-Process / Get-NetTCPConnection commands), with a netstat+taskkill
 *   fallback if PowerShell is unavailable.
 * macOS/Linux: uses pkill + lsof.
 *
 * Idempotent: exits 0 even when nothing was stale.
 * Run manually with `pnpm run kill:stale`, or automatically via
 * `beforeDevCommand` in src-tauri/tauri.conf.json (runs once, before Vite /
 * gabriel start, so it can never kill the current session).
 * NOTE: deliberately NOT wired as a `predev` hook — plain `pnpm dev` must
 * stay innocent, otherwise re-running the frontend while a Tauri session is
 * up would kill that session's own gabriel.exe (it owns port 8080).
 */
import { execFileSync, execSync, spawnSync } from 'node:child_process';
import { platform } from 'node:os';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const STALE_PORTS = [5173, 5174, 8080];
const APP_NAMES = ['gabriel', 'gabriel.exe'];

function run(cmd, args, opts = {}) {
  try {
    return execFileSync(cmd, args, { encoding: 'utf8', stdio: 'pipe', ...opts });
  } catch {
    return '';
  }
}

function killPid(pid, os) {
  const id = Number(pid);
  if (!Number.isInteger(id) || id <= 0 || id === process.pid) return;
  try {
    if (os === 'win32') {
      execFileSync('taskkill', ['/F', '/PID', String(id)], { stdio: 'ignore' });
    } else {
      process.kill(id, 'SIGKILL');
    }
    console.log(`[kill:stale] killed PID ${id}`);
  } catch {
    // Already exited or not ours — ignore.
  }
}

function cleanupWindows() {
  const here = path.dirname(fileURLToPath(import.meta.url));
  const ps1 = path.join(here, 'kill-stale.ps1');
  const ps = spawnSync(
    'powershell',
    ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ps1],
    { stdio: 'inherit' },
  );
  if (ps.status === 0) return;

  // Fallback when PowerShell is unavailable: taskkill + netstat parsing.
  console.log('[kill:stale] PowerShell cleanup failed, falling back to taskkill/netstat');
  for (const name of APP_NAMES) {
    run('taskkill', ['/F', '/IM', name]);
  }
  const out = run('netstat', ['-ano']);
  for (const line of out.split('\n')) {
    const m = line.match(/TCP\s+\S+:(\d+)\s+\S+\s+LISTENING\s+(\d+)/i);
    if (m && STALE_PORTS.includes(Number(m[1]))) killPid(m[2], 'win32');
  }
}

function cleanupUnix() {
  for (const name of APP_NAMES) {
    run('pkill', ['-9', '-x', name]);
    run('pkill', ['-9', '-f', name]);
  }
  for (const port of STALE_PORTS) {
    const out = run('lsof', ['-ti', `:${port}`, '-sTCP:LISTEN']);
    for (const pid of out.split(/[\s,]+/).filter(Boolean)) killPid(pid, 'unix');
    // fuser fallback when lsof is missing.
    if (!out.trim()) run('fuser', [`${port}/tcp`, '-k', '-9']);
  }
}

const os = platform();
if (os === 'win32') cleanupWindows();
else cleanupUnix();
console.log('[kill:stale] done (ports 5173, 5174, 8080 + gabriel processes)');
