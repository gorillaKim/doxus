import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

export interface SearchHit {
  document_id: number;
  chunk_id: number;
  title: string | null;
  snippet: string | null;
  context_content: string | null;
  score: number;
  source_doc_id: string | null;
  file_path: string | null;
  heading_path: string | null;
  project_name: string;
  source_type: string;
  tags: string[];
  updated_at?: number;
  last_indexed?: number;
  cache_ttl?: number;
  metadata: Record<string, any>;
  url: string | null;
  source_project_id: string;
  freshness_score: number;
  retention_tier: string;
}

export interface AllDocument {
  document_id: number;
  title: string;
  source_doc_id: string;
  project_name: string;
  source_type: string;
  file_path: string | null;
  url: string | null;
  tags?: string[];
  updated_at?: number;
  last_indexed?: number;
  cache_ttl?: number;
  source_project_id?: string;
  freshness_score?: number;
  retention_tier?: string;
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
  
  // Optimized storage
  documentsById: Record<string, AllDocument>;
  allDocsLoading: boolean;

  // Actions
  setQuery: (q: string) => void;
  setFilters: (f: Partial<SearchFilters>) => void;
  search: () => Promise<void>;
  clear: () => void;
  listAllDocuments: () => Promise<void>;
  updateDocumentMetadata: (docId: string, meta: Partial<AllDocument>) => void;

  // Derived (for backward compatibility)
  getAllDocuments: () => AllDocument[];
}

const DEFAULT_FILTERS: SearchFilters = { sourceTypes: [], projectNames: [], tagQuery: '' };

export const useSearchStore = create<SearchState>((set, get) => ({
  query: '',
  filters: DEFAULT_FILTERS,
  hits: [],
  isLoading: false,
  error: null,
  queryHistory: [],
  documentsById: {},
  allDocsLoading: false,

  setQuery: (q) => set({ query: q }),
  setFilters: (f) => set((s) => ({ filters: { ...s.filters, ...f } })),

  search: async () => {
    const { query, filters, queryHistory } = get();
    const trimmed = query.trim();
    const tagTrimmed = filters.tagQuery.trim();
    
    if (!trimmed && !tagTrimmed) return;

    if (trimmed) {
      const updated = [trimmed, ...queryHistory.filter((q) => q !== trimmed)].slice(0, 5);
      set({ queryHistory: updated });
    }
    
    set({ isLoading: true, error: null });

    const tags = tagTrimmed
      ? tagTrimmed.split(/[,;\s]+/).map(t => t.trim().replace(/^#/, '')).filter(t => t.length > 0)
      : [];

    try {
      const result = await invoke<{ hits: SearchHit[] }>('search_documents', {
        query: trimmed,
        limit: 50,
        source_types: filters.sourceTypes.length > 0 ? filters.sourceTypes : null,
        project_names: filters.projectNames.length > 0 ? filters.projectNames : null,
        tags: tags.length > 0 ? tags : null,
        mode: trimmed ? undefined : 'fts',
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
      // Limit initial load to 1,000 documents to prevent 10GB memory spike.
      // Search should be used for navigating larger datasets.
      const result = await invoke<{ documents: AllDocument[] }>('list_all_documents', { limit: 1000 });
      const byId: Record<string, AllDocument> = {};
      result.documents.forEach(doc => {
        byId[doc.source_doc_id] = doc;
      });
      set({ documentsById: byId, allDocsLoading: false });
    } catch (e) {
      console.error('[listAllDocuments]', e);
      set({ allDocsLoading: false });
    }
  },

  updateDocumentMetadata: (docIdValue: string, meta: Partial<AllDocument>) => {
    // Note: We use a simple strategy here to avoid O(N) array creation on every single event.
    // In a high-frequency indexing scenario, we update the map in place (semi-mutably) 
    // or batch the updates.
    set((state) => {
      const existing = state.documentsById[docIdValue];
      if (!existing) return state;

      // Update the hit if it's in the current search results (small array)
      const hits = state.hits.map((h) => 
        h.source_doc_id === docIdValue ? { ...h, ...meta } : h
      );

      return {
        documentsById: {
          ...state.documentsById,
          [docIdValue]: { ...existing, ...meta }
        },
        hits,
      };
    });
  },

  getAllDocuments: () => {
    return Object.values(get().documentsById);
  },
}));

// Optimization: Batch updates for document indexing events to prevent O(N^2) UI churn
let updateBuffer: Record<string, Partial<AllDocument>> = {};
let updateTimer: ReturnType<typeof setTimeout> | null = null;

export const throttledUpdateMetadata = (docId: string, meta: Partial<AllDocument>) => {
  updateBuffer[docId] = { ...(updateBuffer[docId] || {}), ...meta };
  
  if (!updateTimer) {
    updateTimer = setTimeout(() => {
      const store = useSearchStore.getState();
      const currentById = { ...store.documentsById };
      let changed = false;
      
      Object.entries(updateBuffer).forEach(([id, m]) => {
        if (currentById[id]) {
          currentById[id] = { ...currentById[id], ...m };
          changed = true;
        }
      });
      
      if (changed) {
        useSearchStore.setState({ documentsById: currentById });
      }
      
      updateBuffer = {};
      updateTimer = null;
    }, 500); // Update UI every 500ms during indexing
  }
};
