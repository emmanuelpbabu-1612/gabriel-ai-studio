import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export interface TelemetrySnapshot {
  cpu_load: number;
  gpu_load: number;
  gpu_name: string;
  vram_used_gb: number;
  vram_total_gb: number;
  ram_used_gb: number;
  ram_total_gb: number;
  temp_c: number | null;
  power_w: number | null;
  fan_percent: number | null;
}

type TelemetryPayload = Partial<TelemetrySnapshot> & {
  gpu_util_percent?: number;
  vram_used_bytes?: number;
  vram_total_bytes?: number;
  ram_used_bytes?: number;
  ram_total_bytes?: number;
  cpu_usage_percent?: number;
};

const bytesToGb = (bytes?: number) => (bytes == null ? 0 : bytes / 1024 ** 3);

function normalizeTelemetry(payload: TelemetryPayload): TelemetrySnapshot {
  return {
    cpu_load: payload.cpu_load ?? payload.cpu_usage_percent ?? 0,
    gpu_load: payload.gpu_load ?? payload.gpu_util_percent ?? 0,
    gpu_name: payload.gpu_name ?? 'GPU',
    vram_used_gb: payload.vram_used_gb ?? bytesToGb(payload.vram_used_bytes),
    vram_total_gb: payload.vram_total_gb ?? bytesToGb(payload.vram_total_bytes),
    ram_used_gb: payload.ram_used_gb ?? bytesToGb(payload.ram_used_bytes),
    ram_total_gb: payload.ram_total_gb ?? bytesToGb(payload.ram_total_bytes),
    temp_c: payload.temp_c ?? null,
    power_w: payload.power_w ?? null,
    fan_percent: payload.fan_percent ?? null,
  };
}

export function useTelemetry() {
  const [telemetry, setTelemetry] = useState<TelemetrySnapshot | null>(null);

  useEffect(() => {
    let cancelled = false;

    // Initial value so the UI doesn't wait a full second
    invoke<TelemetryPayload>('get_telemetry')
      .then((s) => !cancelled && setTelemetry((prev) => prev ?? normalizeTelemetry(s)))
      .catch(console.error);

    const unlistenPromise = listen<TelemetryPayload>('telemetry-update', (e) => {
      if (!cancelled) setTelemetry(normalizeTelemetry(e.payload));
    });

    return () => {
      cancelled = true;
      unlistenPromise.then((fn) => fn());
    };
  }, []);

  return telemetry;
}