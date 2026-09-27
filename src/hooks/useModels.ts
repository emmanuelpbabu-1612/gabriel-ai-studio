import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

export type BackendModel = {
  id: string;
  model_type: 'llm' | 'embedding' | 'image' | 'tts' | 'asr';
  residency: 'gpu' | 'cpu';
  available: boolean;
  vram_bytes: number;
  disk_bytes: number;
  idle_secs: number;
  loaded_at_unix: number;
};

export function useModels() {
  const [models, setModels] = useState<BackendModel[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loadingId, setLoadingId] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const nextModels = await invoke<BackendModel[]>('list_models');
      console.info('[models] list_models returned', nextModels.length, nextModels);
      setModels(nextModels);
      setError(null);
    } catch (cause) {
      console.error('[models] list_models failed', cause);
      setError(cause instanceof Error ? cause.message : String(cause));
      throw cause;
    }
  }, []);

  useEffect(() => {
    refresh().catch(console.error);
  }, [refresh]);

  const registerLocal = useCallback(async (path: string, kind: string) => {
    await invoke('register_local_model', { path, kind });
    await refresh();
  }, [refresh]);

  const runAction = useCallback(async (command: string, modelId: string, modelType?: string) => {
    setLoadingId(modelId);
    try {
      await invoke(command, modelType ? { modelId, modelType } : { modelId });
      await refresh();
    } finally {
      setLoadingId(null);
    }
  }, [refresh]);

  return {
    models,
    error,
    loadingId,
    refresh,
    load: (modelId: string, modelType: string) => runAction('load_model', modelId, modelType),
    unload: (modelId: string) => runAction('unload_model', modelId),
    offload: (modelId: string) => runAction('offload_model', modelId),
    registerLocal,
  };
}