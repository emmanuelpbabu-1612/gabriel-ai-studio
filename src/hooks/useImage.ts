import { useCallback, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

export interface ImageGenParams {
  prompt: string;
  width: number;
  height: number;
  steps: number;
  cfg_scale: number;
  sampler: string;
  negative_prompt?: string;
}

export interface GeneratedImage {
  id: string;
  b64_json: string;
  prompt: string;
  timestamp: string;
}

export function useImage() {
  const [isGenerating, setIsGenerating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [lastGenerated, setLastGenerated] = useState<GeneratedImage | null>(null);

  const generateImage = useCallback(async (
    modelId: string,
    params: ImageGenParams
  ): Promise<GeneratedImage | null> => {
    setIsGenerating(true);
    setError(null);
    
    try {
      const result = await invoke<{ created: number; data: { b64_json: string }[] }>('submit_image', {
        modelId,
        prompt: params.prompt,
        width: params.width,
        height: params.height,
        steps: params.steps,
        cfg_scale: params.cfg_scale,
        sampler: params.sampler,
        negative_prompt: params.negative_prompt,
      });

      const image: GeneratedImage = {
        id: `${Date.now()}`,
        b64_json: result.data[0]?.b64_json || '',
        prompt: params.prompt,
        timestamp: new Date().toLocaleTimeString(),
      };

      setLastGenerated(image);
      setIsGenerating(false);
      return image;
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setError(message);
      setIsGenerating(false);
      return null;
    }
  }, []);

  return {
    isGenerating,
    error,
    lastGenerated,
    generateImage,
  };
}