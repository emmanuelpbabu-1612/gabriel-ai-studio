import type { FC } from 'react';
import { useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import {
  Play,
  Settings,
  Mic,
  ArrowRight,
  MessageSquare,
  Image as ImageIcon,
  Box,
  Sparkles,
  SlidersHorizontal,
  FileText,
  Loader2,
} from 'lucide-react';
import {
  StatusDot,
  RadialGraph,
  ModelBadge,
  AttachMenuPopover,
} from '../components/shared';
import { TopSearchBar } from '../components/layout/TopSearchBar';
import { quickActions } from '../data/mockData';
import { useCardSpotlight } from '../hooks/useCardSpotlight';
import { useModels } from '../hooks/useModels';
import { useTelemetry } from '../hooks/useTelemetry';
import { useAttachment } from '../hooks/useAttachment';
import { useEngineMode, usePagerStatus, useProfile } from '../hooks/useEngineSettings';
import { useAudioTranscription } from '../hooks/useAudioTranscription';
import { Select } from '../components/shared/Select';

export const Home: FC = () => {
  const navigate = useNavigate();
  const [stickyInput, setStickyInput] = useState('');
  const { models, loadingId, load, unload } = useModels();
  const telemetry = useTelemetry();
  const { attachedFile, setAttachedFile, chooseAttachment } = useAttachment();
  const { name } = useProfile();
  const { mode, updateMode } = useEngineMode();
  const pagerStatus = usePagerStatus();

  const { isRecording, isTranscribing, toggleRecording } = useAudioTranscription((transcribedText) => {
    setStickyInput((prev) => (prev ? `${prev} ${transcribedText}` : transcribedText));
  });

  const { ref: greetingRef, onPointerMove: onGreetingPointerMove } = useCardSpotlight();
  const { ref: workloadsRef, onPointerMove: onWorkloadsPointerMove } = useCardSpotlight();

  const getGreeting = () => {
    const hour = new Date().getHours();
    if (hour < 12) return 'Good morning';
    if (hour < 18) return 'Good afternoon';
    return 'Good evening';
  };

  const activeCount = models.filter(model => model.residency === 'gpu').length;

  const handleStickySubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (stickyInput.trim() || attachedFile) {
      const fullText = attachedFile ? `[Attached: ${attachedFile}] ${stickyInput}` : stickyInput;
      setStickyInput('');
      setAttachedFile(null);
      navigate('/chat', { state: { initialMessage: fullText } });
    }
  };

  return (
    <div className="space-y-6 max-w-5xl mx-auto pb-28 text-text-primary select-none">
      <TopSearchBar />

      {/* Greeting Banner */}
      <div
        ref={greetingRef}
        onPointerMove={onGreetingPointerMove}
        className="glass-panel hero-card-gradient p-8 md:p-10 cursor-default"
      >
        <div className="relative z-10">
          <h1 className="text-2xl md:text-3xl font-bold text-text-primary mb-2 tracking-tight">
            {getGreeting()}, {name.split(' ')[0]}.
          </h1>
          <p className="text-sm md:text-base text-text-secondary font-medium">
            {telemetry ? 'Your local AI engine is active and ready on device.' : 'Connecting to your local AI engine...'}
          </p>
        </div>
      </div>

      {/* Quick Action Cards */}
      <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-4 gap-6">
        {quickActions.map(action => (
          <QuickActionCard key={action.id} action={action} />
        ))}
      </div>

      {/* Active Workloads Shelf */}
      <div
        ref={workloadsRef}
        onPointerMove={onWorkloadsPointerMove}
        className="glass-panel aurora-glass p-5 space-y-4"
      >
        <div className="flex items-center justify-between border-b border-[var(--color-border)] pb-3">
          <div className="flex items-center gap-3">
            <h2 className="font-bold text-base text-text-primary">Active Workloads</h2>
            <span className="badge badge-primary font-mono font-bold dynamic-glass-pill">
              {activeCount}
            </span>
          </div>
          <Link to="/system" className="text-xs font-semibold text-primary hover:underline">
            View System Architecture
          </Link>
        </div>

        <div className="space-y-2.5">
          {models.map(model => {
            const isRunning = model.residency === 'gpu';
            const isOffloaded = model.residency === 'cpu';
            const type = model.model_type === 'llm' || model.model_type === 'embedding' ? 'LLM' : model.model_type === 'image' ? 'Image' : 'Voice';

            return (
              <div
                key={model.id}
                className="flex items-center gap-4 p-3 rounded-2xl bg-[var(--color-hover)]/40 hover:bg-[var(--color-hover)] transition-all border border-transparent hover:border-[var(--color-border)] dynamic-glass-pill"
              >
                <div className="flex-shrink-0">
                  <ModelBadge type={type} status={isRunning ? 'running' : 'idle'} size="sm" />
                </div>

                <div className="flex-1 min-w-0">
                  <div className="font-bold text-sm text-text-primary truncate">{model.id}</div>
                  <div className="text-xs text-text-secondary truncate">{isOffloaded ? 'Paged to RAM' : 'Loaded workload'}</div>
                </div>

                <div className="hidden sm:flex items-center gap-2 text-xs font-medium min-w-[130px]">
                  <StatusDot status={isRunning ? 'running' : 'idle'} size={6} />
                  <span className={isRunning ? 'text-secondary font-bold' : 'text-text-secondary'}>
                    {isRunning ? 'Running' : 'Offloaded · Ready'}
                  </span>
                </div>

                <div className="text-xs font-mono text-text-secondary w-24 text-right font-semibold">
                  {(model.vram_bytes / 1024 ** 3).toFixed(1)} GB VRAM
                </div>

                <div className="hidden md:block flex-shrink-0">
                  <RadialGraph
                    percent={isRunning ? Math.round(telemetry?.gpu_load ?? 0) : 0}
                    preset={type === 'Image' ? 'gpu' : 'cpu'}
                    size={44}
                    showCenterText={true}
                  />
                </div>

                <button
                  onClick={() => (isRunning ? unload(model.id) : load(model.id, model.model_type)).catch(console.error)}
                  disabled={loadingId === model.id}
                  className={`p-2 rounded-full transition-colors cursor-pointer ${
                    isRunning
                      ? 'text-text-secondary hover:bg-[var(--color-hover)] hover:text-text-primary'
                      : 'text-primary bg-[var(--color-primary-bg)] hover:bg-primary/20'
                  }`}
                  aria-label={isRunning ? 'Stop workload' : 'Start workload'}
                >
                  {isRunning ? (
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                      <rect x="6" y="6" width="12" height="12" rx="2" />
                    </svg>
                  ) : (
                    <Play size={14} strokeWidth={2.5} />
                  )}
                </button>
              </div>
            );
          })}

          <div className="pt-3 border-t border-[var(--color-border)] flex flex-wrap items-center justify-between gap-3 text-xs">
            <div className="flex items-center gap-2">
              <SlidersHorizontal size={14} className="text-text-secondary" />
              <span className="text-text-secondary">Engine Governor Mode:</span>
              <Select
                options={[
                  { value: 'balanced', label: 'Balanced' },
                  { value: 'performance', label: 'Performance' },
                  { value: 'efficiency', label: 'Efficiency' },
                ]}
                value={mode}
                onChange={(value) => updateMode(value as typeof mode).catch(console.error)}
                className="w-32"
              />
            </div>

            <div className="flex items-center gap-4">
              <button
                onClick={() => navigate('/system')}
                title="View System telemetry"
                className="flex items-center gap-1.5 font-semibold text-secondary dynamic-glass-pill px-3 py-1 cursor-pointer"
              >
                <StatusDot status={pagerStatus === 'active' ? 'running' : 'warning'} size={6} />
                <span>VRAM Pager: {pagerStatus[0].toUpperCase() + pagerStatus.slice(1)}</span>
              </button>

              <button
                onClick={() => navigate('/settings')}
                className="p-1.5 rounded-lg text-text-secondary hover:bg-[var(--color-hover)] hover:text-text-primary transition-colors cursor-pointer"
                aria-label="Engine Settings"
              >
                <Settings size={15} strokeWidth={2} />
              </button>
            </div>
          </div>
        </div>
      </div>

      {/* Floating Bottom Prompt Bar */}
      <form
        onSubmit={handleStickySubmit}
        className="fixed bottom-4 left-[216px] right-[276px] z-40"
      >
        <div className="max-w-4xl mx-auto glass-floating dynamic-glass-pill p-2.5 shadow-2xl flex items-center gap-2 focus-within:ring-2 focus-within:ring-primary/20 transition-all">
          <AttachMenuPopover
            iconSize={18}
            onSelect={chooseAttachment}
          />

          {attachedFile && (
            <span className="badge badge-primary font-mono text-[10px] shrink-0 dynamic-glass-pill">
              <FileText size={12} />
              {attachedFile.split(/[\\/]/).pop()}
              <button
                type="button"
                onClick={() => setAttachedFile(null)}
                className="ml-1 text-primary hover:text-text-primary cursor-pointer"
              >
                ×
              </button>
            </span>
          )}

          <input
            type="text"
            value={stickyInput}
            onChange={(e) => setStickyInput(e.target.value)}
            placeholder="Ask Gabriel anything locally..."
            className="flex-1 bg-transparent border-none focus:outline-none text-xs font-semibold text-text-primary placeholder:text-text-secondary"
          />

          <button
            type="button"
            onClick={toggleRecording}
            title={isRecording ? 'Stop recording voice prompt' : isTranscribing ? 'Transcribing...' : 'Record voice prompt'}
            aria-label="Voice input"
            className={`p-2 rounded-full transition-all shrink-0 cursor-pointer ${
              isRecording
                ? 'bg-rose-500/20 text-rose-500 animate-pulse ring-1 ring-rose-500'
                : isTranscribing
                ? 'text-primary animate-spin'
                : 'text-text-secondary hover:text-text-primary hover:bg-[var(--color-hover)]'
            }`}
          >
            {isTranscribing ? <Loader2 size={17} className="animate-spin" /> : <Mic size={17} strokeWidth={1.8} />}
          </button>

          <button
            type="submit"
            disabled={!stickyInput.trim() && !attachedFile}
            className="p-2 text-white bg-primary rounded-full disabled:opacity-40 disabled:hover:scale-100 hover:scale-105 active:scale-95 transition-all cursor-pointer shrink-0"
            aria-label="Send message"
          >
            <ArrowRight size={16} strokeWidth={2} />
          </button>
        </div>
      </form>
    </div>
  );
};

