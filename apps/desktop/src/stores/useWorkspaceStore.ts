import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

export interface WorkspaceDocument {
  id: number;
  title: string;
  created_at: number;
  content_preview?: string;
}

export interface Workspace {
  id: number;
  name: string;
  description?: string;
  project_ids: string;
  created_at: number;
}

interface WorkspaceState {
  documents: WorkspaceDocument[];
  workspaces: Workspace[];
  isLoading: boolean;
  fetchDocuments: () => Promise<void>;
  addDocument: (doc: WorkspaceDocument) => void;
  removeDocument: (id: number) => void;
  updateDocument: (id: number, patch: Partial<WorkspaceDocument>) => void;
  fetchWorkspaces: () => Promise<void>;
  addWorkspace: (ws: Workspace) => void;
  removeWorkspace: (id: number) => void;
}

export const useWorkspaceStore = create<WorkspaceState>((set) => ({
  documents: [],
  workspaces: [],
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

  fetchWorkspaces: async () => {
    try {
      const wss = await invoke<Workspace[]>('list_workspaces');
      set({ workspaces: wss });
    } catch {
      set({ workspaces: [] });
    }
  },

  addWorkspace: (ws) =>
    set((state) => ({ workspaces: [ws, ...state.workspaces] })),

  removeWorkspace: (id) =>
    set((state) => ({ workspaces: state.workspaces.filter((w) => w.id !== id) })),
}));
