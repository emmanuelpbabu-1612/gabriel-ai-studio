import type { FC } from 'react';
import { useState, useEffect } from 'react';
import {
  Image as ImageIcon,
  Download,
  Heart,
  RotateCcw,
  Sparkles,
  SlidersHorizontal,
  Sliders,
  Loader2,
} from 'lucide-react';
import { ModelBadge } from '../components/shared';
import { Select } from '../components/shared/Select';
import { useModels } from '../hooks/useModels';
import { useImage, type GeneratedImage } from '../hooks/useImage';
import { recentGenerations } from '../data/mockData';

const aspectRatios = ['1:1', '16:9', '9:16', '4:3', '3:4'] as const;
type AspectRatio = typeof aspectRatios[number];

// Stub backend emits BMP bytes (base64 starts with "Qk0"), real Candle
// backend emits PNG (base64 starts with "iVBOR"). Sniff the prefix so both
// paths render without a new Rust PNG dependency in default builds.
function imageSrc(b64: string): string {
  const mime = b64.startsWith('Qk0') ? 'image/bmp' : 'image/png';
  return `data:${mime};base64,${b64}`;
}

// Aspect ratio to dimensions mapping
const aspectRatioDimensions: Record<AspectRatio, { width: number; height: number }> = {
  '1:1': { width: 1024, height: 1024 },
  '16:9': { width: 1344, height: 768 },
  '9:16': { width: 768, height: 1344 },
  '4:3': { width: 1152, height: 864 },
  '3:4': { width: 864, height: 1152 },
};

