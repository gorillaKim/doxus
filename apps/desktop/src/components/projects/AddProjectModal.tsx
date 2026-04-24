import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { useProjectStore } from '../../stores/useProjectStore';

interface ConfigField {
  key: string;
  label: string;
  type: 'text' | 'password' | 'url' | 'email' | 'folder' | 'checkbox';
  required: boolean;
  placeholder: string;
}
type ConfigSchema = ConfigField[];

interface PluginOption {
  id: string;
  label: string;
  description: string;
  icon: string;
  config_schema: ConfigSchema;
}

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

export const AddProjectModal: React.FC<{ onClose: () => void }> = ({ onClose }) => {
  const { addProject } = useProjectStore();
  const [pluginType, setPluginType] = useState<string>('obsidian');
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
        const sorted = [
          ...available.filter((o) => o.id === 'obsidian'),
          ...available.filter((o) => o.id !== 'obsidian'),
        ];
        setAvailableOptions(sorted.length > 0 ? sorted : availableOptions);
      })
      .catch(() => {});
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
    <div className="fixed inset-0 bg-gray-950/80 backdrop-blur-xl flex items-center justify-center z-50 animate-in fade-in duration-500">
      <form
        onSubmit={handleSubmit}
        className="glass-card border-white/10 rounded-[2.5rem] p-10 w-full max-w-xl flex flex-col gap-8 shadow-[0_0_80px_-12px_rgba(99,102,241,0.3)] animate-in zoom-in-95 duration-300 relative overflow-hidden"
      >
        <div className="absolute top-0 right-0 w-64 h-64 bg-indigo-500/10 blur-[100px] -translate-y-1/2 translate-x-1/2" />
        
        <div className="flex items-center justify-between relative z-10">
          <div className="flex flex-col gap-1">
            <h2 className="text-2xl font-extrabold text-white tracking-tight">새 지식 소스 연결</h2>
            <p className="text-sm text-gray-500 font-medium">통합 검색에 포함할 새로운 저장소를 추가합니다.</p>
          </div>
          <button type="button" onClick={onClose} className="w-10 h-10 flex items-center justify-center rounded-2xl bg-white/5 hover:bg-white/10 text-gray-400 hover:text-white transition-all">✕</button>
        </div>

        <div className="flex flex-col gap-4 relative z-10">
          <label className="text-[10px] text-gray-500 font-black uppercase tracking-[0.2em] px-1">소스 유형 선택</label>
          <div className="grid grid-cols-3 gap-4">
            {availableOptions.map((opt) => (
              <button
                key={opt.id}
                type="button"
                onClick={() => { setPluginType(opt.id); setFields({}); }}
                className={`flex flex-col items-center gap-3 p-5 rounded-3xl border transition-all duration-500 ${
                  pluginType === opt.id
                    ? 'border-indigo-500/50 bg-indigo-500/10 text-indigo-300 shadow-[0_0_20px_rgba(99,102,241,0.1)]'
                    : 'border-white/5 bg-white/[0.02] text-gray-500 hover:border-white/10 hover:bg-white/5'
                }`}
              >
                <span className="text-3xl">{opt.icon}</span>
                <span className="font-bold text-xs tracking-tight">{opt.label}</span>
              </button>
            ))}
          </div>
        </div>

        <div className="flex flex-col gap-5 relative z-10">
          {currentOption && (
            <ConfigSchemaForm
              schema={currentOption.config_schema}
              values={fields}
              onChange={(k, v) => setFields((prev) => ({ ...prev, [k]: v }))}
              onPickFolder={handlePickFolder}
            />
          )}
        </div>

        {error && (
          <div className="p-4 bg-red-500/10 border border-red-500/20 rounded-2xl text-red-400 text-xs font-medium relative z-10 flex items-center gap-3">
             <span className="text-lg">⚠️</span>
             {error}
          </div>
        )}

        <div className="flex gap-4 justify-end pt-6 border-t border-white/5 relative z-10">
          <button type="button" onClick={onClose}
            className="px-6 py-3 text-sm font-bold text-gray-500 hover:text-gray-300 transition-colors">
            취소
          </button>
          <button type="submit" disabled={isSubmitting || !canSubmit}
            className="px-8 py-3.5 text-sm font-black bg-white text-gray-950 rounded-2xl hover:bg-indigo-50 hover:scale-[1.02] active:scale-95 disabled:opacity-30 disabled:grayscale transition-all duration-300 shadow-xl shadow-white/5">
            {isSubmitting ? '연결 중...' : '연결 완료'}
          </button>
        </div>
      </form>
    </div>
  );
};
