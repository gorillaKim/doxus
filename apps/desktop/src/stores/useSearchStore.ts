import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

export interface SearchHit {
  document_id: number;
  chunk_id: number;
  title: string | null;
  snippet: string | null;
  score: number;
  file_path: string | null;
  heading_path: string | null;
}

interface SearchState {
  query: string;
  hits: SearchHit[];
  isLoading: boolean;
  error: string | null;
  queryHistory: string[];
  setQuery: (q: string) => void;
  search: () => Promise<void>;
  clear: () => void;
}

export const useSearchStore = create<SearchState>((set, get) => ({
  query: '',
  hits: [],
  isLoading: false,
  error: null,
  queryHistory: [],

  setQuery: (q) => set({ query: q }),

  search: async () => {
    const { query, queryHistory } = get();
    if (!query.trim()) return;
    const trimmed = query.trim();
    const updated = [trimmed, ...queryHistory.filter((q) => q !== trimmed)].slice(0, 5);
    set({ isLoading: true, error: null, queryHistory: updated });
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
