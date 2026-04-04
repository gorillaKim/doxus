import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

export interface SearchHit {
  id: string;
  title: string | null;
  snippet: string;
  score: number;
  file_path: string | null;
  project_name: string | null;
}

interface SearchState {
  query: string;
  hits: SearchHit[];
  isLoading: boolean;
  error: string | null;
  setQuery: (q: string) => void;
  search: () => Promise<void>;
  clear: () => void;
}

export const useSearchStore = create<SearchState>((set, get) => ({
  query: '',
  hits: [],
  isLoading: false,
  error: null,

  setQuery: (q) => set({ query: q }),

  search: async () => {
    const { query } = get();
    if (!query.trim()) return;
    set({ isLoading: true, error: null });
    try {
      const result = await invoke<{ hits: SearchHit[] }>('search_documents', {
        query,
        limit: 20,
      });
      set({ hits: result.hits, isLoading: false });
    } catch (e) {
      set({ error: String(e), isLoading: false });
    }
  },

  clear: () => set({ query: '', hits: [], error: null }),
}));
