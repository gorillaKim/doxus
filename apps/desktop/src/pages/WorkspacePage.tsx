import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useWorkspaceStore, WorkspaceDocument, Workspace } from '../stores/useWorkspaceStore';

interface Template {
  id: string;
  name: string;
  description: string;
}

const BUILTIN_TEMPLATES: Template[] = [
  { id: 'note', name: '메모', description: '간단한 메모와 아이디어' },
  { id: 'meeting', name: '회의록', description: '회의 안건 및 결과 정리' },
  { id: 'decision', name: '의사결정', description: '아키텍처 의사결정 기록 (ADR)' },
  { id: 'journal', name: '일지', description: '일별 작업 일지' },
  { id: 'retrospective', name: '회고', description: '스프린트 회고록' },
  { id: 'todo', name: 'TODO 목록', description: '할 일 체크리스트' },
  { id: 'techspec', name: '기술 명세서', description: '기능 기술 스펙 문서' },
];

const TEMPLATE_ICONS: Record<string, string> = {
  note: '📝',
  meeting: '🗓',
  decision: '⚖️',
  journal: '📖',
  retrospective: '🔄',
  todo: '✅',
  techspec: '🔧',
};

function formatDate(ts: number): string {
  return new Date(ts * 1000).toLocaleDateString('ko-KR', {
    month: 'short',
    day: 'numeric',
    year: 'numeric',
  });
}

interface NewDocModalProps {
  initialTemplateId?: string | null;
  onClose: () => void;
  onCreated: (doc: WorkspaceDocument) => void;
}

