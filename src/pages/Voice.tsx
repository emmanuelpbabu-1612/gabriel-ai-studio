import type { FC } from 'react';
import { useState, useEffect, useRef } from 'react';
import {
  Mic,
  Play,
  Pause,
  Download,
  SkipBack,
  SkipForward,
  Sliders,
  Volume2,
  Loader2,
  Info,
} from 'lucide-react';
import { ModelBadge } from '../components/shared';
import { Select } from '../components/shared/Select';
import { useModels } from '../hooks/useModels';
import { useSpeech } from '../hooks/useSpeech';

export interface VoiceHistoryItem {
  id: string;
  text: string;
  timestamp: string;
  duration: string;
  model: string;
  voice: string;
}

const HISTORY_STORAGE_KEY = 'gabriel_voice_synthesis_history';

function loadPersistedHistory(): VoiceHistoryItem[] {
  try {
    const raw = localStorage.getItem(HISTORY_STORAGE_KEY);
    return raw ? JSON.parse(raw) : [];
  } catch {
    return [];
  }
}

function savePersistedHistory(items: VoiceHistoryItem[]) {
  try {
    localStorage.setItem(HISTORY_STORAGE_KEY, JSON.stringify(items.slice(0, 30)));
  } catch (err) {
    console.error('Failed to save voice history to localStorage:', err);
  }
}

