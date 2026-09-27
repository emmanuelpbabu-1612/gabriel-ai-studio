import type { FC } from 'react';
import { useState, useEffect, useCallback } from 'react';
import {
  ChevronRight,
  Trash2,
  RotateCcw,
  Info,
  Check,
  Database,
  Sliders,
  Palette,
  Keyboard,
  Settings as SettingsIcon,
  Minus,
  Plus,
  Loader2,
} from 'lucide-react';
import { settingsSections } from '../data/mockData';
import { useAccentTheme } from '../hooks/useAccentTheme';
import { useTheme } from '../hooks/useTheme';
import { useDensity, type Density } from '../hooks/useDensity';
import { Select } from '../components/shared/Select';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { useEngineMode, useGovernorSettings, useProfile, useAppSettings } from '../hooks/useEngineSettings';
import { useSettingsActions } from '../hooks/useSettingsActions';

const sections = settingsSections;
type SectionId = typeof sections[number]['id'];

interface SettingRowProps {
  label: string;
  description?: string;
  children: React.ReactNode;
}

const SettingRow: FC<SettingRowProps> = ({ label, description, children }) => (
  <div className="flex items-center justify-between py-3.5 border-b border-[var(--color-border)] gap-4">
    <div className="flex-1 min-w-0">
      <div className="font-bold text-xs text-text-primary">{label}</div>
      {description && <div className="text-[11px] text-text-secondary mt-0.5 truncate">{description}</div>}
    </div>
    <div className="shrink-0">{children}</div>
  </div>
);

interface NumberStepperProps {
  value: string;
  onChange: (value: string) => void;
  min: number;
  max: number;
  ariaLabel: string;
  suffix?: string;
}

const NumberStepper: FC<NumberStepperProps> = ({ value, onChange, min, max, ariaLabel, suffix }) => {
  const changeValue = (delta: number) => {
    const nextValue = Math.max(min, Math.min(max, Number(value || min) + delta));
    onChange(String(nextValue));
  };

  return (
    <div className="number-stepper glass-pill">
      <input
        type="number"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="number-stepper-input"
        min={min}
        max={max}
        aria-label={ariaLabel}
      />
      {suffix && (
        <span className="number-stepper-suffix" aria-hidden="true">
          {suffix}
        </span>
      )}
      <div className="number-stepper-controls">
        <button type="button" onClick={() => changeValue(1)} aria-label={`Increase ${ariaLabel}`}>
          <Plus size={11} strokeWidth={2.5} />
        </button>
        <button type="button" onClick={() => changeValue(-1)} aria-label={`Decrease ${ariaLabel}`}>
          <Minus size={11} strokeWidth={2.5} />
        </button>
      </div>
    </div>
  );
};

interface ShortcutRowProps {
  action: string;
  winKeys: string[];
  macKeys: string[];
  isMac: boolean;
}

const ShortcutRow: FC<ShortcutRowProps> = ({ action, winKeys, macKeys, isMac }) => {
  const keys = isMac ? macKeys : winKeys;
  return (
    <div className="flex items-center justify-between py-2.5 border-b border-[var(--color-border)] text-xs gap-3">
      <span className="text-text-primary font-semibold">{action}</span>
      <div className="flex items-center gap-1 shrink-0">
        {keys.map((key, i) => (
          <kbd
            key={i}
            className="px-2 py-0.5 rounded-md text-[10px] font-mono text-text-primary bg-[var(--color-hover)] border border-[var(--color-border)] font-bold shadow-2xs"
          >
            {key}
          </kbd>
        ))}
      </div>
    </div>
  );
};

