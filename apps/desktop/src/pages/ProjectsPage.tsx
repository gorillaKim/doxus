import { useEffect } from 'react';
import { useProjectStore } from '../stores/useProjectStore';

export function ProjectsPage() {
  const { projects, isLoading, error, fetch } = useProjectStore();

  useEffect(() => {
    fetch();
  }, [fetch]);

  return (
    <div className="flex flex-col h-full p-4 gap-4">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold text-gray-900">Projects</h1>
        <button
          onClick={fetch}
          disabled={isLoading}
          className="text-sm px-3 py-1.5 border border-gray-300 rounded-md hover:bg-gray-50 disabled:opacity-50"
        >
          {isLoading ? 'Loading...' : 'Refresh'}
        </button>
      </div>

      {error && (
        <div className="p-3 bg-red-50 border border-red-200 rounded-md text-red-700 text-sm">
          {error}
        </div>
      )}

      {projects.length === 0 && !isLoading && (
        <div className="text-center py-12 text-gray-500">
          <p>No projects yet.</p>
          <p className="text-sm mt-1">
            Add one with:{' '}
            <code className="bg-gray-100 px-1 rounded">doxus project add &lt;name&gt; &lt;path&gt;</code>
          </p>
        </div>
      )}

      <div className="space-y-2">
        {projects.map((p) => (
          <div
            key={p.name}
            className="p-4 bg-white border border-gray-200 rounded-lg flex items-center justify-between"
          >
            <div>
              <h3 className="font-medium text-gray-900">{p.display_name}</h3>
              <p className="text-sm text-gray-500 truncate max-w-md">{p.path}</p>
            </div>
            <span
              className={`text-xs px-2 py-1 rounded-full font-medium ${
                p.status === 'active'
                  ? 'bg-green-100 text-green-700'
                  : 'bg-gray-100 text-gray-500'
              }`}
            >
              {p.status}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
