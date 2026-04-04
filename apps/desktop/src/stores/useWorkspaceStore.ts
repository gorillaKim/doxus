import { create } from 'zustand';

interface WorkspaceState {
  workspaces: Array<{ id: number; name: string }>;
}

export const useWorkspaceStore = create<WorkspaceState>(() => ({
  workspaces: [],
}));
