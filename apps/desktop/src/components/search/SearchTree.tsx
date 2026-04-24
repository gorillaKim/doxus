import React, { useState, useRef } from 'react';
import { createPortal } from 'react-dom';
import { usePluginStore } from '../../stores/usePluginStore';

export interface DocEntry {
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
  heading_path?: string | null;
  tags?: string[];
  updated_at?: number;
  last_indexed?: number;
  cache_ttl?: number;
  metadata?: Record<string, any>;
  url?: string | null;
  source_project_id: string;
  freshness_score?: number;
  retention_tier?: string;
}

// ── UTILS ──────────────────────────────────────────────────────────────────
function formatUnixDate(ts: number, includeTime = false): string {
  const date = new Date(ts * 1000);
  return includeTime 
    ? date.toLocaleString('ko-KR', { year: 'numeric', month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })
    : date.toLocaleDateString('ko-KR', { year: 'numeric', month: 'short', day: 'numeric' });
}

function formatDuration(seconds: number): string {
  if (seconds <= 0) return '만료됨';
  const mins = Math.floor(seconds / 60);
  if (mins < 60) return `${mins}분 남음`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}시간 남음`;
  return `${Math.floor(hours / 24)}일 남음`;
}

// ── TOOLTIP ────────────────────────────────────────────────────────────────
function DocTooltip({ doc, x, y }: { doc: DocEntry; x: number; y: number }) {
  const getEmoji = usePluginStore(s => s.getEmoji);
  const pluginIcon = (st: string) => getEmoji(`com.doxus.${st.replace(/^com\.doxus\./, '')}`);

  return createPortal(
    <div
      className="fixed z-[100] bg-gray-900/90 border border-white/10 rounded-2xl shadow-2xl p-4 w-72 text-xs backdrop-blur-xl pointer-events-none animate-in fade-in zoom-in-95 duration-200"
      style={{ left: Math.min(x + 15, window.innerWidth - 300), top: Math.min(y, window.innerHeight - 250) }}
    >
      <div className="flex items-center justify-between mb-3">
        <span className="text-[9px] text-indigo-400 font-black uppercase tracking-widest bg-indigo-500/10 px-2 py-0.5 rounded">Metadata</span>
        {doc.score != null && <span className="text-gray-500 font-mono">{doc.score.toFixed(3)}</span>}
      </div>
      <h4 className="text-white font-bold text-sm mb-1 leading-tight">{doc.title}</h4>
      <p className="text-[10px] text-gray-500 font-mono mb-4 truncate">{doc.source_doc_id}</p>
      
      <div className="space-y-2.5">
        <div className="flex items-center justify-between">
          <span className="text-gray-600">Project</span>
          <span className="text-gray-300 font-medium">{pluginIcon(doc.source_type)} {doc.project_name}</span>
        </div>
        {doc.updated_at && (
          <div className="flex items-center justify-between">
            <span className="text-gray-600">Updated</span>
            <span className="text-gray-400">{formatUnixDate(doc.updated_at)}</span>
          </div>
        )}
        {doc.last_indexed && doc.cache_ttl && doc.cache_ttl > 0 && (
          <div className="flex items-center justify-between">
            <span className="text-gray-600">Cache</span>
            <span className="text-amber-400/80">
              {formatDuration((doc.last_indexed + doc.cache_ttl * 60) - Math.floor(Date.now() / 1000))}
            </span>
          </div>
        )}
      </div>

      {doc.tags && doc.tags.length > 0 && (
        <div className="mt-4 pt-3 border-t border-white/5 flex flex-wrap gap-1.5">
          {doc.tags.map(tag => (
            <span key={tag} className="text-[10px] text-indigo-400/70">#{tag}</span>
          ))}
        </div>
      )}
    </div>,
    document.body
  );
}

function useTooltip() {
  const [tooltip, setTooltip] = useState<{ x: number; y: number } | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const onMouseEnter = (e: React.MouseEvent) => {
    const { clientX, clientY } = e;
    timerRef.current = setTimeout(() => setTooltip({ x: clientX, y: clientY }), 800);
  };
  const onMouseLeave = () => { if (timerRef.current) clearTimeout(timerRef.current); setTooltip(null); };
  const onMouseMove = (e: React.MouseEvent) => { if (tooltip) setTooltip({ x: e.clientX, y: e.clientY }); };
  return { tooltip, onMouseEnter, onMouseLeave, onMouseMove };
}

// ── TREE LOGIC ─────────────────────────────────────────────────────────────
interface TreeNode {
  name: string;
  isDir: boolean;
  children: Map<string, TreeNode>;
  doc?: DocEntry;
}

export function buildTree(docs: DocEntry[]): TreeNode {
  const root: TreeNode = { name: '', isDir: true, children: new Map() };
  for (const doc of docs) {
    const parts = (doc.hierarchy_path || doc.title).split('/').filter(Boolean);
    let node = root;
    for (let i = 0; i < parts.length; i++) {
        let part = parts[i];
        if (part.toLowerCase().endsWith('.md')) part = part.slice(0, -3);
        const isLast = i === parts.length - 1;
        if (!node.children.has(part)) {
            node.children.set(part, { name: part, isDir: !isLast, children: new Map(), doc: isLast ? doc : undefined });
        }
        node = node.children.get(part)!;
    }
  }
  return root;
}

export const TreeNodeView: React.FC<{
  node: TreeNode;
  depth: number;
  selectedDoc: DocEntry | null;
  onSelect: (doc: DocEntry) => void;
}> = ({ node, depth, selectedDoc, onSelect }) => {
  const [isOpen, setIsOpen] = useState(depth === 0);
  const { tooltip, onMouseEnter, onMouseLeave, onMouseMove } = useTooltip();
  
  const isSelected = node.doc && selectedDoc?.document_id === node.doc.document_id;
  const hasChildren = node.children.size > 0;
  
  const handleToggle = (e: React.MouseEvent) => {
    e.stopPropagation();
    setIsOpen(!isOpen);
  };

  const handleClick = (e: React.MouseEvent) => {
    if (node.isDir || hasChildren) {
      handleToggle(e);
    } else if (node.doc) {
      onSelect(node.doc);
    }
  };

  return (
    <div className="flex flex-col">
      <div 
        onClick={handleClick}
        onMouseEnter={node.doc ? onMouseEnter : undefined}
        onMouseLeave={node.doc ? onMouseLeave : undefined}
        onMouseMove={node.doc ? onMouseMove : undefined}
        className={`group flex items-center gap-2 py-1 px-2 rounded-xl text-xs transition-all duration-300 cursor-pointer ${
          isSelected 
            ? 'bg-indigo-500/10 text-indigo-300 border border-indigo-500/10' 
            : 'text-gray-500 hover:text-gray-300 hover:bg-white/[0.03] border border-transparent'
        }`}
        style={{ marginLeft: depth * 8 }}
      >
        <button
          onClick={handleToggle}
          className={`w-4 h-4 flex items-center justify-center transition-transform duration-300 ${isOpen ? 'rotate-90' : ''} ${!hasChildren && 'invisible'}`}
        >
          <span className="text-[8px] opacity-40">▸</span>
        </button>
        
        <span className="text-base flex-shrink-0">
          {hasChildren ? (isOpen ? '📂' : '📁') : (node.doc?.retention_tier === 'short' ? '🥛' : node.doc?.retention_tier === 'mid' ? '🍞' : '📄')}
        </span>
        
        <span className={`truncate flex-1 ${node.doc ? 'font-medium' : 'italic opacity-60'} ${node.doc?.freshness_score != null && node.doc.freshness_score < 40 ? 'text-rose-400 line-through opacity-60' : ''}`}>
          {node.doc?.title ?? node.name}
        </span>

        {node.doc?.score != null && (
          <span className="text-[9px] font-mono text-gray-700 opacity-0 group-hover:opacity-100 transition-opacity">
            {node.doc.score.toFixed(2)}
          </span>
        )}
      </div>

      {tooltip && node.doc && <DocTooltip doc={node.doc} x={tooltip.x} y={tooltip.y} />}

      {isOpen && Array.from(node.children.values())
        .sort((a, b) => (a.isDir === b.isDir ? a.name.localeCompare(b.name) : (a.isDir ? -1 : 1)))
        .map(child => (
          <TreeNodeView key={child.name} node={child} depth={depth + 1} selectedDoc={selectedDoc} onSelect={onSelect} />
        ))
      }
    </div>
  );
};
