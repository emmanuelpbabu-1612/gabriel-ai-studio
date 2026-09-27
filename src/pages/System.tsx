import type { FC } from 'react';
import { useState, useEffect, useMemo, useRef } from 'react';
import {
  RotateCcw,
  Activity,
  HardDrive,
  MemoryStick,
  RotateCw,
  SquareStop,
  Sliders,
  CheckCircle2,
  AlertTriangle,
  Info,
  Cpu,
  Loader2,
} from 'lucide-react';
import { StatusDot, ModelBadge, RadialGraph, TimeSeriesChart, type TsPoint, type TsSeriesInput } from '../components/shared';
import { Select } from '../components/shared/Select';
import { useTelemetry } from '../hooks/useTelemetry';
import { useModels } from '../hooks/useModels';
import { useEngineMode } from '../hooks/useEngineSettings';
import { useNotifications } from '../hooks/useNotifications';
import { useEngineStatus, useHardwareSpecs } from '../hooks/useSystem';
import { useSettingsActions } from '../hooks/useSettingsActions';
import { useCardSpotlight } from '../hooks/useCardSpotlight';

const engineModes = ['balanced', 'performance', 'efficiency'] as const;

// Rolling history buffer of timestamped samples for time-series charts.
// 600 samples @1s = 10-minute window at full 1s resolution (the densest the
// telemetry pipeline emits). Every point is a real telemetry tick —
// timestamps are wall-clock sample time, so the x-axis is true time.
function useRollingSeries(getValue: () => number, maxLength = 600) {
  const [history, setHistory] = useState<TsPoint[]>([]);
  const getValueRef = useRef(getValue);
  getValueRef.current = getValue;

  useEffect(() => {
    const interval = setInterval(() => {
      const point: TsPoint = { t: Date.now(), v: getValueRef.current() };
      setHistory(prev => {
        const next = [...prev, point];
        return next.slice(-maxLength);
      });
    }, 1000); // Update every second
    return () => clearInterval(interval);
  }, [maxLength]);

  return history;
}

