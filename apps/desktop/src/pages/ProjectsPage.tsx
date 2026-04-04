import { useEffect, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { useProjectStore } from '../stores/useProjectStore';

type PluginType = 'obsidian' | 'confluence' | 'github';

interface PluginOption {
  id: PluginType;
  label: string;
  description: string;
  icon: string;
}

const PLUGIN_OPTIONS: PluginOption[] = [
  { id: 'obsidian', label: 'Obsidian', description: '로컬 Obsidian 볼트 폴더', icon: '🪨' },
  { id: 'confluence', label: 'Confluence', description: 'Confluence Cloud 또는 Server', icon: '📄' },
  { id: 'github', label: 'GitHub', description: 'GitHub Issues / Wiki / Discussions', icon: '🐙' },
];

function ObsidianForm({ name, setName, path, setPath }: {
  name: string; setName: (v: string) => void;
  path: string; setPath: (v: string) => void;
}) {
  const handlePickFolder = async () => {
    const selected = await open({ directory: true, multiple: false, title: 'Select Obsidian Vault' });
    if (selected && typeof selected === 'string') {
      setPath(selected);
      if (!name) {
        setName(selected.split('/').pop() ?? '');
      }
    }
  };

  return (
    <>
      <div className="flex flex-col gap-1">
        <label className="text-xs text-gray-500">볼트 폴더</label>
        <div className="flex gap-2">
          <input
            type="text"
            value={path}
            onChange={(e) => setPath(e.target.value)}
            placeholder="/Users/you/MyVault"
            className="flex-1 bg-gray-800 border border-gray-700 rounded-lg px-3 py-2 text-sm text-gray-100 placeholder-gray-600 focus:outline-none focus:border-indigo-500"
          />
          <button
            type="button"
            onClick={handlePickFolder}
            className="px-3 py-2 bg-gray-700 hover:bg-gray-600 text-gray-200 text-sm rounded-lg transition-colors"
          >
            찾기…
          </button>
        </div>
      </div>
      <div className="flex flex-col gap-1">
        <label className="text-xs text-gray-500">프로젝트 이름</label>
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="my-vault"
          className="bg-gray-800 border border-gray-700 rounded-lg px-3 py-2 text-sm text-gray-100 placeholder-gray-600 focus:outline-none focus:border-indigo-500"
        />
      </div>
    </>
  );
}

function ConfluenceForm({ name, setName, extraFields, setExtraFields }: {
  name: string; setName: (v: string) => void;
  extraFields: Record<string, string>;
  setExtraFields: (v: Record<string, string>) => void;
}) {
  const set = (key: string, val: string) => setExtraFields({ ...extraFields, [key]: val });
  return (
    <>
      <div className="flex flex-col gap-1">
        <label className="text-xs text-gray-500">프로젝트 이름</label>
        <input type="text" value={name} onChange={(e) => setName(e.target.value)} placeholder="confluence-docs"
          className="bg-gray-800 border border-gray-700 rounded-lg px-3 py-2 text-sm text-gray-100 placeholder-gray-600 focus:outline-none focus:border-indigo-500" />
      </div>
      <div className="flex flex-col gap-1">
        <label className="text-xs text-gray-500">기본 URL</label>
        <input type="text" value={extraFields.base_url ?? ''} onChange={(e) => set('base_url', e.target.value)}
          placeholder="https://yourcompany.atlassian.net"
          className="bg-gray-800 border border-gray-700 rounded-lg px-3 py-2 text-sm text-gray-100 placeholder-gray-600 focus:outline-none focus:border-indigo-500" />
      </div>
      <div className="flex flex-col gap-1">
        <label className="text-xs text-gray-500">스페이스 키</label>
        <input type="text" value={extraFields.space_key ?? ''} onChange={(e) => set('space_key', e.target.value)}
          placeholder="ENG"
          className="bg-gray-800 border border-gray-700 rounded-lg px-3 py-2 text-sm text-gray-100 placeholder-gray-600 focus:outline-none focus:border-indigo-500" />
      </div>
      <div className="flex flex-col gap-1">
        <label className="text-xs text-gray-500">API 토큰</label>
        <input type="password" value={extraFields.api_token ?? ''} onChange={(e) => set('api_token', e.target.value)}
          placeholder="••••••••"
          className="bg-gray-800 border border-gray-700 rounded-lg px-3 py-2 text-sm text-gray-100 placeholder-gray-600 focus:outline-none focus:border-indigo-500" />
      </div>
      <div className="flex flex-col gap-1">
        <label className="text-xs text-gray-500">이메일 (Confluence Cloud)</label>
        <input type="email" value={extraFields.email ?? ''} onChange={(e) => set('email', e.target.value)}
          placeholder="you@company.com"
          className="bg-gray-800 border border-gray-700 rounded-lg px-3 py-2 text-sm text-gray-100 placeholder-gray-600 focus:outline-none focus:border-indigo-500" />
      </div>
    </>
  );
}

function GitHubForm({ name, setName, extraFields, setExtraFields }: {
  name: string; setName: (v: string) => void;
  extraFields: Record<string, string>;
  setExtraFields: (v: Record<string, string>) => void;
}) {
  const set = (key: string, val: string) => setExtraFields({ ...extraFields, [key]: val });
  return (
    <>
      <div className="flex flex-col gap-1">
        <label className="text-xs text-gray-500">프로젝트 이름</label>
        <input type="text" value={name} onChange={(e) => setName(e.target.value)} placeholder="github-docs"
          className="bg-gray-800 border border-gray-700 rounded-lg px-3 py-2 text-sm text-gray-100 placeholder-gray-600 focus:outline-none focus:border-indigo-500" />
      </div>
      <div className="flex flex-col gap-1">
        <label className="text-xs text-gray-500">저장소 (owner/repo)</label>
        <input type="text" value={extraFields.repo ?? ''} onChange={(e) => set('repo', e.target.value)}
          placeholder="myorg/myrepo"
          className="bg-gray-800 border border-gray-700 rounded-lg px-3 py-2 text-sm text-gray-100 placeholder-gray-600 focus:outline-none focus:border-indigo-500" />
      </div>
      <div className="flex flex-col gap-1">
        <label className="text-xs text-gray-500">개인 액세스 토큰</label>
        <input type="password" value={extraFields.token ?? ''} onChange={(e) => set('token', e.target.value)}
          placeholder="ghp_••••••••"
          className="bg-gray-800 border border-gray-700 rounded-lg px-3 py-2 text-sm text-gray-100 placeholder-gray-600 focus:outline-none focus:border-indigo-500" />
      </div>
      <div className="flex flex-col gap-1">
        <label className="text-xs text-gray-500">소스</label>
        <div className="flex gap-3">
          {['issues', 'wiki', 'discussions'].map((src) => (
            <label key={src} className="flex items-center gap-1.5 text-sm text-gray-400 cursor-pointer">
              <input type="checkbox"
                checked={!!(extraFields[src])}
                onChange={(e) => set(src, e.target.checked ? '1' : '')}
                className="accent-indigo-500" />
              {src.charAt(0).toUpperCase() + src.slice(1)}
            </label>
          ))}
        </div>
      </div>
    </>
  );
}

function AddProjectModal({ onClose }: { onClose: () => void }) {
  const { addProject } = useProjectStore();
  const [pluginType, setPluginType] = useState<PluginType>('obsidian');
  const [name, setName] = useState('');
  const [path, setPath] = useState('');
  const [extraFields, setExtraFields] = useState<Record<string, string>>({});
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim()) return;
    setIsSubmitting(true);
    setError(null);
    try {
      const projectPath = pluginType === 'obsidian' ? path.trim() : (extraFields.base_url ?? extraFields.repo ?? pluginType);
      await addProject(name.trim(), projectPath);
      onClose();
    } catch (e) {
      setError(String(e));
      setIsSubmitting(false);
    }
  };

  const canSubmit = name.trim() && (
    pluginType === 'obsidian' ? path.trim() :
    pluginType === 'confluence' ? (extraFields.base_url && extraFields.api_token) :
    pluginType === 'github' ? extraFields.repo :
    true
  );

  return (
    <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50">
      <form
        onSubmit={handleSubmit}
        className="bg-gray-900 border border-gray-800 rounded-2xl p-6 w-full max-w-md flex flex-col gap-5 shadow-2xl"
      >
        <div className="flex items-center justify-between">
          <h2 className="text-base font-semibold text-gray-100">프로젝트 추가</h2>
          <button type="button" onClick={onClose} className="text-gray-500 hover:text-gray-300 text-lg">✕</button>
        </div>

        {/* Plugin type selector */}
        <div className="flex flex-col gap-2">
          <label className="text-xs text-gray-500 uppercase tracking-wider">소스 유형</label>
          <div className="grid grid-cols-3 gap-2">
            {PLUGIN_OPTIONS.map((opt) => (
              <button
                key={opt.id}
                type="button"
                onClick={() => { setPluginType(opt.id); setExtraFields({}); setName(''); setPath(''); }}
                className={`flex flex-col items-center gap-1 p-3 rounded-xl border text-sm transition-colors ${
                  pluginType === opt.id
                    ? 'border-indigo-500 bg-indigo-950 text-indigo-300'
                    : 'border-gray-700 bg-gray-800 text-gray-400 hover:border-gray-600 hover:text-gray-200'
                }`}
              >
                <span className="text-xl">{opt.icon}</span>
                <span className="font-medium">{opt.label}</span>
                <span className="text-xs text-center leading-tight opacity-70">{opt.description}</span>
              </button>
            ))}
          </div>
        </div>

        {/* Plugin-specific fields */}
        {pluginType === 'obsidian' && (
          <ObsidianForm name={name} setName={setName} path={path} setPath={setPath} />
        )}
        {pluginType === 'confluence' && (
          <ConfluenceForm name={name} setName={setName} extraFields={extraFields} setExtraFields={setExtraFields} />
        )}
        {pluginType === 'github' && (
          <GitHubForm name={name} setName={setName} extraFields={extraFields} setExtraFields={setExtraFields} />
        )}

        {error && (
          <p className="text-xs text-red-400 bg-red-950 border border-red-800 rounded-lg px-3 py-2">{error}</p>
        )}

        <div className="flex gap-2 justify-end">
          <button type="button" onClick={onClose}
            className="px-3 py-1.5 text-sm text-gray-400 hover:text-gray-200 transition-colors">
            취소
          </button>
          <button type="submit" disabled={isSubmitting || !canSubmit}
            className="px-4 py-1.5 text-sm bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg disabled:opacity-50 transition-colors">
            {isSubmitting ? '추가 중...' : '프로젝트 추가'}
          </button>
        </div>
      </form>
    </div>
  );
}