export const Image: FC = () => {
  const { models } = useModels();
  const imageModels = models.filter(m => m.model_type === 'image');
  const { isGenerating, error, generateImage } = useImage();

  const [selectedModel, setSelectedModel] = useState<string>('');
  const [prompt, setPrompt] = useState('');
  const [negativePrompt, setNegativePrompt] = useState('');
  const [aspectRatio, setAspectRatio] = useState<AspectRatio>('1:1');
  const [showNegative, setShowNegative] = useState(false);
  const [steps, setSteps] = useState(8);
  const [cfgScale, setCfgScale] = useState(1.5);
  const [sampler, setSampler] = useState('Euler a');
  // In-memory session gallery + favorites (appended from real generations).
  const [liveImages, setLiveImages] = useState<GeneratedImage[]>([]);
  const [favorites, setFavorites] = useState<Set<string>>(new Set());

  // Initialize selected model from available models
  useEffect(() => {
    if (imageModels.length > 0 && !selectedModel) {
      const runningModel = imageModels.find(m => m.residency === 'gpu');
      setSelectedModel(runningModel?.id || imageModels[0].id);
    }
  }, [imageModels, selectedModel]);

  const handleGenerate = async (overridePrompt?: string) => {
    const activePrompt = (overridePrompt ?? prompt).trim();
    if (!activePrompt || !selectedModel) return;
    
    const dimensions = aspectRatioDimensions[aspectRatio];
    const result = await generateImage(selectedModel, {
      prompt: activePrompt,
      width: dimensions.width,
      height: dimensions.height,
      steps,
      cfg_scale: cfgScale,
      sampler,
      negative_prompt: showNegative ? negativePrompt : undefined,
    });
    
    if (result) {
      // Keep a session gallery so Recent History reflects real output.
      setLiveImages(prev => [result, ...prev].slice(0, 8));
      // Clear prompt after successful generation
      setPrompt('');
    }
  };

  const downloadImage = (img: GeneratedImage) => {
    const a = document.createElement('a');
    a.href = imageSrc(img.b64_json);
    a.download = `gabriel-image-${img.id}.png`;
    document.body.appendChild(a);
    a.click();
    a.remove();
  };

  const toggleFavorite = (id: string) => {
    setFavorites(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  return (
    <div className="flex flex-col h-full max-w-6xl mx-auto space-y-4 text-text-primary select-none min-h-[540px]">
      {/* Top Header Shelf */}
      <div className="flex flex-wrap items-center justify-between gap-3 glass-panel aurora-glass p-3">
        <div className="flex items-center gap-3 flex-1 min-w-[240px]">
          <Select
            options={imageModels.map(m => ({ value: m.id, label: m.id }))}
            value={selectedModel}
            onChange={setSelectedModel}
            className="w-56"
          />

          <Select
            options={[
              { value: '8', label: '8 steps (Fast)' },
              { value: '16', label: '16 steps' },
              { value: '25', label: '25 steps (Balanced)' },
              { value: '50', label: '50 steps (Quality)' },
            ]}
            value={steps.toString()}
            onChange={(val) => setSteps(Number(val))}
            className="w-44"
          />
        </div>
      </div>

      {/* Main Canvas & Settings */}
      <div className="flex-1 flex gap-4 min-h-0 overflow-hidden">
        {/* Left Column: Canvas & Prompt (Flex column filling height) */}
        <div className="flex-1 flex flex-col gap-4 h-full min-h-0 overflow-hidden">
          {/* 2x2 Grid Canvas (Shrinks and scrolls rather than colliding) */}
          <div className="glass-panel aurora-glass p-4 flex-1 min-h-0 overflow-y-auto">
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-4 h-full">
              {/* Real session generations first (wired actions) */}
              {liveImages.map((img) => (
                <div
                  key={img.id}
                  className="relative aspect-square rounded-2xl overflow-hidden glass-panel group border border-[var(--color-border)] shadow-xs"
                >
                  <img
                    src={imageSrc(img.b64_json)}
                    alt={img.prompt}
                    className="w-full h-full object-cover"
                  />
                  <div className="absolute top-2.5 right-2.5 flex gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                    <button onClick={() => downloadImage(img)} className="p-1.5 rounded-full glass-pill text-text-primary hover:bg-white transition-colors cursor-pointer" aria-label="Download">
                      <Download size={13} strokeWidth={2} />
                    </button>
                    <button
                      onClick={() => toggleFavorite(img.id)}
                      className={`p-1.5 rounded-full glass-pill transition-colors cursor-pointer ${favorites.has(img.id) ? 'text-red-400' : 'text-text-primary hover:bg-white'}`}
                      aria-label={favorites.has(img.id) ? 'Unfavorite' : 'Favorite'}
                      title={favorites.has(img.id) ? 'Favorited' : 'Favorite'}
                    >
                      <Heart size={13} strokeWidth={2} fill={favorites.has(img.id) ? 'currentColor' : 'none'} />
                    </button>
                    <button onClick={() => handleGenerate(img.prompt)} className="p-1.5 rounded-full glass-pill text-text-primary hover:bg-white transition-colors cursor-pointer" aria-label="Regenerate" title="Regenerate from this prompt">
                      <RotateCcw size={13} strokeWidth={2} />
                    </button>
                  </div>
                  <div className="absolute bottom-2.5 left-2.5 right-2.5 text-[11px] text-white font-semibold truncate drop-shadow-md">
                    {img.prompt}
                  </div>
                </div>
              ))}
              {recentGenerations.slice(0, Math.max(0, 4 - liveImages.length)).map((gen) => (
                <div
                  key={gen.id}
                  className="relative aspect-square rounded-2xl overflow-hidden glass-panel group border border-[var(--color-border)] shadow-xs"
                >
                  <div className="absolute inset-0" style={{ background: gen.thumbnail }} />
                  <div className="absolute inset-0 flex items-center justify-center text-white/50">
                    <ImageIcon size={36} strokeWidth={1.5} />
                  </div>
                  <div className="absolute top-2.5 left-2.5 text-[10px] font-bold text-white/80 bg-black/40 rounded-full px-2 py-0.5">
                    Sample
                  </div>
                  <div className="absolute bottom-2.5 left-2.5 right-2.5 text-[11px] text-white font-semibold truncate drop-shadow-md">
                    {gen.prompt}
                  </div>
                </div>
              ))}
              {liveImages.length === 0 && recentGenerations.length === 0 && (
                <div className="col-span-full text-center text-xs text-text-secondary py-8">
                  No generations yet — describe an image and press Generate.
                </div>
              )}
            </div>
          </div>

          {/* Prompt Bar (flex-shrink-0 and securely pinned at bottom) */}
          <div className="glass-panel aurora-glass p-4 space-y-3 flex-shrink-0 mt-auto">
            <textarea
              placeholder="Describe the image you want to generate locally..."
              className="w-full glass-panel p-3 text-xs text-text-primary placeholder:text-text-secondary focus:outline-none focus:ring-2 focus:ring-primary/20 resize-none font-medium"
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              rows={2}
            />

            <div className="flex flex-wrap items-center justify-between gap-2">
              <button
                type="button"
                onClick={() => setShowNegative(!showNegative)}
                className="text-xs font-semibold text-primary hover:underline flex items-center gap-1 cursor-pointer"
              >
                <SlidersHorizontal size={13} strokeWidth={2} />
                {showNegative ? 'Hide' : 'Show'} negative prompt
              </button>

              <div className="flex items-center gap-1">
                {aspectRatios.map(ratio => (
                  <button
                    key={ratio}
                    onClick={() => setAspectRatio(ratio)}
                    className={`rounded-full px-2.5 py-1 text-[11px] font-semibold transition-all cursor-pointer ${
                      aspectRatio === ratio
                        ? 'bg-primary text-white shadow-xs'
                        : 'bg-[var(--color-hover)] text-text-secondary hover:text-text-primary'
                    }`}
                  >
                    {ratio}
                  </button>
                ))}
              </div>
            </div>

            {showNegative && (
              <textarea
                placeholder="Negative prompt (what to avoid in image)..."
                className="w-full glass-panel p-2.5 text-xs text-text-primary placeholder:text-text-secondary focus:outline-none focus:ring-2 focus:ring-primary/20 resize-none font-medium"
                value={negativePrompt}
                onChange={(e) => setNegativePrompt(e.target.value)}
                rows={2}
              />
            )}

            <button 
              onClick={() => handleGenerate()}
              disabled={isGenerating || !prompt.trim() || !selectedModel}
              className="w-full btn-primary py-2.5 flex items-center justify-center gap-2 text-xs font-semibold shrink-0 disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {isGenerating ? (
                <>
                  <Loader2 size={16} strokeWidth={2} className="animate-spin" />
                  Generating...
                </>
              ) : (
                <>
                  <Sparkles size={16} strokeWidth={2} />
                  Generate Image
                </>
              )}
            </button>
            {error && (
              <div className="rounded-xl border border-red-400/30 bg-red-400/10 px-3 py-2 text-[11px] text-red-200">
                {error}
              </div>
            )}
          </div>
        </div>

        {/* Supplementary Info Panel */}
        <div className="w-72 flex-shrink-0 glass-panel aurora-glass p-4 overflow-y-auto space-y-4 text-xs">
          <div className="space-y-3">
            <div className="font-bold text-text-primary flex items-center gap-1.5 border-b border-[var(--color-border)] pb-2">
              <Sliders size={14} className="text-secondary" />
              Generation Parameters
            </div>
            <p className="text-[10px] text-text-secondary leading-snug">Stub backend: prompt + aspect size are live; steps/CFG/sampler are forwarded but currently ignored.</p>

            <div>
              <div className="flex justify-between mb-1 text-text-secondary font-medium">
                <span>Sampling Steps</span>
                <span className="font-mono font-bold text-text-primary">{steps}</span>
              </div>
              <input type="range" min="1" max="50" value={steps} onChange={(e) => setSteps(Number(e.target.value))} className="w-full accent-primary cursor-pointer" />
            </div>

            <div>
              <div className="flex justify-between mb-1 text-text-secondary font-medium">
                <span>CFG Scale</span>
                <span className="font-mono font-bold text-text-primary">{cfgScale}</span>
              </div>
              <input type="range" min="1" max="20" step="0.5" value={cfgScale} onChange={(e) => setCfgScale(Number(e.target.value))} className="w-full accent-primary cursor-pointer" />
            </div>

            <div>
              <div className="text-text-secondary mb-1 font-medium">Sampler</div>
              <Select
                options={['Euler a', 'Euler', 'DPM++ 2M Karras', 'DPM++ SDE Karras', 'DDIM']}
                value={sampler}
                onChange={setSampler}
                className="w-full"
              />
            </div>
          </div>

          <div className="pt-3 border-t border-[var(--color-border)] space-y-2">
            <div className="font-bold text-text-primary">Model Info</div>
            {(() => {
              const model = models.find(m => m.id === selectedModel);
              if (!model) return (
                <div className="text-text-secondary text-center py-4">Select a model to see details</div>
              );
              return (
                <>
                  <ModelBadge 
                    type="Image" 
                    status={model.residency === 'gpu' ? 'running' : 'idle'} 
                    size="md" 
                  />
                  <div className="space-y-1.5 text-text-secondary pt-1 font-medium">
                    <div className="flex justify-between">
                      <span>VRAM Footprint</span>
                      <span className="font-mono text-text-primary font-bold">
                        {(model.vram_bytes / 1024 ** 3).toFixed(1)} GB
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span>Status</span>
                      <span className={`badge ${model.residency === 'gpu' ? 'badge-secondary' : 'badge-tertiary'} font-mono font-bold`}>
                        {model.residency === 'gpu' ? 'Running (VRAM)' : model.available ? 'Offloaded (RAM)' : 'Available'}
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span>Disk Size</span>
                      <span className="font-mono text-text-primary font-bold">
                        {(model.disk_bytes / 1024 ** 3).toFixed(1)} GB
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span>Idle Time</span>
                      <span className="font-mono text-text-secondary font-bold">
                        {model.idle_secs > 3600 
                          ? `${Math.floor(model.idle_secs / 3600)}h` 
                          : model.idle_secs > 60 
                            ? `${Math.floor(model.idle_secs / 60)}m` 
                            : `${model.idle_secs}s`}
                      </span>
                    </div>
                  </div>
                </>
              );
            })()}
          </div>

          {/* Clean Flexbox Recent History Chips */}
          <div className="pt-3 border-t border-[var(--color-border)] space-y-2">
            <div className="font-bold text-text-primary">Recent History</div>
            <div className="flex flex-wrap gap-3">
              {liveImages.slice(0, 4).map(img => (
                <button
                  key={img.id}
                  onClick={() => handleGenerate(img.prompt)}
                  className="w-16 h-16 rounded-xl aspect-square overflow-hidden relative glass-panel group border border-[var(--color-border)] shrink-0 shadow-xs cursor-pointer"
                  title={`Regenerate: ${img.prompt}`}
                >
                  <img src={imageSrc(img.b64_json)} alt={img.prompt} className="absolute inset-0 w-full h-full object-cover" />
                  <span className="absolute inset-0 flex items-center justify-center text-white opacity-0 group-hover:opacity-100 bg-black/30 transition-opacity">
                    <ImageIcon size={16} strokeWidth={1.5} />
                  </span>
                </button>
              ))}
              {recentGenerations.slice(0, Math.max(0, 4 - liveImages.length)).map(gen => (
                <div
                  key={gen.id}
                  className="w-16 h-16 rounded-xl aspect-square overflow-hidden relative glass-panel group border border-[var(--color-border)] shrink-0 shadow-xs"
                  title="Sample (not generated this session)"
                >
                  <div className="absolute inset-0" style={{ background: gen.thumbnail }} />
                  <div className="absolute inset-0 flex items-center justify-center text-white opacity-0 group-hover:opacity-100 bg-black/30 transition-opacity">
                    <ImageIcon size={16} strokeWidth={1.5} />
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};