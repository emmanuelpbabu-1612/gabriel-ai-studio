import type { FC } from 'react';
import { Activity, CheckCircle2 } from 'lucide-react';
import { RadialGraph } from '../shared';
import { useTelemetry } from '../../hooks/useTelemetry';
import { useEngineStatus } from '../../hooks/useSystem';

const fmt = (v: number | null | undefined, d = 0, unit = '') =>
  v == null ? '—' : `${v.toFixed(d)}${unit}`;

const clampPct = (v: number | null | undefined) =>
  v == null || Number.isNaN(v) ? 0 : Math.max(0, Math.min(100, v));

const ratioPct = (used: number | undefined, total: number | undefined) =>
  !used || !total ? 0 : clampPct((used / total) * 100);

export const SystemMonitor: FC = () => {
  const t = useTelemetry();
  const { status: engine } = useEngineStatus();

  const cpu = clampPct(t?.cpu_load);
  const gpu = clampPct(t?.gpu_load);
  const vramPct = ratioPct(t?.vram_used_gb, t?.vram_total_gb);
  const ramPct = ratioPct(t?.ram_used_gb, t?.ram_total_gb);

  // Live engine rows from get_engine_status (5s poll); empty until first load.
  const engineRows = engine ? [
    { name: 'VRAM Pager', status: engine.pager_status },
    { name: 'Governor', status: engine.governor_status },
    { name: 'Scheduler', status: engine.scheduler_queue_depth > 0 ? `Busy (${engine.scheduler_queue_depth})` : 'Idle' },
    { name: 'Active Jobs', status: engine.active_jobs > 0 ? `${engine.active_jobs} running` : 'Idle' },
    { name: 'Models Tracked', status: `${engine.loaded_models_count}` },
  ] : [];

  return (
    <div className="space-y-4 text-text-primary select-none">
      {/* Header */}
      <div className="flex items-center justify-between pb-2.5 border-b border-[var(--color-border)]">
        <div className="flex items-center gap-2">
          <Activity size={16} className="text-primary" strokeWidth={2.2} />
          <h2 className="font-bold text-xs tracking-tight uppercase">System Monitor</h2>
        </div>
        <span className="flex items-center gap-1.5 px-2.5 py-0.5 rounded-full bg-[var(--color-hover)] text-[10px] text-secondary font-semibold">
          <span className={`w-1.5 h-1.5 rounded-full bg-secondary ${t ? 'animate-pulse' : 'opacity-40'}`} />
          {t ? 'Live' : 'Connecting'}
        </span>
      </div>

      {/* Hardware Load Card (CPU, GPU, VRAM, RAM) */}
      <div className="glass-panel aurora-glass p-3.5 space-y-3">
        <div className="flex items-center justify-between border-b border-[var(--color-border)] pb-2">
          <span className="font-bold text-xs uppercase tracking-tight">Telemetry Load</span>
          <span className="px-2 py-0.5 rounded-full bg-[var(--color-hover)] text-[10px] font-mono font-semibold text-text-secondary">
            {t?.gpu_name || 'GPU'}
          </span>
        </div>

        {/* 2-Column Gauge Layout for CPU and GPU */}
        <div className="grid grid-cols-2 gap-3 py-1">
          <div className="flex flex-col items-center">
            <RadialGraph percent={cpu} preset="cpu" size={68} strokeWidth={6} />
            <span className="text-[11px] font-bold text-text-primary mt-1">CPU Load</span>
          </div>

          <div className="flex flex-col items-center">
            <RadialGraph percent={gpu} preset="gpu" size={68} strokeWidth={6} />
            <span className="text-[11px] font-bold text-text-primary mt-1">GPU Load</span>
          </div>
        </div>

        {/* Memory Bars: VRAM and RAM */}
        <div className="space-y-2 pt-2 border-t border-[var(--color-border)]">
          <div>
            <div className="flex justify-between text-[10px] mb-1">
              <span className="text-text-secondary font-medium">VRAM Usage</span>
              <span className="font-mono text-text-primary font-bold">
                {t ? `${t.vram_used_gb.toFixed(1)} / ${t.vram_total_gb.toFixed(1)} GB` : '—'}
              </span>
            </div>
            <div className="w-full bg-[var(--color-hover)] rounded-full h-1.5 overflow-hidden">
              <div
                className="bg-primary h-full rounded-full transition-all duration-300"
                style={{ width: `${vramPct}%` }}
              />
            </div>
          </div>
          <div>
            <div className="flex justify-between text-[10px] mb-1">
              <span className="text-text-secondary font-medium">System RAM</span>
              <span className="font-mono text-text-primary font-bold">
                {t ? `${t.ram_used_gb.toFixed(1)} / ${t.ram_total_gb.toFixed(1)} GB` : '—'}
              </span>
            </div>
            <div className="w-full bg-[var(--color-hover)] rounded-full h-1.5 overflow-hidden">
              <div
                className="bg-secondary h-full rounded-full transition-all duration-300"
                style={{ width: `${ramPct}%` }}
              />
            </div>
          </div>
        </div>

        {/* Sensor Specs Row */}
        <div className="grid grid-cols-3 gap-1.5 pt-2 border-t border-[var(--color-border)] text-center">
          <div className="bg-[var(--color-hover)] p-1.5 rounded-xl flex flex-col items-center justify-center">
            <span className="font-bold text-xs text-text-primary font-mono">{fmt(t?.temp_c, 0, '°C')}</span>
            <span className="text-[9px] text-text-secondary uppercase mt-0.5 font-semibold">Temp</span>
          </div>
          <div className="bg-[var(--color-hover)] p-1.5 rounded-xl flex flex-col items-center justify-center">
            <span className="font-bold text-xs text-text-primary font-mono">{fmt(t?.power_w, 0, 'W')}</span>
            <span className="text-[9px] text-text-secondary uppercase mt-0.5 font-semibold">Power</span>
          </div>
          <div className="bg-[var(--color-hover)] p-1.5 rounded-xl flex flex-col items-center justify-center">
            <span className="font-bold text-xs text-text-primary font-mono">{fmt(t?.fan_percent, 0, '%')}</span>
            <span className="text-[9px] text-text-secondary uppercase mt-0.5 font-semibold">Fan</span>
          </div>
        </div>
      </div>

      {/* Engine Status Card (live get_engine_status data) */}
      <div className="glass-panel aurora-glass p-3 space-y-2">
        <div className="flex items-center justify-between border-b border-[var(--color-border)] pb-1.5">
          <span className="font-bold text-xs">Engine Status</span>
          <div className="flex items-center gap-1 text-[10px] text-secondary font-bold px-2 py-0.5 rounded-full bg-[var(--color-hover)]">
            <CheckCircle2 size={12} />
            {engine ? 'Operational' : 'Connecting'}
          </div>
        </div>
        <div className="space-y-1 text-[10px]">
          {engineRows.length === 0 ? (
            <p className="text-text-secondary">Waiting for engine…</p>
          ) : engineRows.map(item => (
            <div key={item.name} className="flex items-center justify-between">
              <span className="text-text-secondary font-medium">{item.name}</span>
              <span className="text-secondary font-semibold flex items-center gap-1">
                <span className="w-1.5 h-1.5 rounded-full bg-secondary" />
                {item.status}
              </span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
};