import { useEffect, useRef, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import rehypeRaw from 'rehype-raw';
import remarkGfm from 'remark-gfm';
import { invoke } from '@tauri-apps/api/core';
import { useSearchStore, AllDocument, SearchHit } from '../stores/useSearchStore';
import { usePluginStore } from '../stores/usePluginStore';

// ── 통합 문서 타입 (검색결과 or 전체목록 공통) ────────────────────────────

interface DocEntry {
  document_id: number;
  title: string;
  source_doc_id: string;
  project_name: string;
  source_type: string;
  // 검색 결과에만 존재
  score?: number;
  snippet?: string;
  heading_path?: string;
}

function hitToEntry(hit: SearchHit): DocEntry {
  return {
    document_id: hit.document_id,
    title: hit.title ?? '(제목 없음)',
    source_doc_id: hit.file_path ?? '',
    project_name: '',
    source_type: '',
    score: hit.score,
    snippet: hit.snippet ?? undefined,
    heading_path: hit.heading_path ?? undefined,
  };
}

function allDocToEntry(doc: AllDocument): DocEntry {
  return {
    document_id: doc.document_id,
    title: doc.title,
    source_doc_id: doc.source_doc_id,
    project_name: doc.project_name,
    source_type: doc.source_type,
  };
}

// ── 플러그인 메타 ─────────────────────────────────────────────────────────

const PLUGIN_META: Record<string, { icon: string; label: string }> = {
  obsidian:   { icon: '🪨', label: 'Obsidian' },
  confluence: { icon: '📄', label: 'Confluence' },
  github:     { icon: '🐙', label: 'GitHub' },
};

function pluginIcon(sourceType: string): string {
  const short = sourceType.replace(/^com\.doxus\./, '');
  const pluginId = `com.doxus.${short}`;
  return usePluginStore.getState().getEmoji(pluginId);
}

// ── Tooltip ───────────────────────────────────────────────────────────────

interface TooltipProps {
  doc: DocEntry;
  x: number;
  y: number;
}

function DocTooltip({ doc, x, y }: TooltipProps) {
  return (
    <div
      className="fixed z-50 bg-gray-800 border border-gray-700 rounded-lg shadow-xl p-3 w-64 text-xs pointer-events-none"
      style={{ left: x + 12, top: y }}
    >
      <p className="text-white font-semibold leading-tight mb-1.5">{doc.title}</p>
      <p className="text-gray-400 break-all leading-relaxed">{doc.source_doc_id}</p>
      {doc.project_name && (
        <p className="text-indigo-400 mt-1">
          {pluginIcon(doc.source_type)} {doc.project_name}
        </p>
      )}
      {doc.score != null && (
        <p className="text-gray-500 mt-1">점수: {doc.score.toFixed(3)}</p>
      )}
      {doc.heading_path && (
        <p className="text-indigo-300 mt-1">섹션: {doc.heading_path}</p>
      )}
    </div>
  );
}

// ── 파일 항목 (hover 2.5s 툴팁) ───────────────────────────────────────────

function FileItem({
  doc,
  isSelected,
  onSelect,
  depth = 0,
}: {
  doc: DocEntry;
  isSelected: boolean;
  onSelect: (doc: DocEntry) => void;
  depth?: number;
}) {
  const [tooltip, setTooltip] = useState<{ x: number; y: number } | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const handleMouseEnter = (e: React.MouseEvent) => {
    const { clientX, clientY } = e;
    timerRef.current = setTimeout(() => {
      setTooltip({ x: clientX, y: clientY });
    }, 1000);
  };

  const handleMouseLeave = () => {
    if (timerRef.current) clearTimeout(timerRef.current);
    setTooltip(null);
  };

  const handleMouseMove = (e: React.MouseEvent) => {
    if (tooltip) setTooltip({ x: e.clientX, y: e.clientY });
  };

  return (
    <>
      <button
        onClick={() => onSelect(doc)}
        onMouseEnter={handleMouseEnter}
        onMouseLeave={handleMouseLeave}
        onMouseMove={handleMouseMove}
        className={`w-full text-left py-0.5 text-xs truncate transition-colors rounded ${
          isSelected
            ? 'text-indigo-300 bg-indigo-950/60'
            : 'text-gray-400 hover:text-gray-100 hover:bg-gray-800/40'
        }`}
        style={{ paddingLeft: depth * 12 + 18 }}
        title=""
      >
        <span className="mr-1.5 opacity-50">📄</span>
        {doc.title}
        {doc.score != null && (
          <span className="ml-1.5 text-gray-600">{doc.score.toFixed(2)}</span>
        )}
      </button>
      {tooltip && <DocTooltip doc={doc} x={tooltip.x} y={tooltip.y} />}
    </>
  );
}

// ── 폴더 트리 빌더 ───────────────────────────────────────────────────────

interface TreeNode {
  name: string;
  isDir: boolean;
  children: Map<string, TreeNode>;
  doc?: DocEntry;
}

function buildTree(docs: DocEntry[]): TreeNode {
  const root: TreeNode = { name: '', isDir: true, children: new Map() };
  for (const doc of docs) {
    const parts = doc.source_doc_id.split('/').filter(Boolean);
    let node = root;
    for (let i = 0; i < parts.length; i++) {
      const part = parts[i];
      const isLast = i === parts.length - 1;
      if (!node.children.has(part)) {
        node.children.set(part, {
          name: part,
          isDir: !isLast,
          children: new Map(),
          doc: isLast ? doc : undefined,
        });
      }
      node = node.children.get(part)!;
    }
    // 경로 없는 경우 루트에 직접
    if (parts.length === 0) {
      root.children.set(doc.title, { name: doc.title, isDir: false, children: new Map(), doc });
    }
  }
  return root;
}

function TreeNodeView({
  node,
  depth,
  selectedDoc,
  onSelect,
}: {
  node: TreeNode;
  depth: number;
  selectedDoc: DocEntry | null;
  onSelect: (doc: DocEntry) => void;
}) {
  const [open, setOpen] = useState(false);
  const indent = depth * 12;

  if (node.isDir) {
    return (
      <div>
        <button
          onClick={() => setOpen(v => !v)}
          className="flex items-center gap-1 w-full text-left py-0.5 text-xs text-gray-500 hover:text-gray-300 hover:bg-gray-800/40 transition-colors rounded"
          style={{ paddingLeft: indent + 6 }}
        >
          <span className="text-gray-600 w-3 text-center shrink-0">{open ? '▾' : '▸'}</span>
          <span className="text-yellow-600 mr-1">📁</span>
          <span className="truncate">{node.name}</span>
        </button>
        {open && Array.from(node.children.values()).map(child => (
          <TreeNodeView
            key={child.name}
            node={child}
            depth={depth + 1}
            selectedDoc={selectedDoc}
            onSelect={onSelect}
          />
        ))}
      </div>
    );
  }

  const doc = node.doc!;
  const isSelected = selectedDoc?.document_id === doc.document_id && selectedDoc?.source_doc_id === doc.source_doc_id;
  return (
    <FileItem
      doc={doc}
      isSelected={isSelected}
      onSelect={onSelect}
      depth={depth}
    />
  );
}

// ── 프로젝트 그룹 ─────────────────────────────────────────────────────────

function ProjectGroup({
  projectName,
  sourceType,
  docs,
  selectedDoc,
  onSelect,
}: {
  projectName: string;
  sourceType: string;
  docs: DocEntry[];
  selectedDoc: DocEntry | null;
  onSelect: (doc: DocEntry) => void;
}) {
  const [open, setOpen] = useState(false);
  const tree = buildTree(docs);
  return (
    <div className="mb-1">
      <button
        onClick={() => setOpen(v => !v)}
        className="flex items-center gap-1.5 w-full text-left px-2 py-1 text-xs font-semibold text-gray-500 uppercase tracking-wider hover:text-gray-300 transition-colors"
      >
        <span className="text-xs">{open ? '▾' : '▸'}</span>
        <span>{pluginIcon(sourceType)}</span>
        <span className="truncate">{projectName || '(프로젝트 없음)'}</span>
        <span className="text-gray-700 font-normal ml-auto">{docs.length}</span>
      </button>
      {open && (
        <div className="pl-1">
          {Array.from(tree.children.values()).map(child => (
            <TreeNodeView
              key={child.name}
              node={child}
              depth={0}
              selectedDoc={selectedDoc}
              onSelect={onSelect}
            />
          ))}
        </div>
      )}
    </div>
  );
}

// ── Markdown Preview ──────────────────────────────────────────────────────

function MarkdownPreview({ content }: { content: string }) {
  return (
    <div className="prose prose-invert prose-sm max-w-none text-gray-300
      prose-headings:text-white prose-headings:font-semibold
      prose-h1:text-lg prose-h2:text-base prose-h3:text-sm
      prose-p:text-gray-300 prose-p:leading-relaxed
      prose-strong:text-white prose-strong:font-semibold
      prose-code:text-indigo-300 prose-code:bg-gray-800 prose-code:px-1 prose-code:py-0.5 prose-code:rounded prose-code:text-xs
      prose-pre:bg-gray-800 prose-pre:border prose-pre:border-gray-700
      prose-blockquote:border-indigo-500 prose-blockquote:text-gray-400
      prose-a:text-indigo-400 prose-a:no-underline hover:prose-a:underline
      prose-li:text-gray-300 prose-ul:text-gray-300 prose-ol:text-gray-300
      prose-hr:border-gray-700
      prose-table:w-full prose-table:border-collapse
      prose-th:border prose-th:border-gray-700 prose-th:bg-gray-800 prose-th:px-3 prose-th:py-2 prose-th:text-left prose-th:text-xs prose-th:text-gray-300
      prose-td:border prose-td:border-gray-700 prose-td:px-3 prose-td:py-2 prose-td:text-xs prose-td:text-gray-400">
      <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeRaw]}>{content}</ReactMarkdown>
    </div>
  );
}

