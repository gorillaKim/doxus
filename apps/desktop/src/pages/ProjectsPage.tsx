import React, { useEffect, useState } from 'react';
import { useProjectStore, Project } from '../stores/useProjectStore';
import { usePluginStore } from '../stores/usePluginStore';
import { ProjectCard } from '../components/projects/ProjectCard';
import { AddProjectModal } from '../components/projects/AddProjectModal';

const KNOWN_PLUGINS: Record<string, { label: string; icon: string }> = {
  'obsidian':   { label: 'Obsidian',   icon: '🪨' },
  'confluence': { label: 'Confluence', icon: '📄' },
  'github':     { label: 'GitHub',     icon: '🐙' },
};

function getPluginMeta(sourceType: string) {
  const short = sourceType.replace(/^com\.doxus\./, '');
  const pluginId = `com.doxus.${short}`;
  const base = KNOWN_PLUGINS[short] ?? { label: short, icon: '🔌' };
  const emoji = usePluginStore.getState().getEmoji(pluginId);
  return { ...base, icon: emoji };
}

export function ProjectsPage() {
  const { 
    projects, isLoading, error, fetch, 
    toggleStatus, indexProject, indexingNames, 
    removeProject, updateSensitivityMode 
  } = useProjectStore();
  
  usePluginStore((s) => s.emojiMap);
  const [showModal, setShowModal] = useState(false);
  const [togglingId, setTogglingId] = useState<string | null>(null);
  const [removingId, setRemovingId] = useState<string | null>(null);
  const [updatingId, setUpdatingId] = useState<number | null>(null);
  const [indexResult, setIndexResult] = useState<{ name: string; message: string } | null>(null);

  useEffect(() => { fetch(); }, [fetch]);

  const handleIndex = async (name: string) => {
    try {
      const result = await indexProject(name);
      setIndexResult({ name, message: result.message });
      setTimeout(() => setIndexResult(null), 4000);
    } catch (e) {
      setIndexResult({ name, message: `오류: ${String(e)}` });
      setTimeout(() => setIndexResult(null), 4000);
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
      // toggle_project_status not yet implemented
    } finally {
      setTogglingId(null);
    }
  };

  const handleSensitivityChange = async (projectId: number, val: string) => {
    if (updatingId === projectId) return;
    setUpdatingId(projectId);
    const startTime = Date.now();
    try {
      await updateSensitivityMode(projectId, val);
      const elapsed = Date.now() - startTime;
      if (elapsed < 400) {
        await new Promise<void>(resolve => setTimeout(() => resolve(), 400 - elapsed));
      }
    } finally {
      setUpdatingId(null);
    }
  };

  // Group by source_type
  const groups = projects.reduce<Record<string, Project[]>>((acc, p) => {
    const key = p.source_type ?? 'obsidian';
    (acc[key] ??= []).push(p);
    return acc;
  }, {});

  return (
    <div className="flex flex-col h-full gap-5 max-w-3xl">
      {showModal && <AddProjectModal onClose={() => setShowModal(false)} />}

      {indexResult && (
        <div className="fixed bottom-6 right-6 z-50 px-4 py-3 bg-gray-900 border border-gray-700 rounded-xl shadow-xl text-sm text-gray-200 max-w-xs">
          <span className="font-medium text-indigo-400">{indexResult.name}: </span>
          <span className="ml-1">{indexResult.message}</span>
        </div>
      )}

      <div className="flex items-center justify-between mb-2">
        <div className="flex flex-col gap-1">
          <h1 className="text-3xl font-extrabold text-white tracking-tight">프로젝트 매니저</h1>
          <p className="text-xs text-gray-500">연결된 모든 지식 소스를 관리합니다.</p>
        </div>
        <button 
          onClick={() => setShowModal(true)}
          className="px-5 py-2.5 bg-indigo-600 hover:bg-indigo-500 text-white text-sm font-bold rounded-xl shadow-lg shadow-indigo-600/20 transition-all duration-300 transform hover:-translate-y-0.5"
        >
          + 새 프로젝트
        </button>
      </div>

      {error && (
        <div className="p-3 bg-red-950 border border-red-800 rounded-lg text-red-400 text-sm">{error}</div>
      )}

      {projects.length === 0 && !isLoading && (
        <div className="text-center py-16 text-gray-600">
          <p className="text-4xl mb-3">📁</p>
          <p className="text-gray-400 font-medium">아직 프로젝트가 없습니다</p>
          <p className="text-sm mt-1 text-gray-600">+ 프로젝트 추가를 눌러 시작하세요</p>
        </div>
      )}

      {Object.entries(groups).map(([srcType, items]) => {
        const { label, icon } = getPluginMeta(srcType);
        return (
          <div key={srcType} className="flex flex-col gap-2 mb-6">
            <div className="flex items-center gap-2 px-1">
              <span className="text-base">{icon}</span>
              <span className="text-xs font-semibold text-gray-500 uppercase tracking-wider">{label}</span>
              <span className="text-xs text-gray-700">({items.length})</span>
            </div>
            <div className="grid grid-cols-1 gap-4">
              {items.map((p) => (
                <ProjectCard
                  key={p.name}
                  project={p}
                  isBusy={indexingNames.has(p.name) || togglingId === p.name || updatingId === p.id || removingId === p.name}
                  isIndexing={indexingNames.has(p.name)}
                  isToggling={togglingId === p.name}
                  isUpdating={updatingId === p.id}
                  isRemoving={removingId === p.name}
                  onIndex={() => handleIndex(p.name)}
                  onToggle={() => handleToggleStatus(p.name, p.status)}
                  onSensitivityChange={(val) => handleSensitivityChange(p.id, val)}
                  onRemove={() => handleRemove(p.name, p.display_name)}
                />
              ))}
            </div>
          </div>
        );
      })}
    </div>
  );
}
