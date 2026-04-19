import { useEffect, useState, useMemo, useRef } from 'react';
import ForceGraph2D, { ForceGraphMethods } from 'react-force-graph-2d';
import { invoke } from '@tauri-apps/api/core';
import { useNavigate } from 'react-router-dom';

interface GraphNode {
  id: string;
  label: string;
  node_type: 'doc' | 'tag';
  project?: string;
  x?: number;
  y?: number;
}

interface GraphLink {
  source: string;
  target: string;
  link_type: 'link' | 'tag_rel';
}

interface GraphData {
  nodes: GraphNode[];
  links: GraphLink[];
}

// ── 스타일 설정 ──────────────────────────────────────────────────────────────
const COLORS = {
  doc: '#818cf8', // indigo-400
  tag: '#f472b6', // pink-400
  link: 'rgba(255, 255, 255, 0.15)',
  tag_rel: 'rgba(244, 114, 182, 0.1)',
  text: '#94a3b8',
  highlight: '#fff',
};

export function GraphPage() {
  const [rawData, setRawData] = useState<GraphData | null>(null);
  const [showTags, setShowTags] = useState(true);
  const [showLinks, setShowLinks] = useState(true);
  const [hoverNode, setHoverNode] = useState<GraphNode | null>(null);
  const graphRef = useRef<ForceGraphMethods>();
  const navigate = useNavigate();

  // 데이터 로드
  useEffect(() => {
    invoke<GraphData>('get_graph_data')
      .then(setRawData)
      .catch(console.error);
  }, []);

  // 필터링된 데이터 계산
  const graphData = useMemo(() => {
    if (!rawData) return { nodes: [], links: [] };

    let filteredNodes = rawData.nodes;
    let filteredLinks = rawData.links;

    if (!showTags) {
      filteredNodes = filteredNodes.filter(n => n.node_type !== 'tag');
      filteredLinks = filteredLinks.filter(l => l.link_type !== 'tag_rel');
    }
    if (!showLinks) {
      filteredLinks = filteredLinks.filter(l => l.link_type !== 'link');
    }

    // 연결되지 않은 고립된 태그 노드는 숨김 (선택 사항)
    if (!showLinks && showTags) {
      const activeNodeIds = new Set(filteredLinks.flatMap(l => [
        typeof l.source === 'string' ? l.source : (l.source as any).id,
        typeof l.target === 'string' ? l.target : (l.target as any).id
      ]));
      filteredNodes = filteredNodes.filter(n => n.node_type === 'doc' || activeNodeIds.has(n.id));
    }

    return { nodes: filteredNodes, links: filteredLinks };
  }, [rawData, showTags, showLinks]);

  // Obsidian 스타일 캔버스 렌더링
  const paintNode = (node: any, ctx: CanvasRenderingContext2D, globalScale: number) => {
    const isTag = node.node_type === 'tag';
    const isHovered = hoverNode === node;
    const radius = isTag ? 3 : 4;

    // Glow Effect
    ctx.shadowBlur = isHovered ? 15 : 5;
    ctx.shadowColor = isTag ? COLORS.tag : COLORS.doc;

    ctx.fillStyle = isTag ? COLORS.tag : COLORS.doc;
    ctx.beginPath();
    if (isTag) {
      ctx.rect(node.x - radius, node.y - radius, radius * 2, radius * 2);
    } else {
      ctx.arc(node.x, node.y, radius, 0, 2 * Math.PI, false);
    }
    ctx.fill();

    // Label on Hover or Zoomed in
    if (isHovered || globalScale > 3) {
      const label = node.label;
      const fontSize = 12 / globalScale;
      ctx.font = `${fontSize}px Inter, sans-serif`;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      ctx.fillStyle = COLORS.highlight;
      ctx.fillText(label, node.x, node.y + radius + 5);
    }
  };

  if (!rawData) {
    return (
      <div className="h-full flex items-center justify-center text-gray-500">
        그래프 데이터를 불러오는 중...
      </div>
    );
  }

  return (
    <div className="relative h-full w-full bg-gray-950 rounded-2xl overflow-hidden border border-white/5">
      {/* 컨트롤 패널 */}
      <div className="absolute top-4 left-4 z-10 flex flex-col gap-2 p-3 bg-gray-900/80 backdrop-blur-md rounded-xl border border-white/10 shadow-2xl">
        <h3 className="text-xs font-bold text-gray-400 uppercase tracking-widest mb-1">Graph Views</h3>
        <label className="flex items-center gap-2 text-sm cursor-pointer hover:text-white transition-colors">
          <input 
            type="checkbox" 
            checked={showLinks} 
            onChange={e => setShowLinks(e.target.checked)}
            className="w-4 h-4 rounded border-gray-700 bg-gray-800 text-indigo-500 focus:ring-indigo-500"
          />
          문서 링크
        </label>
        <label className="flex items-center gap-2 text-sm cursor-pointer hover:text-white transition-colors">
          <input 
            type="checkbox" 
            checked={showTags} 
            onChange={e => setShowTags(e.target.checked)}
            className="w-4 h-4 rounded border-gray-700 bg-gray-800 text-pink-500 focus:ring-pink-500"
          />
          태그 노드
        </label>
        <div className="mt-2 pt-2 border-t border-white/5 text-[10px] text-gray-500">
          Scroll to zoom • Drag to pan<br/>
          Click node to open
        </div>
      </div>

      <ForceGraph2D
        ref={graphRef}
        graphData={graphData}
        backgroundColor="rgba(0,0,0,0)"
        nodeCanvasObject={paintNode}
        linkDirectionalArrowLength={l => l.link_type === 'link' ? 3 : 0}
        linkDirectionalArrowRelPos={1}
        linkColor={l => l.link_type === 'link' ? COLORS.link : COLORS.tag_rel}
        linkWidth={l => l.link_type === 'link' ? 1 : 0.5}
        onNodeHover={setHoverNode}
        onNodeClick={(node: any) => {
          if (node.node_type === 'doc') {
            // 문서 클릭 시 검색 페이지로 이동 (향후 특정 문서 바로 열기로 개선 가능)
            navigate(`/search?q=${encodeURIComponent(node.label)}`);
          }
        }}
        cooldownTicks={100}
        d3AlphaDecay={0.02}
        d3VelocityDecay={0.3}
      />
    </div>
  );
}
