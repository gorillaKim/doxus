import React, { useState, useMemo } from 'react';
import { DocEntry, buildTree, TreeNodeView } from './SearchTree';
import { usePluginStore } from '../../stores/usePluginStore';

interface SearchSidebarProps {
  isLoading: boolean;
  itemCount: number;
  groupedEntries: Map<string, { sourceType: string; docs: DocEntry[] }>;
  selectedDoc: DocEntry | null;
  onSelect: (doc: DocEntry) => void;
  hasSearch: boolean;
}

export const SearchSidebar = React.memo<SearchSidebarProps>(({
  isLoading,
  itemCount,
  groupedEntries,
  selectedDoc,
  onSelect,
  hasSearch
}) => {
  const [activeTab, setActiveTab] = useState('all');
  // 기본 상태: 모든 프로젝트 닫힘. 클릭 시 열린 프로젝트만 추적
  const [expandedProjects, setExpandedProjects] = useState<Set<string>>(new Set());
  const getEmoji = usePluginStore(s => s.getEmoji);

  const toggleProject = (name: string) => {
    setExpandedProjects(prev => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  };

  // Group by source type for tab generation
  const tabs = useMemo(() => {
    const types = new Map<string, number>();
    groupedEntries.forEach((val) => {
      const type = val.sourceType.replace(/^com\.doxus\./, '');
      types.set(type, (types.get(type) || 0) + val.docs.length);
    });
    return Array.from(types.entries()).map(([id, count]) => ({
      id,
      label: id.charAt(0).toUpperCase() + id.slice(1),
      icon: getEmoji(`com.doxus.${id}`),
      count
    }));
  }, [groupedEntries, getEmoji]);

  const filteredEntries = useMemo(() => {
    if (activeTab === 'all') return groupedEntries;
    const next = new Map<string, { sourceType: string; docs: DocEntry[] }>();
    groupedEntries.forEach((val, key) => {
      if (val.sourceType.includes(activeTab)) {
        next.set(key, val);
      }
    });
    return next;
  }, [groupedEntries, activeTab]);

  // [Optimization] Cache trees for each project to avoid repeated O(N) tree building
  const projectTrees = useMemo(() => {
    const trees = new Map<string, any>();
    filteredEntries.forEach((group, projectName) => {
      trees.set(projectName, buildTree(group.docs));
    });
    return trees;
  }, [filteredEntries]);

  return (
    <div className="w-80 shrink-0 h-full flex flex-col bg-white/[0.02] border-r border-white/5 overflow-hidden">
      {/* Sidebar Header */}
      <div className="px-5 py-6 flex flex-col gap-4">
        <div className="flex items-center justify-between">
          <span className="text-[10px] text-gray-500 font-black uppercase tracking-[0.2em]">
            {hasSearch ? '검색 결과' : '지식 라이브러리'}
          </span>
          <div className="flex items-center gap-2">
            {isLoading && <div className="w-2 h-2 rounded-full bg-indigo-500 animate-pulse" />}
            <span className="text-[10px] text-gray-700 font-mono font-bold tracking-tighter">
              {itemCount} ITEMS
            </span>
          </div>
        </div>

        {/* Source Filter Tabs */}
        {tabs.length > 0 && (
          <div className="flex items-center gap-1.5 overflow-x-auto no-scrollbar py-1">
            <button
              onClick={() => setActiveTab('all')}
              className={`px-3 py-1.5 rounded-xl text-[10px] font-black uppercase transition-all duration-300 ${activeTab === 'all'
                  ? 'bg-white/10 text-white shadow-lg'
                  : 'text-gray-600 hover:text-gray-400'
                }`}
            >
              All
            </button>
            {tabs.map(tab => (
              <button
                key={tab.id}
                onClick={() => setActiveTab(tab.id)}
                className={`flex items-center gap-2 px-3 py-1.5 rounded-xl text-[10px] font-black uppercase transition-all duration-300 ${activeTab === tab.id
                    ? 'bg-indigo-500/10 text-indigo-400 border border-indigo-500/20 shadow-lg'
                    : 'text-gray-600 hover:text-gray-400 border border-transparent'
                  }`}
              >
                <span>{tab.icon}</span>
                <span>{tab.label}</span>
              </button>
            ))}
          </div>
        )}
      </div>

      {/* Directory Tree */}
      <div className="flex-1 overflow-y-auto no-scrollbar px-3 pb-20">
        {Array.from(filteredEntries.entries()).map(([projectName, group]) => {
          const tree = projectTrees.get(projectName);
          const isCollapsed = !expandedProjects.has(projectName);

          return (
            <div key={projectName} className="mb-4">
              <div
                onClick={() => toggleProject(projectName)}
                className="flex items-center gap-2 px-2 py-2 mb-1 sticky top-0 bg-gray-950/20 backdrop-blur-md z-10 cursor-pointer hover:bg-white/[0.03] transition-colors rounded-xl overflow-hidden group"
              >
                <span className={`text-[8px] transition-transform duration-300 ${isCollapsed ? '' : 'rotate-90'}`}>▸</span>
                <span className="text-base flex-shrink-0">{getEmoji(`com.doxus.${group.sourceType.replace(/^com\.doxus\./, '')}`)}</span>
                <span className="text-[10px] font-black text-gray-400 uppercase tracking-widest truncate group-hover:text-gray-200 transition-colors">{projectName}</span>
                <span className="text-[9px] text-gray-700 ml-auto font-mono group-hover:text-gray-500">{group.docs.length}</span>
              </div>

              {!isCollapsed && tree && (
                <div className="flex flex-col gap-0.5 animate-in fade-in slide-in-from-top-1 duration-300">
                  {Array.from(tree.children.values()).map((child: any) => (
                    <TreeNodeView key={child.name} node={child} depth={0} selectedDoc={selectedDoc} onSelect={onSelect} />
                  ))}
                </div>
              )}
            </div>
          );
        })}

        {filteredEntries.size === 0 && (
          <div className="flex flex-col items-center justify-center py-20 text-center opacity-30">
            <span className="text-4xl mb-2">📁</span>
            <p className="text-xs font-bold uppercase tracking-widest text-gray-500">목록이 비어있습니다</p>
          </div>
        )}
      </div>
    </div>
  );
});
