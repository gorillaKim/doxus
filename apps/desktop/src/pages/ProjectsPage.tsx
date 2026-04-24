import { useEffect, useState, useMemo } from 'react';
import { useProjectStore, Project } from '../stores/useProjectStore';
import { usePluginStore } from '../stores/usePluginStore';
import { ProjectCard } from '../components/projects/ProjectCard';
import { AddProjectModal } from '../components/projects/AddProjectModal';
import { ProjectHeader } from '../components/projects/ProjectHeader';
import { ProjectFilterTabs } from '../components/projects/ProjectFilterTabs';
import { ProjectEmptyState } from '../components/projects/ProjectEmptyState';

const KNOWN_PLUGINS: Record<string, { label: string; icon: string }> = {
  'obsidian':   { label: 'Obsidian',   icon: '🪨' },
  'confluence': { label: 'Confluence', icon: '📄' },
  'github':     { label: 'GitHub',     icon: '🐙' },
};

export function ProjectsPage() {
  const { 
    projects, isLoading, error, fetch, 
    toggleStatus, indexProject, indexingNames, 
    removeProject, updateSensitivityMode 
  } = useProjectStore();
  
  const emojiMap = usePluginStore((s) => s.emojiMap);
  const getEmoji = usePluginStore((s) => s.getEmoji);
  
  const [showModal, setShowModal] = useState(false);
  const [activeTab, setActiveTab] = useState('all');
  const [togglingId, setTogglingId] = useState<string | null>(null);
  const [removingId, setRemovingId] = useState<string | null>(null);
  const [updatingId, setUpdatingId] = useState<number | null>(null);
  const [indexResult, setIndexResult] = useState<{ name: string; message: string } | null>(null);

  useEffect(() => { fetch(); }, [fetch]);

  const handleIndex = async (name: string) => {
    try {
      const result = await indexProject(name);
      setIndexResult({ name, message: result.message });
      setTimeout(() => setIndexResult(null), 5000);
    } catch (e) {
      setIndexResult({ name, message: `오류: ${String(e)}` });
      setTimeout(() => setIndexResult(null), 5000);
    }
  };

  const handleRemove = async (name: string, displayName: string) => {
    if (!window.confirm(`"${displayName}" 프로젝트를 삭제하시겠습니까?\n인덱스 데이터만 삭제되며 원본 파일은 유지됩니다.`)) return;
    setRemovingId(name);
    try {
      await removeProject(name);
    } finally {
      setRemovingId(null);
    }
  };

  const handleToggleStatus = async (name: string, currentStatus: 'active' | 'disabled') => {
    setTogglingId(name);
    try {
      await toggleStatus(name, currentStatus);
    } catch {
      // toggle_project_status not yet implemented in backend
    } finally {
      setTogglingId(null);
    }
  };

  const handleSensitivityChange = async (projectId: number, val: string) => {
    if (updatingId === projectId) return;
    setUpdatingId(projectId);
    try {
      await updateSensitivityMode(projectId, val);
    } finally {
      setUpdatingId(null);
    }
  };

  const getPluginMeta = (sourceType: string) => {
    const short = sourceType.replace(/^com\.doxus\./, '');
    const pluginId = `com.doxus.${short}`;
    const base = KNOWN_PLUGINS[short] ?? { label: short, icon: '🔌' };
    const emoji = getEmoji(pluginId);
    return { ...base, icon: emoji };
  };

  // Memoized derived data
  const { filteredProjects, tabs } = useMemo(() => {
    const groups = projects.reduce<Record<string, Project[]>>((acc, p) => {
      const key = p.source_type?.replace(/^com\.doxus\./, '') ?? 'obsidian';
      (acc[key] ??= []).push(p);
      return acc;
    }, {});

    const tabsData = Object.entries(groups).map(([id, items]) => {
      const meta = getPluginMeta(id);
      return { id, label: meta.label, icon: meta.icon, count: items.length };
    });

    const filtered = activeTab === 'all' 
      ? projects 
      : projects.filter(p => p.source_type?.includes(activeTab));

    return { filteredProjects: filtered, tabs: tabsData };
  }, [projects, activeTab, emojiMap]);

  if (projects.length === 0 && !isLoading) {
    return (
      <div className="flex flex-col h-full max-w-4xl mx-auto py-10">
        <AddProjectModal onClose={() => setShowModal(false)} />
        <ProjectHeader onAddClick={() => setShowModal(true)} projectCount={0} />
        <ProjectEmptyState onAddClick={() => setShowModal(true)} />
      </div>
    );
  }

  return (
    <div className="w-full max-w-6xl mx-auto flex flex-col h-full overflow-hidden min-w-0">
      {showModal && <AddProjectModal onClose={() => setShowModal(false)} />}

      {/* Modern Toast Notification */}
      {indexResult && (
        <div className="fixed bottom-10 right-10 z-[100] animate-in slide-in-from-right duration-500">
          <div className="px-5 py-4 bg-gray-900/90 backdrop-blur-xl border border-white/10 rounded-2xl shadow-2xl flex flex-col gap-1 min-w-[280px]">
            <div className="flex items-center gap-2">
              <span className="w-2 h-2 rounded-full bg-indigo-500 animate-pulse" />
              <span className="text-xs font-black text-indigo-400 uppercase tracking-tighter">Indexing Complete</span>
            </div>
            <div className="flex flex-col text-sm">
              <span className="font-bold text-white uppercase">{indexResult.name}</span>
              <p className="text-xs text-gray-400 mt-1">{indexResult.message}</p>
            </div>
          </div>
        </div>
      )}

      <div className="flex-shrink-0 min-w-0">
        <ProjectHeader onAddClick={() => setShowModal(true)} projectCount={projects.length} />

        {error && (
          <div className="mb-6 p-4 bg-red-500/10 border border-red-500/20 rounded-2xl text-red-400 text-xs font-medium flex items-center gap-3">
            <span className="text-lg">⚠️</span>
            {error}
          </div>
        )}

        <ProjectFilterTabs 
          activeTab={activeTab} 
          tabs={tabs} 
          onTabChange={setActiveTab} 
        />
      </div>

      <div className="flex-1 min-h-0 overflow-y-auto no-scrollbar pb-10">
        <div className="grid grid-cols-1 lg:grid-cols-2 2xl:grid-cols-3 gap-6 w-full">
          {filteredProjects.map((p) => (
            <div key={p.name} className="min-w-0">
              <ProjectCard
                project={p}
                icon={getEmoji(`com.doxus.${p.source_type?.replace(/^com\.doxus\./, '')}`)}
                isBusy={indexingNames.has(p.name) || togglingId === p.name || updatingId === p.id || removingId === p.name}
                isIndexing={indexingNames.has(p.name)}
                onIndex={() => handleIndex(p.name)}
                onToggle={() => handleToggleStatus(p.name, p.status)}
                onSensitivityChange={(val) => handleSensitivityChange(p.id, val)}
                onRemove={() => handleRemove(p.name, p.display_name)}
              />
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
