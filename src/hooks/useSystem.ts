import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

export interface HardwareSpecs {
  cpu_name: string;
  cpu_cores: number;
  cpu_threads: number;
  total_ram_gb: number;
  gpu_name: string;
  gpu_vram_gb: number;
  driver_version: string;
  cuda_version: string;
}

export interface EngineStatus {
  pager_status: string;
  governor_status: string;
  scheduler_queue_depth: number;
  active_jobs: number;
  loaded_models_count: number;
}

export function useHardwareSpecs() {
  const [specs, setSpecs] = useState<HardwareSpecs | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setLoading(true);
      const data = await invoke<HardwareSpecs>('get_hardware_specs');
      setSpecs(data);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  return { specs, loading, error, refresh };
}

export function useEngineStatus() {
  const [status, setStatus] = useState<EngineStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setLoading(true);
      const data = await invoke<EngineStatus>('get_engine_status');
      setStatus(data);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
    const interval = setInterval(refresh, 5000); // Poll every 5 seconds
    return () => clearInterval(interval);
  }, [refresh]);

  return { status, loading, error, refresh };
}