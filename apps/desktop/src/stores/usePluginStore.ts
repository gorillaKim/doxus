import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

export const PLUGIN_DEFAULT_EMOJI: Record<string, string> = {
  'com.doxus.obsidian': '🪨',
  'com.doxus.confluence': '🌊',
  'com.doxus.github': '🐙',
};
const EMOJI_LS_PREFIX = 'plugin-emoji-';

function loadEmojiMap(): Record<string, string> {
  const map: Record<string, string> = {};
  for (let i = 0; i < localStorage.length; i++) {
    const key = localStorage.key(i);
    if (key?.startsWith(EMOJI_LS_PREFIX)) {
      map[key.slice(EMOJI_LS_PREFIX.length)] = localStorage.getItem(key) ?? '';
    }
  }
  return map;
}

interface PluginAuthState {
  configured: boolean;
  loading: boolean;
}

interface PluginStore {
  authStates: Record<string, PluginAuthState>;
  fetchAuthStatus: (pluginId: string) => Promise<void>;
  setConfigured: (pluginId: string, configured: boolean) => void;
  emojiMap: Record<string, string>;
  getEmoji: (pluginId: string) => string;
  setEmoji: (pluginId: string, emoji: string) => void;
}

export const usePluginStore = create<PluginStore>((set, get) => ({
  authStates: {},
  emojiMap: loadEmojiMap(),
  getEmoji: (pluginId) => {
    const map = get().emojiMap;
    return map[pluginId] ?? PLUGIN_DEFAULT_EMOJI[pluginId] ?? '🔌';
  },
  setEmoji: (pluginId, emoji) => {
    if (emoji) {
      localStorage.setItem(`${EMOJI_LS_PREFIX}${pluginId}`, emoji);
    } else {
      localStorage.removeItem(`${EMOJI_LS_PREFIX}${pluginId}`);
    }
    set((s) => ({ emojiMap: { ...s.emojiMap, [pluginId]: emoji } }));
  },
  fetchAuthStatus: async (pluginId: string) => {
    // 이미 캐시된 결과가 있으면 재조회하지 않음 (키체인 반복 프롬프트 방지)
    let skip = false;
    set((s) => {
      const existing = s.authStates[pluginId];
      if (existing && !existing.loading) { skip = true; return s; }
      return { authStates: { ...s.authStates, [pluginId]: { configured: false, loading: true } } };
    });
    if (skip) return;
    try {
      const res = await invoke<{ configured: boolean }>('plugin_get_auth_status', { pluginId });
      set((s) => ({
        authStates: { ...s.authStates, [pluginId]: { configured: res.configured, loading: false } },
      }));
    } catch {
      set((s) => ({
        authStates: { ...s.authStates, [pluginId]: { configured: false, loading: false } },
      }));
    }
  },
  setConfigured: (pluginId, configured) => {
    set((s) => ({
      authStates: {
        ...s.authStates,
        [pluginId]: { ...s.authStates[pluginId], configured, loading: false },
      },
    }));
  },
}));
