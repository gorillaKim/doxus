import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

export interface SearchHit {
  document_id: number;
  chunk_id: number;
  title: string | null;
  snippet: string | null;
  score: number;
  source_doc_id: string | null;
  file_path: string | null;
  heading_path: string | null;
  project_name: string;
  source_type: string;
}

export interface AllDocument {
  document_id: number;
  title: string;
  source_doc_id: string;
  project_name: string;
  source_type: string;
  file_path: string | null;
}

export interface SearchFilters {
  sourceTypes: string[];   // e.g. ['obsidian', 'confluence']
  projectNames: string[];  // e.g. ['my-vault']
  tagQuery: string;        // '#태그' 텍스트 포함 검색
}

interface SearchState {
  query: string;
  filters: SearchFilters;
  hits: SearchHit[];
  isLoading: boolean;
  error: string | null;
  queryHistory: string[];
  allDocuments: AllDocument[];
  allDocsLoading: boolean;
  setQuery: (q: string) => void;
  setFilters: (f: Partial<SearchFilters>) => void;
  search: () => Promise<void>;
  clear: () => void;
  listAllDocuments: () => Promise<void>;
}

const DEFAULT_FILTERS: SearchFilters = { sourceTypes: [], projectNames: [], tagQuery: '' };

export const useSearchStore = create<SearchState>((set, get) => ({
  query: '',
  filters: DEFAULT_FILTERS,
  hits: [],
  isLoading: false,
  error: null,
  queryHistory: [],
  allDocuments: [],
  allDocsLoading: false,

  setQuery: (q) => set({ query: q }),
  setFilters: (f) => set((s) => ({ filters: { ...s.filters, ...f } })),

  search: async () => {
    const { query, filters, queryHistory } = get();
    if (!query.trim()) return;
    const trimmed = query.trim();
    const updated = [trimmed, ...queryHistory.filter((q) => q !== trimmed)].slice(0, 5);
    set({ isLoading: true, error: null, queryHistory: updated });
    // tagQuery는 검색 텍스트에 포함
    const effectiveQuery = filters.tagQuery.trim()
      ? `${trimmed} ${filters.tagQuery.trim()}`
      : trimmed;
    try {
      const result = await invoke<{ hits: SearchHit[] }>('search_documents', {
        query: effectiveQuery,
        limit: 20,
        sourceTypes: filters.sourceTypes.length > 0 ? filters.sourceTypes : null,
        projectNames: filters.projectNames.length > 0 ? filters.projectNames : null,
      });
      set({ hits: result.hits, isLoading: false });
    } catch (e) {
      set({ error: String(e), isLoading: false });
    }
  },

  clear: () => set({ query: '', filters: DEFAULT_FILTERS, hits: [], error: null }),

  listAllDocuments: async () => {
    set({ allDocsLoading: true });
    try {
      const result = await invoke<{ documents: AllDocument[] }>('list_all_documents');
      set({ allDocuments: result.documents, allDocsLoading: false });
    } catch (e) {
      console.error('[listAllDocuments]', e);
      set({ allDocsLoading: false });
    }
  },
}));
