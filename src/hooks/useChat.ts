import { useCallback, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export type ChatEvent = 
  | { type: 'token'; token: string }
  | { type: 'done'; finishReason: string }
  | { type: 'error'; message: string };

export interface GenParams {
  max_tokens: number;
  temperature: number;
  top_p?: number;
}

export function useChat() {
  const [isGenerating, setIsGenerating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const abortControllerRef = useRef<AbortController | null>(null);
  const eventUnlistenRef = useRef<(() => void) | null>(null);

  const sendMessage = useCallback(async (
    modelId: string,
    prompt: string,
    params: GenParams,
    onToken: (token: string) => void,
    onDone: (finishReason: string) => void,
    onError: (error: string) => void
  ) => {
    setIsGenerating(true);
    setError(null);
    
    abortControllerRef.current = new AbortController();
    
    try {
      // Listen for SSE events from the backend
      const unlisten = await listen<ChatEvent>('chat-event', (event) => {
        if (abortControllerRef.current?.signal.aborted) return;
        
        switch (event.payload.type) {
          case 'token':
            onToken(event.payload.token);
            break;
          case 'done':
            onDone(event.payload.finishReason);
            setIsGenerating(false);
            break;
          case 'error':
            onError(event.payload.message);
            setError(event.payload.message);
            setIsGenerating(false);
            break;
        }
      });
      eventUnlistenRef.current = unlisten;

      // Submit the chat request
      await invoke('submit_chat', {
        modelId,
        prompt,
        params,
      });
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setError(message);
      onError(message);
      setIsGenerating(false);
      
      if (eventUnlistenRef.current) {
        eventUnlistenRef.current();
        eventUnlistenRef.current = null;
      }
    }
  }, []);

  const stopGeneration = useCallback(() => {
    if (abortControllerRef.current) {
      abortControllerRef.current.abort();
      abortControllerRef.current = null;
    }
    if (eventUnlistenRef.current) {
      eventUnlistenRef.current();
      eventUnlistenRef.current = null;
    }
    setIsGenerating(false);
  }, []);

  return {
    isGenerating,
    error,
    sendMessage,
    stopGeneration,
  };
}