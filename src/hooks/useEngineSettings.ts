import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

export type EngineMode = 'balanced' | 'performance' | 'efficiency';

export type EngineSettings = {
  engine_mode: EngineMode;
  display_name: string;
  vram_high_watermark: number;
  vram_low_watermark: number;
  bandwidth_ceiling_percent: number;
  max_loaded_models: number;
  auto_load_on_request: boolean;
  idle_offload_after_secs: number;
  app_title: string;
  startup_route: string;
};

export function useAppSettings() {
  const [settings, setSettings] = useState<EngineSettings | null>(null);

  const refresh = useCallback(async () => {
    setSettings(await invoke<EngineSettings>('get_engine_settings'));
  }, []);

  useEffect(() => { refresh().catch(console.error); }, [refresh]);

  const updateAppTitle = useCallback(async (title: string) => {
    await invoke<string>('set_app_title', { title });
    await refresh();
  }, [refresh]);

  const updateStartupRoute = useCallback(async (route: string) => {
    await invoke<string>('set_startup_route', { route });
    await refresh();
  }, [refresh]);

  return { settings, updateAppTitle, updateStartupRoute, refresh };
}

export function useEngineMode() {
  const [mode, setMode] = useState<EngineMode>('balanced');

  const refresh = useCallback(async () => {
    setMode(await invoke<EngineMode>('get_engine_mode'));
  }, []);

  useEffect(() => { refresh().catch(console.error); }, [refresh]);

  const updateMode = useCallback(async (next: EngineMode) => {
    const saved = await invoke<EngineMode>('set_engine_mode', { mode: next });
    setMode(saved);
  }, []);

  return { mode, updateMode, refresh };
}

export function useProfile() {
  const [name, setName] = useState('Jackson');

  const refresh = useCallback(async () => {
    setName(await invoke<string>('get_profile'));
  }, []);

  useEffect(() => { refresh().catch(console.error); }, [refresh]);

  const updateName = useCallback(async (next: string) => {
    setName(await invoke<string>('set_profile', { name: next }));
  }, []);

  return { name, updateName, refresh };
}

export function usePagerStatus() {
  const [status, setStatus] = useState<'active' | 'degraded' | 'inactive'>('inactive');
  useEffect(() => {
    invoke<'active' | 'degraded' | 'inactive'>('get_pager_status').then(setStatus).catch(console.error);
  }, []);
  return status;
}

export function useGovernorSettings() {
  const [settings, setSettings] = useState<EngineSettings | null>(null);
  const refresh = useCallback(async () => {
    setSettings(await invoke<EngineSettings>('get_engine_settings'));
  }, []);
  useEffect(() => { refresh().catch(console.error); }, [refresh]);
  const updateWatermarks = useCallback(async (high: number, low: number) => {
    setSettings(await invoke<EngineSettings>('set_engine_watermarks', { high, low }));
  }, []);
  return { settings, updateWatermarks, refresh };
}