import { useNavigate } from 'react-router-dom';
import { useProjectStore } from '../stores/useProjectStore';
import { useSearchStore } from '../stores/useSearchStore';

function StatCard({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="bg-gray-900 rounded-xl p-5 flex flex-col gap-1 border border-gray-800">
      <span className="text-xs text-gray-500 uppercase tracking-wider">{label}</span>
      <span className="text-2xl font-bold text-white">{value}</span>
    </div>
  );
}

export default function DashboardPage() {
  const navigate = useNavigate();
  const { projects } = useProjectStore();
  const { queryHistory } = useSearchStore();

  const today = new Date().toLocaleDateString('en-US', {
    weekday: 'long',
    year: 'numeric',
    month: 'long',
    day: 'numeric',
  });

  return (
    <div className="flex flex-col gap-8 max-w-3xl">
      {/* Header */}
      <div className="flex flex-col gap-1">
        <h1 className="text-3xl font-bold text-white tracking-tight">Welcome to doxus</h1>
        <p className="text-sm text-gray-500">{today}</p>
      </div>

      {/* Stats */}
      <div className="grid grid-cols-3 gap-4">
        <StatCard label="Projects" value={projects.length} />
        <StatCard label="Indexed Documents" value="—" />
        <StatCard label="Last Sync" value="—" />
      </div>

      {/* Recent searches */}
      <div className="flex flex-col gap-3">
        <h2 className="text-sm font-semibold text-gray-400 uppercase tracking-wider">Recent Searches</h2>
        {queryHistory.length === 0 ? (
          <p className="text-gray-600 text-sm">No recent searches</p>
        ) : (
          <ul className="flex flex-col gap-1">
            {queryHistory.map((q, i) => (
              <li
                key={i}
                className="text-sm text-gray-300 bg-gray-900 border border-gray-800 rounded-lg px-4 py-2 hover:border-gray-700 cursor-pointer transition-colors"
                onClick={() => navigate('/search')}
              >
                {q}
              </li>
            ))}
          </ul>
        )}
      </div>

      {/* Quick actions */}
      <div className="flex gap-3">
        <button
          onClick={() => navigate('/projects')}
          className="px-4 py-2 bg-indigo-600 hover:bg-indigo-500 text-white text-sm font-medium rounded-lg transition-colors"
        >
          Add Project
        </button>
        <button
          onClick={() => navigate('/search')}
          className="px-4 py-2 bg-gray-800 hover:bg-gray-700 text-gray-200 text-sm font-medium rounded-lg border border-gray-700 transition-colors"
        >
          Search
        </button>
      </div>
    </div>
  );
}
