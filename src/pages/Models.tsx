import type { FC } from 'react';
import { useCallback, useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { useLocation } from 'react-router-dom';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  Search,
  Plus,
  HardDrive,
  Cpu,
  Zap,
  X,
} from 'lucide-react';
import {
  StatusDot,
  ProgressBar,
  Select,
  Pill,
} from '../components/shared';
import { useCardSpotlight } from '../hooks/useCardSpotlight';
import { useModels } from '../hooks/useModels';
import { useAttachment } from '../hooks/useAttachment';

type ModelStatus = 'running' | 'offloaded' | 'available';
interface ExtendedModel {
  id: string;
  name: string;
  type: 'LLM' | 'Image' | 'Voice';
  sizeOnDisk: string;
  vramRequired: number;
  vramUsed: number;
  status: ModelStatus;
  uptime?: string;
  role: string;
  icon?: string;
  color?: string;
  backendType: string;
}

interface DiskUsage {
  used_bytes: number;
  total_bytes: number;
  available_bytes: number;
  llm_bytes: number;
  image_bytes: number;
  voice_bytes: number;
  other_bytes: number;
}

interface HfModelResult {
  id: string;
  author: string;
  downloads: number;
  tags: string[];
  files: { name: string; size_bytes?: number }[];
}

type ModelFilter = 'All' | 'LLM' | 'Image' | 'Voice' | 'Loaded' | 'Offloaded' | 'Available';
const filters: ModelFilter[] = ['All', 'LLM', 'Image', 'Voice', 'Loaded', 'Offloaded', 'Available'];

const typeDotColor: Record<string, string> = {
  LLM: '#1D9BF0',
  Image: '#10a37f',
  Voice: '#9CA3AF',
};