export const Voice: FC = () => {
  const { models } = useModels();
  // ONE selector, real data only: registered Voice-kind (TTS) models from the
  // backend registry. No fallback to other model kinds (an LLM is not a voice)
  // and no hardcoded preset names — the old Sarah/Adam/Emma/George list was
  // placeholder: the stub maps every voice string to the same sine wave, so
  // presets changed nothing audible.
  const voiceModels = models.filter(m => m.model_type === 'tts');
  
  const { 
    isSynthesizing, 
    error: speechError, 
    audioUrl, 
    audioRef: sharedAudioRef,
    synthesizeSpeech, 
    playAudio, 
    pauseAudio, 
  } = useSpeech();

  const [selectedModel, setSelectedModel] = useState<string>('');
  const [text, setText] = useState('');
  const [isPlaying, setIsPlaying] = useState(false);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [durationStubNote, setDurationStubNote] = useState(false);
  const [speed, setSpeed] = useState(1.0);
  const [liveHistory, setLiveHistory] = useState<VoiceHistoryItem[]>(loadPersistedHistory);

  // Auto-select registered voice model immediately on load; stays in sync if
  // the registry changes (model removed → fall back to first available).
  useEffect(() => {
    if (voiceModels.length > 0) {
      if (!selectedModel || !voiceModels.some(m => m.id === selectedModel)) {
        const runningModel = voiceModels.find(m => m.residency === 'gpu');
        setSelectedModel(runningModel?.id || voiceModels[0].id);
      }
    } else {
      setSelectedModel('');
    }
  }, [voiceModels, selectedModel]);

  const audioRef = sharedAudioRef;

  const analyzerRef = useRef<{ ctx: AudioContext | null; src: MediaElementAudioSourceNode | null; analyser: AnalyserNode | null; el: HTMLAudioElement | null }>({
    ctx: null, src: null, analyser: null, el: null,
  });
  const [levels, setLevels] = useState<number[]>([]);
  const rafRef = useRef(0);
  const lastPushRef = useRef(0);
  const BAR_COUNT = 36;

  useEffect(() => {
    const el = audioRef.current;
    if (!audioUrl || !el) return;
    const onTime = () => setCurrentTime(el.currentTime);
    const onMeta = () => {
      if (el.duration && !isNaN(el.duration) && isFinite(el.duration)) {
        setDuration(el.duration);
      }
    };
    const onEnded = () => {
      setIsPlaying(false);
      setCurrentTime(0);
    };
    el.addEventListener('timeupdate', onTime);
    el.addEventListener('loadedmetadata', onMeta);
    el.addEventListener('durationchange', onMeta);
    el.addEventListener('canplay', onMeta);
    el.addEventListener('ended', onEnded);

    try {
      let ctx = analyzerRef.current.ctx;
      if (!ctx) {
        const AC = window.AudioContext || (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
        ctx = new AC();
        analyzerRef.current.ctx = ctx;
      }
      if (ctx.state === 'suspended') void ctx.resume();
      if (analyzerRef.current.el !== el) {
        try { analyzerRef.current.src?.disconnect(); } catch { /* noop */ }
        const src = ctx.createMediaElementSource(el);
        const analyser = ctx.createAnalyser();
        analyser.fftSize = 128;
        analyser.smoothingTimeConstant = 0.75;
        src.connect(analyser);
        analyser.connect(ctx.destination);
        analyzerRef.current = { ctx, src, analyser, el };
      }
    } catch { /* analyser fallback */ }
    const analyser = analyzerRef.current.analyser;
    if (analyser) {
      const bins = new Uint8Array(analyser.frequencyBinCount);
      const loop = () => {
        rafRef.current = requestAnimationFrame(loop);
        const now = performance.now();
        if (now - lastPushRef.current < 100) return;
        lastPushRef.current = now;
        analyser.getByteFrequencyData(bins);
        const next: number[] = [];
        for (let i = 0; i < BAR_COUNT; i++) {
          const bin = bins[Math.floor((i / BAR_COUNT) * bins.length)] ?? 0;
          next.push(8 + (bin / 255) * 92);
        }
        setLevels(next);
      };
      rafRef.current = requestAnimationFrame(loop);
    }
    return () => {
      el.removeEventListener('timeupdate', onTime);
      el.removeEventListener('loadedmetadata', onMeta);
      el.removeEventListener('durationchange', onMeta);
      el.removeEventListener('canplay', onMeta);
      el.removeEventListener('ended', onEnded);
      cancelAnimationFrame(rafRef.current);
    };
  }, [audioUrl, audioRef]);

  const formatTime = (t: number) => {
    if (!t || isNaN(t)) return '0:00';
    const m = Math.floor(t / 60);
    const s = Math.floor(t % 60);
    return `${m}:${s.toString().padStart(2, '0')}`;
  };

  const handleSynthesize = async () => {
    if (!text.trim() || !selectedModel) return;

    // The stub backend takes no real voice profile — it uses the voice string
    // only as a pitch hint (and maps unknown ids to one default sine). Pass
    // the selected model id so the request is traceable to a real registry
    // entry instead of a phantom preset name.
    const result = await synthesizeSpeech({
      modelId: selectedModel,
      text,
      voice: selectedModel,
      speed,
    });

    if (result) {
      setIsPlaying(true);
      // Duration honesty: measured from the returned WAV bytes, never
      // estimated from text length. (The stub's clip does scale with text —
      // 0.055s/char clamped to 0.4–30s — but the number shown is parsed audio,
      // not the formula.) A null parse means the payload wasn't decodable WAV.
      if (result.durationSecs !== null) {
        setDuration(result.durationSecs);
        setDurationStubNote(true);
      } else {
        setDuration(0);
        setDurationStubNote(false);
      }

      const now = new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
      const newItem: VoiceHistoryItem = {
        id: `${Date.now()}`,
        text: text.slice(0, 140),
        timestamp: `Today, ${now}`,
        duration: result.durationSecs !== null ? `${result.durationSecs.toFixed(1)}s (stub)` : 'unknown',
        model: selectedModel,
        voice: selectedModel,
      };

      setLiveHistory(prev => {
        const next = [newItem, ...prev.filter(i => i.id !== newItem.id)].slice(0, 30);
        savePersistedHistory(next);
        return next;
      });
    }
  };

  const handlePlayPause = () => {
    if (isPlaying) {
      pauseAudio();
    } else {
      playAudio();
    }
    setIsPlaying(!isPlaying);
  };

  const selectedModelObj = models.find(m => m.id === selectedModel);

  return (
    <div className="flex flex-col h-full max-w-6xl mx-auto space-y-4 text-text-primary select-none">
      {/* Top Header Bar: Single Unified Model Selector (real registry TTS models) */}
      <div className="flex flex-wrap items-center justify-between gap-3 glass-panel aurora-glass p-3">
        <div className="flex items-center gap-3 flex-1 min-w-[280px]">
          <Select
            options={voiceModels.map(m => ({ value: m.id, label: `Voice Model: ${m.id}` }))}
            value={selectedModel}
            onChange={setSelectedModel}
            placeholder="No voice models registered"
            disabled={voiceModels.length === 0}
            className="w-80"
          />
        </div>
        <div className="flex items-center gap-2">
          <div className="relative group">
            <button
              type="button"
              aria-label="About the stub synthesizer"
              className="w-6 h-6 rounded-full text-text-secondary hover:text-text-primary hover:bg-[var(--color-hover)] flex items-center justify-center transition-colors cursor-pointer"
            >
              <Info size={14} />
            </button>
            <div className="pointer-events-none absolute right-0 top-full z-30 mt-2 w-64 rounded-xl border border-[var(--color-border)] bg-[var(--color-card)] p-3 text-[10px] leading-snug text-text-secondary shadow-lg opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 transition-opacity">
              <span className="font-bold text-text-primary">Stub synthesizer.</span> Outputs a
              deterministic sine-wave WAV, not real speech. Voice timbre and speed are accepted
              but have no audible effect yet. Durations are measured from the returned audio.
            </div>
          </div>
        </div>
      </div>

      {voiceModels.length === 0 && (
        <div className="glass-panel aurora-glass p-4 text-xs text-text-secondary leading-relaxed">
          <span className="font-bold text-text-primary">No voice models registered.</span>{' '}
          Synthesis needs a Voice-kind model in the backend registry — add one on the Models page
          (register with kind <span className="font-mono">voice</span>/<span className="font-mono">tts</span>),
          then return here. Nothing on this page will pretend an LLM or a preset name can speak.
        </div>
      )}

      {/* Main Synthesize Workspace & Controls */}
      <div className="flex-1 flex gap-4 min-h-0 overflow-hidden">
        {/* Left Column: Synthesizer & Waveform */}
        <div className="flex-1 flex flex-col gap-4 overflow-y-auto min-w-0">
          <div className="glass-panel aurora-glass p-5 space-y-4">
            {/* Input Row */}
            <div className="flex items-center gap-3 glass-panel aurora-glass p-2.5 focus-within:ring-2 focus-within:ring-primary/20">
              <input
                type="text"
                placeholder="Type or paste text to synthesize speech locally..."
                className="flex-1 bg-transparent border-none outline-none text-xs md:text-sm text-text-primary placeholder:text-text-secondary font-medium px-2"
                value={text}
                onChange={(e) => setText(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && !e.shiftKey) {
                    e.preventDefault();
                    handleSynthesize();
                  }
                }}
              />
              <span className="text-[11px] text-text-secondary font-mono shrink-0 hidden sm:inline mr-1">
                {text.length} chars
              </span>
              <button
                onClick={handleSynthesize}
                disabled={isSynthesizing || !text.trim() || !selectedModelObj}
                className="btn-primary py-2 px-4 flex items-center gap-2 text-xs font-semibold shrink-0 disabled:opacity-50 disabled:cursor-not-allowed"
              >
                {isSynthesizing ? (
                  <>
                    <Loader2 size={15} strokeWidth={2.2} className="animate-spin" />
                    Synthesizing...
                  </>
                ) : (
                  <>
                    <Mic size={15} strokeWidth={2.2} />
                    Synthesize Speech
                  </>
                )}
              </button>
            </div>
            {speechError && (
              <div className="rounded-xl border border-red-400/30 bg-red-400/10 px-3 py-2 text-[11px] text-red-200">
                {speechError}
              </div>
            )}

            {/* Dynamic Bar-Wave Audio Visualizer */}
            <div className="pt-2 border-t border-[var(--color-border)]">
              <div
                className="relative h-24 rounded-2xl bg-[var(--color-hover)] overflow-hidden border border-[var(--color-border)] px-4 py-2 flex items-center justify-between gap-1 cursor-pointer select-none"
                onClick={(e) => {
                  const rect = e.currentTarget.getBoundingClientRect();
                  const clickX = e.clientX - rect.left;
                  const newProgress = Math.max(0, Math.min(1, clickX / rect.width));
                  setCurrentTime(newProgress * duration);
                  if (audioRef.current && duration > 0) {
                    audioRef.current.currentTime = newProgress * duration;
                  }
                }}
              >
                {/* 36 bars: live spectrum when audio plays, static preview when idle */}
                {[
                  18, 28, 42, 65, 80, 52, 38, 70, 92, 78, 45, 60, 88, 95, 72, 50,
                  35, 68, 85, 90, 64, 40, 55, 82, 94, 76, 48, 30, 58, 86, 70, 44,
                  32, 50, 26, 16,
                ].map((baseHeight, idx, arr) => {
                  const barProgress = (idx / arr.length) * 100;
                  const currentProgress = duration > 0 ? (currentTime / duration) * 100 : 0;
                  const isPassed = barProgress <= currentProgress;
                  const live = levels.length === arr.length && isPlaying;
                  const heightPct = live ? levels[idx] : baseHeight;

                  return (
                    <div
                      key={idx}
                      className="flex-1 h-full flex items-center justify-center"
                    >
                      <div
                        className={`w-1 sm:w-1.5 rounded-full transition-all duration-150 ${
                          !live && isPlaying ? 'voice-bar-animating' : ''
                        }`}
                        style={{
                          height: `${heightPct}%`,
                          minHeight: '6px',
                          background: isPassed
                            ? 'linear-gradient(180deg, #1D9BF0 0%, #10a37f 100%)'
                            : 'linear-gradient(180deg, rgba(29, 155, 240, 0.4) 0%, rgba(16, 163, 127, 0.3) 100%)',
                          boxShadow: isPassed && isPlaying
                            ? '0 0 10px rgba(29, 155, 240, 0.45)'
                            : 'none',
                          animationDelay: `-${(idx * 0.075).toFixed(2)}s`,
                          animationDuration: `${0.75 + (idx % 4) * 0.15}s`,
                        }}
                      />
                    </div>
                  );
                })}

                {/* Progress Playhead Line */}
                <div
                  className="absolute top-0 bottom-0 w-0.5 bg-primary shadow-[0_0_8px_var(--color-primary)] transition-all duration-75 pointer-events-none z-10"
                  style={{ left: `${duration > 0 ? (currentTime / duration) * 100 : 0}%` }}
                />
              </div>
              <p className="mt-1.5 text-[10px] text-text-secondary leading-snug">
                {isPlaying && levels.length > 0 ? 'Live spectrum' : 'Decorative preview'}
              </p>
            </div>

            {/* Transport Controls */}
            <div className="pt-2 border-t border-[var(--color-border)] flex items-center justify-between gap-3">
              <div className="flex items-center gap-1.5 shrink-0">
                <button 
                  onClick={() => { if (audioRef.current) audioRef.current.currentTime = Math.max(0, audioRef.current.currentTime - 2); }}
                  className="p-1.5 rounded-full text-text-secondary hover:text-text-primary hover:bg-[var(--color-hover)] transition-colors cursor-pointer" aria-label="Skip back">
                  <SkipBack size={16} />
                </button>
                <button
                  onClick={handlePlayPause}
                  disabled={!audioUrl}
                  className="w-9 h-9 rounded-full bg-primary text-white flex items-center justify-center hover:opacity-90 transition-opacity cursor-pointer shrink-0 shadow-xs disabled:opacity-40 disabled:cursor-not-allowed"
                  aria-label={isPlaying ? 'Pause' : 'Play'}
                >
                  {isPlaying ? <Pause size={16} /> : <Play size={16} className="ml-0.5" />}
                </button>
                <button 
                  onClick={() => { if (audioRef.current) audioRef.current.currentTime = Math.min(duration, audioRef.current.currentTime + 2); }}
                  className="p-1.5 rounded-full text-text-secondary hover:text-text-primary hover:bg-[var(--color-hover)] transition-colors cursor-pointer" aria-label="Skip forward">
                  <SkipForward size={16} />
                </button>
              </div>

              <input
                type="range"
                min="0"
                max={duration || 1}
                step="0.05"
                value={currentTime}
                disabled={!audioUrl}
                onChange={(e) => {
                  const time = parseFloat(e.target.value);
                  setCurrentTime(time);
                  if (audioRef.current) {
                    audioRef.current.currentTime = time;
                  }
                }}
                onMouseDown={() => { if (audioRef.current) audioRef.current.pause(); }}
                onMouseUp={() => { if (audioRef.current && isPlaying) audioRef.current.play(); }}
                className="flex-1 accent-primary cursor-pointer mx-2 disabled:opacity-40"
              />

              <div className="text-[11px] text-text-secondary font-mono shrink-0 font-bold whitespace-nowrap flex items-center gap-1.5">
                <span>{formatTime(currentTime)} / {formatTime(duration)}</span>
                {duration > 0 && durationStubNote && (
                  <span className="text-[10px] text-text-secondary font-normal">
                    ({duration.toFixed(1)}s measured stub audio)
                  </span>
                )}
              </div>

              <button
                onClick={() => {
                  if (!audioUrl) return;
                  const a = document.createElement('a');
                  a.href = audioUrl;
                  a.download = `gabriel-tts-${Date.now()}.wav`;
                  document.body.appendChild(a);
                  a.click();
                  a.remove();
                }}
                disabled={!audioUrl}
                className="p-1.5 rounded-full text-text-secondary hover:text-text-primary hover:bg-[var(--color-hover)] transition-colors cursor-pointer shrink-0 ml-1 disabled:opacity-40 disabled:cursor-not-allowed"
                aria-label="Download synthesized audio"
                title={audioUrl ? 'Download WAV' : 'Synthesize first to download'}
              >
                <Download size={15} />
              </button>
            </div>
          </div>

          {/* Recent Synthesis History (persisted in localStorage) */}
          <div className="glass-panel aurora-glass p-4 space-y-3">
            <div className="flex items-center justify-between">
              <h3 className="font-bold text-xs text-text-primary">Recent Synthesis History</h3>
              {liveHistory.length > 0 && (
                <button
                  onClick={() => {
                    setLiveHistory([]);
                    savePersistedHistory([]);
                  }}
                  className="text-[10px] text-text-secondary hover:text-red-400 font-semibold cursor-pointer"
                >
                  Clear History
                </button>
              )}
            </div>
            <div className="space-y-2">
              {liveHistory.length === 0 && (
                <p className="text-[11px] text-text-secondary italic">No syntheses yet — generate speech above.</p>
              )}
              {liveHistory.map(item => (
                <div
                  key={item.id}
                  onClick={() => setText(item.text)}
                  className="flex items-center gap-3 p-2.5 rounded-xl bg-[var(--color-hover)] hover:bg-[var(--color-active)] transition-colors cursor-pointer group"
                  title="Click to load text into input"
                >
                  <span className="w-7 h-7 rounded-full bg-[var(--color-card)] text-primary flex items-center justify-center shrink-0 shadow-2xs group-hover:scale-105 transition-transform">
                    <Volume2 size={13} />
                  </span>
                  <div className="flex-1 min-w-0">
                    <div className="font-bold text-xs text-text-primary truncate">{item.text}</div>
                    <div className="text-[10px] text-text-secondary font-mono">
                      {item.timestamp} · <span className="text-primary font-semibold">{item.model}</span> ({item.voice})
                    </div>
                  </div>
                  <div className="text-xs text-text-secondary font-mono font-semibold">{item.duration}</div>
                </div>
              ))}
            </div>
          </div>
        </div>

        {/* Supplementary Info Sidebar */}
        <div className="w-72 flex-shrink-0 glass-panel aurora-glass p-4 overflow-y-auto space-y-4 text-xs">
          <div className="space-y-3">
            <div className="font-bold text-text-primary flex items-center gap-1.5 border-b border-[var(--color-border)] pb-2">
              <Sliders size={14} className="text-primary" />
              Voice Tuning
            </div>

            <div>
              <div className="flex justify-between items-center mb-1 text-text-secondary font-medium">
                <span>Speed</span>
                <span className="font-mono font-bold text-text-primary">{speed}x</span>
              </div>
              <input type="range" min="0.5" max="2" step="0.1" value={speed} onChange={(e) => setSpeed(Number(e.target.value))} className="w-full accent-primary cursor-pointer" />
              <p className="mt-2 text-[10px] text-text-secondary leading-snug">Speed is forwarded; the stub produces no audible change.</p>
            </div>
          </div>

          <div className="pt-3 border-t border-[var(--color-border)] space-y-2">
            <div className="font-bold text-text-primary">Model Info</div>
            {(() => {
              const model = selectedModelObj;
              if (!model) return (
                <div className="text-text-secondary text-center py-4">Select a voice model to see details</div>
              );
              return (
                <>
                  <ModelBadge 
                    type="Voice" 
                    status={model.residency === 'gpu' ? 'running' : 'idle'} 
                    size="md" 
                  />
                  <div className="space-y-1.5 text-text-secondary pt-1 font-medium">
                    <div className="flex justify-between">
                      <span>Model ID</span>
                      <span className="font-mono text-text-primary font-bold truncate max-w-[130px]">{model.id}</span>
                    </div>
                    <div className="flex justify-between">
                      <span>VRAM</span>
                      <span className="font-mono text-text-primary font-bold">
                        {(model.vram_bytes / 1024 ** 3).toFixed(1)} GB
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span>Status</span>
                      <span className={`badge ${model.residency === 'gpu' ? 'badge-secondary' : 'badge-tertiary'} font-mono font-bold`}>
                        {model.residency === 'gpu' && model.vram_bytes > 0
                          ? 'Running (VRAM)'
                          : model.residency === 'gpu'
                            ? 'Running (RAM)'
                            : model.available
                              ? 'Offloaded (RAM)'
                              : 'Available'}
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span>Disk Size</span>
                      <span className="font-mono text-text-primary font-bold">
                        {(model.disk_bytes / 1024 ** 3).toFixed(1)} GB
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span>Sample Rate</span>
                      <span className="font-mono text-text-primary font-bold">22 kHz</span>
                    </div>
                  </div>
                </>
              );
            })()}
          </div>
        </div>
      </div>
    </div>
  );
};