import { useState, useEffect, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';

type TrustLevel = 'official' | 'verified' | 'unverified';

interface Plugin {
  id: string;
  name: string;
  version: string;
  trust: TrustLevel;
  description: string;
  installed: boolean;
  builtin?: boolean;
}

const MOCK_PLUGINS: Plugin[] = [
  {
    id: 'com.doxus.confluence',
    name: 'Confluence',
    version: '1.0.0',
    trust: 'official',
    description: 'Confluence Cloud/Server REST API 연동',
    installed: false,
  },
  {
    id: 'com.doxus.github',
    name: 'GitHub',
    version: '1.0.0',
    trust: 'official',
    description: 'GitHub Issues, Wiki, Discussions 연동',
    installed: true,
  },
  {
    id: 'com.doxus.obsidian',
    name: 'Obsidian',
    version: '1.0.0',
    trust: 'official',
    description: 'Obsidian 볼트 연동 (기본 내장)',
    installed: true,
  },
];

type FilterKey = 'all' | TrustLevel;

const FILTERS: { key: FilterKey; label: string }[] = [
  { key: 'all', label: '전체' },
  { key: 'official', label: '공식' },
  { key: 'verified', label: '검증됨' },
  { key: 'unverified', label: '미검증' },
];

const TRUST_BADGE: Record<TrustLevel, { label: string; className: string }> = {
  official: {
    label: '✓ 공식',
    className: 'bg-green-900/50 text-green-400 border border-green-800',
  },
  verified: {
    label: '✓ 검증됨',
    className: 'bg-blue-900/50 text-blue-400 border border-blue-800',
  },
  unverified: {
    label: '⚠ 미검증',
    className: 'bg-yellow-900/50 text-yellow-400 border border-yellow-800',
  },
};

export default function MarketPage() {
  const [plugins, setPlugins] = useState<Plugin[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [query, setQuery] = useState('');
  const [filter, setFilter] = useState<FilterKey>('all');
  const [pendingIds, setPendingIds] = useState<Set<string>>(new Set());

  useEffect(() => {
    invoke('market_list_installed')
      .then((res) => {
        const arr = Array.isArray(res) ? res : (res as { plugins?: Plugin[] })?.plugins ?? null;
        setPlugins(Array.isArray(arr) ? arr : MOCK_PLUGINS);
      })
      .catch(() => setPlugins(MOCK_PLUGINS))
      .finally(() => setIsLoading(false));
  }, []);

  const filtered = useMemo(() => {
    return plugins.filter((p) => {
      const matchesFilter = filter === 'all' || p.trust === filter;
      const q = query.trim().toLowerCase();
      const matchesQuery =
        !q ||
        p.name.toLowerCase().includes(q) ||
        p.description.toLowerCase().includes(q);
      return matchesFilter && matchesQuery;
    });
  }, [plugins, filter, query]);

  const handleToggle = async (plugin: Plugin) => {
    if (pendingIds.has(plugin.id)) return;
    setPendingIds((prev) => new Set(prev).add(plugin.id));

    const command = plugin.installed ? 'market_uninstall_plugin' : 'market_install_plugin';
    try {
      await invoke(command, { pluginId: plugin.id });
    } catch {
      // Tauri 커맨드 미연결 — 낙관적 업데이트
    } finally {
      setPlugins((prev) =>
        prev.map((p) => (p.id === plugin.id ? { ...p, installed: !p.installed } : p))
      );
      setPendingIds((prev) => {
        const next = new Set(prev);
        next.delete(plugin.id);
        return next;
      });
    }
  };

  const installedCount = plugins.filter((p) => p.installed).length;

  return (
    <div className="flex flex-col h-full bg-gray-950 p-6 gap-5">
      {/* 헤더 */}
      <div>
        <h1 className="text-white text-xl font-semibold tracking-tight">플러그인 마켓</h1>
        <p className="text-gray-400 text-sm mt-0.5">문서 소스 플러그인으로 doxus를 확장하세요</p>
      </div>

      {/* 검색 + 필터 */}
      <div className="flex gap-3 flex-wrap">
        <input
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="플러그인 검색..."
          className="flex-1 min-w-48 px-3 py-2 bg-gray-900 border border-gray-800 rounded-lg text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-indigo-500 text-sm"
        />
        <div className="flex gap-1 p-1 bg-gray-900 border border-gray-800 rounded-xl">
          {FILTERS.map((f) => (
            <button
              key={f.key}
              onClick={() => setFilter(f.key)}
              className={`px-3 py-1 rounded-lg text-sm font-medium transition-colors ${
                filter === f.key
                  ? 'bg-indigo-600 text-white'
                  : 'text-gray-400 hover:text-white'
              }`}
            >
              {f.label}
            </button>
          ))}
        </div>
      </div>

      {/* 플러그인 목록 */}
      <div className="flex-1 overflow-auto">
        {isLoading ? (
          <div className="flex items-center justify-center h-32">
            <p className="text-gray-500 text-sm">플러그인 불러오는 중...</p>
          </div>
        ) : filtered.length === 0 ? (
          <div className="flex items-center justify-center h-32">
            <p className="text-gray-500 text-sm">검색 결과가 없습니다.</p>
          </div>
        ) : (
          <div className="grid gap-3">
            {filtered.map((plugin) => {
              const badge = TRUST_BADGE[plugin.trust];
              const isPending = pendingIds.has(plugin.id);

              return (
                <div
                  key={plugin.id}
                  className="bg-gray-900 border border-gray-800 rounded-xl p-4 flex items-start gap-4 hover:border-gray-700 transition-colors"
                >
                  {/* 아이콘 */}
                  <div className="w-10 h-10 rounded-lg bg-gray-800 border border-gray-700 flex items-center justify-center shrink-0">
                    <span className="text-gray-400 text-base font-bold">
                      {plugin.name[0]}
                    </span>
                  </div>

                  {/* 정보 */}
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 flex-wrap">
                      <h3 className="text-white font-semibold">{plugin.name}</h3>
                      <span className="text-gray-600 text-xs">v{plugin.version}</span>
                      <span className={`text-xs px-2 py-0.5 rounded-full font-medium ${badge.className}`}>
                        {badge.label}
                      </span>
                      {plugin.builtin && (
                        <span className="text-xs px-2 py-0.5 rounded-full font-medium bg-gray-800 text-gray-400 border border-gray-700">
                          기본 내장
                        </span>
                      )}
                    </div>
                    <p className="text-gray-400 text-sm mt-1">{plugin.description}</p>
                    <p className="text-gray-600 text-xs mt-1 font-mono">{plugin.id}</p>
                  </div>

                  {/* 액션 */}
                  <div className="shrink-0 pt-0.5">
                    {plugin.builtin ? (
                      <span className="text-xs text-gray-600 px-3 py-1.5">포함됨</span>
                    ) : plugin.installed ? (
                      <button
                        onClick={() => handleToggle(plugin)}
                        disabled={isPending}
                        className="px-3 py-1.5 rounded-lg text-sm border border-gray-700 text-gray-400 hover:text-red-400 hover:border-red-800 disabled:opacity-50 transition-colors"
                      >
                        {isPending ? '...' : '제거'}
                      </button>
                    ) : (
                      <button
                        onClick={() => handleToggle(plugin)}
                        disabled={isPending}
                        className="bg-indigo-600 hover:bg-indigo-700 disabled:opacity-50 text-white px-3 py-1.5 rounded-lg text-sm transition-colors"
                      >
                        {isPending ? '...' : '설치'}
                      </button>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* 푸터 */}
      <p className="text-gray-600 text-xs">
        {installedCount}개 설치됨 · 전체 {plugins.length}개
      </p>
    </div>
  );
}
