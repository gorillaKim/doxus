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
  fetch: () => Promise<void>;
  addProject: (name: string, path: string) => Promise<void>;
  toggleStatus: (name: string, currentStatus: 'active' | 'disabled') => Promise<void>;
}

export const useProjectStore = create<ProjectState>((set, get) => ({
  projects: [],
  isLoading: false,
  error: null,

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
}));
