import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

interface PluginAuthState {
  configured: boolean;
  loading: boolean;
}

interface PluginStore {
  authStates: Record<string, PluginAuthState>;
  fetchAuthStatus: (pluginId: string) => Promise<void>;
  setConfigured: (pluginId: string, configured: boolean) => void;
}

export const usePluginStore = create<PluginStore>((set) => ({
  authStates: {},
  fetchAuthStatus: async (pluginId: string) => {
    set((s) => ({
      authStates: { ...s.authStates, [pluginId]: { configured: false, loading: true } },
    }));
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
