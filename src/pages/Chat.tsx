import type { FC } from 'react';
import { useState, useEffect, useCallback } from 'react';
import { useLocation } from 'react-router-dom';
import { invoke } from '@tauri-apps/api/core';
import {
  ChevronRight,
  Plus,
  History,
  Download,
  ArrowRight,
  Mic,
  Info,
  Sliders,
  Sparkles,
  Zap,
  Square,
  FileText,
  X,
  Loader2,
} from 'lucide-react';
import {
  StatusDot,
  AttachMenuPopover,
} from '../components/shared';
import { Select } from '../components/shared/Select';
import { useCardSpotlight } from '../hooks/useCardSpotlight';
import { useAttachment } from '../hooks/useAttachment';
import { useModels } from '../hooks/useModels';
import { useChat } from '../hooks/useChat';
import { useAudioTranscription } from '../hooks/useAudioTranscription';

interface Message {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  timestamp: string;
}

export const Chat: FC = () => {
  const location = useLocation();
  const initialMsgFromHome = location.state?.initialMessage;

  const { models } = useModels();
  const llmModels = models.filter(m => m.model_type === 'llm');
  
  const [selectedModel, setSelectedModel] = useState<string>('');
  const [messages, setMessages] = useState<Message[]>(() => {
    if (initialMsgFromHome) {
      return [
        {
          id: Date.now().toString(),
          role: 'user' as const,
          content: initialMsgFromHome,
          timestamp: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }),
        },
      ];
    }
    return [];
  });

  const [inputValue, setInputValue] = useState('');
  // Generation params actually sent to submit_chat (were hardcoded 2048/0.7).
  const [temperature, setTemperature] = useState('0.7');
  const [maxTokens, setMaxTokens] = useState('2048');
  // Measured stats from the last completed response (replaces "Live" placeholders).
  const [genStats, setGenStats] = useState<{ ttftMs: number | null; tokPerSec: number | null }>({ ttftMs: null, tokPerSec: null });
  const { attachedFile, setAttachedFile, chooseAttachment } = useAttachment();
  const [showInfoSidebar, setShowInfoSidebar] = useState(true);
  const [activeTab, setActiveTab] = useState<'info' | 'history'>('info');
  const [historyList, setHistoryList] = useState<{ id: string; title: string; timestamp: string; messageCount: number }[]>([]);
  // In-memory per-conversation message store (restored on history click).
  const [convMessages, setConvMessages] = useState<Record<string, Message[]>>({});
  const [activeConvId, setActiveConvId] = useState<string | null>(null);

  const { 
    isGenerating, 
    sendMessage, 
    stopGeneration,
    error: chatError 
  } = useChat();

  const { isRecording, isTranscribing, toggleRecording } = useAudioTranscription((transcribedText) => {
    setInputValue((prev) => (prev ? `${prev} ${transcribedText}` : transcribedText));
  });

  const { ref: infoCardRef, onPointerMove: onInfoCardPointerMove } = useCardSpotlight();

  // Initialize selected model from available models
  useEffect(() => {
    if (llmModels.length > 0 && !selectedModel) {
      const runningModel = llmModels.find(m => m.residency === 'gpu');
      setSelectedModel(runningModel?.id || llmModels[0].id);
    }
  }, [llmModels, selectedModel]);

  const handleNewChat = useCallback(() => {
    stopGeneration();
    // Persist current thread under the active id before clearing.
    if (activeConvId) {
      setConvMessages(prev => ({ ...prev, [activeConvId]: messages }));
      setHistoryList(prev => prev.map(c => c.id === activeConvId ? { ...c, messageCount: messages.length } : c));
    }

    const newId = Date.now().toString();
    const timeStr = new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    const newEntry = {
      id: newId,
      title: 'New Conversation',
      timestamp: `Today, ${timeStr}`,
      messageCount: 0,
    };

    setHistoryList(prev => [newEntry, ...prev]);
    setConvMessages(prev => ({ ...prev, [newId]: [] }));
    setActiveConvId(newId);
    setMessages([]);
    setInputValue('');
    setAttachedFile(null);
  }, [stopGeneration, activeConvId, messages, setAttachedFile]);

  // Keep the active conversation's stored thread in sync as messages stream in.
  useEffect(() => {
    if (activeConvId) {
      setConvMessages(prev => ({ ...prev, [activeConvId]: messages }));
    }
  }, [messages, activeConvId]);

  // Handle Ctrl+N trigger from global shortcut router
  useEffect(() => {
    if (location.state?.newChat) {
      handleNewChat();
    }
  }, [location.state, handleNewChat]);

  // Contextual shortcut: Ctrl + . to Stop Generation
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === '.') {
        e.preventDefault();
        stopGeneration();
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [stopGeneration]);

  const handleSubmit = useCallback(async (e?: React.FormEvent) => {
    if (e) e.preventDefault();
    if (inputValue.trim() && !isGenerating && selectedModel) {
      // Read real attachment content so the model receives file data,
      // not just a filename prefix.
      let userText = inputValue;
      const attachedPath = attachedFile;
      if (attachedPath) {
        try {
          const preview = await invoke<{
            kind: string;
            name: string;
            size_bytes: number;
            text_preview?: string | null;
          }>('read_attachment_preview', { path: attachedPath });
          if (preview.kind === 'text' && preview.text_preview) {
            userText = `[File ${preview.name} content:\n${preview.text_preview}]\n${inputValue}`;
          } else if (preview.kind === 'image') {
            userText = `[Image attached: ${preview.name} (${preview.size_bytes} bytes, vision input not yet supported — answering from text only)] ${inputValue}`;
          } else {
            userText = `[Attached: ${preview.name} (${preview.size_bytes} bytes)] ${inputValue}`;
          }
        } catch (err) {
          console.error('Attachment read failed, sending filename only', err);
          userText = `[Attached: ${attachedPath}] ${inputValue}`;
        }
      }
      const newMsg: Message = {
        id: Date.now().toString(),
        role: 'user' as const,
        content: userText,
        timestamp: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }),
      };
      setMessages(prev => [...prev, newMsg]);
      setInputValue('');
      setAttachedFile(null);

      // Create placeholder for assistant message
      const assistantMsgId = (Date.now() + 1).toString();
      const assistantMsg: Message = {
        id: assistantMsgId,
        role: 'assistant' as const,
        content: '',
        timestamp: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }),
      };
      setMessages(prev => [...prev, assistantMsg]);

      // Stream the response (params come from the editable Session Info controls)
      const t0 = performance.now();
      let firstTokenAt = 0;
      let tokenCount = 0;
      const parsedTemp = Math.min(2, Math.max(0, Number(temperature) || 0.7));
      const parsedMaxTokens = Math.min(8192, Math.max(1, Math.floor(Number(maxTokens) || 2048)));
      sendMessage(
        selectedModel,
        userText,
        { max_tokens: parsedMaxTokens, temperature: parsedTemp },
        (token) => {
          if (!firstTokenAt) firstTokenAt = performance.now();
          tokenCount += 1;
          setMessages(prev => prev.map(msg => 
            msg.id === assistantMsgId ? { ...msg, content: msg.content + token } : msg
          ));
        },
        (finishReason) => {
          const doneAt = performance.now();
          setGenStats({
            ttftMs: firstTokenAt ? firstTokenAt - t0 : null,
            tokPerSec: doneAt > t0 ? tokenCount / ((doneAt - t0) / 1000) : null,
          });
          console.log('Chat finished:', finishReason);
        },
        (error) => {
          setMessages(prev => prev.map(msg => 
            msg.id === assistantMsgId ? { ...msg, content: `Error: ${error}` } : msg
          ));
        }
      );
    }
  }, [inputValue, isGenerating, selectedModel, attachedFile, sendMessage, temperature, maxTokens]);

  return (
    <div className="flex flex-col h-full max-w-6xl mx-auto space-y-4 text-text-primary">
      {/* Top Header Bar */}
      <div className="flex items-center justify-between gap-3 glass-panel aurora-glass p-3">
        {/* Left: Model Selector */}
        <div className="flex items-center gap-3">
          <Select
            options={llmModels.map(m => ({ value: m.id, label: m.id }))}
            value={selectedModel}
            onChange={setSelectedModel}
            className="w-64"
            icon={<StatusDot status="running" size={6} />}
          />
        </div>

        {/* Right: Actions & New Chat */}
        <div className="flex items-center gap-2">
          <button
            onClick={handleNewChat}
            className="btn-primary py-2 px-4 flex flex-row items-center gap-2 text-xs font-bold cursor-pointer active:scale-95 transition-all shadow-sm"
            title="Start new conversation"
          >
            <Plus size={15} strokeWidth={2.5} />
            <span className="whitespace-nowrap">New Chat</span>
          </button>

          <div className="w-px h-6 bg-[var(--color-border)] mx-1" />

          <button
            onClick={() => {
              setShowInfoSidebar(true);
              setActiveTab('history');
            }}
            className="p-2.5 rounded-xl text-text-secondary hover:bg-[var(--color-hover)] hover:text-text-primary active:scale-90 transition-all cursor-pointer"
            title="History"
            aria-label="History"
          >
            <History size={18} strokeWidth={2} />
          </button>

          <button
            onClick={() => {
              const md = messages
                .map((m) => `## ${m.role === 'user' ? 'User' : 'Gabriel'} (${m.timestamp})\n\n${m.content}\n`)
                .join('\n---\n\n');
              const blob = new Blob([`# Gabriel Chat Export\n\n${md}`], { type: 'text/markdown' });
              const url = URL.createObjectURL(blob);
              const a = document.createElement('a');
              a.href = url;
              a.download = `gabriel-chat-${Date.now()}.md`;
              document.body.appendChild(a);
              a.click();
              a.remove();
              setTimeout(() => URL.revokeObjectURL(url), 1000);
            }}
            className="p-2.5 rounded-xl text-text-secondary hover:bg-[var(--color-hover)] hover:text-text-primary active:scale-90 transition-all cursor-pointer"
            title="Export conversation as Markdown"
            aria-label="Export"
          >
            <Download size={18} strokeWidth={2} />
          </button>

          <button
            onClick={() => setShowInfoSidebar(!showInfoSidebar)}
            className={`p-2.5 rounded-xl active:scale-90 transition-all cursor-pointer ${
              showInfoSidebar ? 'bg-[var(--color-primary-bg)] text-primary' : 'text-text-secondary hover:bg-[var(--color-hover)] hover:text-text-primary'
            }`}
            title="Toggle Session Info"
            aria-label="Toggle info panel"
          >
            <Info size={18} strokeWidth={2} />
          </button>
        </div>
      </div>

      {/* Main Message Thread + Session Info Sidebar */}
      <div className="flex-1 flex gap-4 min-h-0 overflow-hidden">
        {/* Message Thread Box */}
        <div className="flex-1 flex flex-col min-w-0 glass-panel aurora-glass overflow-hidden">
          <div className="flex-1 overflow-y-auto p-4 space-y-4">
            {messages.length === 0 ? (
              <div className="h-full flex flex-col items-center justify-center text-center p-8 text-text-secondary space-y-3">
                <div className="w-12 h-12 rounded-full bg-[var(--color-primary-bg)] text-primary flex items-center justify-center">
                  <Sparkles size={24} />
                </div>
                <h3 className="font-bold text-base text-text-primary">Fresh Conversation Started</h3>
                <p className="text-xs max-w-sm">Ask Gabriel anything about local model execution, code, workflows, or hardware optimization.</p>
              </div>
            ) : (
              messages.map((msg) => (
                <MessageBubble key={msg.id} message={msg} />
              ))
            )}
          </div>

          {/* Floating Glass Input */}
          <form onSubmit={handleSubmit} className="p-3 bg-transparent">
            {chatError && (
              <div className="mb-2 rounded-xl border border-red-400/30 bg-red-400/10 px-3 py-2 text-[11px] text-red-200">
                {chatError}
              </div>
            )}
            <div className="flex items-center gap-2 rounded-full glass-floating dynamic-glass-pill pl-3 pr-3.5 py-2.5 focus-within:ring-2 focus-within:ring-primary/20 transition-all">
              <AttachMenuPopover iconSize={16} onSelect={chooseAttachment} />

              {attachedFile && (
                <span className="flex items-center gap-1 rounded-full bg-[var(--color-primary-bg)] px-2 py-1 text-[10px] font-mono text-primary max-w-48 shrink-0">
                  <FileText size={12} />
                  <span className="truncate">{attachedFile.split(/[\\/]/).pop()}</span>
                  <button
                    type="button"
                    onClick={() => setAttachedFile(null)}
                    className="ml-0.5 hover:text-text-primary"
                    aria-label="Remove attachment"
                  >
                    <X size={11} />
                  </button>
                </span>
              )}

              <input
                type="text"
                placeholder="Ask Gabriel anything locally... (Ctrl+Enter to send)"
                className="flex-1 bg-transparent border-none focus:outline-none text-xs text-text-primary placeholder:text-text-secondary font-medium"
                value={inputValue}
                onChange={(e) => setInputValue(e.target.value)}
                onKeyDown={(e) => {
                  if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
                    e.preventDefault();
                    handleSubmit();
                  }
                }}
              />

              <button
                type="button"
                onClick={toggleRecording}
                title={isRecording ? 'Stop recording voice prompt' : isTranscribing ? 'Transcribing...' : 'Record voice prompt'}
                aria-label="Voice input"
                className={`p-1.5 rounded-full transition-all shrink-0 cursor-pointer ${
                  isRecording
                    ? 'bg-rose-500/20 text-rose-500 animate-pulse ring-1 ring-rose-500'
                    : isTranscribing
                    ? 'text-primary animate-spin'
                    : 'text-text-secondary hover:text-text-primary hover:bg-[var(--color-hover)]'
                }`}
              >
                {isTranscribing ? <Loader2 size={16} className="animate-spin" /> : <Mic size={16} strokeWidth={1.8} />}
              </button>

              {isGenerating ? (
                <button
                  type="button"
                  onClick={stopGeneration}
                  className="p-1.5 text-white bg-primary rounded-full hover:scale-105 active:scale-95 transition-all cursor-pointer shrink-0"
                  aria-label="Stop generation (Ctrl+.)"
                  title="Stop generation (Ctrl+.)"
                >
                  <Square size={13} fill="currentColor" />
                </button>
              ) : (
                <button
                  type="submit"
                  disabled={!inputValue.trim() && !attachedFile}
                  className="p-1.5 text-white bg-primary rounded-full disabled:opacity-40 disabled:hover:scale-100 hover:scale-105 active:scale-95 transition-all cursor-pointer shrink-0"
                  aria-label="Send (Ctrl+Enter)"
                  title="Send (Ctrl+Enter)"
                >
                  <ArrowRight size={14} strokeWidth={2} />
                </button>
              )}
            </div>
          </form>
        </div>

        {/* Supplementary Info Panel */}
        {showInfoSidebar && (
          <div
            ref={infoCardRef}
            onPointerMove={onInfoCardPointerMove}
            className="w-72 flex-shrink-0 glass-panel aurora-glass p-4 overflow-y-auto space-y-4 flex flex-col"
          >
            {/* Tabs */}
            <div className="flex items-center p-1 bg-[var(--color-hover)] rounded-xl text-xs font-semibold relative z-10">
              <button
                onClick={() => setActiveTab('info')}
                className={`flex-1 py-1.5 rounded-lg transition-all active:scale-95 cursor-pointer ${
                  activeTab === 'info' ? 'bg-[var(--color-card)] text-primary font-bold shadow-2xs' : 'text-text-secondary hover:text-text-primary'
                }`}
              >
                Session Info
              </button>
              <button
                onClick={() => setActiveTab('history')}
                className={`flex-1 py-1.5 rounded-lg transition-all active:scale-95 cursor-pointer ${
                  activeTab === 'history' ? 'bg-[var(--color-card)] text-primary font-bold shadow-2xs' : 'text-text-secondary hover:text-text-primary'
                }`}
              >
                History ({historyList.length})
              </button>
            </div>

            {activeTab === 'info' ? (
              <div className="space-y-4 text-xs relative z-10">
                <div className="space-y-3">
                  <div className="font-bold text-text-primary flex items-center gap-1.5">
                    <Sliders size={14} className="text-primary" />
                    Model Details
                  </div>
                  <div className="p-3 bg-[var(--color-hover)] rounded-xl space-y-2">
                    {(() => {
                      const model = models.find(m => m.id === selectedModel);
                      if (!model) return (
                        <div className="text-text-secondary text-center py-4">Select a model to see details</div>
                      );
                      return (
                        <>
                          <div className="flex items-center justify-between">
                            <span className="text-text-secondary font-medium">Model</span>
                            <span className="font-bold text-text-primary truncate max-w-[120px]">{model.id}</span>
                          </div>
                          <div className="flex items-center justify-between">
                            <span className="text-text-secondary font-medium">Type</span>
                            <span className="badge badge-primary font-mono font-bold">{model.model_type.toUpperCase()}</span>
                          </div>
                          <div className="flex items-center justify-between">
                            <span className="text-text-secondary font-medium">Status</span>
                            <span className={`badge ${model.residency === 'gpu' ? 'badge-secondary' : 'badge-tertiary'} font-mono font-bold`}>
                              {model.residency === 'gpu' ? 'Running (VRAM)' : model.available ? 'Offloaded (RAM)' : 'Available'}
                            </span>
                          </div>
                          <div className="flex items-center justify-between">
                            <span className="text-text-secondary font-medium">VRAM</span>
                            <span className="font-mono text-text-primary font-bold">
                              {(model.vram_bytes / 1024 ** 3).toFixed(1)} GB
                            </span>
                          </div>
                          <div className="flex items-center justify-between">
                            <span className="text-text-secondary font-medium">Disk</span>
                            <span className="font-mono text-text-primary font-bold">
                              {(model.disk_bytes / 1024 ** 3).toFixed(1)} GB
                            </span>
                          </div>
                          <div className="flex items-center justify-between">
                            <span className="text-text-secondary font-medium">Idle</span>
                            <span className="font-mono text-text-secondary font-bold">
                              {model.idle_secs > 3600 
                                ? `${Math.floor(model.idle_secs / 3600)}h` 
                                : model.idle_secs > 60 
                                  ? `${Math.floor(model.idle_secs / 60)}m` 
                                  : `${model.idle_secs}s`}
                            </span>
                          </div>
                        </>
                      );
                    })()}
                  </div>

                  <div className="space-y-1.5 pt-2 border-t border-[var(--color-border)] font-medium">
                    <div className="flex justify-between items-center text-text-secondary">
                      <span>Temperature</span>
                      <input
                        type="number"
                        min={0}
                        max={2}
                        step={0.1}
                        value={temperature}
                        onChange={(e) => setTemperature(e.target.value)}
                        className="w-20 glass-pill px-2 py-1 text-xs font-mono text-text-primary text-right focus:outline-none focus:ring-2 focus:ring-primary/20"
                        aria-label="Temperature"
                        title="Sampling temperature actually sent with each request (0–2)"
                      />
                    </div>
                    <div className="flex justify-between items-center text-text-secondary">
                      <span>Max Tokens</span>
                      <input
                        type="number"
                        min={1}
                        max={8192}
                        step={1}
                        value={maxTokens}
                        onChange={(e) => setMaxTokens(e.target.value)}
                        className="w-20 glass-pill px-2 py-1 text-xs font-mono text-text-primary text-right focus:outline-none focus:ring-2 focus:ring-primary/20"
                        aria-label="Max tokens"
                        title="Max tokens actually sent with each request (1–8192)"
                      />
                    </div>
                  </div>
                </div>

                {/* Session Real-time Speed (measured from the last response) */}
                <div className="pt-3 border-t border-[var(--color-border)] space-y-2">
                  <div className="font-bold text-text-primary flex items-center gap-1.5">
                    <Zap size={14} className="text-secondary" />
                    Inference Telemetry
                  </div>
                  <div className="grid grid-cols-2 gap-2 text-center">
                    <div className="p-2 rounded-xl bg-[var(--color-hover)]">
                      <span className="text-[10px] text-text-secondary font-semibold uppercase block">Throughput</span>
                      <span className="font-mono font-bold text-sm text-secondary">
                        {genStats.tokPerSec != null ? `${genStats.tokPerSec.toFixed(1)} tok/s` : '—'}
                      </span>
                    </div>
                    <div className="p-2 rounded-xl bg-[var(--color-hover)]">
                      <span className="text-[10px] text-text-secondary font-semibold uppercase block">First token</span>
                      <span className="font-mono font-bold text-sm text-primary">
                        {genStats.ttftMs != null ? `${(genStats.ttftMs / 1000).toFixed(2)}s` : '—'}
                      </span>
                    </div>
                  </div>
                </div>
              </div>
            ) : (
              <div className="space-y-2 text-xs relative z-10">
                <div className="font-bold text-text-primary mb-2">Chat History</div>
                {historyList.length === 0 && (
                  <p className="text-[11px] text-text-secondary italic">No conversations yet — start chatting.</p>
                )}
                {historyList.map(conv => (
                  <button
                    key={conv.id}
                    onClick={() => {
                      stopGeneration();
                      if (activeConvId) {
                        setConvMessages(prev => ({ ...prev, [activeConvId]: messages }));
                      }
                      setActiveConvId(conv.id);
                      setMessages(convMessages[conv.id] ?? []);
                    }}
                    className="w-full flex items-center justify-between p-2.5 rounded-xl bg-[var(--color-hover)] hover:bg-[var(--color-active)] transition-all text-left group cursor-pointer"
                  >
                    <div className="flex-1 min-w-0 pr-2">
                      <div className="font-bold text-text-primary truncate">{conv.title}</div>
                      <div className="text-[10px] text-text-secondary font-mono">{conv.timestamp} · {conv.messageCount} msgs</div>
                    </div>
                    <ChevronRight size={14} className="text-text-secondary group-hover:text-text-primary shrink-0 transition-transform group-hover:translate-x-0.5" />
                  </button>
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
};

const MessageBubble: FC<{ message: Message }> = ({ message }) => {
  const isUser = message.role === 'user';

  return (
    <div className={`flex gap-3 ${isUser ? 'flex-row-reverse' : ''}`}>
      {!isUser && (
        <div className="w-7 h-7 rounded-full bg-[var(--color-primary-bg)] text-primary flex items-center justify-center shrink-0 text-xs font-bold">
          G
        </div>
      )}
      <div className={`flex-1 ${isUser ? 'text-right' : ''}`}>
        <div
          className={`inline-block max-w-[82%] rounded-2xl p-3.5 text-xs ${
            isUser
              ? 'bg-primary text-white font-medium rounded-br-xs'
              : 'glass-panel text-text-primary rounded-bl-xs'
          }`}
        >
          <div className="whitespace-pre-wrap leading-relaxed">
            {message.content}
          </div>
        </div>
        <div className={`flex items-center gap-1.5 mt-1 text-[10px] text-text-secondary ${isUser ? 'justify-end' : ''}`}>
          <span className="font-mono">{message.timestamp}</span>
        </div>
      </div>
    </div>
  );
};