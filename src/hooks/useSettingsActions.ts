import { useCallback, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

export interface BinaryVerification {
  status: string;
  path: string;
  sha256: string;
  size_bytes: number;
  modified_unix?: number;
}

export function useSettingsActions() {
  const [loading, setLoading] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const executeAction = useCallback(async (
    action: () => Promise<any>,
    actionName: string
  ) => {
    setLoading(actionName);
    setError(null);
    try {
      const result = await action();
      setLoading(null);
      return result;
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setError(message);
      setLoading(null);
      throw err;
    }
  }, []);

  const verifyEngineBinary = useCallback(async () => {
    return executeAction(
      () => invoke<BinaryVerification>('verify_engine_binary'),
      'verify_engine_binary'
    );
  }, [executeAction]);

  const setModelsDirAction = useCallback(async (path: string) => {
    return executeAction(
      () => invoke<string>('set_models_dir', { path }),
      'set_models_dir'
    );
  }, [executeAction]);

  const resetSettings = useCallback(async () => {
    return executeAction(
      () => invoke('reset_settings'),
      'reset_settings'
    );
  }, [executeAction]);

  const clearModelCaches = useCallback(async () => {
    return executeAction(
      () => invoke('clear_model_caches'),
      'clear_model_caches'
    );
  }, [executeAction]);

  const restartEngine = useCallback(async () => {
    return executeAction(
      () => invoke('restart_engine'),
      'restart_engine'
    );
  }, [executeAction]);

  const setBandwidthCeiling = useCallback(async (percent: number) => {
    return executeAction(
      () => invoke('set_bandwidth_ceiling', { percent }),
      'set_bandwidth_ceiling'
    );
  }, [executeAction]);

  const setMaxLoadedModels = useCallback(async (maxModels: number) => {
    return executeAction(
      () => invoke('set_max_loaded_models', { maxModels }),
      'set_max_loaded_models'
    );
  }, [executeAction]);

  const setAutoLoadOnRequest = useCallback(async (enabled: boolean) => {
    return executeAction(
      () => invoke('set_auto_load_on_request', { enabled }),
      'set_auto_load_on_request'
    );
  }, [executeAction]);

  const setIdleOffloadTimeout = useCallback(async (seconds: number) => {
    return executeAction(
      () => invoke('set_idle_offload_timeout', { seconds }),
      'set_idle_offload_timeout'
    );
  }, [executeAction]);

  return {
    loading,
    error,
    verifyEngineBinary,
    setModelsDirAction,
    resetSettings,
    clearModelCaches,
    restartEngine,
    setBandwidthCeiling,
    setMaxLoadedModels,
    setAutoLoadOnRequest,
    setIdleOffloadTimeout,
  };
}