import { useId, useMemo, useState, type FC } from 'react';
import {
  Area,
  AreaChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts';

export interface TsPoint {
  /** Epoch millis of the real sample — the x-axis is true wall-clock time. */
  t: number;
  v: number;
}

export interface TsSeriesInput {
  name: string;
  color: string;
  points: TsPoint[];
  unit: string;
  decimals?: number;
}

interface TimeSeriesChartProps {
  /** One entry per series; all series are expected to share the same 1s tick
   * cadence and are merged by sample index. Timestamps come from the first
   * series. Nothing is interpolated or fabricated. */
  series: TsSeriesInput[];
  height?: number;
  /** Fixed y-domain (e.g. [0, 100] for %). Defaults to padded data range. */
  yDomain?: [number, number];
  yTickFormatter?: (v: number) => string;
}

const fmtHM = (t: number) => {
  const d = new Date(t);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
};

const fmtHMS = (t: number) => {
  const d = new Date(t);
  return `${fmtHM(t)}:${String(d.getSeconds()).padStart(2, '0')}`;
};

const sanitizeId = (id: string) => id.replace(/[^a-zA-Z0-9_-]/g, '');

interface TipProps {
  active?: boolean;
  payload?: Array<{ name?: string | number; value?: number | string; color?: string }>;
  label?: number | string;
}

const TsTooltip: FC<TipProps & { hovered: string | null; decimalsOf: (name: string) => number; unitOf: (name: string) => string }> = ({
  active,
  payload,
  label,
  hovered,
  decimalsOf,
  unitOf,
}) => {
  if (!active || !payload || payload.length === 0 || typeof label !== 'number') return null;
  return (
    <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-card)] px-2.5 py-2 shadow-lg">
      <div className="font-mono text-[10px] font-bold text-text-secondary mb-1">{fmtHMS(label)}</div>
      {payload.map((entry) => {
        const name = String(entry.name ?? '');
        const isHovered = hovered !== null && hovered === name;
        return (
          <div
            key={name}
            className="flex items-center gap-1.5 font-mono text-[11px]"
            style={{ fontWeight: isHovered ? 800 : 500, color: entry.color }}
          >
            <span className="w-2 h-2 rounded-[3px] shrink-0" style={{ background: entry.color }} />
            <span className="text-text-secondary font-sans font-medium">{name}</span>
            <span className="ml-auto pl-3">
              {typeof entry.value === 'number' ? entry.value.toFixed(decimalsOf(name)) : entry.value}
              {unitOf(name)}
            </span>
          </div>
        );
      })}
    </div>
  );
};

/**
 * Shared Grafana-style time-series panel: gradient areas, real-time x-axis,
 * hover crosshair + tooltip, clickable legend, live Mean/Last/Max/Min table.
 * Data-agnostic — every number shown comes from the `series` buffers.
 */
