import { useEffect, useState, useMemo, useRef } from 'react';
import ForceGraph2D, { ForceGraphMethods } from 'react-force-graph-2d';
import { invoke } from '@tauri-apps/api/core';
import { useNavigate } from 'react-router-dom';

interface GraphNode {
  id: string;
  label: string;
  node_type: 'doc' | 'tag';
  project?: string;
  plugin_id?: string;
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
  const [selectedProjects, setSelectedProjects] = useState<Set<string>>(new Set());
  const [selectedPlugins, setSelectedPlugins] = useState<Set<string>>(new Set());
  const [hoverNode, setHoverNode] = useState<GraphNode | null>(null);
  const graphRef = useRef<ForceGraphMethods>();
  const navigate = useNavigate();

  // 데이터 로드 및 초기 필터 설정
  useEffect(() => {
    invoke<GraphData>('get_graph_data')
      .then(data => {
        setRawData(data);
        // 초기에는 모든 프로젝트와 플러그인을 선택 상태로 설정
        const projects = new Set<string>();
        const plugins = new Set<string>();
        data.nodes.forEach(n => {
          if (n.project) projects.add(n.project);
          if (n.plugin_id) plugins.add(n.plugin_id);
        });
        setSelectedProjects(projects);
        setSelectedPlugins(plugins);
      })
      .catch(console.error);
  }, []);

  // 사용 가능한 프로젝트 및 플러그인 목록 (정렬됨)
  const availableFilters = useMemo(() => {
    if (!rawData) return { projects: [], plugins: [] };
    const projects = Array.from(new Set(rawData.nodes.map(n => n.project).filter(Boolean) as string[])).sort();
    const plugins = Array.from(new Set(rawData.nodes.map(n => n.plugin_id).filter(Boolean) as string[])).sort();
    return { projects, plugins };
  }, [rawData]);

  // 필터링된 데이터 계산
  const graphData = useMemo(() => {
    if (!rawData) return { nodes: [], links: [] };

    let filteredNodes = rawData.nodes.filter(n => {
      if (n.node_type === 'tag') return true; // 태그 노드는 아래에서 고립 여부로 판단
      const projectMatch = n.project ? selectedProjects.has(n.project) : true;
      const pluginMatch = n.plugin_id ? selectedPlugins.has(n.plugin_id) : true;
      return projectMatch && pluginMatch;
    });

    let filteredLinks = rawData.links.filter(l => {
      const sourceId = typeof l.source === 'string' ? l.source : (l.source as any).id;
      const targetId = typeof l.target === 'string' ? l.target : (l.target as any).id;
      
      // 소스나 타겟 노드 중 하나라도 현재 활성 노드 목록에 없으면 링크도 숨김
      const sourceActive = filteredNodes.some(n => n.id === sourceId);
      const targetActive = filteredNodes.some(n => n.id === targetId);
      
      if (!sourceActive || !targetActive) return false;

      if (!showLinks && l.link_type === 'link') return false;
      if (!showTags && l.link_type === 'tag_rel') return false;
      
      return true;
    });

    // 태그 표시 옵션이 꺼져있으면 태그 노드 제거
    if (!showTags) {
      filteredNodes = filteredNodes.filter(n => n.node_type !== 'tag');
    }

    // 연결선 표시 옵션이 꺼져있을 때, 연결되지 않은 고립된 노드 무시 (선택 사항)
    if (!showLinks && showTags) {
      const activeNodeIds = new Set(filteredLinks.flatMap(l => [
        typeof l.source === 'string' ? l.source : (l.source as any).id,
        typeof l.target === 'string' ? l.target : (l.target as any).id
      ]));
      filteredNodes = filteredNodes.filter(n => n.node_type === 'doc' || activeNodeIds.has(n.id));
    }

    return { nodes: filteredNodes, links: filteredLinks };
  }, [rawData, showTags, showLinks, selectedProjects, selectedPlugins]);

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

  const handleNodeClick = (node: any) => {
    if (node.node_type === 'doc') {
      // 'doc_123' 형태이므로 숫자 부분만 추출
      const numericId = node.id.replace('doc_', '');
      navigate(`/search?docId=${numericId}`);
    } else if (node.node_type === 'tag') {
      // 태그 이름(레이블)을 기반으로 검색 페이지 필터링
      navigate(`/search?tag=${node.label}`);
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
      <div className="absolute top-4 left-4 z-10 flex flex-col gap-4 p-4 bg-gray-900/80 backdrop-blur-md rounded-xl border border-white/10 shadow-2xl max-h-[90%] overflow-y-auto w-48">
        <div>
          <h3 className="text-xs font-bold text-gray-400 uppercase tracking-widest mb-2">Display</h3>
          <div className="flex flex-col gap-2">
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
          </div>
        </div>

        {availableFilters.plugins.length > 0 && (
          <div>
            <h3 className="text-xs font-bold text-gray-400 uppercase tracking-widest mb-2">Plugins</h3>
            <div className="flex flex-col gap-1.5">
              {availableFilters.plugins.map(plugin => (
                <label key={plugin} className="flex items-center gap-2 text-[13px] cursor-pointer hover:text-white transition-colors truncate" title={plugin}>
                  <input 
                    type="checkbox" 
                    checked={selectedPlugins.has(plugin)}
                    onChange={e => {
                      const next = new Set(selectedPlugins);
                      if (e.target.checked) next.add(plugin);
                      else next.delete(plugin);
                      setSelectedPlugins(next);
                    }}
                    className="w-3.5 h-3.5 rounded border-gray-700 bg-gray-800 text-indigo-400 focus:ring-indigo-400"
                  />
                  {plugin.replace('com.doxus.', '')}
                </label>
              ))}
            </div>
          </div>
        )}

        {availableFilters.projects.length > 0 && (
          <div>
            <h3 className="text-xs font-bold text-gray-400 uppercase tracking-widest mb-2">Projects</h3>
            <div className="flex flex-col gap-1.5">
              {availableFilters.projects.map(project => (
                <label key={project} className="flex items-center gap-2 text-[13px] cursor-pointer hover:text-white transition-colors truncate" title={project}>
                  <input 
                    type="checkbox" 
                    checked={selectedProjects.has(project)}
                    onChange={e => {
                      const next = new Set(selectedProjects);
                      if (e.target.checked) next.add(project);
                      else next.delete(project);
                      setSelectedProjects(next);
                    }}
                    className="w-3.5 h-3.5 rounded border-gray-700 bg-gray-800 text-indigo-400 focus:ring-indigo-400"
                  />
                  {project}
                </label>
              ))}
            </div>
          </div>
        )}

        <div className="pt-2 border-t border-white/5 text-[10px] text-gray-500">
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
        onNodeClick={handleNodeClick}
        cooldownTicks={100}
        d3AlphaDecay={0.02}
        d3VelocityDecay={0.3}
      />
    </div>
  );
}
