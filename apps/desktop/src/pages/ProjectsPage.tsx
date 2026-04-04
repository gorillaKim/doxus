import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useProjectStore } from '../stores/useProjectStore';

function AddProjectForm({ onClose, onSuccess }: { onClose: () => void; onSuccess: () => void }) {
  const [name, setName] = useState('');
  const [path, setPath] = useState('');
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim() || !path.trim()) return;
    setIsSubmitting(true);
    setError(null);
    try {
      await invoke('add_project', { name: name.trim(), path: path.trim() });
      onSuccess();
      onClose();
    } catch (e) {
      setError(String(e));
      setIsSubmitting(false);
    }
  };

  return (
    <form
      onSubmit={handleSubmit}
      className="bg-gray-900 border border-gray-700 rounded-xl p-5 flex flex-col gap-4"
    >
      <h2 className="text-sm font-semibold text-gray-200 uppercase tracking-wider">Add Project</h2>
      {error && (
        <p className="text-xs text-red-400 bg-red-950 border border-red-800 rounded-lg px-3 py-2">
          {error}
        </p>
      )}
      <div className="flex flex-col gap-1">
        <label className="text-xs text-gray-500">Name</label>
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="my-vault"
          className="bg-gray-800 border border-gray-700 rounded-lg px-3 py-2 text-sm text-gray-100 placeholder-gray-600 focus:outline-none focus:border-indigo-500"
          autoFocus
        />
      </div>
      <div className="flex flex-col gap-1">
        <label className="text-xs text-gray-500">Path</label>
        <input
          type="text"
          value={path}
          onChange={(e) => setPath(e.target.value)}
          placeholder="/Users/you/vault"
          className="bg-gray-800 border border-gray-700 rounded-lg px-3 py-2 text-sm text-gray-100 placeholder-gray-600 focus:outline-none focus:border-indigo-500"
        />
      </div>
      <div className="flex gap-2 justify-end">
        <button
          type="button"
          onClick={onClose}
          className="px-3 py-1.5 text-sm text-gray-400 hover:text-gray-200 transition-colors"
        >
          Cancel
        </button>
        <button
          type="submit"
          disabled={isSubmitting || !name.trim() || !path.trim()}
          className="px-4 py-1.5 text-sm bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg disabled:opacity-50 transition-colors"
        >
          {isSubmitting ? 'Adding...' : 'Add'}
        </button>
      </div>
    </form>
  );
}

export function ProjectsPage() {
  const { projects, isLoading, error, fetch } = useProjectStore();
  const [showForm, setShowForm] = useState(false);
  const [togglingId, setTogglingId] = useState<string | null>(null);

  useEffect(() => {
    fetch();
  }, [fetch]);

  const handleToggleStatus = async (name: string, currentStatus: 'active' | 'disabled') => {
    setTogglingId(name);
    try {
      await invoke('toggle_project_status', { name, status: currentStatus === 'active' ? 'disabled' : 'active' });
      await fetch();
    } catch (e) {
      console.log('toggle_project_status not yet implemented:', e);
    } finally {
      setTogglingId(null);
    }
  };

  return (
    <div className="flex flex-col h-full gap-5 max-w-3xl">
      {/* Header */}
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold text-gray-100">Projects</h1>
        <div className="flex gap-2">
          <button
            onClick={fetch}
            disabled={isLoading}
            className="text-sm px-3 py-1.5 border border-gray-700 text-gray-400 rounded-lg hover:bg-gray-800 hover:text-gray-200 disabled:opacity-50 transition-colors"
          >
            {isLoading ? 'Loading...' : 'Refresh'}
          </button>
          <button
            onClick={() => setShowForm((v) => !v)}
            className="text-sm px-3 py-1.5 bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg transition-colors"
          >
            {showForm ? 'Cancel' : 'Add Project'}
          </button>
        </div>
      </div>

      {/* Inline form */}
      {showForm && (
        <AddProjectForm
          onClose={() => setShowForm(false)}
          onSuccess={() => fetch()}
        />
      )}

      {/* Error */}
      {error && (
        <div className="p-3 bg-red-950 border border-red-800 rounded-lg text-red-400 text-sm">
          {error}
        </div>
      )}

      {/* Empty state */}
      {projects.length === 0 && !isLoading && (
        <div className="text-center py-12 text-gray-600">
          <p className="text-gray-500">No projects yet.</p>
          <p className="text-sm mt-1">
            Use <span className="text-gray-400">"Add Project"</span> above or run:{' '}
            <code className="bg-gray-800 text-gray-400 px-1.5 py-0.5 rounded text-xs">
              doxus project add &lt;name&gt; &lt;path&gt;
            </code>
          </p>
        </div>
      )}

      {/* Project list */}
      <div className="flex flex-col gap-2">
        {projects.map((p) => (
          <div
            key={p.name}
            className="p-4 bg-gray-900 border border-gray-800 rounded-xl flex items-center justify-between hover:border-gray-700 transition-colors"
          >
            <div className="min-w-0">
              <h3 className="font-medium text-gray-100">{p.display_name}</h3>
              <p className="text-sm text-gray-500 truncate max-w-md">{p.path}</p>
            </div>
            <div className="flex items-center gap-3 flex-shrink-0 ml-4">
              <span
                className={`text-xs px-2 py-1 rounded-full font-medium ${
                  p.status === 'active'
                    ? 'bg-emerald-950 text-emerald-400 border border-emerald-800'
                    : 'bg-gray-800 text-gray-500 border border-gray-700'
                }`}
              >
                {p.status}
              </span>
              <button
                onClick={() => handleToggleStatus(p.name, p.status)}
                disabled={togglingId === p.name}
                className="text-xs px-2.5 py-1 border border-gray-700 text-gray-400 rounded-lg hover:bg-gray-800 hover:text-gray-200 disabled:opacity-50 transition-colors"
              >
                {togglingId === p.name
                  ? '...'
                  : p.status === 'active'
                  ? 'Disable'
                  : 'Enable'}
              </button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
