import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

export type Notification = {
  id: number;
  title: string;
  message: string;
  level: string;
  read: boolean;
  created_at_unix: number;
};

export function useNotifications() {
  const [notifications, setNotifications] = useState<Notification[]>([]);
  const refresh = useCallback(async () => {
    setNotifications(await invoke<Notification[]>('get_notifications'));
  }, []);
  useEffect(() => {
    refresh().catch(console.error);
    const timer = window.setInterval(() => refresh().catch(console.error), 2000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const markRead = useCallback(async (id: number) => {
    await invoke('mark_notification_read', { id });
    await refresh();
  }, [refresh]);

  return { notifications, unreadCount: notifications.filter(item => !item.read).length, markRead, refresh };
}