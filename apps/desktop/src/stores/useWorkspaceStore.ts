import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

// ── 타입 ─────────────────────────────────────────────────────────────────────

export interface WorkspaceProject {
  id: number;
  name: string;
  display_name: string;
  description?: string;
  path: string;
  is_default: boolean;
  created_at: number;
}

export interface WorkspaceDocument {
  id: number;
  title: string;
  created_at: number;
  content_preview?: string;
  doc_type?: string;
  status?: string;
}

export interface DocumentSection {
  heading: string;
  level: number;
  content: string;
  start_line: number;
  end_line: number;
}

export interface Template {
  id: number;
  project_id?: number;
  name: string;
  description?: string;
  doc_type: string;
  content: string;
  created_at: number;
}

// ── 상태 ─────────────────────────────────────────────────────────────────────

interface WorkspaceState {
  workspace: WorkspaceProject | null;
  documents: WorkspaceDocument[];
  templates: Template[];
  isLoading: boolean;

  // 워크스페이스 초기화
  initWorkspace: () => Promise<void>;

  // 문서
  fetchDocuments: () => Promise<void>;
  createDocument: (title: string, templateId?: string) => Promise<WorkspaceDocument>;
  updateDocument: (id: number, title: string, content: string) => Promise<void>;
  deleteDocument: (id: number) => Promise<void>;

  // 섹션
  getSections: (docId: number) => Promise<DocumentSection[]>;
  updateSection: (docId: number, heading: string, newContent: string, occurrence?: number) => Promise<void>;
  insertSection: (docId: number, afterHeading: string | null, newSectionContent: string) => Promise<void>;
  deleteSection: (docId: number, heading: string, occurrence?: number) => Promise<void>;

  // 템플릿
  fetchTemplates: () => Promise<void>;
  createTemplate: (name: string, docType: string, content: string, description?: string) => Promise<Template>;
  updateTemplate: (id: number, name: string, docType: string, content: string, description?: string) => Promise<void>;
  deleteTemplate: (id: number) => Promise<void>;
  createDocumentFromTemplate: (templateId: number, projectId: number, path?: string) => Promise<WorkspaceDocument>;
}

// ── 스토어 ────────────────────────────────────────────────────────────────────

export const useWorkspaceStore = create<WorkspaceState>((set) => ({
  workspace: null,
  documents: [],
  templates: [],
  isLoading: false,

  // ── 워크스페이스 초기화 ────────────────────────────────────────────────────

  initWorkspace: async () => {
    try {
      const ws = await invoke<WorkspaceProject>('ensure_default_workspace_cmd');
      set({ workspace: ws });
    } catch (e) {
      console.error('워크스페이스 초기화 실패:', e);
    }
  },

  // ── 문서 ────────────────────────────────────────────────────────────────────

  fetchDocuments: async () => {
    set({ isLoading: true });
    try {
      const documents = await invoke<WorkspaceDocument[]>('list_workspace_documents', {
        workspaceId: null,
      });
      set({ documents });
    } catch (e) {
      console.error('문서 목록 조회 실패:', e);
      set({ documents: [] });
    } finally {
      set({ isLoading: false });
    }
  },

  createDocument: async (title, templateId) => {
    const doc = await invoke<WorkspaceDocument>('create_workspace_document', {
      title,
      templateId: templateId ?? null,
      workspaceId: null,
    });
    set((state) => ({ documents: [doc, ...state.documents] }));
    return doc;
  },

  updateDocument: async (id, title, content) => {
    await invoke('update_workspace_document', { id, title, content });
    set((state) => ({
      documents: state.documents.map((d) =>
        d.id === id ? { ...d, title, content_preview: content.slice(0, 100) } : d
      ),
    }));
  },

  deleteDocument: async (id) => {
    await invoke('delete_workspace_document', { id });
    set((state) => ({ documents: state.documents.filter((d) => d.id !== id) }));
  },

  // ── 섹션 ────────────────────────────────────────────────────────────────────

  getSections: async (docId) => {
    return await invoke<DocumentSection[]>('get_document_sections', { docId });
  },

  updateSection: async (docId, heading, newContent, occurrence = 0) => {
    await invoke('update_document_section', { docId, heading, newContent, occurrence });
  },

  insertSection: async (docId, afterHeading, newSectionContent) => {
    await invoke('insert_document_section', {
      docId,
      afterHeading: afterHeading ?? null,
      newSectionContent,
    });
  },

  deleteSection: async (docId, heading, occurrence = 0) => {
    await invoke('delete_document_section', { docId, heading, occurrence });
  },

  // ── 템플릿 ──────────────────────────────────────────────────────────────────

  fetchTemplates: async () => {
    try {
      const res = await invoke<{ templates: (Template & { source?: string })[] }>('list_templates', { projectId: null });
      // builtin 제외, custom(DB) 템플릿만 저장
      const templates = (res.templates ?? []).filter(t => t.source === 'custom') as Template[];
      set({ templates });
    } catch (e) {
      console.error('템플릿 목록 조회 실패:', e);
    }
  },

  createTemplate: async (name, docType, content, description) => {
    const tmpl = await invoke<Template>('create_template', {
      name,
      docType,
      content,
      description: description ?? null,
      projectId: null,
    });
    set((state) => ({ templates: [...state.templates, tmpl] }));
    return tmpl;
  },

  updateTemplate: async (id, name, docType, content, description) => {
    await invoke('update_template', { id, name, docType, content, description: description ?? null });
    set((state) => ({
      templates: state.templates.map((t) =>
        t.id === id ? { ...t, name, doc_type: docType, content, description } : t
      ),
    }));
  },

  deleteTemplate: async (id) => {
    await invoke('delete_template', { id });
    set((state) => ({ templates: state.templates.filter((t) => t.id !== id) }));
  },

  createDocumentFromTemplate: async (templateId, projectId, path) => {
    const doc = await invoke<WorkspaceDocument>('create_document_from_template', {
      templateId,
      projectId,
      path: path ?? null,
    });
    return doc;
  },
}));
