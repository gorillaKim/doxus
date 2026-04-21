import { useEffect, useRef, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import { createPortal } from 'react-dom';
import ReactMarkdown from 'react-markdown';
import rehypeRaw from 'rehype-raw';
import remarkGfm from 'remark-gfm';
import { invoke } from '@tauri-apps/api/core';
import { useSearchStore, AllDocument, SearchHit, SearchFilters } from '../stores/useSearchStore';
import { usePluginStore } from '../stores/usePluginStore';

// ── Frontmatter strip ─────────────────────────────────────────────────────
function stripFrontmatter(content: string): string {
  if (!content.startsWith('---')) return content;
  const end = content.indexOf('\n---', 3);
  if (end === -1) return content;
  return content.slice(end + 4).trimStart();
}

// ── 통합 문서 타입 (검색결과 or 전체목록 공통) ────────────────────────────

interface DocEntry {
  document_id: number;
  chunk_id: number;
  title: string;
  source_doc_id: string;
  hierarchy_path: string;
  project_name: string;
  source_type: string;
  score?: number;
  snippet?: string;
  context_content?: string | null;
  heading_path?: string | null; // 섹션 경로 필드 추가
  tags?: string[];
  updated_at?: number;
  last_indexed?: number;
  cache_ttl?: number;
  metadata?: Record<string, any>;
  url?: string | null;
  source_project_id: string;
}

interface PreviewMeta {
  tags: string[];
  aliases: string[];
  created_at: number | null;
  updated_at: number | null;
  last_indexed: number | null;
  cache_ttl: number | null;
  metadata: Record<string, unknown>;
  url: string | null;
  source_project_id: string;
  source_doc_id: string;
}

function hitToEntry(hit: SearchHit): DocEntry {
  return {
    document_id: hit.document_id,
    chunk_id: hit.chunk_id,
    title: hit.title ?? '(제목 없음)',
    source_doc_id: hit.source_doc_id ?? String(hit.document_id),
    hierarchy_path: hit.file_path ?? hit.source_doc_id ?? '',
    project_name: hit.project_name ?? '',
    source_type: hit.source_type ?? '',
    score: hit.score,
    snippet: hit.snippet ?? undefined,
    context_content: hit.context_content,
    heading_path: hit.heading_path,
    tags: hit.tags,
    updated_at: hit.updated_at,
    last_indexed: hit.last_indexed,
    cache_ttl: hit.cache_ttl,
    metadata: hit.metadata,
    url: hit.url,
    source_project_id: hit.source_project_id,
  };
}

function allDocToEntry(doc: AllDocument): DocEntry {
  return {
    document_id: doc.document_id,
    chunk_id: 0, // 전체 문서이므로 기본값 0 사용
    title: doc.title,
    source_doc_id: doc.source_doc_id,
    hierarchy_path: doc.file_path || doc.source_doc_id,
    project_name: doc.project_name,
    source_type: doc.source_type,
    heading_path: null, // 전체 목록에서는 섹션 정보 없음
    tags: doc.tags,
    updated_at: doc.updated_at,
    last_indexed: doc.last_indexed,
    cache_ttl: doc.cache_ttl,
    url: doc.url,
    source_project_id: doc.source_project_id || '',
  };
}

// ── 플러그인 메타 ─────────────────────────────────────────────────────────

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
  const dateStr = doc.updated_at ? formatUnixDate(doc.updated_at) : null;
  
  return createPortal(
    <div
      className="fixed z-50 bg-gray-900 border border-indigo-500/30 rounded-xl shadow-2xl p-4 w-80 text-xs backdrop-blur-md pointer-events-none ring-1 ring-white/10"
      style={{ left: Math.min(x + 12, window.innerWidth - 340), top: Math.min(y, window.innerHeight - 300) }}
    >
      <div className="flex items-center justify-between mb-2">
        <span className="text-indigo-400 font-mono text-[10px] uppercase tracking-tighter">Document Info</span>
        {doc.score != null && (
          <span className="px-1.5 py-0.5 rounded bg-indigo-500/20 text-indigo-300 border border-indigo-500/30">
             {doc.score.toFixed(3)}
          </span>
        )}
      </div>
      
      <p className="text-white font-bold leading-tight mb-1 text-[13px]">{doc.title}</p>
      <p className="text-gray-500 font-mono text-[10px] truncate mb-3">{doc.source_doc_id}</p>
      
      {doc.context_content && (
        <div className="bg-black/40 rounded-lg p-2.5 mb-3 border border-gray-800/50">
          <p className="text-gray-400 leading-relaxed line-clamp-4 italic">
            "{doc.context_content.replace(/---/g, '').trim()}"
          </p>
        </div>
      )}

      <div className="space-y-2">
        <div className="flex items-center gap-2">
          <span className="w-12 text-gray-600">Project</span>
          <span className="text-gray-300 truncate">
            {pluginIcon(doc.source_type)} {doc.project_name}
          </span>
        </div>
        
        {dateStr && (
          <div className="flex items-center gap-2">
            <span className="w-12 text-gray-600">Updated</span>
            <span className="text-gray-400">{dateStr}</span>
          </div>
        )}

        {doc.last_indexed && (
          <div className="flex items-center gap-2">
            <span className="w-12 text-gray-600">Indexed</span>
            <span className="text-indigo-400/80">{formatUnixDate(doc.last_indexed, true)}</span>
          </div>
        )}

        {doc.last_indexed && doc.cache_ttl && doc.cache_ttl > 0 && (
          <div className="flex items-center gap-2">
            <span className="w-12 text-gray-600">Cache</span>
            <span className="text-amber-400/80">
              {formatDuration((doc.last_indexed + doc.cache_ttl * 60) - Math.floor(Date.now() / 1000))}
            </span>
          </div>
        )}

        {doc.tags && doc.tags.length > 0 && (
          <div className="flex gap-1.5 flex-wrap pt-1">
            {doc.tags.slice(0, 3).map(t => (
              <span key={t} className="px-1.5 py-0.5 rounded bg-gray-800 text-gray-400 border border-gray-700">
                #{t}
              </span>
            ))}
            {doc.tags.length > 3 && <span className="text-gray-600">+{doc.tags.length - 3}</span>}
          </div>
        )}
      </div>
    </div>,
    document.body
  );
}