const Toggle: FC<{ defaultChecked?: boolean; checked?: boolean; onChange?: (val: boolean) => void }> = ({
  defaultChecked = false,
  checked: propChecked,
  onChange,
}) => {
  const [internalChecked, setInternalChecked] = useState(defaultChecked);
  const isControlled = propChecked !== undefined;
  const isChecked = isControlled ? propChecked : internalChecked;

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const val = e.target.checked;
    if (!isControlled) setInternalChecked(val);
    if (onChange) onChange(val);
  };

  return (
    <label className="toggle-switch">
      <input type="checkbox" checked={isChecked} onChange={handleChange} />
      <span className="toggle-slider" />
    </label>
  );
};

export const Settings: FC = () => {
  const [activeSection, setActiveSection] = useState<SectionId>('general');
  const [startupPage, setStartupPage] = useState('/');
  const { density, setDensity } = useDensity();
  const { refresh: refreshMode } = useEngineMode();
  const { settings: governorSettings, updateWatermarks, refresh: refreshGovernor } = useGovernorSettings();
  const { name, updateName, refresh: refreshProfile } = useProfile();
  const { settings: appSettings, updateAppTitle, updateStartupRoute } = useAppSettings();
  const [profileDraft, setProfileDraft] = useState('');
  const [appTitleDraft, setAppTitleDraft] = useState('Gabriel');
  const [modelsDir, setModelsDir] = useState('');
  const [isMac, setIsMac] = useState(false);

  // Engine Governor Settings — hydrated from persisted backend settings
  // (defaults match backend: 80%, 4 models, auto-load off, 120s idle).
  const vramHighWatermark = governorSettings?.vram_high_watermark ?? 85;
  const vramLowWatermark = governorSettings?.vram_low_watermark ?? 70;
  const [idleOffloadTimeout, setIdleOffloadTimeout] = useState('2');
  const [bandwidthCeiling, setBandwidthCeiling] = useState('80');
  const [maxLoadedModels, setMaxLoadedModels] = useState('4');
  const [autoLoadOnRequest, setAutoLoadOnRequest] = useState(false);

  // Hydrate ephemeral controls once persisted settings load.
  useEffect(() => {
    if (!governorSettings) return;
    setBandwidthCeiling(String(Math.round(governorSettings.bandwidth_ceiling_percent ?? 80)));
    setMaxLoadedModels(String(governorSettings.max_loaded_models ?? 4));
    setAutoLoadOnRequest(!!governorSettings.auto_load_on_request);
    const secs = governorSettings.idle_offload_after_secs ?? 120;
    const mins = Math.round(secs / 60);
    setIdleOffloadTimeout(['2', '5', '15', '30', '60'].includes(String(mins)) ? String(mins) : '2');
  }, [governorSettings]);

  const { 
    loading: settingsLoading, 
    setBandwidthCeiling: doSetBandwidthCeiling,
    setMaxLoadedModels: doSetMaxLoadedModels,
    setAutoLoadOnRequest: doSetAutoLoadOnRequest,
    setIdleOffloadTimeout: doSetIdleOffloadTimeout,
    setModelsDirAction,
    verifyEngineBinary,
    resetSettings,
    clearModelCaches,
  } = useSettingsActions();

  const handleBandwidthCeilingChange = useCallback(async (value: string) => {
    setBandwidthCeiling(value);
    await doSetBandwidthCeiling(Number(value));
  }, [doSetBandwidthCeiling]);

  const handleMaxLoadedModelsChange = useCallback(async (value: string) => {
    setMaxLoadedModels(value);
    await doSetMaxLoadedModels(Number(value));
  }, [doSetMaxLoadedModels]);

  const handleAutoLoadOnRequestChange = useCallback(async (value: boolean) => {
    setAutoLoadOnRequest(value);
    await doSetAutoLoadOnRequest(value);
  }, [doSetAutoLoadOnRequest]);

  const handleIdleOffloadTimeoutChange = useCallback(async (value: string) => {
    setIdleOffloadTimeout(value);
    if (value !== 'never') {
      await doSetIdleOffloadTimeout(Number(value) * 60); // Convert minutes to seconds
    }
  }, [doSetIdleOffloadTimeout]);

  const { accentColor, setAccentColor, presets } = useAccentTheme();
  const { isDark, toggleTheme } = useTheme();

  useEffect(() => setProfileDraft(name), [name]);

  useEffect(() => {
    if (appSettings?.app_title) setAppTitleDraft(appSettings.app_title);
    if (appSettings?.startup_route) setStartupPage(appSettings.startup_route);
  }, [appSettings]);

  useEffect(() => {
    invoke<string>('get_models_dir').then(setModelsDir).catch(console.error);
  }, []);

  useEffect(() => {
    if (typeof navigator !== 'undefined') {
      const platform = navigator.platform || navigator.userAgent || '';
      setIsMac(/Mac|iPod|iPhone|iPad/.test(platform));
    }
  }, []);

  const getSectionIcon = (id: SectionId) => {
    switch (id) {
      case 'general':        return <SettingsIcon size={16} />;
      case 'appearance':     return <Palette size={16} />;
      case 'governor':       return <Sliders size={16} />;
      case 'shortcuts':      return <Keyboard size={16} />;
    }
  };

  const renderSectionContent = () => {
    switch (activeSection) {
      case 'general':
        return (
          <div className="space-y-1">
            <SettingRow label="App Title" description="Persisted to local settings">
              <input
                type="text"
                className="w-48 glass-pill px-3 py-1.5 text-xs text-text-primary focus:outline-none focus:ring-2 focus:ring-primary/20"
                value={appTitleDraft}
                onChange={(event) => setAppTitleDraft(event.target.value)}
                onBlur={() => updateAppTitle(appTitleDraft).catch(console.error)}
              />
            </SettingRow>

            <SettingRow label="Display Name" description="Name shown in the Home greeting">
              <input
                type="text"
                className="w-48 glass-pill px-3 py-1.5 text-xs text-text-primary focus:outline-none focus:ring-2 focus:ring-primary/20"
                value={profileDraft}
                onChange={(event) => setProfileDraft(event.target.value)}
                onBlur={() => updateName(profileDraft).catch(console.error)}
              />
            </SettingRow>

            <SettingRow label="Startup Route" description="Persisted to local settings">
              <Select
                options={[
                  { value: '/', label: 'Home' },
                  { value: '/chat', label: 'Chat' },
                  { value: '/image', label: 'Image' },
                  { value: '/voice', label: 'Voice' },
                  { value: '/models', label: 'Models' },
                  { value: '/system', label: 'System' },
                ]}
                value={startupPage}
                onChange={(v) => {
                  setStartupPage(v);
                  updateStartupRoute(v).catch(console.error);
                }}
                className="w-44"
              />
            </SettingRow>

            <SettingRow label="Models Directory" description="Local directory on disk for weights and safetensors">
              <div className="flex items-center gap-2">
                <span
                  className="glass-pill px-3 py-1.5 font-mono text-[11px] text-text-primary tracking-tight truncate max-w-[260px] select-none cursor-default inline-flex items-center"
                  title={modelsDir || 'loading…'}
                >
                  {modelsDir || 'loading…'}
                </span>
                <button
                  onClick={async () => {
                    try {
                      const sel = await open({ directory: true, multiple: false });
                      if (typeof sel === 'string' && sel.trim()) {
                        await setModelsDirAction(sel);
                        setModelsDir(sel);
                      }
                    } catch (e) {
                      console.error('Failed to set models directory:', e);
                    }
                  }}
                  className="btn-secondary py-1 px-3 text-xs font-semibold shrink-0 cursor-pointer"
                >
                  Browse
                </button>
              </div>
            </SettingRow>
          </div>
        );

      case 'appearance':
        return (
          <div className="space-y-1">
            <SettingRow label="Theme Mode" description="Toggle between light and dark mode">
              <Toggle checked={isDark} onChange={toggleTheme} />
            </SettingRow>

            <SettingRow label="Accent Color" description="Changes app primary accent color globally">
              <div className="flex items-center gap-2.5">
                {presets.map(preset => {
                  const isSelected = accentColor.toLowerCase() === preset.hex.toLowerCase();
                  return (
                    <button
                      key={preset.hex}
                      type="button"
                      onClick={() => setAccentColor(preset.hex)}
                      className={`w-7 h-7 rounded-full flex items-center justify-center transition-all duration-200 hover:scale-110 cursor-pointer shadow-sm relative ${
                        isSelected
                          ? 'ring-2 ring-offset-2 ring-offset-[var(--color-card)] ring-[var(--color-text-primary)] scale-110'
                          : 'hover:opacity-90 opacity-80'
                      }`}
                      style={{
                        backgroundColor: preset.hex,
                        border: isSelected ? '2px solid white' : '1px solid rgba(255, 255, 255, 0.25)',
                      }}
                      title={preset.label}
                      aria-label={`Set accent color to ${preset.label}`}
                    >
                      {isSelected && <Check size={14} className="text-white drop-shadow-sm" strokeWidth={3} />}
                    </button>
                  );
                })}
              </div>
            </SettingRow>

            <SettingRow label="UI Density" description="Adjust text sizing and UI spacing across the application">
              <Select
                options={[
                  { value: 'compact', label: 'Compact (12px)' },
                  { value: 'medium', label: 'Medium (13px)' },
                  { value: 'spacious', label: 'Spacious (14px)' },
                ]}
                value={density}
                onChange={(v) => setDensity(v as Density)}
                className="w-44"
              />
            </SettingRow>
          </div>
        );

      case 'governor':
        return (
          <div className="space-y-2">
            <SettingRow label="VRAM High Watermark" description="Threshold percentage to trigger automatic model offloading">
              <div className="flex items-center gap-3">
                <input
                  type="range"
                  min="50"
                  max="98"
                  value={vramHighWatermark}
                  onChange={(e) => updateWatermarks(Number(e.target.value), vramLowWatermark).catch(console.error)}
                  className="w-28 accent-primary cursor-pointer"
                />
                <span className="glass-pill px-3 py-1 font-mono font-bold text-xs text-text-primary w-14 text-center">
                  {vramHighWatermark}%
                </span>
              </div>
            </SettingRow>

            <SettingRow label="VRAM Low Watermark" description="Target baseline VRAM utilization after paging out">
              <div className="flex items-center gap-3">
                <input
                  type="range"
                  min="30"
                  max="80"
                  value={vramLowWatermark}
                  onChange={(e) => updateWatermarks(vramHighWatermark, Number(e.target.value)).catch(console.error)}
                  className="w-28 accent-primary cursor-pointer"
                />
                <span className="glass-pill px-3 py-1 font-mono font-bold text-xs text-text-primary w-14 text-center">
                  {vramLowWatermark}%
                </span>
              </div>
            </SettingRow>

            <SettingRow label="Idle Offload Timeout" description="Inactivity duration before paging model to System RAM">
              <Select
                options={[
                  { value: '2', label: '2 minutes (default)' },
                  { value: '5', label: '5 minutes' },
                  { value: '15', label: '15 minutes' },
                  { value: '30', label: '30 minutes' },
                  { value: '60', label: '1 hour' },
                  { value: 'never', label: 'Never Offload' },
                ]}
                value={idleOffloadTimeout}
                onChange={handleIdleOffloadTimeoutChange}
                className="w-40"
              />
            </SettingRow>

            <SettingRow label="Bandwidth Ceiling" description="Maximum memory bus transfer rate limit (percent of bus)">
              <NumberStepper
                value={bandwidthCeiling}
                onChange={handleBandwidthCeilingChange}
                min={10}
                max={100}
                ariaLabel="Bandwidth ceiling percent"
                suffix="%"
              />
            </SettingRow>

            <SettingRow label="Max Loaded Models" description="Maximum concurrent active models in memory">
              <NumberStepper
                value={maxLoadedModels}
                onChange={handleMaxLoadedModelsChange}
                min={1}
                max={32}
                ariaLabel="Maximum loaded models"
              />
            </SettingRow>

            <SettingRow label="Auto-load on Request" description="Automatically page model into VRAM when prompted">
              <Toggle checked={autoLoadOnRequest} onChange={handleAutoLoadOnRequestChange} />
            </SettingRow>
          </div>
        );

      case 'shortcuts':
        return (
          <div className="space-y-1">
            <ShortcutRow action="Open Command Palette / Search" winKeys={['Ctrl', 'K']} macKeys={['⌘', 'K']} isMac={isMac} />
            <ShortcutRow action="New Chat Session" winKeys={['Ctrl', 'N']} macKeys={['⌘', 'N']} isMac={isMac} />
            <ShortcutRow action="Toggle Light/Dark Theme" winKeys={['Ctrl', 'T']} macKeys={['⌘', 'T']} isMac={isMac} />
            <ShortcutRow action="Open Settings" winKeys={['Ctrl', ',']} macKeys={['⌘', ',']} isMac={isMac} />
            <ShortcutRow action="Send Prompt" winKeys={['Ctrl', 'Enter']} macKeys={['⌘', 'Enter']} isMac={isMac} />
            <ShortcutRow action="Stop Generation" winKeys={['Ctrl', '.']} macKeys={['⌘', '.']} isMac={isMac} />
          </div>
        );
    }
  };

  return (
    <div className="flex flex-col h-full max-w-6xl mx-auto space-y-4 text-text-primary">
      {/* Top Header Bar */}
      <div className="flex items-center justify-between glass-panel aurora-glass p-3.5">
        <div className="flex items-center gap-2.5">
          <SettingsIcon className="text-primary" size={20} />
          <h1 className="font-bold text-lg text-text-primary">Engine & Application Settings</h1>
        </div>
      </div>

      {/* Main Workspace */}
      <div className="flex-1 flex gap-4 min-h-0 overflow-hidden">
        {/* Section Navigation Tabs */}
        <nav className="w-52 shrink-0 glass-panel aurora-glass p-3 space-y-1" aria-label="Settings sections">
          {sections.map(section => (
            <button
              key={section.id}
              onClick={() => setActiveSection(section.id as SectionId)}
              className={`w-full flex items-center gap-2.5 px-3 py-2.5 rounded-xl text-xs font-semibold transition-all cursor-pointer text-left ${
                activeSection === section.id
                  ? 'bg-[var(--color-primary-bg)] text-primary font-bold'
                  : 'text-text-secondary hover:bg-[var(--color-hover)] hover:text-text-primary'
              }`}
            >
              {getSectionIcon(section.id as SectionId)}
              <span>{section.label}</span>
            </button>
          ))}
        </nav>

        {/* Main Section Content Box */}
        <div className="flex-1 glass-panel aurora-glass p-5 overflow-y-auto min-w-0">
          {renderSectionContent()}
        </div>

        {/* Right Info Sidebar */}
        <div className="w-72 flex-shrink-0 glass-panel aurora-glass p-4 overflow-y-auto space-y-4 text-xs">
          {/* Engine Details */}
          <div className="space-y-2">
            <div className="font-bold text-text-primary flex items-center justify-between border-b border-[var(--color-border)] pb-2">
              <span>Backend Specifications</span>
              <Info size={14} className="text-primary" />
            </div>
            <div className="space-y-1.5 text-text-secondary">
              <div className="flex justify-between py-1 border-b border-[var(--color-border)]">
                <span>Engine</span>
                <span className="font-semibold text-text-primary">Gabriel Rust Engine</span>
              </div>
              <div className="flex justify-between py-1 border-b border-[var(--color-border)]">
                <span>Version</span>
                <span className="font-mono font-bold text-text-primary">1.2.3-native</span>
              </div>
              <div className="flex justify-between py-1 border-b border-[var(--color-border)]">
                <span>Architecture</span>
                <span className="font-semibold text-primary">Zero-Crash Local</span>
              </div>
              <div className="flex justify-between py-1">
                <span>Telemetry</span>
                <span className="text-text-primary font-semibold">Local Only</span>
              </div>
            </div>
            <button 
              onClick={() => {
                verifyEngineBinary()
                  .then((res) => {
                    const modified = res.modified_unix ? new Date(res.modified_unix * 1000).toLocaleString() : 'N/A';
                    const sizeMb = (res.size_bytes / (1024 * 1024)).toFixed(2);
                    window.alert(
                      `Engine Binary Integrity Report:\n\n` +
                      `• Status: ${res.status.toUpperCase()}\n` +
                      `• Path: ${res.path}\n` +
                      `• SHA-256: ${res.sha256}\n` +
                      `• Size: ${sizeMb} MB (${res.size_bytes.toLocaleString()} bytes)\n` +
                      `• Last Modified: ${modified}`
                    );
                  })
                  .catch((err) => {
                    window.alert(`Verification failed: ${err}`);
                  });
              }}
              disabled={settingsLoading === 'verify_engine_binary'}
              className="btn-secondary w-full py-1.5 mt-2 flex items-center justify-center gap-1.5 text-xs font-semibold cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
              title="Compute cryptographic SHA-256 hash and binary metadata"
            >
              {settingsLoading === 'verify_engine_binary' ? (
                <>
                  <Loader2 size={14} className="animate-spin" />
                  Verifying Binary...
                </>
              ) : (
                <>
                  <RotateCcw size={14} />
                  Verify Engine Binary
                </>
              )}
            </button>
          </div>

          {/* Maintenance Actions */}
          <div className="pt-3 border-t border-[var(--color-border)] space-y-2">
            <div className="font-bold text-text-primary">Maintenance & Reset</div>
            <div className="space-y-2">
              <button 
                onClick={() => {
                  resetSettings()
                    .then(() => Promise.all([refreshMode(), refreshProfile(), refreshGovernor()]))
                    .catch(console.error);
                }}
                disabled={settingsLoading === 'reset_settings'}
                className="w-full flex items-center justify-between p-2.5 rounded-xl hover:bg-[var(--color-hover)] border border-[var(--color-border)] transition-colors text-left group cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
              >
                <div className="flex items-center gap-2">
                  <div className="p-1.5 rounded-lg bg-[var(--color-hover)] text-text-secondary"><Trash2 size={14} /></div>
                  <div>
                    <div className="font-semibold text-[11px] text-text-primary">Reset Settings</div>
                    <div className="text-[9px] text-text-secondary">Restore factory defaults</div>
                  </div>
                </div>
                {settingsLoading === 'reset_settings' ? (
                  <Loader2 size={14} className="animate-spin" />
                ) : (
                  <ChevronRight size={14} className="text-text-secondary" />
                )}
              </button>

              <button 
                onClick={() => {
                  if (window.confirm('Clear all cached model files? This deletes the entire local models directory and cannot be undone.')) {
                    clearModelCaches().catch(console.error);
                  }
                }}
                disabled={settingsLoading === 'clear_model_caches'}
                className="w-full flex items-center justify-between p-2.5 rounded-xl hover:bg-[var(--color-hover)] border border-[var(--color-border)] transition-colors text-left group cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
              >
                <div className="flex items-center gap-2">
                  <div className="p-1.5 rounded-lg bg-[var(--color-hover)] text-text-secondary"><Database size={14} /></div>
                  <div>
                    <div className="font-semibold text-[11px] text-text-primary">Clear Model Caches</div>
                    <div className="text-[9px] text-text-secondary">Remove cached tensors</div>
                  </div>
                </div>
                {settingsLoading === 'clear_model_caches' ? (
                  <Loader2 size={14} className="animate-spin" />
                ) : (
                  <ChevronRight size={14} className="text-text-secondary" />
                )}
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};