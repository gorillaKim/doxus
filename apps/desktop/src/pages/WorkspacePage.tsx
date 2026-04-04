import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface WorkspaceDocument {
  id: number;
  title: string;
  created_at: number;
  content_preview?: string;
}

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
];

const TEMPLATE_ICONS: Record<string, string> = {
  note: '📝',
  meeting: '🗓',
  decision: '⚖️',
  journal: '📖',
  retrospective: '🔄',
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
    } catch {
      const stub: WorkspaceDocument = {
        id: Date.now(),
        title: title.trim(),
        created_at: Math.floor(Date.now() / 1000),
        content_preview: selectedTemplate ? `템플릿: ${selectedTemplate}` : undefined,
      };
      onCreated(stub);
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

type TabKey = 'documents' | 'templates';

export default function WorkspacePage() {
  const [activeTab, setActiveTab] = useState<TabKey>('documents');
  const [documents, setDocuments] = useState<WorkspaceDocument[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [expandedId, setExpandedId] = useState<number | null>(null);
  const [showModal, setShowModal] = useState(false);
  const [preselectedTemplate, setPreselectedTemplate] = useState<string | null>(null);

  useEffect(() => {
    invoke<WorkspaceDocument[]>('list_workspace_documents')
      .then(setDocuments)
      .catch(() => setDocuments([]))
      .finally(() => setIsLoading(false));
  }, []);

  const handleCreated = (doc: WorkspaceDocument) => {
    setDocuments((prev) => [doc, ...prev]);
    setShowModal(false);
    setPreselectedTemplate(null);
    setActiveTab('documents');
  };

  const openModalWithTemplate = (templateId: string) => {
    setPreselectedTemplate(templateId);
    setShowModal(true);
  };

  const tabs: { key: TabKey; label: string }[] = [
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
                <button
                  key={doc.id}
                  onClick={() => setExpandedId(expandedId === doc.id ? null : doc.id)}
                  className="w-full text-left bg-gray-900 border border-gray-800 rounded-xl p-4 hover:border-gray-700 transition-colors"
                >
                  <div className="flex items-start justify-between gap-3">
                    <div className="flex-1 min-w-0">
                      <h3 className="text-white font-semibold truncate">{doc.title}</h3>
                      <p className="text-gray-400 text-sm mt-0.5">{formatDate(doc.created_at)}</p>
                    </div>
                    <span className="text-gray-600 text-xs mt-1 shrink-0">
                      {expandedId === doc.id ? '▲' : '▼'}
                    </span>
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
                </button>
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
    </div>
  );
}