// ── 툴팁 핸들러 훅 ──────────────────────────────────────────────────────────

function useTooltip() {
  const [tooltip, setTooltip] = useState<{ x: number; y: number } | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const onMouseEnter = (e: React.MouseEvent) => {
    const { clientX, clientY } = e;
    if (timerRef.current) clearTimeout(timerRef.current);
    timerRef.current = setTimeout(() => {
      setTooltip({ x: clientX, y: clientY });
    }, 1000);
  };

  const onMouseLeave = () => {
    if (timerRef.current) clearTimeout(timerRef.current);
    setTooltip(null);
  };

  const onMouseMove = (e: React.MouseEvent) => {
    if (tooltip) setTooltip({ x: e.clientX, y: e.clientY });
  };

  return { tooltip, onMouseEnter, onMouseLeave, onMouseMove };
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
  const { tooltip, onMouseEnter, onMouseLeave, onMouseMove } = useTooltip();

  return (
    <>
      <button
        onClick={() => onSelect(doc)}
        onMouseEnter={onMouseEnter}
        onMouseLeave={onMouseLeave}
        onMouseMove={onMouseMove}
        className={`w-full text-left py-0.5 text-xs truncate transition-colors rounded ${
          isSelected
            ? 'text-indigo-300 bg-indigo-950/60'
            : 'text-gray-400 hover:text-gray-100 hover:bg-gray-800/40'
        }`}
        style={{ paddingLeft: depth * 12 + 25 }}
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
    const fullPath = doc.hierarchy_path;
    const parts = fullPath.split('/').map(p => p.trim()).filter(Boolean);
    let node = root;
    for (let i = 0; i < parts.length; i++) {
      let part = parts[i];
      const isLast = i === parts.length - 1;
      
      if (part.toLowerCase().endsWith('.md')) {
        part = part.slice(0, -3);
      }

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

  const isSelected = node.doc && selectedDoc?.document_id === node.doc.document_id && selectedDoc?.source_doc_id === node.doc.source_doc_id;
  const hasChildren = node.children.size > 0;

  const { tooltip, onMouseEnter, onMouseLeave, onMouseMove } = useTooltip();

  if (node.isDir || hasChildren) {
    return (
      <div className="flex flex-col">
        <div 
          className={`flex items-center gap-1 w-full text-left py-0.5 text-xs rounded transition-colors group ${
            isSelected ? 'text-indigo-300 bg-indigo-950/60' : 'text-gray-500 hover:bg-gray-800/40'
          }`}
          style={{ paddingLeft: indent + 5 }}
        >
          <button
            onClick={(e) => { e.stopPropagation(); setOpen(v => !v); }}
            className={`w-4 h-4 flex items-center justify-center hover:text-gray-300 transition-colors ${!hasChildren && 'invisible'}`}
          >
            <span className="text-[10px]">{open ? '▾' : '▸'}</span>
          </button>
          
          <div 
            onClick={() => node.doc && onSelect(node.doc)}
            onMouseEnter={node.doc ? onMouseEnter : undefined}
            onMouseLeave={node.doc ? onMouseLeave : undefined}
            onMouseMove={node.doc ? onMouseMove : undefined}
            className={`flex items-center gap-1 flex-1 min-w-0 ${node.doc ? 'cursor-pointer hover:text-gray-200' : 'cursor-default'}`}
          >
            <span className={hasChildren ? "text-yellow-600/80 mr-0.5" : "text-gray-600 mr-0.5"}>
              {hasChildren ? '📁' : '📄'}
            </span>
            <span className={`truncate ${node.doc ? 'font-medium' : 'italic opacity-70'}`}>
              {node.doc?.title ?? node.name}
            </span>
          </div>
        </div>
        
        {tooltip && node.doc && <DocTooltip doc={node.doc} x={tooltip.x} y={tooltip.y} />}
        
        {open && Array.from(node.children.values())
          .sort((a, b) => {
            if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
            return a.name.localeCompare(b.name);
          })
          .map(child => (
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
  return (
    <FileItem
      doc={doc}
      isSelected={isSelected || false}
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
  const [open, setOpen] = useState(true);
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

// ── 문서 메타데이터 패널 ──────────────────────────────────────────────────

function formatUnixDate(ts: number, includeTime = false): string {
  const date = new Date(ts * 1000);
  if (includeTime) {
    return date.toLocaleString('ko-KR', {
      year: 'numeric', month: 'short', day: 'numeric',
      hour: '2-digit', minute: '2-digit', second: '2-digit'
    });
  }
  return date.toLocaleDateString('ko-KR', {
    year: 'numeric', month: 'short', day: 'numeric',
  });
}

function formatDuration(seconds: number): string {
  if (seconds <= 0) return '만료됨';
  const mins = Math.floor(seconds / 60);
  const hours = Math.floor(mins / 60);
  const days = Math.floor(hours / 24);

  if (days > 0) return `${days}일 남음`;
  if (hours > 0) return `${hours}시간 ${mins % 60}분 남음`;
  if (mins > 0) return `${mins}분 남음`;
  return `${seconds}초 남음`;
}
function DocMetaPanel({
  tags,
  aliases,
  created_at,
  updated_at,
  last_indexed,
  cache_ttl,
  metadata,
  // url,
  source_project_id,
  source_doc_id,
}: PreviewMeta) {
  const displayMeta = Object.entries(metadata).filter(([k]) => k !== 'links');
  
  const now = Math.floor(Date.now() / 1000);
  const remainingSeconds = (last_indexed && cache_ttl && cache_ttl > 0) 
    ? (last_indexed + cache_ttl * 60) - now 
    : null;

  return (
    <div className="flex flex-col gap-5">
      {/* doxus:// 가상 링크 복사 영역 */}
      <div className="bg-indigo-500/10 border border-indigo-500/20 rounded-xl p-3 flex flex-col gap-2">
        <div className="flex items-center justify-between">
          <span className="text-[10px] text-indigo-400 font-bold uppercase tracking-wider">Doxus Link</span>
          <button
            onClick={() => {
              const link = `doxus://${source_project_id}/${source_doc_id}`;
              navigator.clipboard.writeText(link);
              alert('Doxus 링크가 복사되었습니다!');
            }}
            className="text-[10px] bg-indigo-500/20 hover:bg-indigo-500/30 text-indigo-300 px-2 py-1 rounded-lg border border-indigo-500/30 transition-colors"
          >
            Copy URI
          </button>
        </div>
        <div className="text-[11px] text-gray-400 font-mono break-all bg-black/30 p-2 rounded-lg border border-white/5">
          doxus://{source_project_id}/{source_doc_id}
        </div>
      </div>

      {/* 기본 정보 */}
      <div className="grid grid-cols-2 gap-4">
        <div className="bg-white/[0.03] rounded-xl p-3 border border-white/5">
          <span className="text-[10px] text-gray-500 font-bold uppercase tracking-wider block mb-1">Created At</span>
          <span className="text-sm text-gray-300">{created_at ? formatUnixDate(created_at) : 'N/A'}</span>
        </div>
        <div className="bg-white/[0.03] rounded-xl p-3 border border-white/5">
          <span className="text-[10px] text-gray-500 font-bold uppercase tracking-wider block mb-1">Updated At</span>
          <span className="text-sm text-gray-300">{updated_at ? formatUnixDate(updated_at) : 'N/A'}</span>
        </div>
      </div>

      <div className="grid grid-cols-2 gap-4">
        <div className="bg-indigo-500/[0.05] rounded-xl p-3 border border-indigo-500/10">
          <span className="text-[10px] text-indigo-400/60 font-bold uppercase tracking-wider block mb-1">Last Indexed</span>
          <span className="text-sm text-indigo-300">{last_indexed ? formatUnixDate(last_indexed, true) : 'N/A'}</span>
        </div>
        <div className="bg-amber-500/[0.05] rounded-xl p-3 border border-amber-500/10">
          <span className="text-[10px] text-amber-400/60 font-bold uppercase tracking-wider block mb-1">Cache Status</span>
          <span className={`text-sm font-medium ${remainingSeconds != null && remainingSeconds <= 0 ? 'text-rose-400' : 'text-amber-300'}`}>
            {remainingSeconds != null ? formatDuration(remainingSeconds) : (cache_ttl === 0 ? '영구적' : 'N/A')}
          </span>
        </div>
      </div>

      {/* 태그 & 별칭 */}
      {(tags.length > 0 || aliases.length > 0) && (
        <div className="flex flex-col gap-3">
          <span className="text-[10px] text-gray-500 font-bold uppercase tracking-wider">Taxonomy</span>
          <div className="flex flex-wrap gap-2">
            {tags.map(t => (
              <span key={t} className="px-2 py-1 rounded-lg bg-indigo-500/10 text-indigo-400 border border-indigo-500/20 text-xs text-center min-w-[3rem]">
                #{t}
              </span>
            ))}
            {aliases.map(a => (
              <span key={a} className="px-2 py-1 rounded-lg bg-emerald-500/10 text-emerald-400 border border-emerald-500/20 text-xs">
                🏷️ {a}
              </span>
            ))}
          </div>
        </div>
      )}

      {/* 추가 메타데이터 */}
      {displayMeta.length > 0 && (
        <div className="flex flex-col gap-3">
          <span className="text-[10px] text-gray-500 font-bold uppercase tracking-wider">Extended Properties</span>
          <div className="space-y-2">
            {displayMeta.map(([k, v]) => (
              <div key={k} className="flex items-center justify-between text-xs py-1 border-b border-white/5">
                <span className="text-gray-500">{k}</span>
                <span className="text-gray-300">{String(v)}</span>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

// ── Markdown Preview ──────────────────────────────────────────────────────

function MarkdownPreview({ content }: { content: string }) {
  return (
    <div className="prose prose-invert max-w-none text-gray-300 prose-headings:text-white prose-a:text-indigo-400 prose-code:text-indigo-300 prose-pre:bg-black/50 prose-pre:border prose-pre:border-white/10">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[rehypeRaw]}
        components={{
          a: ({ ...props }) => <a {...props} target="_blank" rel="noopener noreferrer" />,
        }}
      >
        {content}
      </ReactMarkdown>
    </div>
  );
}

// ── 필터 고도화 패널 ──────────────────────────────────────────────────────

function AdvancedSearchPanel({
  filters,
  availablePlugins,
  availableProjects,
  onChange,
}: {
  filters: SearchFilters;
  availablePlugins: { id: string; label: string; icon: string }[];
  availableProjects: string[];
  onChange: (f: Partial<SearchFilters>) => void;
}) {
  return (
    <div className="glass-card border-white/10 rounded-2xl px-5 py-4 flex flex-col gap-4 shrink-0 shadow-2xl animate-in fade-in zoom-in-95 duration-200">
      <div className="flex flex-col gap-1.5">
        <span className="text-[10px] text-gray-500 font-bold uppercase tracking-widest">Plugins</span>
        <div className="flex flex-wrap gap-2">
          {availablePlugins.map((p) => {
            const isActive = filters.sourceTypes.includes(p.id);
            return (
              <button
                key={p.id}
                type="button"
                onClick={() => {
                  const val = isActive
                    ? filters.sourceTypes.filter((v) => v !== p.id)
                    : [...filters.sourceTypes, p.id];
                  onChange({ sourceTypes: val });
                }}
                className={`flex items-center gap-1.5 px-3 py-1.5 rounded-xl border transition-all text-xs font-medium ${
                  isActive
                    ? 'bg-indigo-500/20 border-indigo-500/50 text-indigo-300 shadow-lg shadow-indigo-500/10'
                    : 'bg-white/[0.03] border-white/5 text-gray-500 hover:border-white/20'
                }`}
              >
                <span>{p.icon}</span>
                <span>{p.label}</span>
              </button>
            );
          })}
        </div>
      </div>

      <div className="flex flex-col gap-1.5">
        <span className="text-[10px] text-gray-500 font-bold uppercase tracking-widest">Projects</span>
        <div className="flex flex-wrap gap-2">
          {availableProjects.map((projectName) => {
            const isActive = filters.projectNames.includes(projectName);
            return (
              <button
                key={projectName}
                type="button"
                onClick={() => {
                  const val = isActive
                    ? filters.projectNames.filter((v) => v !== projectName)
                    : [...filters.projectNames, projectName];
                  onChange({ projectNames: val });
                }}
                className={`px-3 py-1.5 rounded-xl border transition-all text-xs font-medium ${
                  isActive
                    ? 'bg-emerald-500/20 border-emerald-500/50 text-emerald-300 shadow-lg shadow-emerald-500/10'
                    : 'bg-white/[0.03] border-white/5 text-gray-500 hover:border-white/20'
                }`}
              >
                {projectName}
              </button>
            );
          })}
        </div>
      </div>

      <div className="flex flex-col gap-1.5">
        <span className="text-[10px] text-gray-500 font-bold uppercase tracking-widest">Tags</span>
        <input
          type="text"
          placeholder="#태그이름"
          value={filters.tagQuery}
          onChange={(e) => onChange({ tagQuery: e.target.value })}
          className="bg-white/[0.03] border border-white/5 rounded-xl px-4 py-2 text-xs text-white placeholder-gray-600 focus:outline-none focus:border-indigo-500/50 transition-colors"
        />
      </div>

      {(filters.sourceTypes.length > 0 || filters.projectNames.length > 0 || filters.tagQuery) && (
        <div className="flex items-center gap-2 pt-1 border-t border-gray-800">
          <span className="text-xs text-gray-600">적용 중:</span>
          {filters.sourceTypes.map((t) => (
            <span key={t} className="text-xs text-indigo-400 bg-indigo-950 px-2 py-0.5 rounded-full">{t}</span>
          ))}
          {filters.projectNames.map((n) => (
            <span key={n} className="text-xs text-emerald-400 bg-emerald-950 px-2 py-0.5 rounded-full">{n}</span>
          ))}
          {filters.tagQuery && (
            <span className="text-xs text-yellow-400 bg-yellow-950 px-2 py-0.5 rounded-full">{filters.tagQuery}</span>
          )}
          <button
            type="button"
            onClick={() => onChange({ sourceTypes: [], projectNames: [], tagQuery: '' })}
            className="ml-auto text-xs text-gray-600 hover:text-gray-300 transition-colors"
          >
            초기화
          </button>
        </div>
      )}
    </div>
  );
}

// ── SearchPage ────────────────────────────────────────────────────────────

export function SearchPage() {
  const [searchParams, setSearchParams] = useSearchParams();
  const { query, filters, hits, isLoading, error, setQuery, setFilters, search, clear, allDocuments, allDocsLoading, listAllDocuments, updateDocumentMetadata } = useSearchStore();
  usePluginStore((s) => s.emojiMap);
  const [inputValue, setInputValue] = useState(query);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [selectedDoc, setSelectedDoc] = useState<DocEntry | null>(null);
  const [previewContent, setPreviewContent] = useState<string | null>(null);
  const [previewMeta, setPreviewMeta] = useState<PreviewMeta | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [refreshToast, setRefreshToast] = useState<string | null>(null);
  const refreshToastTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const processedDocIdRef = useRef<string | null>(null);

  const allDocsRef = useRef(allDocuments);
  useEffect(() => { allDocsRef.current = allDocuments; }, [allDocuments]);

  useEffect(() => { listAllDocuments(); }, [listAllDocuments]);

  // 1. 초기 로드 시 또는 데이터 준비 시 URL 기반 자동 선택
  const [initialSyncDone, setInitialSyncDone] = useState(false);
  useEffect(() => {
    if (initialSyncDone || allDocsLoading || allDocuments.length === 0) return;

    const docId = searchParams.get('docId');
    if (docId) {
      const numericId = parseInt(docId, 10);
      const doc = allDocuments.find(d => d.document_id === numericId);
      if (doc) {
        processedDocIdRef.current = docId;
        handleSelectDoc(allDocToEntry(doc));
        setInitialSyncDone(true);
      }
    } else {
      setInitialSyncDone(true);
    }
  }, [allDocuments, allDocsLoading, initialSyncDone]);

  // 2. 브라우저 내비게이션(URL 변경) 대응
  useEffect(() => {
    const docId = searchParams.get('docId');
    const tag = searchParams.get('tag');

    // 이미 처리된 URL이면 무시
    if (docId === processedDocIdRef.current) return;

    if (docId) {
      const numericId = parseInt(docId, 10);
      // allDocsRef를 사용하여 스토어 업데이트에 의한 재실행을 방지
      const doc = allDocsRef.current.find(d => d.document_id === numericId);
      if (doc) {
        processedDocIdRef.current = docId;
        handleSelectDoc(allDocToEntry(doc));
      }
    } else if (tag) {
      setFilters({ tagQuery: `#${tag}` });
      search();
      processedDocIdRef.current = null;
    } else {
      // URL에서 docId가 사라진 경우 (초기화)
      setSelectedDoc(null);
      setPreviewContent(null);
      setPreviewMeta(null);
      processedDocIdRef.current = null;
    }
  }, [searchParams]); // 오직 searchParams 변경에만 반응

  const availablePlugins = (() => {
    const seen = new Set<string>();
    return allDocuments.reduce<{ id: string; label: string; icon: string }[]>((acc, d) => {
      const short = d.source_type.replace(/^com\.doxus\./, '');
      if (!seen.has(short)) {
        seen.add(short);
        const pluginId = `com.doxus.${short}`;
        acc.push({ id: short, label: short.charAt(0).toUpperCase() + short.slice(1), icon: usePluginStore.getState().getEmoji(pluginId) });
      }
      return acc;
    }, []);
  })();

  const availableProjects = (() => {
    const seen = new Set<string>();
    return allDocuments.reduce<string[]>((acc, d) => {
      if (d.project_name && !seen.has(d.project_name)) { seen.add(d.project_name); acc.push(d.project_name); }
      return acc;
    }, []);
  })();

  const activeFilterCount = filters.sourceTypes.length + filters.projectNames.length + (filters.tagQuery ? 1 : 0);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    setQuery(inputValue);
    setSelectedDoc(null);
    setPreviewContent(null);
    search();
  };

  const [resetKey, setResetKey] = useState(0);

  const handleClear = () => {
    clear();
    setSelectedDoc(null);
    setPreviewContent(null);
    setPreviewError(null);
    setInputValue('');
    setResetKey(prev => prev + 1);
  };

  const fetchPreview = async (doc: DocEntry, forceRefresh = false) => {
    const identifier = doc.source_doc_id;
    if (!identifier) return;
    setPreviewLoading(true);
    setPreviewError(null);
    try {
      const result = await invoke<{
        content: string; from_cache?: boolean;
        tags?: string[]; aliases?: string[];
        created_at?: number | null; updated_at?: number | null;
        last_indexed?: number | null; cache_ttl?: number | null;
        metadata?: Record<string, unknown>;
        url?: string | null;
        source_project_id?: string;
        source_doc_id?: string;
      }>('get_document_content', {
        filePath: identifier,
        projectName: doc.project_name || undefined,
        forceRefresh,
      });
      setPreviewContent(stripFrontmatter(result.content));
      setPreviewMeta({
        tags: result.tags ?? [],
        aliases: result.aliases ?? [],
        created_at: result.created_at ?? null,
        updated_at: result.updated_at ?? null,
        last_indexed: result.last_indexed ?? doc.last_indexed ?? null,
        cache_ttl: result.cache_ttl ?? doc.cache_ttl ?? null,
        metadata: result.metadata ?? {},
        url: result.url ?? doc.url ?? null,
        source_project_id: result.source_project_id ?? doc.source_project_id ?? '',
        source_doc_id: result.source_doc_id ?? doc.source_doc_id ?? '',
      });

      // 전역 스토어 상태 동기화 (리스트 툴팁 등 갱신)
      updateDocumentMetadata(identifier, {
        tags: result.tags,
        updated_at: result.updated_at ?? undefined,
        last_indexed: result.last_indexed ?? undefined,
        cache_ttl: result.cache_ttl ?? undefined,
      });

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
    setPreviewMeta(null);
    setPreviewError(null);
    
    // URL 파라미터 업데이트 (새로고침 시 상태 유지용)
    const newId = doc.document_id.toString();
    processedDocIdRef.current = newId; 
    setSearchParams({ docId: newId }, { replace: true });

    if (doc.document_id) {
      invoke('increment_view_count', { documentId: doc.document_id }).catch(() => {});
    }
    await fetchPreview(doc);
  };

  const handleRefresh = () => {
    if (selectedDoc) fetchPreview(selectedDoc, true);
  };

  const hasSearch = query.trim().length > 0;

  const groupedEntries = (() => {
    const entries: DocEntry[] = hasSearch
      ? hits.map(hitToEntry)
      : allDocuments.map(allDocToEntry);

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

  return (
    <div className="flex flex-col h-full gap-3">
      {/* 검색 바 영역 */}
      <div className="flex flex-col gap-2 shrink-0">
        <form onSubmit={handleSubmit} className="flex gap-2 items-center">
          <div className="relative flex-1 group">
            <input
              type="text"
              placeholder="궁금한 지식을 검색하세요..."
              value={inputValue}
              onChange={(e) => setInputValue(e.target.value)}
              className="w-full bg-white/[0.03] border border-white/5 rounded-2xl px-12 py-3.5 text-sm text-white focus:outline-none focus:ring-2 focus:ring-indigo-500/30 focus:border-indigo-500/50 transition-all placeholder-gray-600 group-hover:bg-white/[0.05]"
            />
            <div className="absolute left-4 top-1/2 -translate-y-1/2 text-gray-500 group-focus-within:text-indigo-400 transition-colors">
              <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/></svg>
            </div>
          </div>
          <button
            type="button"
            onClick={() => setAdvancedOpen(!advancedOpen)}
            className={`flex items-center gap-2 px-5 py-3.5 rounded-2xl border transition-all text-sm font-semibold ${
              advancedOpen || activeFilterCount > 0
                ? 'bg-indigo-500/10 border-indigo-500/30 text-indigo-400'
                : 'bg-white/[0.03] border-white/5 text-gray-400 hover:border-white/10'
            }`}
          >
            <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round"><polygon points="22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3"/></svg>
            <span>필터</span>
            {activeFilterCount > 0 && (
              <span className="flex items-center justify-center bg-indigo-500 text-white text-[10px] w-4 h-4 rounded-full">
                {activeFilterCount}
              </span>
            )}
          </button>
          <button
            type="submit"
            disabled={isLoading || !inputValue.trim()}
            className="px-8 py-3.5 bg-indigo-600 hover:bg-indigo-500 disabled:bg-gray-800 disabled:text-gray-600 text-white rounded-2xl font-bold text-sm shadow-xl shadow-indigo-500/20 transition-all active:scale-95 flex items-center gap-2"
          >
            {isLoading ? 'Searching...' : 'Search'}
          </button>
          {(hits.length > 0 || query) && (
            <button
              type="button"
              onClick={handleClear}
              className="px-4 py-3 text-gray-500 hover:text-gray-300 text-sm font-medium transition-colors"
            >
              Clear
            </button>
          )}
        </form>

        {/* 고급 검색 패널 */}
        {advancedOpen && (
          <AdvancedSearchPanel
            filters={filters}
            availablePlugins={availablePlugins}
            availableProjects={availableProjects}
            onChange={setFilters}
          />
        )}
      </div>

      {error && (
        <div className="p-3 bg-red-950 border border-red-800 rounded-lg text-red-400 text-sm shrink-0">
          {error}
        </div>
      )}

      {/* 하단 2-panel */}
      <div className="flex-1 overflow-hidden flex gap-4">
        {/* 좌측: 파일 목록 */}
        <div 
          key={resetKey}
          className="w-80 shrink-0 overflow-auto glass-card border-white/5 rounded-2xl flex flex-col shadow-inner"
        >
          {/* 헤더 */}
          <div className="p-4 flex items-center justify-between border-b border-white/5 bg-white/[0.02]">
            <span className="text-[10px] text-gray-500 font-bold uppercase tracking-[0.2em]">
              {hasSearch ? `Search Results` : `All Knowledge`}
            </span>
            {(isLoading || allDocsLoading) ? (
              <div className="w-3 h-3 border-2 border-indigo-500 border-t-transparent rounded-full animate-spin" />
            ) : (
              <span className="text-[10px] text-indigo-500/60 font-mono">
                {hasSearch ? hits.length : allDocuments.length} items
              </span>
            )}
          </div>
          {/* 본문 */}
          <div className="p-2">
            {Array.from(groupedEntries.entries()).map(([projectName, group]) => (
              <ProjectGroup
                key={projectName}
                projectName={projectName}
                sourceType={group.sourceType}
                docs={group.docs}
                selectedDoc={selectedDoc}
                onSelect={handleSelectDoc}
              />
            ))}
          </div>
        </div>

        {/* 우측: 프리뷰 패널 */}
        <div className="flex-1 bg-gray-900 border border-gray-800 rounded-xl flex flex-col overflow-hidden">
          {selectedDoc ? (
            <>
              {/* 프리뷰 헤더 */}
              <div className="px-6 py-5 border-b border-white/5 bg-white/[0.01]">
                <div className="flex flex-col gap-1 mb-4">
                  <div className="flex items-center gap-2">
                    <span className="px-2 py-0.5 rounded bg-indigo-500/10 text-indigo-400 text-[10px] font-bold tracking-wider uppercase border border-indigo-500/20">
                      {selectedDoc.source_type.replace(/^com\.doxus\./, '')}
                    </span>
                    <span className="text-gray-500 text-xs truncate max-w-[200px]">
                      {selectedDoc.project_name}
                    </span>
                  </div>
                  <h2 className="text-xl font-bold text-white tracking-tight">{selectedDoc.title}</h2>
                  {selectedDoc.heading_path && (
                    <div className="flex items-center gap-2 mt-1">
                      <span className="text-[10px] text-gray-600 font-bold uppercase tracking-wider">Section</span>
                      <span className="text-xs text-indigo-300 opacity-80">{selectedDoc.heading_path}</span>
                    </div>
                  )}
                </div>
                <div className="flex items-center gap-2">
                  <button
                    onClick={() => {
                      const finalUrl = previewMeta?.url || selectedDoc.url;
                      if (finalUrl) {
                        invoke('plugin_open_url', { url: finalUrl });
                      } else {
                        invoke('open_file_in_editor', { filePath: selectedDoc.source_doc_id, projectName: selectedDoc.project_name });
                      }
                    }}
                    className="flex items-center gap-2 px-4 py-2 bg-indigo-600 hover:bg-indigo-500 text-white rounded-xl text-xs font-bold transition-all shadow-lg shadow-indigo-500/20"
                  >
                    <span>원문 열기</span>
                    <span className="opacity-70">↗</span>
                  </button>
                  <button
                    onClick={handleRefresh}
                    className="p-2 text-gray-500 hover:text-white hover:bg-white/5 rounded-lg transition-colors"
                    title="콘텐츠 새로고침"
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" className={previewLoading ? 'animate-spin' : ''}>
                      <path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" />
                      <path d="M21 3v5h-5" />
                      <path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" />
                      <path d="M8 16H3v5" />
                    </svg>
                  </button>
                  <button
                    onClick={() => setSelectedDoc(null)}
                    className="ml-auto text-xs text-gray-500 hover:text-gray-300"
                  >
                    닫기
                  </button>
                </div>
              </div>

              {/* 프리뷰 본문 */}
              <div className="flex-1 overflow-auto p-5">
                {previewLoading ? (
                  <div className="flex flex-col items-center justify-center h-full gap-4 text-gray-500">
                    <div className="w-8 h-8 border-4 border-indigo-500 border-t-transparent rounded-full animate-spin" />
                    <p className="text-sm font-medium">콘텐츠를 불러오는 중...</p>
                  </div>
                ) : previewError ? (
                  <div className="flex flex-col items-center justify-center h-full p-8 text-center gap-4">
                    <p className="text-red-400 text-sm font-medium">미리보기를 불러올 수 없습니다.</p>
                    <p className="text-gray-600 text-xs max-w-sm">{previewError}</p>
                    <MarkdownPreview content={selectedDoc.snippet || ''} />
                  </div>
                ) : (
                  <div className="flex flex-col gap-8">
                    {previewMeta && (
                      <DocMetaPanel
                        tags={previewMeta.tags}
                        aliases={previewMeta.aliases}
                        created_at={previewMeta.created_at}
                        updated_at={previewMeta.updated_at}
                        last_indexed={previewMeta.last_indexed}
                        cache_ttl={previewMeta.cache_ttl}
                        metadata={previewMeta.metadata}
                        url={previewMeta.url}
                        source_project_id={previewMeta.source_project_id}
                        source_doc_id={previewMeta.source_doc_id}
                      />
                    )}
                    
                    {previewContent ? (
                      <div className="pt-4 border-t border-white/5">
                        <MarkdownPreview content={previewContent} />
                      </div>
                    ) : (
                      <div className="bg-indigo-500/5 border border-indigo-500/10 rounded-2xl p-4">
                        <span className="text-[10px] text-gray-500 font-bold uppercase tracking-widest block mb-1">Snippet Preview</span>
                        <MarkdownPreview content={selectedDoc.snippet || ''} />
                      </div>
                    )}
                  </div>
                )}
              </div>
            </>
          ) : (
            /* 선택 없을 때 empty state */
            <div className="flex flex-col items-center justify-center h-full gap-3 text-center px-8">
              <div className="w-16 h-16 bg-white/[0.02] rounded-3xl flex items-center justify-center mb-2 border border-white/5">
                <span className="text-4xl opacity-20">📂</span>
              </div>
              <p className="text-sm font-bold text-gray-400">선택된 문서가 없습니다</p>
              <p className="text-xs text-gray-600 leading-relaxed max-w-[240px]">
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