export function ProjectsPage() {
  const { projects, isLoading, error, fetch, toggleStatus } = useProjectStore();
  const [showModal, setShowModal] = useState(false);
  const [togglingId, setTogglingId] = useState<string | null>(null);

  useEffect(() => { fetch(); }, [fetch]);

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

  return (
    <div className="flex flex-col h-full gap-5 max-w-3xl">
      {showModal && <AddProjectModal onClose={() => setShowModal(false)} />}

      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold text-gray-100">프로젝트</h1>
        <div className="flex gap-2">
          <button onClick={fetch} disabled={isLoading}
            className="text-sm px-3 py-1.5 border border-gray-700 text-gray-400 rounded-lg hover:bg-gray-800 hover:text-gray-200 disabled:opacity-50 transition-colors">
            {isLoading ? '로딩 중...' : '새로고침'}
          </button>
          <button onClick={() => setShowModal(true)}
            className="text-sm px-3 py-1.5 bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg transition-colors">
            + 프로젝트 추가
          </button>
        </div>
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

      <div className="flex flex-col gap-2">
        {projects.map((p) => (
          <div key={p.name}
            className="p-4 bg-gray-900 border border-gray-800 rounded-xl flex items-center justify-between hover:border-gray-700 transition-colors">
            <div className="min-w-0">
              <h3 className="font-medium text-gray-100">{p.display_name}</h3>
              <p className="text-sm text-gray-500 truncate max-w-md">{p.path}</p>
            </div>
            <div className="flex items-center gap-3 flex-shrink-0 ml-4">
              <span className={`text-xs px-2 py-1 rounded-full font-medium ${
                p.status === 'active'
                  ? 'bg-emerald-950 text-emerald-400 border border-emerald-800'
                  : 'bg-gray-800 text-gray-500 border border-gray-700'
              }`}>
                {p.status}
              </span>
              <button onClick={() => handleToggleStatus(p.name, p.status)} disabled={togglingId === p.name}
                className="text-xs px-2.5 py-1 border border-gray-700 text-gray-400 rounded-lg hover:bg-gray-800 hover:text-gray-200 disabled:opacity-50 transition-colors">
                {togglingId === p.name ? '...' : p.status === 'active' ? '비활성화' : '활성화'}
              </button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
