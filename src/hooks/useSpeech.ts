import { useCallback, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

export interface SpeechParams {
  text: string;
  voice: string;
  speed?: number;
  modelId: string;
}

export interface SpeechResult {
  url: string;
  /** Duration in seconds measured from the returned WAV bytes (never estimated). Null when the payload is not decodable WAV. */
  durationSecs: number | null;
}

/**
 * Measure duration from a WAV payload: sampleRate (u32 LE @24),
 * data chunk size (u32 LE @40). The backend emits mono 16-bit PCM, so
 * bytesPerSecond = sampleRate * 2. Returns null for non-WAV payloads.
 */
export function parseWavDurationSecs(bytes: Uint8Array): number | null {
  try {
    if (bytes.length < 44) return null;
    const tag = (o: number) => String.fromCharCode(bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]);
    if (tag(0) !== 'RIFF' || tag(8) !== 'WAVE') return null;
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const sampleRate = view.getUint32(24, true);
    const dataSize = view.getUint32(40, true);
    if (!sampleRate || !dataSize) return null;
    return dataSize / (sampleRate * 2);
  } catch {
    return null;
  }
}

export function useSpeech() {
  const [isSynthesizing, setIsSynthesizing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [audioUrl, setAudioUrl] = useState<string | null>(null);
  const audioRef = useRef<HTMLAudioElement | null>(null);

  const synthesizeSpeech = useCallback(async (params: SpeechParams): Promise<SpeechResult | null> => {
    setIsSynthesizing(true);
    setError(null);

    try {
      // Clean up previous audio
      if (audioRef.current) {
        audioRef.current.pause();
        audioRef.current = null;
      }
      if (audioUrl) {
        URL.revokeObjectURL(audioUrl);
      }

      const wavBase64 = await invoke<string>('submit_speech', {
        modelId: params.modelId,
        text: params.text,
        voice: params.voice,
        speed: params.speed || 1.0,
      });

      // Convert base64 to blob and create object URL
      const binaryString = atob(wavBase64);
      const bytes = new Uint8Array(binaryString.length);
      for (let i = 0; i < binaryString.length; i++) {
        bytes[i] = binaryString.charCodeAt(i);
      }
      const durationSecs = parseWavDurationSecs(bytes);
      const blob = new Blob([bytes], { type: 'audio/wav' });
      const url = URL.createObjectURL(blob);

      setAudioUrl(url);
      audioRef.current = new Audio(url);
      setIsSynthesizing(false);
      return { url, durationSecs };
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setError(message);
      setIsSynthesizing(false);
      return null;
    }
  }, [audioUrl]);

  const playAudio = useCallback(() => {
    if (audioRef.current) {
      audioRef.current.play().catch(console.error);
    }
  }, []);

  const pauseAudio = useCallback(() => {
    if (audioRef.current) {
      audioRef.current.pause();
    }
  }, []);

  const stopAudio = useCallback(() => {
    if (audioRef.current) {
      audioRef.current.pause();
      audioRef.current.currentTime = 0;
    }
  }, []);

  return {
    isSynthesizing,
    error,
    audioUrl,
    audioRef,
    synthesizeSpeech,
    playAudio,
    pauseAudio,
    stopAudio,
  };
}