export const System: FC = () => {
  const { mode, updateMode } = useEngineMode();
  const telemetry = useTelemetry();
  const { models, load, unload, refresh: refreshModels } = useModels();
  const { notifications } = useNotifications();
  const { specs: hardwareSpecs } = useHardwareSpecs();
  const { status: engineStatus } = useEngineStatus();
  const { restartEngine, loading: restartLoading } = useSettingsActions();

  // Rolling timestamped history for time-series charts
  const gpuUtilSeries = useRollingSeries(() => telemetry?.gpu_load ?? 0);
  const vramUsedSeries = useRollingSeries(() => telemetry?.vram_used_gb ?? 0);
  const vramTotalSeries = useRollingSeries(() => telemetry?.vram_total_gb ?? 0);
  const ramUsedSeries = useRollingSeries(() => telemetry?.ram_used_gb ?? 0);
  const ramTotalSeries = useRollingSeries(() => telemetry?.ram_total_gb ?? 0);

  // "Free" is derived per-tick from two real samples (total − used), never
  // estimated. Timestamps track the used-sample tick (same 1s cadence).
  const freeOf = (used: TsPoint[], total: TsPoint[]): TsPoint[] =>
    used.map((p, i) => ({ t: p.t, v: Math.max(0, (total[i]?.v ?? 0) - p.v) }));
  const ramFreeSeries = useMemo(() => freeOf(ramUsedSeries, ramTotalSeries), [ramUsedSeries, ramTotalSeries]);
  const vramFreeSeries = useMemo(() => freeOf(vramUsedSeries, vramTotalSeries), [vramUsedSeries, vramTotalSeries]);

  // Live clock so uptime increments every second between data refreshes.
  const [nowMs, setNowMs] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNowMs(Date.now()), 1000);
    return () => clearInterval(t);
  }, []);

  // Re-poll the model registry so residency/uptime track backend reality.
  // (useModels fetches once on mount; without this the table is frozen.)
  useEffect(() => {
    const t = setInterval(() => refreshModels().catch(console.error), 5000);
    return () => clearInterval(t);
  }, [refreshModels]);

  // No NVIDIA GPU on this machine → the "no-gpu-backend" fallback reports
  // zeros. Never fake movement there; mark the cards unavailable instead.
  const gpuUnavailable = (telemetry?.gpu_name ?? '') === 'no-gpu-backend';

  const formatUptime = (totalSecs: number) =>
    totalSecs > 3600
      ? `${Math.floor(totalSecs / 3600)}h ${Math.floor((totalSecs % 3600) / 60)}m`
      : totalSecs > 60
        ? `${Math.floor(totalSecs / 60)}m ${totalSecs % 60}s`
        : `${totalSecs}s`;

  // Derive process table from models
  const processTableData = models.map(model => ({
    id: model.id,
    backendType: model.model_type,
    name: model.id,
    type: (model.model_type === 'llm' || model.model_type === 'embedding' ? 'LLM' :
          model.model_type === 'image' ? 'Image' : 'Voice') as 'LLM' | 'Image' | 'Voice',
    status: (model.residency === 'gpu' ? 'running' : 'idle') as 'running' | 'idle',
    residency: model.residency,
    // Residency pill mirrors the Voice page's Status logic: the backend marks
    // any loaded model residency=gpu, but a 0-VRAM model (e.g. CPU-run TTS)
    // never touches VRAM — label by where it physically runs.
    residencyLabel: (model.residency === 'gpu' && model.vram_bytes > 0 ? 'VRAM' : 'RAM') as 'VRAM' | 'RAM',
    vram: model.residency === 'gpu' ? `${(model.vram_bytes / 1024 ** 3).toFixed(1)} GB` : '0 GB',
    // Per-model host-RAM footprint is not tracked by the backend (snapshot
    // zeroes vram_bytes for cpu residency), so show an em dash, not fake 0 GB.
    ram: '—',
    // Uptime from the real resident-since timestamp, ticking live. Non-resident
    // models have no uptime to show.
    uptime: model.residency === 'gpu' && model.loaded_at_unix > 0
      ? formatUptime(Math.max(0, Math.floor((nowMs - model.loaded_at_unix * 1000) / 1000)))
      : '—',
  }));

  // Engine status derived from the real get_engine_status signal.
  // (No separate Memory Manager health signal exists on the backend, so no
  // such row is shown rather than a hardcoded "Running".)
  const engineStatusItems = engineStatus ? [
    { name: 'Model Manager', status: `${engineStatus.loaded_models_count} tracked` },
    { name: 'Bandwidth Governor', status: engineStatus.governor_status },
    { name: 'VRAM Pager', status: engineStatus.pager_status || 'Active' },
    { name: 'Scheduler', status: engineStatus.scheduler_queue_depth > 0 ? 'Busy' : 'Idle' },
    { name: 'Active Jobs', status: engineStatus.active_jobs > 0 ? 'Running' : 'Idle' },
  ] : [];

  // System logs from notifications
  const systemLogs = notifications.slice(0, 20).map(n => ({
    id: n.id.toString(),
    type: n.level === 'error' ? 'warning' : n.level === 'success' ? 'success' : 'info',
    message: `${n.title}: ${n.message}`,
    timestamp: new Date(n.created_at_unix * 1000).toLocaleTimeString(),
  }));

  return (
    <div className="flex flex-col h-full max-w-6xl mx-auto space-y-4 text-text-primary">
      {/* Top Controls Header */}
      <div className="flex flex-wrap items-center justify-between gap-3 glass-panel aurora-glass p-3.5">
        <div className="flex items-center gap-3">
          <Sliders className="text-primary" size={20} />
          <h1 className="font-bold text-lg text-text-primary">Engine Architecture & Telemetry</h1>
        </div>
        <div className="flex items-center gap-3">
          <Select
            options={engineModes.map(m => ({ value: m, label: `Mode: ${m}` }))}
            value={mode}
            onChange={(v) => updateMode(v as typeof mode).catch(console.error)}
            className="w-44"
          />
          <button onClick={() => restartEngine().catch(console.error)} disabled={restartLoading === 'restart_engine'} className="btn-secondary min-w-[150px] py-1.5 px-3 flex items-center gap-1.5 text-xs font-semibold cursor-pointer disabled:opacity-50">
            {restartLoading === 'restart_engine' ? (
              <Loader2 size={14} strokeWidth={2} className="animate-spin" />
            ) : (
              <RotateCcw size={14} strokeWidth={2} />
            )}
            Restart Engine
          </button>
        </div>
      </div>

      {/* Main Content Workspace */}
      <div className="flex-1 flex gap-4 min-h-0 overflow-hidden">
        {/* Left Column */}
        <div className="flex-1 flex flex-col gap-4 overflow-y-auto min-w-0 pr-1">

          {/* Telemetry Cards: GPU Util, VRAM Usage, System RAM */}
          <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
            <TelemetryCard
              title="GPU Utilization"
              primaryLabel="Utilization"
              secondaryLabel="Peak (window)"
              primaryValue={`${Math.round(telemetry?.gpu_load ?? 0)}%`}
              secondaryValue={gpuUtilSeries.length > 0 ? `${Math.round(Math.max(...gpuUtilSeries.map(p => p.v)))}%` : '—'}
              icon={Activity}
              iconColor="primary"
              series={[{ name: 'Utilization', color: '#635BFF', points: gpuUtilSeries, unit: '%', decimals: 0 }]}
              yDomain={[0, 100]}
              yTickFormatter={(v) => `${Math.round(v)}`}
              unavailable={gpuUnavailable}
            />
            <TelemetryCard
              title="VRAM Usage"
              primaryLabel="Used"
              secondaryLabel="Free"
              primaryValue={`${(telemetry?.vram_used_gb ?? 0).toFixed(1)} / ${(telemetry?.vram_total_gb ?? 0).toFixed(1)} GB`}
              secondaryValue={`${((telemetry?.vram_total_gb ?? 0) - (telemetry?.vram_used_gb ?? 0)).toFixed(1)} GB free`}
              icon={HardDrive}
              iconColor="primary"
              series={[
                { name: 'Used', color: '#635BFF', points: vramUsedSeries, unit: ' GB' },
                { name: 'Free', color: '#10B981', points: vramFreeSeries, unit: ' GB' },
              ]}
              yTickFormatter={(v) => v.toFixed(0)}
              unavailable={gpuUnavailable}
            />
            <TelemetryCard
              title="RAM Usage"
              primaryLabel="Used"
              secondaryLabel="Free"
              primaryValue={`${(telemetry?.ram_used_gb ?? 0).toFixed(1)} / ${(telemetry?.ram_total_gb ?? 0).toFixed(1)} GB`}
              secondaryValue={`${((telemetry?.ram_total_gb ?? 0) - (telemetry?.ram_used_gb ?? 0)).toFixed(1)} GB free`}
              icon={MemoryStick}
              iconColor="secondary"
              series={[
                { name: 'Used', color: '#10B981', points: ramUsedSeries, unit: ' GB' },
                { name: 'Free', color: '#635BFF', points: ramFreeSeries, unit: ' GB' },
              ]}
              yTickFormatter={(v) => v.toFixed(0)}
            />
          </div>

          {/* Process Table */}
          <div className="glass-panel aurora-glass flex min-h-0 flex-col overflow-hidden">
            <div className="px-4 py-3 border-b border-[var(--color-border)] flex items-center justify-between">
              <h2 className="font-bold text-sm text-text-primary">Process Table</h2>
              <span className="badge badge-gray font-mono">{processTableData.length} active processes</span>
            </div>

            <div className="min-h-0 max-h-[34vh] overflow-auto">
              <table className="text-xs w-full" style={{ minWidth: '780px' }}>
                <thead>
                  <tr className="text-left text-text-secondary border-b border-[var(--color-border)] bg-[var(--color-hover)]">
                    <th className="p-3 font-semibold">Name</th>
                    <th className="p-3 font-semibold">Type & Status</th>
                    <th className="p-3 font-semibold">VRAM (GPU)</th>
                    <th className="p-3 font-semibold">Host RAM (CPU)</th>
                    <th className="p-3 font-semibold">Uptime</th>
                    <th className="p-3 font-semibold text-right">Actions</th>
                  </tr>
                </thead>
                <tbody>
                  {processTableData.map((process, i) => (
                    <tr
                      key={process.name}
                      className={`border-b border-[var(--color-border)] hover:bg-[var(--color-hover)] transition-colors ${
                        i === processTableData.length - 1 ? 'border-0' : ''
                      }`}
                    >
                      <td className="p-3 font-bold text-text-primary truncate max-w-[200px]">
                        {process.name}
                      </td>

                      <td className="p-3">
                        <div className="flex items-center gap-1.5 flex-nowrap">
                          <ModelBadge type={process.type} status={process.status} size="sm" />
                          <span
                            className="pill text-[11px] font-semibold"
                            title={`Residency: ${process.residencyLabel === 'VRAM' ? 'VRAM' : 'host RAM'} (backend: ${process.residency})`}
                            style={{
                              backgroundColor: 'rgba(156,163,175,0.12)',
                              color: '#9CA3AF',
                            }}
                          >
                            {process.residencyLabel}
                          </span>
                        </div>
                      </td>

                      <td className="p-3 font-mono text-text-secondary font-semibold whitespace-nowrap">{process.vram}</td>
                      <td className="p-3 font-mono text-text-secondary font-semibold whitespace-nowrap">{process.ram}</td>
                      <td className="p-3 font-mono text-text-secondary font-semibold whitespace-nowrap">{process.uptime}</td>
                      <td className="p-3 text-right">
                        <div className="flex items-center justify-end gap-1">
                          <button
                            onClick={() => load(process.id, process.backendType).catch(console.error)}
                            className="p-1.5 rounded-lg text-text-secondary hover:bg-[var(--color-hover)] hover:text-text-primary transition-colors cursor-pointer"
                            aria-label="Restart"
                          >
                            <RotateCw size={13} />
                          </button>
                          <button
                            onClick={() => unload(process.id).catch(console.error)}
                            className="p-1.5 rounded-lg text-text-secondary hover:bg-[var(--color-hover)] hover:text-text-primary transition-colors cursor-pointer"
                            aria-label="Stop"
                          >
                            <SquareStop size={13} />
                          </button>
                        </div>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>

          {/* GPU Details & Telemetry Gauges */}
          <div className="glass-panel aurora-glass flex min-h-0 flex-col p-4 space-y-4 overflow-hidden">
            <h2 className="font-bold text-sm text-text-primary">GPU Details & Thermal Telemetry</h2>
            <div className="min-h-0 max-h-[42vh] overflow-y-auto grid grid-cols-1 md:grid-cols-2 gap-4">
              <div className="space-y-2.5 text-xs">
                <div className="flex justify-between border-b border-[var(--color-border)] pb-1.5">
                  <span className="text-text-secondary">GPU Hardware</span>
                  <span className="font-bold text-text-primary">{telemetry?.gpu_name ?? hardwareSpecs?.gpu_name ?? 'Unknown'}</span>
                </div>
                <div className="flex justify-between border-b border-[var(--color-border)] pb-1.5">
                  <span className="text-text-secondary">Driver Version</span>
                  <span className="font-mono font-bold text-text-primary">{hardwareSpecs?.driver_version ?? 'N/A'}</span>
                </div>
                <div className="flex justify-between border-b border-[var(--color-border)] pb-1.5">
                  <span className="text-text-secondary">CUDA Compute</span>
                  <span className="font-mono font-bold text-text-primary">{hardwareSpecs?.cuda_version ?? 'N/A'}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-text-secondary">Total Memory</span>
                  <span className="font-mono font-bold text-text-primary">
                    {(telemetry?.vram_total_gb ?? hardwareSpecs?.gpu_vram_gb ?? 0).toFixed(1)} GB
                  </span>
                </div>
              </div>

              {/* Radial Gauges Stack */}
              <div className="flex flex-col gap-3">
                <div className="p-3 rounded-xl bg-[var(--color-hover)] border border-[var(--color-border)] flex items-center gap-4">
                  <RadialGraph
                    percent={telemetry?.temp_c ? Math.round((telemetry.temp_c / 100) * 100) : 0}
                    preset="temp"
                    size={72}
                    showCenterText={true}
                  />
                  <div>
                    <div className="text-[10px] font-bold text-text-secondary uppercase tracking-wider">GPU Temperature</div>
                    <div className="font-mono font-bold text-base text-text-primary mt-0.5">
                      {telemetry?.temp_c ? `${telemetry.temp_c}°C` : 'N/A'}
                    </div>
                    <div className="text-[10px] text-text-secondary mt-0.5">Target Max: 85°C · Optimal</div>
                  </div>
                </div>

                <div className="p-3 rounded-xl bg-[var(--color-hover)] border border-[var(--color-border)] flex items-center gap-4">
                  <RadialGraph
                    percent={telemetry?.power_w ? Math.round((telemetry.power_w / 450) * 100) : 0}
                    preset="power"
                    size={72}
                    showCenterText={true}
                  />
                  <div>
                    <div className="text-[10px] font-bold text-text-secondary uppercase tracking-wider">Power Draw</div>
                    <div className="font-mono font-bold text-base text-text-primary mt-0.5">
                      {telemetry?.power_w ? `${telemetry.power_w}W` : 'N/A'}
                    </div>
                    <div className="text-[10px] text-text-secondary mt-0.5">TDP Cap: 450W</div>
                  </div>
                </div>
              </div>
            </div>
          </div>

        </div>

        {/* Right Info Sidebar */}
        <div className="w-80 flex-shrink-0 glass-panel aurora-glass p-4 overflow-y-auto space-y-4 text-xs">
          <div className="space-y-2">
            <div className="font-bold text-text-primary border-b border-[var(--color-border)] pb-2 flex items-center gap-1.5">
              <Cpu size={14} className="text-primary" />
              Hardware Specifications
            </div>
            <div className="space-y-1.5 text-text-secondary">
              {[
                { label: 'GPU', value: telemetry?.gpu_name ?? hardwareSpecs?.gpu_name ?? 'Unknown' },
                { label: 'VRAM', value: `${(telemetry?.vram_total_gb ?? hardwareSpecs?.gpu_vram_gb ?? 0).toFixed(1)} GB`, mono: true },
                { label: 'CPU', value: hardwareSpecs?.cpu_name ?? 'Unknown' },
                { label: 'CPU Cores', value: `${hardwareSpecs?.cpu_cores ?? 0} cores / ${hardwareSpecs?.cpu_threads ?? 0} threads`, mono: true },
                { label: 'System RAM', value: `${(hardwareSpecs?.total_ram_gb ?? 0).toFixed(1)} GB`, mono: true },
              ].map(row => (
                <div key={row.label} className="flex justify-between py-1 border-b border-[var(--color-border)] last:border-0">
                  <span>{row.label}</span>
                  <span className={`font-bold text-text-primary ${row.mono ? 'font-mono' : ''}`}>{row.value}</span>
                </div>
              ))}
            </div>
          </div>

          <div className="pt-3 border-t border-[var(--color-border)] space-y-2">
            <div className="font-bold text-text-primary flex items-center justify-between">
              <span>Backend Engine Status</span>
              <CheckCircle2 size={14} className="text-secondary" />
            </div>
            <div className="space-y-1.5">
              {engineStatusItems.map(item => (
                <div key={item.name} className="flex items-center justify-between p-2 rounded-xl bg-[var(--color-hover)]">
                  <span className="text-text-primary font-semibold text-[11px]">{item.name}</span>
                  <span className="text-secondary font-bold text-[10px] flex items-center gap-1">
                    <StatusDot status="running" size={5} />
                    {item.status}
                  </span>
                </div>
              ))}
            </div>
          </div>

          <div className="pt-3 border-t border-[var(--color-border)] space-y-2">
            <div className="font-bold text-text-primary">System Logs & Events</div>
            <div className="space-y-2 max-h-[34vh] overflow-y-auto">
              {systemLogs.map(log => {
                const isSuccess = log.type === 'success';
                const isWarning = log.type === 'warning';
                return (
                  <div key={log.id} className="flex items-start gap-2.5 p-2.5 rounded-xl bg-[var(--color-hover)]">
                    <div className={`p-1.5 rounded-lg shrink-0 mt-0.5 ${isSuccess ? 'bg-[var(--color-secondary-bg)] text-secondary' : isWarning ? 'bg-[var(--color-warning-bg)] text-yellow-400' : 'bg-[var(--color-primary-bg)] text-primary'}`}>
                      {isSuccess ? <CheckCircle2 size={13} /> : isWarning ? <AlertTriangle size={13} /> : <Info size={13} />}
                    </div>
                    <div className="flex-1 min-w-0">
                      <p className="font-semibold text-[11px] text-text-primary leading-snug">{log.message}</p>
                      <span className="text-[9px] text-text-secondary font-mono mt-0.5 block">{log.timestamp}</span>
                    </div>
                  </div>
                );
              })}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};

const TelemetryCard: FC<{
  title: string;
  primaryLabel: string;
  secondaryLabel: string;
  primaryValue: string;
  secondaryValue: string;
  icon: React.ComponentType<{ size?: number; className?: string }>;
  iconColor: 'primary' | 'secondary' | 'tertiary';
  /** Real timestamped series rendered by the shared TimeSeriesChart. */
  series: TsSeriesInput[];
  yDomain?: [number, number];
  yTickFormatter?: (v: number) => string;
  /** No real backend behind this card (e.g. no NVIDIA GPU): grey out and say
   * so explicitly instead of drawing a flat 0% chart. */
  unavailable?: boolean;
}> = ({ title, primaryLabel, secondaryLabel, primaryValue, secondaryValue, icon: Icon, series, yDomain, yTickFormatter, unavailable = false }) => {
  const { ref, onPointerMove } = useCardSpotlight();

  return (
    <div ref={ref} onPointerMove={onPointerMove} className={`glass-panel aurora-glass p-3.5 space-y-2 cursor-default${unavailable ? ' opacity-60' : ''}`}>
      <div className="flex items-center gap-2">
        <div className="p-1.5 rounded-lg bg-[var(--color-primary-bg)] text-primary">
          <Icon size={15} />
        </div>
        <div className="text-[10px] font-bold text-text-secondary uppercase tracking-wider">{title}</div>
      </div>

      <div className="flex items-start gap-4">
        <div className="flex-1 min-w-0">
          <div className="flex h-6 items-center gap-1.5">
            <span className="w-2 h-2 rounded-full shrink-0" style={{ background: series[0]?.color ?? '#635BFF' }} />
            <span className="font-mono font-bold text-base text-text-primary truncate">{unavailable ? 'N/A' : primaryValue}</span>
          </div>
          <div className="text-[10px] font-semibold" style={{ color: series[0]?.color ?? '#635BFF' }}>{primaryLabel}</div>
        </div>
        <div className="flex-1 min-w-0">
          <div className="flex h-6 items-center gap-1.5">
            <span className="w-2 h-2 rounded-full shrink-0" style={{ background: series[1]?.color ?? '#10B981' }} />
            <span className="font-mono text-xs text-text-secondary truncate">{unavailable ? 'N/A' : secondaryValue}</span>
          </div>
          <div className="text-[10px] font-semibold" style={{ color: series[1]?.color ?? '#10B981' }}>{secondaryLabel}</div>
        </div>
      </div>

      <div className="pt-1">
        {unavailable ? (
          <div className="h-[90px] flex flex-col items-center justify-center gap-1 rounded-xl bg-[var(--color-hover)] border border-[var(--color-border)] text-center px-2">
            <span className="text-[11px] font-bold text-text-secondary">No GPU detected</span>
            <span className="text-[10px] text-text-secondary">Not available — no NVIDIA GPU detected</span>
          </div>
        ) : (
          <TimeSeriesChart series={series} height={170} yDomain={yDomain} yTickFormatter={yTickFormatter} />
        )}
      </div>
    </div>
  );
};