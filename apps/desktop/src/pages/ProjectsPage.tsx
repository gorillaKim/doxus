import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { useProjectStore } from '../stores/useProjectStore';
import { usePluginStore } from '../stores/usePluginStore';

// PluginType is now an open string — supports any installed plugin id
type PluginType = string;

interface ConfigField {
  key: string;
  label: string;
  type: 'text' | 'password' | 'url' | 'email' | 'folder' | 'checkbox';
  required: boolean;
  placeholder: string;
}
type ConfigSchema = ConfigField[];

interface PluginOption {
  id: PluginType;
  label: string;
  description: string;
  icon: string;
  config_schema: ConfigSchema;
}

// Known built-in plugin metadata (used as display hints)
const KNOWN_PLUGINS: Record<string, Omit<PluginOption, 'id' | 'config_schema'>> = {
  'obsidian':   { label: 'Obsidian',   description: '로컬 Obsidian 볼트 폴더',               icon: '🪨' },
  'confluence': { label: 'Confluence', description: 'Confluence Cloud 또는 Server',           icon: '📄' },
  'github':     { label: 'GitHub',     description: 'GitHub Issues / Wiki / Discussions',     icon: '🐙' },
};

function pluginMeta(sourceType: string) {
  const short = sourceType.replace(/^com\.doxus\./, '');
  const pluginId = `com.doxus.${short}`;
  const base = KNOWN_PLUGINS[short] ?? { label: short, description: '', icon: '🔌' };
  return { ...base, icon: usePluginStore.getState().getEmoji(pluginId) };
}

function ConfigSchemaForm({
  schema,
  values,
  onChange,
  onPickFolder,
}: {
  schema: ConfigSchema;
  values: Record<string, string>;
  onChange: (key: string, val: string) => void;
  onPickFolder?: (key: string) => void;
}) {
  return (
    <>
      {schema.map((field) => (
        <div key={field.key} className="flex flex-col gap-1">
          <label className="text-xs text-gray-500">
            {field.label}{field.required && <span className="text-red-400 ml-0.5">*</span>}
          </label>
          {field.type === 'folder' ? (
            <div className="flex gap-2">
              <input
                type="text"
                value={values[field.key] ?? ''}
                onChange={(e) => onChange(field.key, e.target.value)}
                placeholder={field.placeholder}
                className="flex-1 bg-gray-800 border border-gray-700 rounded-lg px-3 py-2 text-sm text-gray-100 placeholder-gray-600 focus:outline-none focus:border-indigo-500"
              />
              <button type="button" onClick={() => onPickFolder?.(field.key)}
                className="px-3 py-2 bg-gray-700 hover:bg-gray-600 text-gray-200 text-sm rounded-lg transition-colors">
                찾기…
              </button>
            </div>
          ) : (
            <input
              type={field.type === 'password' ? 'password' : field.type === 'email' ? 'email' : 'text'}
              value={values[field.key] ?? ''}
              onChange={(e) => onChange(field.key, e.target.value)}
              placeholder={field.placeholder}
              className="bg-gray-800 border border-gray-700 rounded-lg px-3 py-2 text-sm text-gray-100 placeholder-gray-600 focus:outline-none focus:border-indigo-500"
            />
          )}
        </div>
      ))}
    </>
  );
}

