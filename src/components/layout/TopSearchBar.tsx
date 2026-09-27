import type { FC } from 'react';
import { useState, useRef, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { Search, Bell } from 'lucide-react';
import { useNotifications } from '../../hooks/useNotifications';
import { usePopover, FloatingPanel } from '../shared/FloatingPanel';

export const TopSearchBar: FC<{ showNotifications?: boolean }> = ({ showNotifications = true }) => {
  const navigate = useNavigate();
  const [query, setQuery] = useState('');
  const [isMac, setIsMac] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const notifications = usePopover({ align: 'right', minWidth: 320, maxHeight: 420, offset: 8 });
  const { notifications: items, unreadCount, markRead } = useNotifications();

  useEffect(() => {
    if (typeof navigator !== 'undefined') {
      const platform = navigator.platform || navigator.userAgent || '';
      setIsMac(/Mac|iPod|iPhone|iPad/.test(platform));
    }

    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        inputRef.current?.focus();
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, []);

  return (
    <div className="flex items-center gap-3 mb-6 relative z-30">
      <div className="flex-1 relative">
        <Search className="absolute left-4 top-1/2 -translate-y-1/2 text-text-secondary" size={17} strokeWidth={2} />
        <input
          ref={inputRef}
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && query.trim()) {
              navigate('/models', { state: { query: query.trim() } });
            }
          }}
          placeholder="Search models… (Enter)"
          className="home-search-input w-full glass-pill aurora-glass py-2.5 pl-11 pr-20 text-xs font-medium text-text-primary placeholder:text-text-secondary focus:outline-none focus:ring-2 focus:ring-[var(--color-accent)]/20 focus:border-[var(--color-accent)] transition-all"
        />
        <kbd className="absolute right-4 top-1/2 -translate-y-1/2 hidden sm:inline-flex items-center gap-0.5 px-2 py-0.5 rounded-md text-[10px] font-mono text-text-secondary bg-[var(--color-hover)] border border-[var(--color-border)] font-bold">
          {isMac ? '⌘K' : 'Ctrl+K'}
        </kbd>
      </div>

      {showNotifications && (
        <div className="relative">
          <button
            type="button"
            {...notifications.triggerProps}
            className="relative p-2.5 rounded-full glass-pill aurora-glass text-text-secondary hover:text-text-primary hover:bg-[var(--color-hover)] transition-colors cursor-pointer flex items-center justify-center"
            aria-label="Notifications"
            title="Notifications"
          >
            <Bell size={17} strokeWidth={2} />
            {unreadCount > 0 && <span className="absolute top-1.5 right-1.5 w-2 h-2 rounded-full bg-[var(--color-accent)] ring-2 ring-[var(--color-card)]" />}
          </button>
          <FloatingPanel api={notifications} role="dialog" className="w-80 glass-floating aurora-glass rounded-2xl p-4 space-y-3 shadow-2xl">
            <h3 className="font-bold text-xs text-text-primary uppercase tracking-wider">Notifications</h3>
            {items.length === 0 ? <p className="text-xs text-text-secondary">No notifications yet.</p> : items.map(item => (
              <button key={item.id} type="button" onClick={() => markRead(item.id)} className={`block w-full rounded-xl p-2.5 text-left ${item.read ? 'bg-[var(--color-hover)]/40' : 'bg-[var(--color-hover)]'}`}>
                <div className="font-semibold text-xs text-text-primary">{item.title}</div>
                <p className="text-[11px] text-text-secondary mt-0.5">{item.message}</p>
              </button>
            ))}
          </FloatingPanel>
        </div>
      )}
    </div>
  );
};