export const TimeSeriesChart: FC<TimeSeriesChartProps> = ({
  series,
  height = 170,
  yDomain,
  yTickFormatter = (v: number) => String(Math.round(v)),
}) => {
  const rawId = useId();
  const uid = useMemo(() => sanitizeId(rawId), [rawId]);
  const [hidden, setHidden] = useState<ReadonlySet<string>>(new Set());
  const [hovered, setHovered] = useState<string | null>(null);

  const visible = useMemo(() => series.filter((s) => !hidden.has(s.name)), [series, hidden]);

  const rows = useMemo(() => {
    if (series.length === 0) return [];
    const n = Math.max(...series.map((s) => s.points.length));
    const out: Array<Record<string, number>> = [];
    for (let i = 0; i < n; i++) {
      const row: Record<string, number> = { t: series[0].points[i]?.t ?? 0 };
      for (const s of series) {
        const p = s.points[i];
        if (p) row[s.name] = p.v;
      }
      out.push(row);
    }
    return out.filter((r) => r.t > 0);
  }, [series]);

  const domain = useMemo<[number, number]>(() => {
    if (yDomain) return yDomain;
    const vals: number[] = [];
    for (const s of visible) for (const p of s.points) if (Number.isFinite(p.v)) vals.push(p.v);
    if (vals.length === 0) return [0, 1];
    let lo = Math.min(...vals);
    let hi = Math.max(...vals);
    const pad = (hi - lo) * 0.15 || Math.max(Math.abs(hi) * 0.05, 1e-6);
    return [lo - pad, hi + pad];
  }, [visible, yDomain]);

  const stats = useMemo(
    () =>
      series.map((s) => {
        const vals = s.points.map((p) => p.v).filter((v) => Number.isFinite(v));
        if (vals.length === 0) return { name: s.name, color: s.color, unit: s.unit, decimals: s.decimals ?? 1, mean: null as number | null, last: null as number | null, max: null as number | null, min: null as number | null };
        const sum = vals.reduce((a, b) => a + b, 0);
        return {
          name: s.name,
          color: s.color,
          unit: s.unit,
          decimals: s.decimals ?? 1,
          mean: sum / vals.length,
          last: vals[vals.length - 1],
          max: Math.max(...vals),
          min: Math.min(...vals),
        };
      }),
    [series],
  );

  const decimalsOf = (name: string) => series.find((s) => s.name === name)?.decimals ?? 1;
  const unitOf = (name: string) => series.find((s) => s.name === name)?.unit ?? '';

  if (rows.length < 2) {
    return (
      <div
        className="w-full flex items-center justify-center rounded-xl bg-[var(--color-hover)] border border-[var(--color-border)] text-[11px] text-text-secondary"
        style={{ height }}
      >
        Collecting samples…
      </div>
    );
  }

  const fmtVal = (v: number | null, decimals: number, unit: string) =>
    v === null ? '—' : `${v.toFixed(decimals)}${unit}`;

  return (
    <div className="w-full">
      {/* Legend: swatch + name per series, click toggles visibility */}
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 pb-1.5">
        {series.map((s) => {
          const isHidden = hidden.has(s.name);
          return (
            <button
              key={s.name}
              type="button"
              onClick={() =>
                setHidden((prev) => {
                  const next = new Set(prev);
                  if (next.has(s.name)) next.delete(s.name);
                  else next.add(s.name);
                  return next;
                })
              }
              title={isHidden ? `Show ${s.name}` : `Hide ${s.name}`}
              className="flex items-center gap-1.5 text-[10px] font-semibold cursor-pointer transition-opacity"
              style={{ opacity: isHidden ? 0.35 : 1, color: 'var(--color-text-secondary)' }}
            >
              <span className="w-2.5 h-2.5 rounded-[3px] shrink-0" style={{ background: s.color }} />
              {s.name}
            </button>
          );
        })}
      </div>

      <div style={{ height }}>
        <ResponsiveContainer width="100%" height="100%">
          <AreaChart data={rows} margin={{ top: 4, right: 6, bottom: 0, left: 0 }}>
            <defs>
              {visible.map((s) => (
                <linearGradient key={s.name} id={`${uid}-${sanitizeId(s.name)}`} x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stopColor={s.color} stopOpacity={0.22} />
                  <stop offset="100%" stopColor={s.color} stopOpacity={0} />
                </linearGradient>
              ))}
            </defs>
            <CartesianGrid vertical={false} stroke="rgba(148,163,184,0.14)" />
            <XAxis
              dataKey="t"
              type="number"
              domain={['dataMin', 'dataMax']}
              tickFormatter={fmtHM}
              minTickGap={48}
              tick={{ fill: '#8b93a7', fontSize: 10 }}
              tickLine={false}
              axisLine={{ stroke: 'rgba(148,163,184,0.25)' }}
            />
            <YAxis
              width={40}
              domain={domain}
              tickFormatter={yTickFormatter}
              tick={{ fill: '#8b93a7', fontSize: 10 }}
              tickLine={false}
              axisLine={false}
            />
            <Tooltip
              content={<TsTooltip hovered={hovered} decimalsOf={decimalsOf} unitOf={unitOf} />}
              cursor={{ stroke: 'rgba(148,163,184,0.55)', strokeDasharray: '4 4' }}
              allowEscapeViewBox={{ x: false, y: false }}
              isAnimationActive={false}
            />
            {visible.map((s) => (
              <Area
                key={s.name}
                // Monotone, not spline: passes through every real sample with
                // no overshoot, so dense 1s data reads crisp like the Grafana
                // reference instead of inventing curves between points.
                type="monotone"
                dataKey={s.name}
                name={s.name}
                stroke={s.color}
                strokeWidth={1.5}
                strokeLinejoin="round"
                strokeLinecap="round"
                fill={`url(#${uid}-${sanitizeId(s.name)})`}
                dot={false}
                activeDot={{ r: 3.5, strokeWidth: 0 }}
                isAnimationActive={false}
                connectNulls
                onMouseEnter={() => setHovered(s.name)}
                onMouseLeave={() => setHovered(null)}
              />
            ))}
          </AreaChart>
        </ResponsiveContainer>
      </div>

      {/* Stats table: fixed layout + tabular numerals so every column stays
          aligned as values update live. Computed from the live buffers. */}
      <table className="w-full table-fixed">
        <thead>
          <tr className="text-[10px] font-bold uppercase tracking-wider text-text-secondary/70">
            <th className="w-[34%] text-left font-bold" />
            <th className="text-right font-bold">Mean</th>
            <th className="text-right font-bold">Last</th>
            <th className="text-right font-bold">Max</th>
            <th className="text-right font-bold">Min</th>
          </tr>
        </thead>
        <tbody>
          {stats.map((row) => (
            <tr
              key={row.name}
              className="text-[10px]"
              style={{ opacity: hidden.has(row.name) ? 0.35 : 1 }}
            >
              <td className="py-px">
                <span className="flex items-center gap-1.5 font-semibold text-text-secondary truncate">
                  <span className="w-2 h-2 rounded-[3px] shrink-0" style={{ background: row.color }} />
                  <span className="truncate">{row.name}</span>
                </span>
              </td>
              <td className="py-px text-right font-mono tabular-nums text-text-primary">{fmtVal(row.mean, row.decimals, row.unit)}</td>
              <td className="py-px text-right font-mono tabular-nums text-text-primary">{fmtVal(row.last, row.decimals, row.unit)}</td>
              <td className="py-px text-right font-mono tabular-nums text-text-secondary">{fmtVal(row.max, row.decimals, row.unit)}</td>
              <td className="py-px text-right font-mono tabular-nums text-text-secondary">{fmtVal(row.min, row.decimals, row.unit)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
};