function AddProjectModal({ onClose }: { onClose: () => void }) {
  const { addProject } = useProjectStore();
  const [pluginType, setPluginType] = useState<PluginType>('obsidian');
  const [fields, setFields] = useState<Record<string, string>>({});
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [availableOptions, setAvailableOptions] = useState<PluginOption[]>([
    {
      id: 'obsidian',
      ...KNOWN_PLUGINS['obsidian'],
      config_schema: [
        { key: 'path', label: '볼트 폴더', type: 'folder', required: true, placeholder: '/Users/you/MyVault' },
        { key: 'name', label: '프로젝트 이름', type: 'text', required: true, placeholder: 'my-vault' },
      ],
    },
  ]);

  useEffect(() => {
    invoke<{ plugins: { id: string; name: string; description: string; installed: boolean; builtin?: boolean; config_schema?: ConfigSchema }[] }>('market_list_installed')
      .then(({ plugins }) => {
        const available: PluginOption[] = plugins
          .filter((p) => p.installed)
          .map((p) => {
            // Strip com.doxus. prefix to get short id
            const shortId = p.id.replace(/^com\.doxus\./, '');
            const known = KNOWN_PLUGINS[shortId];
            return {
              id: shortId,
              label: known?.label ?? p.name,
              description: known?.description ?? p.description,
              icon: known?.icon ?? '🔌',
              config_schema: p.config_schema ?? [],
            };
          });
        // Always ensure obsidian is first
        const sorted = [
          ...available.filter((o) => o.id === 'obsidian'),
          ...available.filter((o) => o.id !== 'obsidian'),
        ];
        setAvailableOptions(sorted.length > 0 ? sorted : availableOptions);
        if (!sorted.find((o) => o.id === pluginType)) {
          setPluginType(sorted[0]?.id ?? 'obsidian');
        }
      })
      .catch(() => {
        // keep default availableOptions
      });
  }, []);

  const currentOption = availableOptions.find((o) => o.id === pluginType) ?? availableOptions[0];

  const handlePickFolder = async (key: string) => {
    const selected = await open({ directory: true, multiple: false, title: 'Select Folder' });
    if (selected && typeof selected === 'string') {
      setFields((prev) => ({ ...prev, [key]: selected }));
    }
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setIsSubmitting(true);
    setError(null);
    try {
      const projectPath = fields.path ?? fields.base_url ?? fields.repo ?? fields.endpoint ?? pluginType;
      const name = fields.name ?? '';
      // name/path 제외한 나머지 필드를 config로 전달
      const { name: _n, path: _p, ...configFields } = fields;
      await addProject(name.trim(), projectPath, pluginType, configFields);
      onClose();
    } catch (e) {
      setError(String(e));
      setIsSubmitting(false);
    }
  };

  const canSubmit = currentOption?.config_schema.every(
    (f) => !f.required || (fields[f.key] ?? '').trim() !== ''
  ) ?? false;

  return (
    <div className="fixed inset-0 bg-gray-950/60 backdrop-blur-md flex items-center justify-center z-50 animate-in fade-in duration-300">
      <form
        onSubmit={handleSubmit}
        className="glass-card border-white/10 rounded-3xl p-8 w-full max-w-lg flex flex-col gap-6 shadow-[0_0_50px_-12px_rgba(99,102,241,0.25)] animate-in zoom-in-95 duration-300"
      >
        <div className="flex items-center justify-between">
          <div className="flex flex-col gap-1">
            <h2 className="text-xl font-bold text-white tracking-tight">새 지식 저장소 추가</h2>
            <p className="text-xs text-gray-500">통합 검색에 포함할 새로운 소스를 연결합니다.</p>
          </div>
          <button type="button" onClick={onClose} className="w-8 h-8 flex items-center justify-center rounded-full hover:bg-white/5 text-gray-500 hover:text-white transition-colors">✕</button>
        </div>

        {/* Plugin type selector */}
        <div className="flex flex-col gap-3">
          <label className="text-[10px] text-gray-500 font-bold uppercase tracking-widest px-1">소스 유형 선택</label>
          <div className="grid grid-cols-3 gap-3">
            {availableOptions.map((opt) => (
              <button
                key={opt.id}
                type="button"
                onClick={() => { setPluginType(opt.id); setFields({}); }}
                className={`flex flex-col items-center gap-2 p-4 rounded-2xl border text-sm transition-all duration-300 ${
                  pluginType === opt.id
                    ? 'border-indigo-500 bg-indigo-500/10 text-indigo-300 shadow-lg shadow-indigo-500/10'
                    : 'border-white/5 bg-white/[0.02] text-gray-500 hover:border-white/10 hover:bg-white/5'
                }`}
              >
                <span className="text-2xl mb-1">{opt.icon}</span>
                <span className="font-bold tracking-tight">{opt.label}</span>
              </button>
            ))}
          </div>
        </div>

        {/* Schema-driven fields */}
        {currentOption && (
          <ConfigSchemaForm
            schema={currentOption.config_schema}
            values={fields}
            onChange={(k, v) => setFields((prev) => ({ ...prev, [k]: v }))}
            onPickFolder={handlePickFolder}
          />
        )}

        {error && (
          <p className="text-xs text-red-400 bg-red-950 border border-red-800 rounded-lg px-3 py-2">{error}</p>
        )}

        <div className="flex gap-4 justify-end pt-4 border-t border-white/5">
          <button type="button" onClick={onClose}
            className="px-4 py-2 text-sm font-semibold text-gray-500 hover:text-gray-200 transition-colors">
            닫기
          </button>
          <button type="submit" disabled={isSubmitting || !canSubmit}
            className="px-6 py-2.5 text-sm font-bold bg-white text-gray-950 rounded-xl hover:bg-gray-200 disabled:opacity-30 transition-all duration-300">
            {isSubmitting ? '추가 중...' : '연결 완료'}
          </button>
        </div>
      </form>
    </div>
  );
}

