import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

export interface Project {
  name: string;
  display_name: string;
  path: string;
  status: 'active' | 'disabled';
}

interface ProjectState {
  projects: Project[];
  isLoading: boolean;
  error: string | null;
  indexingNames: Set<string>;
  fetch: () => Promise<void>;
  addProject: (name: string, path: string) => Promise<void>;
  toggleStatus: (name: string, currentStatus: 'active' | 'disabled') => Promise<void>;
  indexProject: (name: string) => Promise<{ indexed: number; message: string }>;
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

  addProject: async (name: string, path: string) => {
    await invoke('add_project', { name, path });
    await get().fetch();
  },

  toggleStatus: async (name: string, currentStatus: 'active' | 'disabled') => {
    const newStatus = currentStatus === 'active' ? 'disabled' : 'active';
    await invoke('toggle_project_status', { name, status: newStatus });
    await get().fetch();
  },

  indexProject: async (name: string) => {
    set((s) => ({ indexingNames: new Set(s.indexingNames).add(name) }));
    try {
      const result = await invoke<{ indexed: number; message: string }>('index_project', { name });
      return result;
    } finally {
      set((s) => {
        const next = new Set(s.indexingNames);
        next.delete(name);
        return { indexingNames: next };
      });
    }
  },
}));