function NewDocModal({ initialTemplateId, onClose, onCreated }: NewDocModalProps) {
  const [title, setTitle] = useState('');
  const [selectedTemplate, setSelectedTemplate] = useState<string | null>(
    initialTemplateId ?? null
  );
  const [isCreating, setIsCreating] = useState(false);

  const handleCreate = async () => {
    if (!title.trim()) return;
    setIsCreating(true);
    try {
      const doc = await invoke<WorkspaceDocument>('create_workspace_document', {
        title: title.trim(),
        templateId: selectedTemplate,
      });
      onCreated(doc);
    } catch (e) {
      console.error('문서 생성 실패:', e);
      // 실패 시 모달 열린 상태 유지 — 사용자가 재시도 가능
    } finally {
      setIsCreating(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
      <div className="bg-gray-900 border border-gray-800 rounded-2xl p-6 w-full max-w-md shadow-2xl">
        <h2 className="text-white font-semibold text-lg mb-4">새 문서</h2>

        <label className="block text-gray-400 text-sm mb-1">제목</label>
        <input
          autoFocus
          type="text"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && handleCreate()}
          placeholder="제목 없음"
          className="w-full px-3 py-2 bg-gray-800 border border-gray-700 rounded-lg text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-indigo-500 text-sm mb-4"
        />

        <label className="block text-gray-400 text-sm mb-2">템플릿 (선택)</label>
        <div className="grid grid-cols-2 gap-2 mb-5">
          <button
            onClick={() => setSelectedTemplate(null)}
            className={`px-3 py-2 rounded-lg text-sm text-left border transition-colors ${
              selectedTemplate === null
                ? 'bg-indigo-600 border-indigo-500 text-white'
                : 'bg-gray-800 border-gray-700 text-gray-400 hover:border-gray-600'
            }`}
          >
            빈 문서
          </button>
          {BUILTIN_TEMPLATES.map((t) => (
            <button
              key={t.id}
              onClick={() => setSelectedTemplate(t.id)}
              className={`px-3 py-2 rounded-lg text-sm text-left border transition-colors ${
                selectedTemplate === t.id
                  ? 'bg-indigo-600 border-indigo-500 text-white'
                  : 'bg-gray-800 border-gray-700 text-gray-400 hover:border-gray-600'
              }`}
            >
              {TEMPLATE_ICONS[t.id]} {t.name}
            </button>
          ))}
        </div>

        <div className="flex justify-end gap-2">
          <button
            onClick={onClose}
            className="px-4 py-1.5 rounded-lg text-sm text-gray-400 hover:text-white border border-gray-700 hover:border-gray-600 transition-colors"
          >
            취소
          </button>
          <button
            onClick={handleCreate}
            disabled={!title.trim() || isCreating}
            className="bg-indigo-600 hover:bg-indigo-700 disabled:opacity-50 text-white px-4 py-1.5 rounded-lg text-sm transition-colors"
          >
            {isCreating ? '생성 중...' : '생성'}
          </button>
        </div>
      </div>
    </div>
  );
}

type TabKey = 'workspaces' | 'documents' | 'templates';

interface EditingDoc {
  id: number;
  title: string;
  content: string;
}

interface NewWorkspaceModalProps {
  onClose: () => void;
  onCreated: (ws: Workspace) => void;
}

function NewWorkspaceModal({ onClose, onCreated }: NewWorkspaceModalProps) {
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [isCreating, setIsCreating] = useState(false);

  const handleCreate = async () => {
    if (!name.trim()) return;
    setIsCreating(true);
    try {
      const ws = await invoke<Workspace>('create_workspace', {
        name: name.trim(),
        description: description.trim() || null,
      });
      onCreated(ws);
    } catch (e) {
      console.error('workspace create failed', e);
    } finally {
      setIsCreating(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
      <div className="bg-gray-900 border border-gray-800 rounded-2xl p-6 w-full max-w-md shadow-2xl">
        <h2 className="text-white font-semibold text-lg mb-4">새 워크스페이스</h2>
        <label className="block text-gray-400 text-sm mb-1">이름</label>
        <input
          autoFocus
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && handleCreate()}
          placeholder="워크스페이스 이름"
          className="w-full px-3 py-2 bg-gray-800 border border-gray-700 rounded-lg text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-indigo-500 text-sm mb-3"
        />
        <label className="block text-gray-400 text-sm mb-1">설명 (선택)</label>
        <input
          type="text"
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder="간단한 설명"
          className="w-full px-3 py-2 bg-gray-800 border border-gray-700 rounded-lg text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-indigo-500 text-sm mb-5"
        />
        <div className="flex justify-end gap-2">
          <button
            onClick={onClose}
            className="px-4 py-1.5 rounded-lg text-sm text-gray-400 hover:text-white border border-gray-700 hover:border-gray-600 transition-colors"
          >
            취소
          </button>
          <button
            onClick={handleCreate}
            disabled={!name.trim() || isCreating}
            className="bg-indigo-600 hover:bg-indigo-700 disabled:opacity-50 text-white px-4 py-1.5 rounded-lg text-sm transition-colors"
          >
            {isCreating ? '생성 중...' : '생성'}
          </button>
        </div>
      </div>
    </div>
  );
}

export default function WorkspacePage() {
  const {
    documents, isLoading, fetchDocuments, addDocument, removeDocument, updateDocument,
    workspaces, fetchWorkspaces, addWorkspace, removeWorkspace,
  } = useWorkspaceStore();

  const [activeTab, setActiveTab] = useState<TabKey>('workspaces');
  const [expandedId, setExpandedId] = useState<number | null>(null);
  const [showModal, setShowModal] = useState(false);
  const [showWsModal, setShowWsModal] = useState(false);
  const [preselectedTemplate, setPreselectedTemplate] = useState<string | null>(null);
  const [editingDoc, setEditingDoc] = useState<EditingDoc | null>(null);

  useEffect(() => {
    fetchDocuments();
    fetchWorkspaces();
  }, [fetchDocuments, fetchWorkspaces]);

  const handleCreated = (doc: WorkspaceDocument) => {
    addDocument(doc);
    setShowModal(false);
    setPreselectedTemplate(null);
    setActiveTab('documents');
  };

  const handleDelete = async (id: number) => {
    try {
      await invoke('delete_workspace_document', { id });
      removeDocument(id);
      if (expandedId === id) setExpandedId(null);
    } catch (e) {
      console.error('delete failed', e);
    }
  };

  const openModalWithTemplate = (templateId: string) => {
    setPreselectedTemplate(templateId);
    setShowModal(true);
  };

  const tabs: { key: TabKey; label: string }[] = [
    { key: 'workspaces', label: '워크스페이스' },
    { key: 'documents', label: '문서' },
    { key: 'templates', label: '템플릿' },
  ];

  return (
    <div className="flex flex-col h-full bg-gray-950 p-6 gap-5">
      {/* 헤더 */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-white text-xl font-semibold tracking-tight">워크스페이스</h1>
          <p className="text-gray-400 text-sm mt-0.5">개인 문서 및 템플릿 관리</p>
        </div>
        {activeTab === 'workspaces' && (
          <button
            onClick={() => setShowWsModal(true)}
            className="bg-indigo-600 hover:bg-indigo-700 text-white px-3 py-1.5 rounded-lg text-sm transition-colors"
          >
            + 새 워크스페이스
          </button>
        )}
        {activeTab === 'documents' && (
          <button
            onClick={() => {
              setPreselectedTemplate(null);
              setShowModal(true);
            }}
            className="bg-indigo-600 hover:bg-indigo-700 text-white px-3 py-1.5 rounded-lg text-sm transition-colors"
          >
            + 새 문서
          </button>
        )}
      </div>

      {/* 탭 */}
      <div className="flex gap-1 p-1 bg-gray-900 border border-gray-800 rounded-xl w-fit">
        {tabs.map((tab) => (
          <button
            key={tab.key}
            onClick={() => setActiveTab(tab.key)}
            className={`px-4 py-1.5 rounded-lg text-sm font-medium transition-colors ${
              activeTab === tab.key
                ? 'bg-indigo-600 text-white'
                : 'text-gray-400 hover:text-white'
            }`}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {/* 워크스페이스 탭 */}
      {activeTab === 'workspaces' && (
        <div className="flex-1 overflow-auto">
          {workspaces.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-48 gap-3">
              <p className="text-gray-500 text-sm">워크스페이스가 없습니다.</p>
              <button
                onClick={() => setShowWsModal(true)}
                className="bg-indigo-600 hover:bg-indigo-700 text-white px-3 py-1.5 rounded-lg text-sm transition-colors"
              >
                첫 번째 워크스페이스 만들기
              </button>
            </div>
          ) : (
            <div className="grid gap-3">
              {workspaces.map((ws) => (
                <div
                  key={ws.id}
                  className="bg-gray-900 border border-gray-800 rounded-xl p-4 flex items-center justify-between hover:border-gray-700 transition-colors"
                >
                  <div>
                    <h3 className="text-white font-semibold">{ws.name}</h3>
                    {ws.description && (
                      <p className="text-gray-400 text-sm mt-0.5">{ws.description}</p>
                    )}
                    <p className="text-gray-600 text-xs mt-1">{formatDate(ws.created_at)}</p>
                  </div>
                  <button
                    onClick={async () => {
                      try {
                        await invoke('delete_workspace', { id: ws.id });
                        removeWorkspace(ws.id);
                      } catch (e) {
                        console.error('delete workspace failed', e);
                      }
                    }}
                    className="text-gray-500 hover:text-red-400 text-xs px-2 py-1 rounded border border-gray-700 hover:border-red-500 transition-colors shrink-0"
                  >
                    삭제
                  </button>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* 문서 탭 */}
      {activeTab === 'documents' && (
        <div className="flex-1 overflow-auto">
          {isLoading ? (
            <div className="flex items-center justify-center h-32">
              <p className="text-gray-500 text-sm">불러오는 중...</p>
            </div>
          ) : documents.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-48 gap-3">
              <p className="text-gray-500 text-sm">문서가 없습니다.</p>
              <button
                onClick={() => {
                  setPreselectedTemplate(null);
                  setShowModal(true);
                }}
                className="bg-indigo-600 hover:bg-indigo-700 text-white px-3 py-1.5 rounded-lg text-sm transition-colors"
              >
                첫 번째 문서 만들기
              </button>
            </div>
          ) : (
            <div className="grid gap-3">
              {documents.map((doc) => (
                <div
                  key={doc.id}
                  className="w-full text-left bg-gray-900 border border-gray-800 rounded-xl p-4 hover:border-gray-700 transition-colors"
                >
                  <div className="flex items-start justify-between gap-3">
                    <button
                      className="flex-1 min-w-0 text-left"
                      onClick={() => setExpandedId(expandedId === doc.id ? null : doc.id)}
                    >
                      <h3 className="text-white font-semibold truncate">{doc.title}</h3>
                      <p className="text-gray-400 text-sm mt-0.5">{formatDate(doc.created_at)}</p>
                    </button>
                    <div className="flex items-center gap-2 shrink-0">
                      <button
                        onClick={() =>
                          setEditingDoc({
                            id: doc.id,
                            title: doc.title,
                            content: doc.content_preview ?? '',
                          })
                        }
                        className="text-gray-500 hover:text-indigo-400 text-xs px-2 py-1 rounded border border-gray-700 hover:border-indigo-500 transition-colors"
                      >
                        수정
                      </button>
                      <button
                        onClick={() => handleDelete(doc.id)}
                        className="text-gray-500 hover:text-red-400 text-xs px-2 py-1 rounded border border-gray-700 hover:border-red-500 transition-colors"
                      >
                        삭제
                      </button>
                      <span className="text-gray-600 text-xs">
                        {expandedId === doc.id ? '▲' : '▼'}
                      </span>
                    </div>
                  </div>
                  {expandedId === doc.id && (
                    <div className="mt-3 pt-3 border-t border-gray-800">
                      {doc.content_preview ? (
                        <p className="text-gray-400 text-sm leading-relaxed">
                          {doc.content_preview}
                        </p>
                      ) : (
                        <p className="text-gray-600 text-sm italic">빈 문서입니다</p>
                      )}
                    </div>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* 템플릿 탭 */}
      {activeTab === 'templates' && (
        <div className="flex-1 overflow-auto">
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {BUILTIN_TEMPLATES.map((t) => (
              <div
                key={t.id}
                className="bg-gray-900 border border-gray-800 rounded-xl p-4 flex flex-col gap-3"
              >
                <div className="flex items-center gap-2">
                  <span className="text-2xl">{TEMPLATE_ICONS[t.id]}</span>
                  <h3 className="text-white font-semibold">{t.name}</h3>
                </div>
                <p className="text-gray-400 text-sm flex-1">{t.description}</p>
                <button
                  onClick={() => openModalWithTemplate(t.id)}
                  className="bg-indigo-600 hover:bg-indigo-700 text-white px-3 py-1.5 rounded-lg text-sm transition-colors w-fit"
                >
                  사용하기
                </button>
              </div>
            ))}
          </div>
        </div>
      )}

      {showWsModal && (
        <NewWorkspaceModal
          onClose={() => setShowWsModal(false)}
          onCreated={(ws) => {
            addWorkspace(ws);
            setShowWsModal(false);
          }}
        />
      )}

      {showModal && (
        <NewDocModal
          initialTemplateId={preselectedTemplate}
          onClose={() => {
            setShowModal(false);
            setPreselectedTemplate(null);
          }}
          onCreated={handleCreated}
        />
      )}

      {editingDoc && (
        <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50">
          <div className="bg-gray-900 border border-gray-800 rounded-2xl p-6 w-full max-w-2xl flex flex-col gap-4 shadow-2xl">
            <div className="flex items-center justify-between">
              <input
                value={editingDoc.title}
                onChange={(e) =>
                  setEditingDoc((prev) => (prev ? { ...prev, title: e.target.value } : null))
                }
                className="text-base font-semibold text-gray-100 bg-transparent border-b border-gray-700 focus:outline-none focus:border-indigo-500 flex-1"
              />
              <button
                onClick={() => setEditingDoc(null)}
                className="text-gray-500 hover:text-gray-300 ml-4"
              >
                ✕
              </button>
            </div>
            <textarea
              value={editingDoc.content}
              onChange={(e) =>
                setEditingDoc((prev) => (prev ? { ...prev, content: e.target.value } : null))
              }
              className="bg-gray-800 border border-gray-700 rounded-lg px-4 py-3 text-sm text-gray-100 placeholder-gray-600 focus:outline-none focus:border-indigo-500 resize-none h-64 font-mono"
            />
            <div className="flex gap-2 justify-end">
              <button
                onClick={() => setEditingDoc(null)}
                className="px-3 py-1.5 text-sm text-gray-400 hover:text-gray-200"
              >
                취소
              </button>
              <button
                onClick={async () => {
                  if (!editingDoc) return;
                  try {
                    await invoke('update_workspace_document', {
                      id: editingDoc.id,
                      title: editingDoc.title,
                      content: editingDoc.content,
                    });
                    updateDocument(editingDoc.id, {
                      title: editingDoc.title,
                      content_preview: editingDoc.content.slice(0, 100) || undefined,
                    });
                    setEditingDoc(null);
                  } catch (e) {
                    console.error(e);
                  }
                }}
                className="px-4 py-1.5 text-sm bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg transition-colors"
              >
                저장
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
