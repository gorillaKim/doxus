import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

export interface Project {
  id: number;
  name: string;
  display_name: string;
  path: string;
  status: 'active' | 'disabled';
  source_type: string;
  freshness_policy_json?: string | null;
}

interface ProjectState {
  projects: Project[];
  isLoading: boolean;
  error: string | null;
  indexingNames: Set<string>;
  fetch: () => Promise<void>;
  addProject: (name: string, path: string, sourceType?: string, config?: Record<string, string>) => Promise<void>;
  removeProject: (name: string) => Promise<void>;
  toggleStatus: (name: string, currentStatus: 'active' | 'disabled') => Promise<void>;
  indexProject: (name: string, full?: boolean) => Promise<{ indexed: number; message: string }>;
  updateSensitivityMode: (projectId: number, mode: string) => Promise<void>;
}

export const useProjectStore = create<ProjectState>((set, get) => ({
  projects: [],
  isLoading: false,
  error: null,
  indexingNames: new Set(),

  fetch: async () => {
    set({ isLoading: true, error: null });
    try {
      const result = await invoke<{ projects: Project[] }>('list_projects');
      set({ projects: result.projects, isLoading: false });
    } catch (e) {
      set({ error: String(e), isLoading: false });
    }
  },

  addProject: async (name: string, path: string, sourceType?: string, config?: Record<string, string>) => {
    await invoke('add_project', { name, path, sourceType, config });
    await get().fetch();
  },

  removeProject: async (name: string) => {
    await invoke('remove_project', { name });
    await get().fetch();
  },

  toggleStatus: async (name: string, currentStatus: 'active' | 'disabled') => {
    const newStatus = currentStatus === 'active' ? 'disabled' : 'active';
    await invoke('toggle_project_status', { name, status: newStatus });
    await get().fetch();
  },

  indexProject: async (name: string, full: boolean = false) => {
    set((s) => ({ indexingNames: new Set(s.indexingNames).add(name) }));
    try {
      const result = await invoke<{ indexed: number; message: string }>('index_project', { name, full });
      return result;
    } finally {
      set((s) => {
        const next = new Set(s.indexingNames);
        next.delete(name);
        return { indexingNames: next };
      });
    }
  },

  updateSensitivityMode: async (projectId: number, mode: string) => {
    // 1. Optimistic Update
    set(s => ({
      projects: s.projects.map(p => {
        if (p.id === projectId) {
          const policy = p.freshness_policy_json ? JSON.parse(p.freshness_policy_json) : {};
          policy.sensitivity_mode = mode;
          return { ...p, freshness_policy_json: JSON.stringify(policy) };
        }
        return p;
      })
    }));

    try {
      await invoke('update_sensitivity_mode', { projectId, mode });
      // Final fetch to synchronize any other backend changes (like recalculated scores)
      await get().fetch();
    } catch (e) {
      // Revert on error by fetching original data
      await get().fetch();
      throw e;
    }
  },
}));
