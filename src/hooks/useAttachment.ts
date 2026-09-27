import { useCallback, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';

export type AttachmentKind = 'file' | 'image' | 'prompt' | 'gguf' | 'safetensors';

export function useAttachment() {
  const [attachedFile, setAttachedFile] = useState<string | null>(null);

  const chooseAttachment = useCallback(async (kind: AttachmentKind) => {
    const selected = await open({
      multiple: false,
      directory: false,
      filters: kind === 'image'
        ? [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'gif', 'webp'] }]
        : kind === 'gguf'
          ? [{ name: 'GGUF Models', extensions: ['gguf', 'ggml'] }]
          : kind === 'safetensors'
            ? [{ name: 'Safetensors Models', extensions: ['safetensors'] }]
            : [{ name: 'Files', extensions: ['*'] }],
    });
    if (typeof selected === 'string') {
      setAttachedFile(selected);
      return selected;
    }
    return null;
  }, []);

  return { attachedFile, setAttachedFile, chooseAttachment };
}