export const Models: FC = () => {
  const location = useLocation();
  const [search, setSearch] = useState('');
  const [activeFilter, setActiveFilter] = useState<ModelFilter>('All');
  const { models: backendModels, error: modelsError, loadingId, load, unload, offload, registerLocal, refresh: refreshModels } = useModels();
  const { chooseAttachment } = useAttachment();
  const [diskUsage, setDiskUsage] = useState<DiskUsage | null>(null);
  const [showRegistry, setShowRegistry] = useState(false);
  const [showHfBrowser, setShowHfBrowser] = useState(false);
  const [pendingImport, setPendingImport] = useState<string | null>(null);
  const [pendingKind, setPendingKind] = useState<'llm' | 'image' | 'tts'>('llm');
  const [hfQuery, setHfQuery] = useState('');
  const [hfResults, setHfResults] = useState<HfModelResult[]>([]);
  const [hfSearching, setHfSearching] = useState(false);
  const [hfDownloading, setHfDownloading] = useState<string | null>(null);
  const [hfErrors, setHfErrors] = useState<Record<string, string>>({});
  const [hfProgress, setHfProgress] = useState<Record<string, { downloaded: number; total: number | null }>>({});
  // download_id per row key (`${repoId}:${filename}`), for cancel targeting.
  const [hfDownloadIds, setHfDownloadIds] = useState<Record<string, string>>({});
  const [hfSearchError, setHfSearchError] = useState<string | null>(null);
  const [expandedHfResult, setExpandedHfResult] = useState<string | null>(null);
  const [selectedHfFiles, setSelectedHfFiles] = useState<Record<string, string>>({});
  const [selectedHfKinds, setSelectedHfKinds] = useState<Record<string, string>>({});
  const [hfOnly, setHfOnly] = useState(true);
  const [registryMessage, setRegistryMessage] = useState<string | null>(null);
  const [modelsList, setModelsList] = useState<ExtendedModel[]>([]);
  const addModelButtonRef = useRef<HTMLButtonElement>(null);
  const registryMenuRef = useRef<HTMLDivElement>(null);
  const [registryMenuPosition, setRegistryMenuPosition] = useState({ top: 0, right: 0 });

  useEffect(() => {
    const q = (location.state as { query?: string } | null)?.query;
    if (typeof q === 'string' && q) setSearch(q);
  }, [location.state]);

  useEffect(() => {
    setModelsList(backendModels.map(model => {
      const type = model.model_type === 'llm' || model.model_type === 'embedding' ? 'LLM' : model.model_type === 'image' ? 'Image' : 'Voice';
      const status: ModelStatus = model.residency === 'gpu'
        ? 'running'
        : !model.available
          ? 'offloaded'
          : 'available';
      return {
        id: model.id,
        name: model.id,
        type,
        sizeOnDisk: `${(model.disk_bytes / 1024 ** 3).toFixed(1)} GB`,
        vramRequired: model.vram_bytes / 1024 ** 3,
        status,
        vramUsed: model.vram_bytes / 1024 ** 3,
        role: type === 'Image' ? 'Image Generation' : type === 'Voice' ? 'Speech' : 'Language Model',
        backendType: model.model_type,
      };
    }));
  }, [backendModels]);

  useEffect(() => {
    invoke<DiskUsage>('get_disk_usage').then(setDiskUsage).catch(console.error);
  }, [backendModels]);

  // Real download progress for the HF Add flow (bytes, not a spinner).
  // Completion arrives separately on "hf-download-done" (see below) because
  // the start command returns a download id immediately.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<{ download_id: string; repo_id: string; filename: string; downloaded_bytes: number; total_bytes: number | null }>(
      'hf-download-progress',
      (e) => {
        const key = `${e.payload.repo_id}:${e.payload.filename}`;
        setHfProgress(prev => ({
          ...prev,
          [key]: { downloaded: e.payload.downloaded_bytes, total: e.payload.total_bytes },
        }));
      },
    )
      .then(fn => { unlisten = fn; })
      .catch(console.error);
    return () => unlisten?.();
  }, []);

  // Terminal state for a started HF download: success refreshes + closes,
  // cancellation just resets the row, real errors surface on the row.
  const handleHfDone = useCallback((payload: {
    download_id: string;
    repo_id: string;
    filename: string;
    ok: boolean;
    cancelled: boolean;
    model_id?: string | null;
    error?: string | null;
  }) => {
    const key = `${payload.repo_id}:${payload.filename}`;
    setHfDownloading(current => (current === key ? null : current));
    setHfDownloadIds(prev => {
      if (!(key in prev)) return prev;
      const next = { ...prev };
      delete next[key];
      return next;
    });
    setHfProgress(prev => {
      if (!(key in prev)) return prev;
      const next = { ...prev };
      delete next[key];
      return next;
    });
    if (payload.ok) {
      refreshModels()
        .then(() => invoke<DiskUsage>('get_disk_usage').then(setDiskUsage))
        .then(() => { setRegistryMessage(`Added ${payload.filename}`); setShowRegistry(false); })
        .catch(console.error);
    } else if (!payload.cancelled) {
      setHfErrors(prev => ({ ...prev, [payload.repo_id]: payload.error || 'Download failed' }));
    }
  }, [refreshModels]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<{
      download_id: string;
      repo_id: string;
      filename: string;
      ok: boolean;
      cancelled: boolean;
      model_id?: string | null;
      error?: string | null;
    }>('hf-download-done', (e) => handleHfDone(e.payload))
      .then(fn => { unlisten = fn; })
      .catch(console.error);
    return () => unlisten?.();
  }, [handleHfDone]);

  const resetHfRow = useCallback((rowKey: string, repoId: string) => {
    setHfDownloading(current => (current === rowKey ? null : current));
    setHfDownloadIds(prev => {
      if (!(rowKey in prev)) return prev;
      const next = { ...prev };
      delete next[rowKey];
      return next;
    });
    setHfProgress(prev => {
      if (!(rowKey in prev)) return prev;
      const next = { ...prev };
      delete next[rowKey];
      return next;
    });
    setHfErrors(prev => ({ ...prev, [repoId]: '' }));
  }, []);

  useEffect(() => {
    if (!showRegistry) return;
    const updateMenuPosition = () => {
      const button = addModelButtonRef.current;
      if (!button) return;
      const rect = button.getBoundingClientRect();
      setRegistryMenuPosition({ top: rect.bottom + 8, right: window.innerWidth - rect.right });
    };
    const closeOnOutsideClick = (event: MouseEvent) => {
      const target = event.target as Node;
      if (!registryMenuRef.current?.contains(target) && !addModelButtonRef.current?.contains(target)) {
        setShowRegistry(false);
      }
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setShowRegistry(false);
    };
    updateMenuPosition();
    window.addEventListener('resize', updateMenuPosition);
    window.addEventListener('scroll', updateMenuPosition, true);
    // Defer the outside-click listener by one tick so it doesn't catch the
    // very button click that opened the panel (classic click-outside race).
    const clickListenerTimer = setTimeout(() => {
      document.addEventListener('click', closeOnOutsideClick);
    }, 0);
    document.addEventListener('keydown', closeOnEscape);
    return () => {
      clearTimeout(clickListenerTimer);
      document.removeEventListener('click', closeOnOutsideClick);
      document.removeEventListener('keydown', closeOnEscape);
      window.removeEventListener('resize', updateMenuPosition);
      window.removeEventListener('scroll', updateMenuPosition, true);
    };
  }, [showRegistry]);

  const importLocal = async (kind: 'gguf' | 'safetensors') => {
    const path = await chooseAttachment(kind);
    if (!path) return;
    if (kind === 'gguf') {
      await registerLocal(path, 'llm');
      setShowRegistry(false);
    } else {
      setPendingImport(path);
    }
  };

  const searchHuggingFace = async () => {
    setHfSearching(true);
    setRegistryMessage(null);
    setHfSearchError(null);
    try {
      const results = await invoke<HfModelResult[]>('search_huggingface_models', { query: hfQuery });
      console.info('[models] Hugging Face results', results);
      setHfResults(results);
    } catch (cause) {
      console.error('[models] Hugging Face search failed', cause);
      setHfSearchError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setHfSearching(false);
    }
  };

  const filteredModels = modelsList.filter(model => {
    const matchesSearch = model.name.toLowerCase().includes(search.toLowerCase());
    const matchesFilter =
      activeFilter === 'All' ||
      activeFilter === model.type ||
      (activeFilter === 'Loaded' && model.status === 'running') ||
      (activeFilter === 'Offloaded' && model.status === 'offloaded') ||
      (activeFilter === 'Available' && model.status === 'available');
    return matchesSearch && matchesFilter;
  });

  const activeVramLoaded = modelsList.filter(m => m.status === 'running');
  const offloadedRam = modelsList.filter(m => m.status === 'offloaded');

  return (
    <div className="relative flex flex-col h-full max-w-6xl mx-auto space-y-4 text-text-primary">
      {/* Top Controls Shelf */}
      <div className="flex items-center gap-3 glass-panel aurora-glass p-3.5">
        <div className="relative flex-1 min-w-0 max-w-md">
          <Search className="absolute left-3.5 top-1/2 -translate-y-1/2 text-text-secondary" size={16} strokeWidth={2} />
          <input
            type="text"
            placeholder="Search local models..."
            className="w-full glass-pill py-2 pl-9 pr-4 text-xs font-semibold text-text-primary placeholder:text-text-secondary focus:outline-none focus:ring-2 focus:ring-[var(--color-primary)]/20"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>

        <div className="flex min-w-0 items-center gap-2">
          <div className="hidden md:flex items-center gap-1.5 whitespace-nowrap">
            {filters.map(filter => (
              <button
                key={filter}
                onClick={() => setActiveFilter(filter)}
                className={`rounded-full px-3 py-1 text-xs font-semibold transition-all cursor-pointer ${
                  activeFilter === filter
                    ? 'bg-primary text-white shadow-xs'
                    : 'bg-[var(--color-hover)] text-text-secondary hover:text-text-primary'
                }`}
              >
                {filter}
              </button>
            ))}
          </div>

          <Select
            options={filters.map(f => ({ value: f, label: `Filter: ${f}` }))}
            value={activeFilter}
            onChange={(val) => setActiveFilter(val as ModelFilter)}
            className="w-36 md:hidden"
          />
        </div>

        <div className="shrink-0">
          <button ref={addModelButtonRef} onClick={() => setShowRegistry(value => !value)} className="btn-primary py-2 px-3.5 flex items-center gap-1.5 text-xs font-bold whitespace-nowrap">
            <Plus size={15} strokeWidth={2.5} />
            Add Model
          </button>
        </div>
      </div>

      {showRegistry && createPortal(
        <div
          ref={registryMenuRef}
          className="fixed z-[1000] w-96 max-h-[70vh] overflow-y-auto glass-floating rounded-2xl p-3 text-xs space-y-2"
          style={{ top: registryMenuPosition.top, right: registryMenuPosition.right }}
        >
          {pendingImport ? (
            <>
              <div className="font-semibold">Choose model kind</div>
              <div className="truncate text-text-secondary" title={pendingImport}>{pendingImport.split(/[\\/]/).pop()}</div>
              <Select options={[{ value: 'llm', label: 'LLM' }, { value: 'image', label: 'Image' }, { value: 'tts', label: 'Voice' }]} value={pendingKind} onChange={(value) => setPendingKind(value as typeof pendingKind)} />
              <div className="flex gap-2 justify-end">
                <button onClick={() => { setPendingImport(null); setShowRegistry(false); }} className="btn-secondary px-2 py-1">Cancel</button>
                <button onClick={() => registerLocal(pendingImport, pendingKind).then(() => { setPendingImport(null); setShowRegistry(false); }).catch(console.error)} className="btn-primary px-2 py-1">Register</button>
              </div>
            </>
          ) : (
            <>
              {registryMessage && <div className="rounded-lg bg-[var(--color-secondary-bg)] p-2 text-[10px] text-secondary">{registryMessage}</div>}
              <button onClick={() => importLocal('gguf').catch(console.error)} className="w-full btn-secondary text-left">Import local GGUF</button>
              <button onClick={() => importLocal('safetensors').catch(console.error)} className="w-full btn-secondary text-left">Import local safetensors</button>
              <div className="border-t border-[var(--color-border)] pt-2 space-y-2">
                <button onClick={() => setShowHfBrowser(value => !value)} className="font-semibold text-text-primary text-left">Browse Hugging Face</button>
                {showHfBrowser && <>
                  <label className="flex items-center gap-2 text-[11px] text-text-secondary">
                    <input type="checkbox" checked={hfOnly} onChange={(event) => setHfOnly(event.target.checked)} />
                    GGUF only
                  </label>
                  <div className="flex gap-1.5">
                    <input
                      value={hfQuery}
                      onChange={(event) => setHfQuery(event.target.value)}
                      onKeyDown={(event) => { if (event.key === 'Enter' && hfQuery.trim() && !hfSearching) searchHuggingFace().catch(console.error); }}
                      placeholder="Search models…"
                      className="min-w-0 flex-1 glass-pill px-2 py-1 text-[11px]"
                    />
                    <button onClick={() => searchHuggingFace().catch(console.error)} disabled={hfSearching || !hfQuery.trim()} className="btn-secondary px-2 py-1">{hfSearching ? '…' : 'Search'}</button>
                  </div>
                  {hfSearchError && <div className="text-[10px] text-red-300">{hfSearchError}</div>}
                  <div className="max-h-56 overflow-y-auto space-y-1.5 pr-0.5">
                    {(hfOnly
                      ? hfResults
                          .map(r => ({
                            ...r,
                            files: r.files.filter(f => f.name.endsWith('.gguf') || f.name.endsWith('.ggml')),
                          }))
                          .filter(r => r.files.length > 0)
                      : hfResults
                    ).map(result => {
                      const supportedFiles = result.files;
                      const isSingle = supportedFiles.length === 1;
                      // Auto-select the only file so the Add button shows without an extra click.
                      const effectiveFile = isSingle && !selectedHfFiles[result.id]
                        ? supportedFiles[0].name
                        : selectedHfFiles[result.id];
                      const effectiveKind = selectedHfKinds[result.id] ?? (effectiveFile && !effectiveFile.endsWith('.safetensors') ? 'llm' : undefined);
                      const isExpanded = isSingle || expandedHfResult === result.id;
                      const isDownloading = hfDownloading?.startsWith(`${result.id}:`);
                      const canAdd = !!effectiveFile && (effectiveFile.endsWith('.safetensors') ? !!effectiveKind : true);
                      return (
                        <div
                          key={result.id}
                          className="rounded-xl border border-[var(--color-border)] bg-[var(--color-hover)]/60 hover:border-[var(--color-primary)]/30 hover:bg-[var(--color-hover)] transition-all p-2.5 space-y-1.5"
                        >
                          {/* Repo header — clicking header on multi-file repos toggles expansion */}
                          <div
                            className={`flex items-start justify-between gap-2 ${!isSingle ? 'cursor-pointer' : ''}`}
                            onClick={() => { if (!isSingle) setExpandedHfResult(expandedHfResult === result.id ? null : result.id); }}
                          >
                            <div className="min-w-0">
                              <div className="font-semibold truncate text-[11px] text-text-primary">{result.id}</div>
                              <div className="text-[10px] text-text-secondary mt-0.5">{result.author} · {result.downloads.toLocaleString()} dl</div>
                            </div>
                            {!isSingle && (
                              <span className="shrink-0 text-[10px] text-primary font-medium mt-0.5">
                                {expandedHfResult === result.id ? '▲' : `${supportedFiles.length} files ▼`}
                              </span>
                            )}
                            {isSingle && (
                              <span className="shrink-0 text-[10px] text-text-secondary mt-0.5">
                                {supportedFiles[0].size_bytes ? `${(supportedFiles[0].size_bytes / 1024 ** 3).toFixed(2)} GB` : ''}
                              </span>
                            )}
                          </div>

                          {/* File list — always shown for single-file repos; toggled for multi */}
                          {isExpanded && (
                            <div className="space-y-1">
                              {supportedFiles.map(file => (
                                <label
                                  key={file.name}
                                  className={`flex items-center gap-1.5 text-[10px] rounded-lg px-1.5 py-1 cursor-pointer transition-colors ${
                                    effectiveFile === file.name
                                      ? 'bg-[var(--color-primary)]/15 text-text-primary'
                                      : 'hover:bg-[var(--color-hover)] text-text-secondary'
                                  }`}
                                >
                                  {!isSingle && (
                                    <input
                                      type="radio"
                                      name={`hf-file-${result.id}`}
                                      checked={effectiveFile === file.name}
                                      onChange={() => {
                                        setSelectedHfFiles(prev => ({ ...prev, [result.id]: file.name }));
                                        if (!file.name.endsWith('.safetensors')) setSelectedHfKinds(prev => ({ ...prev, [result.id]: 'llm' }));
                                      }}
                                      className="accent-primary"
                                    />
                                  )}
                                  <span className="truncate flex-1" title={file.name}>{file.name}</span>
                                  <span className="text-text-secondary shrink-0">{file.size_bytes ? `${(file.size_bytes / 1024 ** 3).toFixed(2)} GB` : 'n/a'}</span>
                                </label>
                              ))}
                            </div>
                          )}

                          {/* Action row — kind selector + Add button */}
                          {isExpanded && (
                            <div className="flex items-center gap-1.5 pt-0.5">
                              {effectiveFile?.endsWith('.safetensors') ? (
                                <select
                                  value={effectiveKind ?? ''}
                                  onChange={(e) => setSelectedHfKinds(prev => ({ ...prev, [result.id]: e.target.value }))}
                                  className="flex-1 bg-transparent text-[10px] border border-[var(--color-border)] rounded px-1 py-0.5"
                                >
                                  <option value="">Choose kind…</option>
                                  <option value="llm">LLM</option>
                                  <option value="image">Image</option>
                                  <option value="tts">Voice</option>
                                </select>
                              ) : (
                                <span className="flex-1 text-[10px] text-text-secondary">
                                  {effectiveFile ? 'GGUF · LLM' : 'Select a file'}
                                </span>
                              )}
                              <button
                                onClick={() => {
                                  // Use the resolved effective values captured in closure — avoids
                                  // stale-state issues with selectedHfFiles/selectedHfKinds.
                                  const filename = effectiveFile;
                                  const kind = effectiveKind ?? 'llm';
                                  if (!filename) return;
                                  const downloadKey = `${result.id}:${filename}`;
                                  const rowKey = downloadKey;
                                  const repoId = result.id;
                                  setHfDownloading(downloadKey);
                                  setHfErrors(prev => ({ ...prev, [repoId]: '' }));
                                  setHfProgress(prev => {
                                    const next = { ...prev };
                                    delete next[downloadKey];
                                    return next;
                                  });
                                  // Fire-and-forget start: resolves with a download id
                                  // immediately; completion arrives on hf-download-done.
                                  invoke<{ download_id: string }>('download_huggingface_model', { repoId, filename, kind })
                                    .then((res) => {
                                      setHfDownloadIds(prev => ({ ...prev, [rowKey]: res.download_id }));
                                    })
                                    .catch((cause: unknown) => {
                                      resetHfRow(rowKey, repoId);
                                      setHfErrors(prev => ({ ...prev, [repoId]: cause instanceof Error ? cause.message : String(cause) }));
                                    });
                                }}
                                disabled={!canAdd || isDownloading}
                                className="btn-primary px-2 py-0.5 text-[10px] shrink-0"
                              >
                                {isDownloading ? 'Adding…' : 'Add'}
                              </button>
                              {isDownloading && (
                                <button
                                  onClick={() => {
                                    const rowKey = `${result.id}:${effectiveFile ?? ''}`;
                                    const downloadId = hfDownloadIds[rowKey];
                                    // Reset the row immediately; the backend aborts at the
                                    // next chunk, deletes the partial, and its done event
                                    // becomes a no-op for the already-reset row.
                                    if (downloadId) {
                                      invoke<boolean>('cancel_hf_download', { downloadId }).catch(console.error);
                                    }
                                    resetHfRow(rowKey, result.id);
                                  }}
                                  className="btn-secondary px-1.5 py-0.5 text-[10px] shrink-0"
                                  title="Cancel download"
                                  aria-label={`Cancel download of ${effectiveFile ?? 'file'}`}
                                >
                                  <X size={11} strokeWidth={2.5} />
                                </button>
                              )}
                            </div>
                          )}

                          {/* Real byte progress while this row downloads */}
                          {isDownloading && (() => {
                            const p = hfProgress[`${result.id}:${effectiveFile ?? ''}`];
                            const fmtBytes = (n: number) =>
                              n >= 1024 ** 3 ? `${(n / 1024 ** 3).toFixed(2)} GB` : `${(n / 1024 ** 2).toFixed(1)} MB`;
                            const downloaded = p?.downloaded ?? 0;
                            const total = p?.total ?? null;
                            return (
                              <div className="pt-1 space-y-1">
                                {total ? (
                                  <ProgressBar value={downloaded} max={total} color="primary" height={5} />
                                ) : null}
                                <div className="text-[10px] text-text-secondary font-mono">
                                  {total
                                    ? `${fmtBytes(downloaded)} / ${fmtBytes(total)}`
                                    : downloaded > 0
                                      ? `${fmtBytes(downloaded)} downloaded…`
                                      : 'Connecting…'}
                                </div>
                              </div>
                            );
                          })()}

                          {hfErrors[result.id] && (
                            <div className="text-[10px] text-red-300 mt-1">{hfErrors[result.id]}</div>
                          )}
                        </div>
                      );
                    })}
                  </div>
                </>}
              </div>
            </>
          )}
        </div>,
        document.body,
      )}

      {/* Main Grid + Memory Sidebar */}
      <div className="flex-1 flex gap-4 min-h-0 overflow-hidden">
        {/* Left Column: Model Cards */}
        <div className="flex-1 overflow-y-auto min-w-0 pr-1">
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4 auto-rows-fr">
            {modelsError && <div className="md:col-span-2 rounded-xl border border-red-400/30 bg-red-400/10 p-4 text-xs text-red-200">Unable to load models: {modelsError}</div>}
            {!modelsError && filteredModels.length === 0 && <div className="md:col-span-2 rounded-xl border border-[var(--color-border)] bg-[var(--color-hover)]/40 p-8 text-center text-xs text-text-secondary">No models yet. Import a GGUF or browse Hugging Face.</div>}
            {filteredModels.map(model => (
              <ModelCard
                key={model.id}
                model={model}
                busy={loadingId === model.id}
                onLoad={() => load(model.id, model.backendType).catch(console.error)}
                onOffload={() => offload(model.id).catch(console.error)}
                onUnload={() => unload(model.id).catch(console.error)}
              />
            ))}
          </div>
        </div>

        {/* Right Info Panel */}
        <div className="w-80 flex-shrink-0 glass-panel aurora-glass p-4 overflow-y-auto space-y-4 text-xs">
          {/* Disk Storage */}
          <div className="space-y-3">
            <div className="font-bold text-text-primary flex items-center gap-2 border-b border-[var(--color-border)] pb-2">
              <HardDrive size={14} className="text-primary" />
              Disk Storage Usage
            </div>
            <div>
              <div className="flex justify-between text-text-secondary mb-1 font-medium">
                <span>Total Used</span>
                <span className="font-mono font-bold text-text-primary">{diskUsage ? `${(diskUsage.used_bytes / 1024 ** 3).toFixed(1)} GB / ${(diskUsage.total_bytes / 1024 ** 3).toFixed(1)} GB` : 'No model storage yet'}</span>
              </div>
              <ProgressBar value={diskUsage?.used_bytes ?? 0} max={diskUsage?.total_bytes || 1} color="primary" height={6} />
            </div>

            <div className="space-y-2 pt-2 border-t border-[var(--color-border)]">
              {(diskUsage ? [
                { type: 'LLM', used: diskUsage.llm_bytes, color: 'primary' },
                { type: 'Image', used: diskUsage.image_bytes, color: 'secondary' },
                { type: 'Voice', used: diskUsage.voice_bytes, color: 'tertiary' },
                { type: 'Other', used: diskUsage.other_bytes, color: 'gray' },
              ] : []).map(item => (
                <div key={item.type} className="flex items-center gap-2">
                  <span
                    className="w-2.5 h-2.5 rounded-full"
                    style={{
                      backgroundColor:
                        item.color === 'primary' ? '#1D9BF0' : item.color === 'secondary' ? '#10a37f' : '#9CA3AF',
                    }}
                  />
                  <span className="text-text-primary font-medium">{item.type}</span>
                  <span className="font-mono text-text-secondary ml-auto">{(item.used / 1024 ** 3).toFixed(1)} GB</span>
                </div>
              ))}
            </div>
          </div>

          {/* VRAM / RAM Resident Models */}
          <div className="pt-3 border-t border-[var(--color-border)] space-y-2">
            <div className="font-bold text-text-primary flex items-center justify-between">
              <span>Active in VRAM ({activeVramLoaded.length})</span>
              <Zap size={14} className="text-primary" />
            </div>

            <div className="space-y-1.5">
              {activeVramLoaded.length === 0 ? (
                <p className="text-[11px] text-text-secondary italic">No models in VRAM</p>
              ) : (
                activeVramLoaded.map(model => (
                  <div key={model.id} className="flex items-center gap-2.5 p-2 rounded-xl bg-[var(--color-hover)]">
                    <StatusDot status="running" size={6} />
                    <div className="flex-1 min-w-0">
                      <div className="font-bold text-[11px] text-text-primary truncate">{model.name}</div>
                      <div className="text-[10px] text-text-secondary">{model.role}</div>
                    </div>
                    <span className="font-mono text-[11px] text-primary font-bold">{model.vramUsed} GB</span>
                  </div>
                ))
              )}
            </div>
          </div>

          {/* Offloaded in System RAM */}
          <div className="pt-3 border-t border-[var(--color-border)] space-y-2">
            <div className="font-bold text-text-primary flex items-center justify-between">
              <span>Paged to RAM ({offloadedRam.length})</span>
              <Cpu size={14} className="text-secondary" />
            </div>

            <div className="space-y-1.5">
              {offloadedRam.length === 0 ? (
                <p className="text-[11px] text-text-secondary italic">No models paged to System RAM</p>
              ) : (
                offloadedRam.map(model => (
                  <div key={model.id} className="flex items-center gap-2.5 p-2 rounded-xl bg-[var(--color-hover)]">
                    <span className="w-2 h-2 rounded-full bg-secondary" />
                    <div className="flex-1 min-w-0">
                      <div className="font-bold text-[11px] text-text-primary truncate">{model.name}</div>
                      <div className="text-[10px] text-text-secondary">Paged to RAM</div>
                    </div>
                    <span className="font-mono text-[10px] text-text-secondary font-bold">RAM</span>
                  </div>
                ))
              )}
            </div>
          </div>

        </div>
      </div>
    </div>
  );
};