const QuickActionCard: FC<{ action: typeof quickActions[0] }> = ({ action }) => {
  const { ref, onPointerMove } = useCardSpotlight();

  const isPurple = action.color === 'primary';
  const isGreen = action.color === 'secondary';

  const iconColorClass = isPurple
    ? 'bg-[var(--color-primary-bg)] text-primary'
    : isGreen
    ? 'bg-[var(--color-secondary-bg)] text-secondary'
    : 'bg-[var(--color-hover)] text-text-secondary';

  const renderIcon = () => {
    if (action.icon === 'MessageSquare') return <MessageSquare size={18} strokeWidth={2} />;
    if (action.icon === 'Image') return <ImageIcon size={18} strokeWidth={2} />;
    if (action.icon === 'Mic') return <Sparkles size={18} strokeWidth={2} />;
    return <Box size={18} strokeWidth={2} />;
  };

  return (
    <Link
      ref={ref as any}
      onPointerMove={onPointerMove as any}
      to={action.route}
      className="glass-panel aurora-glass dynamic-glass-card p-4 flex flex-col justify-between group cursor-pointer hover:-translate-y-1 transition-all"
    >
      <div className="space-y-3 relative z-10">
        <div className={`w-9 h-9 rounded-xl ${iconColorClass} flex items-center justify-center transition-transform group-hover:scale-105`}>
          {renderIcon()}
        </div>
        <div>
          <h3 className="font-bold text-xs text-text-primary group-hover:text-primary transition-colors">
            {action.label}
          </h3>
          <p className="text-[11px] text-text-secondary mt-1 line-clamp-2 leading-snug">
            {action.description}
          </p>
        </div>
      </div>
    </Link>
  );
};