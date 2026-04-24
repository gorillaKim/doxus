import React from 'react';
import ReactMarkdown from 'react-markdown';
import rehypeRaw from 'rehype-raw';
import remarkGfm from 'remark-gfm';
import { invoke } from '@tauri-apps/api/core';
import { DocEntry } from './SearchTree';

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

interface SearchPreviewProps {
  selectedDoc: DocEntry | null;
  previewContent: string | null;
  previewMeta: PreviewMeta | null;
  previewLoading: boolean;
  previewError: string | null;
  onRefresh: () => void;
  onClose: () => void;
  onTagClick: (tag: string) => void;
}

// ── SUB-COMPONENTS ──────────────────────────────────────────────────────────

const formatUnixDate = (ts: number, includeTime = false) => {
  const date = new Date(ts * 1000);
  return includeTime 
    ? date.toLocaleString('ko-KR', { year: 'numeric', month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })
    : date.toLocaleDateString('ko-KR', { year: 'numeric', month: 'short', day: 'numeric' });
};

const formatDuration = (seconds: number) => {
  if (seconds <= 0) return '만료됨';
  const mins = Math.floor(seconds / 60);
  if (mins < 60) return `${mins}분 남음`;
  const hours = Math.floor(mins / 60);
  return hours < 24 ? `${hours}시간 남음` : `${Math.floor(hours / 24)}일 남음`;
};