export function ProjectsPage() {
  const { projects, isLoading, error, fetch, toggleStatus, indexProject, indexingNames, removeProject } = useProjectStore();
  usePluginStore((s) => s.emojiMap); // emoji 변경 시 리렌더 트리거
  const [showModal, setShowModal] = useState(false);
  const [togglingId, setTogglingId] = useState<string | null>(null);
  const [removingId, setRemovingId] = useState<string | null>(null);
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

  return (
    <div className="flex flex-col h-full gap-5 max-w-3xl">
      {showModal && <AddProjectModal onClose={() => setShowModal(false)} />}

      {indexResult && (
        <div className="fixed bottom-6 right-6 z-50 px-4 py-3 bg-gray-900 border border-gray-700 rounded-xl shadow-xl text-sm text-gray-200 max-w-xs">
          <span className="font-medium text-indigo-400">{indexResult.name}</span>
          <span className="ml-2">{indexResult.message}</span>
        </div>
      )}

      <div className="flex items-center justify-between mb-2">
        <div className="flex flex-col gap-1">
          <h1 className="text-3xl font-extrabold text-white tracking-tight">프로젝트 매니저</h1>
          <p className="text-xs text-gray-500">연결된 모든 지식 소스를 관리합니다.</p>
        </div>
        <div className="flex gap-2">
          <button onClick={() => setShowModal(true)}
            className="px-5 py-2.5 bg-indigo-600 hover:bg-indigo-500 text-white text-sm font-bold rounded-xl shadow-lg shadow-indigo-600/20 transition-all duration-300 transform hover:-translate-y-0.5">
            + 새 프로젝트
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

      {(() => {
        // Group by source_type
        const groups = projects.reduce<Record<string, typeof projects>>((acc, p) => {
          const key = p.source_type ?? 'obsidian';
          (acc[key] ??= []).push(p);
          return acc;
        }, {});
        return Object.entries(groups).map(([srcType, items]) => {
          const { label, icon } = pluginMeta(srcType);
          return (
            <div key={srcType} className="flex flex-col gap-2">
              <div className="flex items-center gap-2 px-1">
                <span className="text-base">{icon}</span>
                <span className="text-xs font-semibold text-gray-500 uppercase tracking-wider">{label}</span>
                <span className="text-xs text-gray-700">({items.length})</span>
              </div>
              <div className="grid grid-cols-1 gap-4">
                {items.map((p) => (
                  <div key={p.name}
                    className="glass-card border-white/5 rounded-2xl p-5 flex flex-col gap-5 hover:border-white/10 transition-all duration-300 group">
                    <div className="flex items-start justify-between">
                      <div className="flex flex-col gap-1 min-w-0">
                        <div className="flex items-center gap-2">
                          <h3 className="font-bold text-gray-100 truncate">{p.display_name}</h3>
                          <span className={`text-[9px] px-1.5 py-0.5 rounded uppercase font-bold tracking-tighter ${
                            p.status === 'active'
                              ? 'bg-emerald-500/10 text-emerald-400 ring-1 ring-emerald-500/20'
                              : 'bg-gray-800 text-gray-500'
                          }`}>
                            {p.status}
                          </span>
                        </div>
                        <p className="text-xs text-gray-500 font-mono truncate">{p.path}</p>
                      </div>
                      
                      <div className="flex items-center gap-1.5">
                        <button
                          onClick={() => handleIndex(p.name)}
                          disabled={indexingNames.has(p.name)}
                          className="px-3 py-1.5 bg-white/5 hover:bg-indigo-500 text-gray-300 hover:text-white rounded-lg text-[11px] font-bold transition-all duration-300 disabled:opacity-30"
                        >
                          {indexingNames.has(p.name) ? 'Indexing...' : 'Index Now'}
                        </button>
                      </div>
                    </div>

                    <div className="flex items-center justify-between pt-4 border-t border-white/5">
                      <div className="flex items-center gap-4">
                        <button onClick={() => handleToggleStatus(p.name, p.status)} disabled={togglingId === p.name}
                          className="text-[11px] font-semibold text-gray-500 hover:text-gray-200 transition-colors">
                          {togglingId === p.name ? '...' : p.status === 'active' ? '비활성화' : '활성화'}
                        </button>
                      </div>
                      <button onClick={() => handleRemove(p.name, p.display_name)} disabled={removingId === p.name}
                        className="text-[11px] font-semibold text-gray-600 hover:text-red-400 transition-colors">
                        {removingId === p.name ? '...' : '삭제'}
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          );
        });
      })()}
    </div>
  );
}