const ModelCard: FC<{
  model: ExtendedModel;
  onLoad: () => void;
  onOffload: () => void;
  onUnload: () => void;
  busy: boolean;
}> = ({ model, onLoad, onOffload, onUnload, busy }) => {
  const isRunning = model.status === 'running';
  const isOffloaded = model.status === 'offloaded';
  const { ref, onPointerMove } = useCardSpotlight();

  return (
    <div
      ref={ref}
      onPointerMove={onPointerMove}
      className="glass-panel aurora-glass p-5 cursor-default hover:border-primary/40 transition-all flex flex-col gap-4 min-h-[220px] h-full overflow-hidden"
    >
      {/* Top Metadata Container (Flex with Wrapping) */}
      <div className="flex flex-wrap items-start justify-between gap-2.5 w-full">
        <div className="flex flex-wrap items-center gap-2 flex-1 min-w-0">
          <Pill variant="glass" size="xs" className="font-bold shrink-0">
            <span
              className="w-1.5 h-1.5 rounded-full shrink-0"
              style={{ backgroundColor: typeDotColor[model.type] ?? '#9CA3AF' }}
            />
            {model.type}
          </Pill>

          <Pill
            variant={isRunning ? 'secondary' : isOffloaded ? 'tertiary' : 'default'}
            size="xs"
            className="font-bold shrink-0"
          >
            <StatusDot status={isRunning ? 'running' : isOffloaded ? 'warning' : 'idle'} size={5} />
            {isRunning ? 'Running (VRAM)' : isOffloaded ? 'Offloaded (RAM)' : 'Available'}
          </Pill>
        </div>

        <Pill variant="glass" mono size="xs" className="font-semibold shrink-0 ml-auto">
          {model.sizeOnDisk}
        </Pill>
      </div>

      {/* Middle Content: Title & Description in Normal Document Flow */}
      <div className="flex-1 min-w-0">
        <h4 className="font-bold text-sm text-text-primary truncate">{model.name}</h4>
        <p className="text-xs text-text-secondary mt-1 line-clamp-2 leading-relaxed">{model.role}</p>
      </div>

      {/* Bottom Action Area: mt-auto Ensures Pinned to Bottom */}
      <div className="mt-auto pt-3.5 border-t border-[var(--color-border)] flex flex-wrap items-center justify-between gap-3 w-full">
        <div className="text-xs flex items-center gap-1.5 shrink-0">
          <span className="text-text-secondary">Req VRAM:</span>
          <Pill variant="primary" mono size="xs" className="font-bold shrink-0">
            {model.vramRequired} GB
          </Pill>
        </div>

        {/* Action Button Stack (Load, Offload, Unload) */}
        <div className="flex items-center gap-1.5 flex-wrap ml-auto shrink-0">
          {isRunning && (
            <>
              <button
                onClick={onOffload}
                disabled={busy}
                title="Page weights to system RAM"
                className="model-action-button btn-secondary transition-all cursor-pointer shadow-xs hover:-translate-y-0.5 active:scale-95 shrink-0"
              >
                {busy ? 'Working...' : 'Offload'}
              </button>
              <button
                onClick={onUnload}
                disabled={busy}
                className="model-action-button btn-secondary transition-all cursor-pointer shadow-xs hover:-translate-y-0.5 active:scale-95 shrink-0"
              >
                {busy ? 'Working...' : 'Unload'}
              </button>
            </>
          )}

          {isOffloaded && (
            <>
              <button
                onClick={onLoad}
                disabled={busy}
                className="model-action-button btn-primary shrink-0"
              >
                {busy ? 'Working...' : 'Load VRAM'}
              </button>
              <button
                onClick={onUnload}
                disabled={busy}
                className="model-action-button btn-secondary transition-all cursor-pointer shadow-xs hover:-translate-y-0.5 active:scale-95 shrink-0"
              >
                {busy ? 'Working...' : 'Unload'}
              </button>
            </>
          )}

          {model.status === 'available' && (
            <button
              onClick={onLoad}
              disabled={busy}
              className="model-action-button btn-primary shrink-0"
            >
              {busy ? 'Loading...' : 'Load Model'}
            </button>
          )}
        </div>
      </div>
    </div>
  );
};