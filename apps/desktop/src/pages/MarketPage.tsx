import { useState, useEffect, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { usePluginStore } from '../stores/usePluginStore';

type TrustLevel = 'official' | 'verified' | 'unverified';

interface ConfigField {
  key: string;
  label: string;
  type: 'text' | 'password' | 'url' | 'email' | 'folder';
  required: boolean;
  placeholder: string;
}

interface Plugin {
  id: string;
  name: string;
  version: string;
  trust: TrustLevel;
  description: string;
  installed: boolean;
  builtin?: boolean;
  auth_type?: 'none' | 'api_token' | 'oauth';
  auth_schema?: ConfigField[];
}

const MOCK_PLUGINS: Plugin[] = [
  {
    id: 'com.doxus.confluence',
    name: 'Confluence',
    version: '1.0.0',
    trust: 'official',
    description: 'Confluence Cloud/Server REST API 연동',
    installed: false,
    auth_type: 'oauth',
    auth_schema: [
      { key: 'client_id', label: 'Client ID', type: 'text', required: true, placeholder: 'your-atlassian-app-client-id' },
    ],
  },
  {
    id: 'com.doxus.github',
    name: 'GitHub',
    version: '1.0.0',
    trust: 'official',
    description: 'GitHub Issues, Wiki, Discussions 연동',
    installed: true,
    auth_type: 'api_token',
    auth_schema: [
      { key: 'token', label: 'Personal Access Token', type: 'password', required: false, placeholder: 'ghp_••••••••' },
    ],
  },
  {
    id: 'com.doxus.obsidian',
    name: 'Obsidian',
    version: '1.0.0',
    trust: 'official',
    description: 'Obsidian 볼트 연동 (기본 내장)',
    installed: true,
    auth_type: 'none',
    auth_schema: [],
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

function PluginSettingsModal({ plugin, onClose, onAuthChange }: { plugin: Plugin; onClose: () => void; onAuthChange: (pluginId: string, configured: boolean) => void }) {
  const [fields, setFields] = useState<Record<string, string>>({});
  const [isSaving, setIsSaving] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const [authStatus, setAuthStatus] = useState<boolean | null>(null);
  const [oauthStep, setOauthStep] = useState<'idle' | 'waiting' | 'done'>('idle');

  useEffect(() => {
    invoke<{ configured: boolean }>('plugin_get_auth_status', { pluginId: plugin.id })
      .then((res) => setAuthStatus(res.configured))
      .catch(() => setAuthStatus(false));
  }, [plugin.id]);

  const handleSave = async () => {
    setIsSaving(true);
    setResult(null);
    try {
      await invoke('plugin_save_auth', { pluginId: plugin.id, authFields: fields });
      setResult('✓ 저장되었습니다');
      setAuthStatus(true);
      onAuthChange(plugin.id, true);
    } catch (e) {
      setResult(`✗ ${String(e)}`);
    } finally {
      setIsSaving(false);
    }
  };

  const handleOAuth = async () => {
    const clientId = (fields['client_id'] ?? '').trim();
    const clientSecret = (fields['client_secret'] ?? '').trim();
    if (!clientId || !clientSecret) return;
    setResult(null);

    try {
      // 1. Start OAuth — get auth URL
      const res = await invoke<{ auth_url: string }>('plugin_start_oauth', {
        pluginId: plugin.id,
        clientId,
        clientSecret,
      });

      // 2. 브라우저 열기 전에 리스너 먼저 등록 (race condition 방지)
      const { listen } = await import('@tauri-apps/api/event');
      const eventName = `oauth-callback-${plugin.id.replace(/\./g, "_")}`;
      // 3. 리스너 등록 후 브라우저 오픈
      const unlisten = await listen<string>(eventName, async (event) => {
        unlisten();
        const callbackUrl = event.payload;
        const url = new URL(callbackUrl);
        const code = url.searchParams.get('code');
        if (!code) {
          setResult('✗ OAuth 콜백에서 code를 찾을 수 없습니다');
          setOauthStep('idle');
          return;
        }

        // 4. Exchange code for token
        try {
          await invoke('plugin_oauth_exchange', { pluginId: plugin.id, code });
          setOauthStep('done');
          setAuthStatus(true);
          onAuthChange(plugin.id, true);
          setResult('✓ OAuth 인증 완료! 토큰이 키체인에 저장됐습니다.');
        } catch (e) {
          setResult(`✗ 토큰 교환 실패: ${String(e)}`);
          setOauthStep('idle');
        }
      });

      // 브라우저 열기 (리스너 등록 완료 후)
      await invoke('plugin_open_url', { url: res.auth_url });
      setOauthStep('waiting');
    } catch (e) {
      setResult(`✗ OAuth 시작 실패: ${String(e)}`);
    }
  };

  const authType = plugin.auth_type ?? 'none';
  const schema = plugin.auth_schema ?? [];

  return (
    <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50">
      <div className="bg-gray-900 border border-gray-800 rounded-2xl p-6 w-full max-w-md flex flex-col gap-5 shadow-2xl">
        {/* Header */}
        <div className="flex items-center justify-between">
          <div>
            <h2 className="text-base font-semibold text-gray-100">{plugin.name} 설정</h2>
            <p className="text-xs text-gray-500 mt-0.5">{plugin.id}</p>
          </div>
          <button type="button" onClick={onClose} className="text-gray-500 hover:text-gray-300 text-lg">✕</button>
        </div>

        {/* Auth status badge */}
        {authStatus !== null && (
          <div className={`flex items-center gap-2 text-xs px-3 py-2 rounded-lg border ${
            authStatus
              ? 'bg-emerald-950 border-emerald-800 text-emerald-400'
              : 'bg-gray-800 border-gray-700 text-gray-400'
          }`}>
            <span>{authStatus ? '●' : '○'}</span>
            <span>{authStatus ? '인증 완료' : '인증 미설정'}</span>
          </div>
        )}

        {/* Auth UI by type */}
        {authType === 'none' && (
          <p className="text-sm text-gray-400 text-center py-4">이 플러그인은 별도 인증이 필요하지 않습니다.</p>
        )}

        {authType === 'api_token' && (
          <div className="flex flex-col gap-3">
            <p className="text-xs text-gray-500">API 토큰을 입력하세요. 키체인에 안전하게 저장됩니다.</p>
            {schema.map((field) => (
              <div key={field.key} className="flex flex-col gap-1">
                <label className="text-xs text-gray-500">
                  {field.label}{field.required && <span className="text-red-400 ml-0.5">*</span>}
                </label>
                <input
                  type={field.type === 'password' ? 'password' : field.type === 'email' ? 'email' : 'text'}
                  value={fields[field.key] ?? ''}
                  onChange={(e) => setFields((prev) => ({ ...prev, [field.key]: e.target.value }))}
                  placeholder={field.placeholder}
                  className="bg-gray-800 border border-gray-700 rounded-lg px-3 py-2 text-sm text-gray-100 placeholder-gray-600 focus:outline-none focus:border-indigo-500"
                />
              </div>
            ))}
          </div>
        )}

        {authType === 'oauth' && (
          <div className="flex flex-col gap-4">
            <p className="text-xs text-gray-500">
              Atlassian Developer Console에서 발급한 Client ID와 Client Secret을 입력하세요.
              <br />앱 설정의 Authorization → Callback URL에{' '}
              <code className="text-indigo-400">http://localhost:14920</code>를 등록하세요.
            </p>
            <div className="flex flex-col gap-1">
              <label className="text-xs text-gray-500">Client ID <span className="text-red-400">*</span></label>
              <input
                type="text"
                value={fields['client_id'] ?? ''}
                onChange={(e) => setFields((prev) => ({ ...prev, client_id: e.target.value }))}
                placeholder="your-atlassian-app-client-id"
                className="bg-gray-800 border border-gray-700 rounded-lg px-3 py-2 text-sm text-gray-100 placeholder-gray-600 focus:outline-none focus:border-indigo-500"
              />
            </div>
            <div className="flex flex-col gap-1">
              <label className="text-xs text-gray-500">Client Secret <span className="text-red-400">*</span></label>
              <input
                type="password"
                value={fields['client_secret'] ?? ''}
                onChange={(e) => setFields((prev) => ({ ...prev, client_secret: e.target.value }))}
                placeholder="your-atlassian-app-client-secret"
                className="bg-gray-800 border border-gray-700 rounded-lg px-3 py-2 text-sm text-gray-100 placeholder-gray-600 focus:outline-none focus:border-indigo-500"
              />
            </div>
            {oauthStep === 'idle' && (
              <button
                onClick={handleOAuth}
                disabled={!(fields['client_id'] ?? '').trim() || !(fields['client_secret'] ?? '').trim()}
                className="px-4 py-2 bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 text-white rounded-lg text-sm transition-colors"
              >
                브라우저에서 Atlassian 로그인
              </button>
            )}
            {oauthStep === 'waiting' && (
              <div className="flex items-center gap-2 text-sm text-yellow-400">
                <span className="animate-pulse">●</span>
                <span>브라우저에서 인증 대기 중... (콜백을 기다리는 중)</span>
              </div>
            )}
            {oauthStep === 'done' && (
              <div className="flex items-center gap-2 text-sm text-emerald-400">
                <span>●</span>
                <span>인증 완료!</span>
              </div>
            )}
          </div>
        )}

        {result && (
          <p className={`text-xs px-3 py-2 rounded-lg border ${
            result.startsWith('✓')
              ? 'bg-emerald-950 border-emerald-800 text-emerald-400'
              : 'bg-red-950 border-red-800 text-red-400'
          }`}>{result}</p>
        )}

        {/* Footer */}
        <div className="flex gap-2 justify-end">
          <button type="button" onClick={onClose}
            className="px-3 py-1.5 text-sm text-gray-400 hover:text-gray-200 transition-colors">
            닫기
          </button>
          {authType === 'api_token' && (
            <button
              onClick={handleSave}
              disabled={isSaving || schema.filter(f => f.required).some(f => !(fields[f.key] ?? '').trim())}
              className="px-4 py-1.5 text-sm bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg disabled:opacity-50 transition-colors"
            >
              {isSaving ? '저장 중...' : '저장'}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

export default function MarketPage() {
  const [plugins, setPlugins] = useState<Plugin[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [query, setQuery] = useState('');
  const [filter, setFilter] = useState<FilterKey>('all');
  const [pendingIds, setPendingIds] = useState<Set<string>>(new Set());
  const [settingsPlugin, setSettingsPlugin] = useState<Plugin | null>(null);
  const { authStates, fetchAuthStatus, setConfigured } = usePluginStore();

  useEffect(() => {
    invoke('market_list_installed')
      .then((res) => {
        const arr = Array.isArray(res) ? res : (res as { plugins?: Plugin[] })?.plugins ?? null;
        const list: Plugin[] = Array.isArray(arr) ? arr : MOCK_PLUGINS;
        setPlugins(list);
        // 설치된 플러그인 인증 상태 일괄 조회
        list.filter((p) => p.installed).forEach((p) => fetchAuthStatus(p.id));
      })
      .catch(() => setPlugins(MOCK_PLUGINS))
      .finally(() => setIsLoading(false));
  }, [fetchAuthStatus]);

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
      {settingsPlugin && (
        <PluginSettingsModal
          plugin={settingsPlugin}
          onClose={() => setSettingsPlugin(null)}
          onAuthChange={(pluginId, configured) => setConfigured(pluginId, configured)}
        />
      )}

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
              const isAuthenticated = authStates[plugin.id]?.configured ?? false;

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
                      {plugin.installed && isAuthenticated && (
                        <span className="text-xs px-2 py-0.5 rounded-full font-medium bg-emerald-950 text-emerald-400 border border-emerald-800">
                          ● 인증됨
                        </span>
                      )}
                      {plugin.installed && !isAuthenticated && plugin.auth_type !== 'none' && (
                        <span className="text-xs px-2 py-0.5 rounded-full font-medium bg-yellow-950 text-yellow-500 border border-yellow-800">
                          ○ 미인증
                        </span>
                      )}
                    </div>
                    <p className="text-gray-400 text-sm mt-1">{plugin.description}</p>
                    <p className="text-gray-600 text-xs mt-1 font-mono">{plugin.id}</p>
                  </div>

                  {/* 액션 */}
                  <div className="shrink-0 pt-0.5 flex flex-col gap-1.5 items-end">
                    {/* Settings button for any installed plugin */}
                    {plugin.installed && (
                      <button
                        onClick={() => setSettingsPlugin(plugin)}
                        className="px-3 py-1.5 rounded-lg text-sm border border-gray-700 text-gray-400 hover:text-indigo-400 hover:border-indigo-800 transition-colors"
                      >
                        설정
                      </button>
                    )}
                    {/* Install/remove button */}
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