const DocMetaPanel: React.FC<PreviewMeta & { onTagClick: (t: string) => void }> = (props) => {
  const displayMeta = Object.entries(props.metadata).filter(([k]) => k !== 'links' && k !== 'tags' && k !== 'aliases');
  const now = Math.floor(Date.now() / 1000);
  const remaining = (props.last_indexed && props.cache_ttl) ? (props.last_indexed + props.cache_ttl * 60) - now : null;

  return (
    <div className="flex flex-col gap-6 mb-10">
      {/* URI & Copy */}
      <div className="bg-indigo-500/5 border border-indigo-500/10 rounded-2xl p-4 flex flex-col gap-3 group">
        <div className="flex items-center justify-between">
          <span className="text-[9px] text-indigo-400 font-black uppercase tracking-widest px-2 py-0.5 rounded bg-indigo-500/10">Reference URI</span>
          <button
            onClick={() => {
              navigator.clipboard.writeText(`doxus://${props.source_project_id}/${props.source_doc_id}`);
            }}
            className="text-[10px] text-gray-500 hover:text-white transition-colors"
          >
            Copy Link
          </button>
        </div>
        <div className="text-[11px] text-gray-400 font-mono break-all py-1.5 px-3 bg-black/20 rounded-xl border border-white/5 group-hover:border-indigo-500/20 transition-colors">
          doxus://{props.source_project_id}/{props.source_doc_id}
        </div>
      </div>

      <div className="grid grid-cols-2 gap-4">
        <div className="bg-white/[0.02] border border-white/5 rounded-2xl p-4 transition-colors hover:bg-white/[0.04]">
          <span className="text-[9px] text-gray-600 font-black uppercase tracking-widest block mb-1">Creation</span>
          <span className="text-sm text-gray-300 font-medium">{props.created_at ? formatUnixDate(props.created_at) : 'N/A'}</span>
        </div>
        <div className="bg-white/[0.02] border border-white/5 rounded-2xl p-4 transition-colors hover:bg-white/[0.04]">
          <span className="text-[9px] text-gray-600 font-black uppercase tracking-widest block mb-1">Last Update</span>
          <span className="text-sm text-gray-300 font-medium">{props.updated_at ? formatUnixDate(props.updated_at) : 'N/A'}</span>
        </div>
        <div className="bg-indigo-500/[0.03] border border-indigo-500/10 rounded-2xl p-4">
          <span className="text-[9px] text-indigo-400 font-black uppercase tracking-widest block mb-1">Indexing</span>
          <span className="text-sm text-indigo-300 font-medium">{props.last_indexed ? formatUnixDate(props.last_indexed, true) : 'N/A'}</span>
        </div>
        <div className="bg-amber-500/[0.03] border border-amber-500/10 rounded-2xl p-4">
          <span className="text-[9px] text-amber-400 font-black uppercase tracking-widest block mb-1">Cache Status</span>
          <span className={`text-sm font-bold ${remaining != null && remaining <= 0 ? 'text-red-400' : 'text-amber-400'}`}>
            {remaining != null ? formatDuration(remaining) : (props.cache_ttl === 0 ? 'Permanent' : 'N/A')}
          </span>
        </div>
      </div>

      {/* Tags Display */}
      {props.tags.length > 0 && (
        <div className="flex flex-col gap-3">
          <span className="text-[9px] text-gray-600 font-black uppercase tracking-widest px-1">Tags</span>
          <div className="flex flex-wrap gap-2">
            {props.tags.map(tag => (
              <button
                key={tag}
                onClick={() => props.onTagClick(tag)}
                className="px-2.5 py-1 rounded-lg bg-indigo-500/10 text-indigo-400 border border-indigo-500/10 text-[11px] font-bold hover:bg-indigo-500/20 hover:border-indigo-500/30 transition-all active:scale-95"
              >
                #{tag}
              </button>
            ))}
          </div>
        </div>
      )}

      {displayMeta.length > 0 && (
        <div className="flex flex-col gap-3">
          <span className="text-[9px] text-gray-600 font-black uppercase tracking-widest px-1">Extended Properties</span>
          <div className="grid grid-cols-1 gap-2">
            {displayMeta.map(([k, v]) => (
              <div key={k} className="flex items-center justify-between py-2.5 px-4 bg-white/[0.01] rounded-xl border border-white/5 text-[11px]">
                <span className="text-gray-500 font-medium uppercase tracking-tighter">{k}</span>
                <span className="text-gray-300 font-mono">{String(v)}</span>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
};

export const SearchPreview: React.FC<SearchPreviewProps> = ({
  selectedDoc,
  previewContent,
  previewMeta,
  previewLoading,
  previewError,
  onRefresh,
  onClose,
  onTagClick
}) => {
  if (!selectedDoc) {
    return (
      <div className="flex-1 h-full flex flex-col items-center justify-center text-center px-10 gap-6 animate-in fade-in duration-700 bg-gray-950/20">
        <div className="w-24 h-24 bg-white/[0.02] border border-white/5 rounded-[2.5rem] flex items-center justify-center shadow-inner relative overflow-hidden">
            <div className="absolute inset-0 bg-indigo-500/5 blur-2xl" />
            <span className="text-5xl opacity-20 relative z-10">📂</span>
        </div>
        <div className="flex flex-col gap-2">
          <h3 className="text-lg font-bold text-gray-400 tracking-tight">선택된 문서가 없습니다</h3>
          <p className="text-xs text-gray-600 leading-relaxed max-w-[280px] mx-auto">
            좌측 라이브러리에서 문서를 클릭하여 내용을 확인하거나 검색 결과를 탐색해보세요.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="flex-1 h-full flex flex-col bg-gray-950/40 overflow-hidden relative">
      {/* Background Decor */}
      <div className="absolute top-0 right-0 w-[500px] h-[500px] bg-indigo-500/[0.02] blur-[120px] rounded-full -translate-y-1/2 translate-x-1/2 pointer-events-none" />

      {/* Header */}
      <div className="px-8 py-10 border-b border-white/5 relative z-10">
        <div className="flex flex-col gap-2 mb-8">
            <div className="flex items-center gap-3">
                <span className="px-3 py-1 rounded-xl bg-indigo-500/10 text-indigo-400 text-[10px] font-black uppercase tracking-[0.1em] border border-indigo-500/10 shadow-lg shadow-indigo-500/5">
                    {selectedDoc.source_type.replace(/^com\.doxus\./, '')}
                </span>
                <span className="text-[10px] text-gray-600 font-bold uppercase tracking-widest truncate max-w-[300px]">
                    {selectedDoc.project_name}
                </span>
                <button onClick={onClose} className="ml-auto p-2 text-gray-600 hover:text-white transition-colors">✕</button>
            </div>
            <h2 className="text-3xl font-black text-white tracking-tighter leading-none py-2">{selectedDoc.title}</h2>
            {selectedDoc.heading_path && (
                <div className="flex items-center gap-2 mt-1">
                    <span className="text-[10px] text-indigo-500 font-black uppercase tracking-widest opacity-60">Section</span>
                    <span className="text-xs text-indigo-300/80 font-medium">{selectedDoc.heading_path}</span>
                </div>
            )}
        </div>

        <div className="flex items-center gap-4">
            <button
                onClick={() => {
                    const finalUrl = previewMeta?.url || selectedDoc.url;
                    if (finalUrl) { invoke('plugin_open_url', { url: finalUrl }); }
                    else { invoke('open_file_in_editor', { filePath: selectedDoc.source_doc_id, projectName: selectedDoc.project_name }); }
                }}
                className="px-8 py-3.5 bg-white text-gray-950 rounded-2xl text-xs font-black uppercase transition-all duration-300 hover:scale-[1.02] active:scale-95 shadow-xl shadow-white/5"
            >
                원문 열기 ↗
            </button>
            <button
                onClick={onRefresh}
                className={`p-3.5 bg-white/5 hover:bg-white/10 text-gray-400 hover:text-white rounded-2xl border border-white/5 transition-all ${previewLoading ? 'opacity-50' : ''}`}
                title="새로고침"
                disabled={previewLoading}
            >
               <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round" className={previewLoading ? 'animate-spin' : ''}>
                  <path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" /><path d="M21 3v5h-5" /><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" /><path d="M8 16H3v5" />
               </svg>
            </button>
        </div>
      </div>

      {/* Content Container */}
      <div className="flex-1 overflow-y-auto no-scrollbar p-8 relative z-10">
        <div className="max-w-4xl mx-auto flex flex-col pb-40">
            {/* Show Meta Panel: Prefer previewMeta, but fallback to selectedDoc for tags & basic info while loading */}
            {previewMeta ? (
                <DocMetaPanel {...previewMeta} onTagClick={onTagClick} />
            ) : selectedDoc ? (
                <div className="flex flex-col gap-6 mb-10 opacity-60 grayscale-[0.5] pointer-events-none">
                    {/* Simplified Loading Meta Panel */}
                    <div className="grid grid-cols-2 gap-4">
                        {selectedDoc.updated_at && (
                            <div className="bg-white/[0.02] border border-white/5 rounded-2xl p-4">
                                <span className="text-[9px] text-gray-600 font-black uppercase tracking-widest block mb-1">Last Update</span>
                                <span className="text-sm text-gray-300 font-medium">{formatUnixDate(selectedDoc.updated_at)}</span>
                            </div>
                        )}
                        {selectedDoc.last_indexed && (
                            <div className="bg-indigo-500/[0.03] border border-indigo-500/10 rounded-2xl p-4">
                                <span className="text-[9px] text-indigo-400 font-black uppercase tracking-widest block mb-1">Indexing</span>
                                <span className="text-sm text-indigo-300 font-medium">{formatUnixDate(selectedDoc.last_indexed, true)}</span>
                            </div>
                        )}
                    </div>
                    {selectedDoc.tags && selectedDoc.tags.length > 0 && (
                        <div className="flex flex-col gap-3">
                            <span className="text-[9px] text-gray-600 font-black uppercase tracking-widest px-1">Tags</span>
                            <div className="flex flex-wrap gap-2">
                                {selectedDoc.tags.map(tag => (
                                    <span key={tag} className="px-2.5 py-1 rounded-lg bg-indigo-500/10 text-indigo-400 border border-indigo-500/10 text-[11px] font-bold">
                                        #{tag}
                                    </span>
                                ))}
                            </div>
                        </div>
                    )}
                </div>
            ) : null}

            {previewLoading ? (
                <div className="flex flex-col items-center justify-center h-64 gap-4 text-gray-500 animate-in fade-in duration-500">
                    <div className="w-10 h-10 border-4 border-indigo-500 border-t-transparent rounded-full animate-spin" />
                    <p className="text-xs font-black uppercase tracking-widest opacity-50">Loading Content</p>
                </div>
            ) : previewError ? (
                <div className="flex flex-col gap-6 max-w-2xl mx-auto py-10">
                    <div className="p-6 bg-red-500/5 border border-red-500/10 rounded-3xl text-center">
                        <p className="text-red-400 text-sm font-bold">오류: 미리보기를 로드할 수 없습니다</p>
                        <p className="text-gray-600 text-[10px] mt-2 font-mono uppercase tracking-tighter">{previewError}</p>
                    </div>
                    <div className="opacity-40 grayscale blur-[0.5px] scale-95 pointer-events-none">
                        <ReactMarkdown className="prose prose-invert max-w-none" remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeRaw]}>
                            {selectedDoc.snippet || '내용 없음'}
                        </ReactMarkdown>
                    </div>
                </div>
            ) : (
                <div className="prose prose-invert max-w-none text-gray-400 prose-headings:text-white prose-headings:tracking-tighter prose-a:text-indigo-400 prose-code:text-indigo-300 prose-code:bg-white/5 prose-code:px-1.5 prose-code:py-0.5 prose-code:rounded-lg prose-pre:bg-black/40 prose-pre:border prose-pre:border-white/5 prose-pre:rounded-2xl prose-img:rounded-3xl shadow-2xl">
                    <ReactMarkdown 
                        remarkPlugins={[remarkGfm]} 
                        rehypePlugins={[rehypeRaw]}
                        components={{ a: ({ ...props }) => <a {...props} target="_blank" rel="noopener noreferrer" /> }}
                    >
                        {previewContent || selectedDoc.snippet || ''}
                    </ReactMarkdown>
                </div>
            )}
        </div>
      </div>
    </div>
  );
};
