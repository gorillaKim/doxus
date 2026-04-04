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
}

export const useProjectStore = create<ProjectState>((set) => ({
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
}));
