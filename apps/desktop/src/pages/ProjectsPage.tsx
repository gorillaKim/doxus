import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { useProjectStore } from '../stores/useProjectStore';

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
      await addProject(name.trim(), projectPath);
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
            {availableOptions.map((opt) => (
              <button
                key={opt.id}
                type="button"
                onClick={() => { setPluginType(opt.id); setFields({}); }}
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
