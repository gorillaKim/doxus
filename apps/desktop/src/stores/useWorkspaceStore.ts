import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

export interface WorkspaceDocument {
  id: number;
  title: string;
  created_at: number;
  content_preview?: string;
}

interface WorkspaceState {
  documents: WorkspaceDocument[];
  isLoading: boolean;
  fetchDocuments: () => Promise<void>;
  addDocument: (doc: WorkspaceDocument) => void;
  removeDocument: (id: number) => void;
  updateDocument: (id: number, patch: Partial<WorkspaceDocument>) => void;
}

export const useWorkspaceStore = create<WorkspaceState>((set) => ({
  documents: [],
  isLoading: false,

  fetchDocuments: async () => {
    set({ isLoading: true });
    try {
      const docs = await invoke<WorkspaceDocument[]>('list_workspace_documents');
      set({ documents: docs });
    } catch {
      set({ documents: [] });
    } finally {
      set({ isLoading: false });
    }
  },

  addDocument: (doc) =>
    set((state) => ({ documents: [doc, ...state.documents] })),

  removeDocument: (id) =>
    set((state) => ({ documents: state.documents.filter((d) => d.id !== id) })),

  updateDocument: (id, patch) =>
    set((state) => ({
      documents: state.documents.map((d) => (d.id === id ? { ...d, ...patch } : d)),
    })),
}));