// ── SearchPage ────────────────────────────────────────────────────────────

export function SearchPage() {
  const { query, hits, isLoading, error, setQuery, search, clear, allDocuments, allDocsLoading, listAllDocuments } = useSearchStore();
  usePluginStore((s) => s.emojiMap); // emoji 변경 시 리렌더 트리거
  const [inputValue, setInputValue] = useState(query);
  const [selectedDoc, setSelectedDoc] = useState<DocEntry | null>(null);
  const [previewContent, setPreviewContent] = useState<string | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [refreshToast, setRefreshToast] = useState<string | null>(null);
  const refreshToastTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // 초기 로드
  useEffect(() => {
    listAllDocuments();
  }, [listAllDocuments]);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    setQuery(inputValue);
    setSelectedDoc(null);
    setPreviewContent(null);
    search();
  };

  const handleClear = () => {
    clear();
    setSelectedDoc(null);
    setPreviewContent(null);
    setPreviewError(null);
    setInputValue('');
  };

  const fetchPreview = async (doc: DocEntry, forceRefresh = false) => {
    const identifier = doc.source_doc_id;
    if (!identifier) return;
    setPreviewLoading(true);
    setPreviewError(null);
    try {
      const result = await invoke<{ content: string; from_cache?: boolean }>('get_document_content', {
        filePath: identifier,
        projectName: doc.project_name || undefined,
        forceRefresh,
      });
      setPreviewContent(result.content);
      if (forceRefresh) {
        if (refreshToastTimer.current) clearTimeout(refreshToastTimer.current);
        setRefreshToast('최신 콘텐츠로 업데이트됨');
        refreshToastTimer.current = setTimeout(() => setRefreshToast(null), 3000);
      }
    } catch (e) {
      console.error('[preview] get_document_content failed:', identifier, e);
      setPreviewError(String(e));
    } finally {
      setPreviewLoading(false);
    }
  };

  const handleSelectDoc = async (doc: DocEntry) => {
    setSelectedDoc(doc);
    setPreviewContent(null);
    setPreviewError(null);
    if (doc.document_id) {
      invoke('increment_view_count', { documentId: doc.document_id }).catch(() => {});
    }
    await fetchPreview(doc);
  };

  const handleRefresh = () => {
    if (selectedDoc) fetchPreview(selectedDoc, true);
  };

  // 파일 목록 데이터 결정: 검색 후 → hits, 검색 전 → allDocuments
  const hasSearch = hits.length > 0;

  // 그룹화
  const groupedEntries = (() => {
    const entries: DocEntry[] = hasSearch
      ? hits.map(hitToEntry)
      : allDocuments.map(allDocToEntry);

    // project_name 기준 그룹화 (검색결과는 project_name이 없을 수 있음)
    const groups = new Map<string, { sourceType: string; docs: DocEntry[] }>();
    for (const entry of entries) {
      const key = entry.project_name || '검색결과';
      if (!groups.has(key)) {
        groups.set(key, { sourceType: entry.source_type || 'obsidian', docs: [] });
      }
      groups.get(key)!.docs.push(entry);
    }
    return groups;
  })();

  const totalCount = hasSearch ? hits.length : allDocuments.length;

  return (
    <div className="flex flex-col h-full gap-3">
      {/* 검색 폼 */}
      <form onSubmit={handleSubmit} className="flex gap-2 shrink-0">
        <input
          type="text"
          value={inputValue}
          onChange={(e) => setInputValue(e.target.value)}
          placeholder="문서를 검색하세요..."
          className="flex-1 px-3 py-2 bg-gray-900 border border-gray-800 rounded-lg text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-indigo-500 text-sm"
        />
        <button
          type="submit"
          disabled={isLoading}
          className="px-4 py-2 bg-indigo-600 text-white rounded-lg hover:bg-indigo-500 disabled:opacity-50 text-sm font-medium transition-colors"
        >
          {isLoading ? '검색 중...' : '검색'}
        </button>
        {hits.length > 0 && (
          <button
            type="button"
            onClick={handleClear}
            className="px-4 py-2 border border-gray-700 text-gray-400 rounded-lg hover:bg-gray-800 hover:text-gray-200 text-sm transition-colors"
          >
            초기화
          </button>
        )}
      </form>

      {error && (
        <div className="p-3 bg-red-950 border border-red-800 rounded-lg text-red-400 text-sm shrink-0">
          {error}
        </div>
      )}

      {/* 하단 2-panel */}
      <div className="flex-1 overflow-hidden flex gap-3">
        {/* 좌측: 파일 목록 */}
        <div className="w-72 shrink-0 overflow-auto bg-gray-950 border border-gray-800 rounded-xl py-2">
          {/* 헤더 */}
          <div className="px-3 pb-1.5 flex items-center justify-between">
            <span className="text-xs text-gray-600 uppercase tracking-wider">
              {hasSearch ? `검색결과` : `전체 문서`}
            </span>
            {(isLoading || allDocsLoading) ? (
              <span className="text-xs text-gray-700">로딩 중...</span>
            ) : (
              <span className="text-xs text-gray-700">{totalCount}</span>
            )}
          </div>

          {/* 그룹 목록 */}
          {groupedEntries.size === 0 && !isLoading && !allDocsLoading ? (
            <div className="px-3 py-8 text-center text-xs text-gray-600">
              {hasSearch ? '검색 결과 없음' : '인덱싱된 문서 없음'}
            </div>
          ) : (
            Array.from(groupedEntries.entries()).map(([projectName, { sourceType, docs }]) => (
              <ProjectGroup
                key={projectName}
                projectName={projectName}
                sourceType={sourceType}
                docs={docs}
                selectedDoc={selectedDoc}
                onSelect={handleSelectDoc}
              />
            ))
          )}
        </div>

        {/* 우측: 문서 프리뷰 */}
        <div className="flex-1 bg-gray-900 border border-gray-800 rounded-xl flex flex-col overflow-hidden">
          {selectedDoc ? (
            <>
              {/* 프리뷰 헤더 */}
              <div className="px-5 py-3 border-b border-gray-800 flex items-center justify-between shrink-0">
                <div className="min-w-0">
                  <h2 className="text-white font-semibold text-sm truncate">
                    {selectedDoc.title}
                  </h2>
                  {selectedDoc.source_doc_id && (
                    <p className="text-xs text-gray-600 truncate mt-0.5">{selectedDoc.source_doc_id}</p>
                  )}
                </div>
                <div className="flex items-center gap-1 shrink-0 ml-3">
                  <button
                    onClick={handleRefresh}
                    disabled={previewLoading}
                    title="최신 내용으로 새로고침"
                    className="text-gray-600 hover:text-gray-300 disabled:opacity-40 transition-colors p-1 rounded"
                  >
                    <svg
                      xmlns="http://www.w3.org/2000/svg"
                      width="14"
                      height="14"
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="2"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      className={previewLoading ? 'animate-spin' : ''}
                    >
                      <path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" />
                      <path d="M21 3v5h-5" />
                      <path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" />
                      <path d="M8 16H3v5" />
                    </svg>
                  </button>
                  <button
                    onClick={() => { setSelectedDoc(null); setPreviewContent(null); }}
                    className="text-gray-600 hover:text-gray-300 transition-colors text-lg p-1"
                  >
                    ✕
                  </button>
                </div>
              </div>

              {/* 프리뷰 내용 */}
              <div className="flex-1 overflow-auto p-5">
                {previewLoading ? (
                  <div className="flex items-center justify-center h-32">
                    <p className="text-gray-500 text-sm">불러오는 중...</p>
                  </div>
                ) : previewError ? (
                  <div className="space-y-3">
                    <p className="text-xs text-red-400 bg-red-950 border border-red-800 rounded-lg px-3 py-2">
                      {previewError}
                    </p>
                    {selectedDoc.snippet && <MarkdownPreview content={selectedDoc.snippet} />}
                  </div>
                ) : previewContent ? (
                  <MarkdownPreview content={previewContent} />
                ) : selectedDoc.snippet ? (
                  <div className="space-y-4">
                    {selectedDoc.heading_path && (
                      <div className="flex items-center gap-2">
                        <span className="text-xs text-gray-500">섹션</span>
                        <span className="text-xs text-indigo-400 bg-indigo-950 px-2 py-1 rounded">
                          {selectedDoc.heading_path}
                        </span>
                      </div>
                    )}
                    <MarkdownPreview content={selectedDoc.snippet} />
                  </div>
                ) : (
                  <p className="text-gray-600 text-sm">미리볼 내용이 없습니다.</p>
                )}
              </div>
            </>
          ) : (
            /* 선택 없을 때 empty state */
            <div className="flex flex-col items-center justify-center h-full gap-3 text-center px-8">
              <p className="text-4xl">📖</p>
              <p className="text-gray-400 font-medium">문서를 선택하세요</p>
              <p className="text-sm text-gray-600">
                {hasSearch
                  ? '검색 결과에서 문서를 클릭하면 내용을 미리 볼 수 있습니다'
                  : '좌측 파일 목록에서 문서를 클릭하면 내용을 미리 볼 수 있습니다'}
              </p>
            </div>
          )}
        </div>
      </div>

      {refreshToast && (
        <div className="fixed bottom-6 right-6 z-50 px-4 py-3 bg-gray-900 border border-gray-700 rounded-xl shadow-xl text-sm text-gray-200 max-w-xs">
          ✅ {refreshToast}
        </div>
      )}
    </div>
  );
}
