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
  allDocuments: AllDocument[];
  allDocsLoading: boolean;
  setQuery: (q: string) => void;
  setFilters: (f: Partial<SearchFilters>) => void;
  search: () => Promise<void>;
  clear: () => void;
  listAllDocuments: () => Promise<void>;
  updateDocumentMetadata: (docId: string, meta: Partial<AllDocument>) => void;
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
    const trimmed = query.trim();
    const tagTrimmed = filters.tagQuery.trim();
    
    // allow search if either text or tag is present
    if (!trimmed && !tagTrimmed) return;

    if (trimmed) {
      const updated = [trimmed, ...queryHistory.filter((q) => q !== trimmed)].slice(0, 5);
      set({ queryHistory: updated });
    }
    
    set({ isLoading: true, error: null });

    // Parse tags from tagQuery (e.g. "#tag1 #tag2" -> ["tag1", "tag2"])
    const tags = tagTrimmed
      ? tagTrimmed.split(/\s+/).filter(t => t.startsWith('#')).map(t => t.slice(1)).filter(t => t.length > 0)
      : [];

    try {
      const result = await invoke<{ hits: SearchHit[] }>('search_documents', {
        query: trimmed,
        limit: 50,
        source_types: filters.sourceTypes.length > 0 ? filters.sourceTypes : null,
        project_names: filters.projectNames.length > 0 ? filters.projectNames : null,
        tags: tags.length > 0 ? tags : null,
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

  updateDocumentMetadata: (docId: string, meta: Partial<AllDocument>) => {
    set((state) => ({
      allDocuments: state.allDocuments.map((d) => 
        d.source_doc_id === docId ? { ...d, ...meta } : d
      ),
      hits: state.hits.map((h) => 
        h.source_doc_id === docId ? { ...h, ...meta } : h
      ),
    }));
  },
}));
