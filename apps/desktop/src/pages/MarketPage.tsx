import { useState, useEffect, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import rehypeHighlight from 'rehype-highlight';
import 'highlight.js/styles/github-dark.css';
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
  guide_url?: string;
}

interface RegistryEntry {
  plugin_id: string;
  version: string;
  display_name: string;
  download_url: string;
  checksum_sha256: string;
  public_key_hex: string;
  auth_type?: string;
  guide_url?: string;
}

const PLUGIN_AUTH_SCHEMAS: Record<string, ConfigField[]> = {
  'com.doxus.confluence': [
    { key: 'email', label: 'Atlassian 계정 이메일', type: 'email', required: true, placeholder: 'you@company.com' },
    { key: 'api_token', label: 'Personal API Token', type: 'password', required: true, placeholder: 'ATATT3xFfGF...' },
  ],
  'com.doxus.github': [
    { key: 'token', label: 'Personal Access Token', type: 'password', required: true, placeholder: 'ghp_••••••••' },
  ],
};

function registryEntryToPlugin(entry: RegistryEntry, installedIds: Set<string>): Plugin {
  return {
    id: entry.plugin_id,
    name: entry.display_name,
    version: entry.version,
    trust: 'verified' as TrustLevel,
    description: `${entry.display_name} (${entry.plugin_id})`,
    installed: installedIds.has(entry.plugin_id),
    auth_type: (entry.auth_type || 'none') as Plugin['auth_type'],
    auth_schema: PLUGIN_AUTH_SCHEMAS[entry.plugin_id],
    guide_url: entry.guide_url,
  };
}

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

function PluginGuideModal({ plugin, onClose }: { plugin: Plugin; onClose: () => void }) {
  const [content, setContent] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!plugin.guide_url) {
      setError('이 플러그인에는 가이드가 없습니다.');
      setIsLoading(false);
      return;
    }
    invoke<string>('market_fetch_guide', { guideUrl: plugin.guide_url })
      .then((md) => setContent(md))
      .catch((e) => setError(String(e)))
      .finally(() => setIsLoading(false));
  }, [plugin.guide_url]);

  return (
    <div className="fixed inset-0 bg-gray-950/60 backdrop-blur-md flex items-center justify-center z-50 animate-in fade-in duration-300">
      <div className="glass-card border-white/10 rounded-3xl w-full max-w-3xl max-h-[85vh] flex flex-col shadow-[0_0_50px_-12px_rgba(99,102,241,0.25)] animate-in zoom-in-95 duration-300 overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between px-8 py-6 border-b border-white/5 bg-white/[0.02]">
          <div className="flex flex-col gap-1">
            <h2 className="text-xl font-bold text-white tracking-tight">{plugin.name} 완벽 가이드</h2>
            <p className="text-[10px] text-gray-500 font-mono tracking-wider">{plugin.id}</p>
          </div>
          <button type="button" onClick={onClose} className="w-8 h-8 flex items-center justify-center rounded-full hover:bg-white/5 text-gray-500 hover:text-white transition-colors">✕</button>
        </div>
        {/* Content */}
        <div className="flex-1 overflow-auto px-6 py-4">
          {isLoading && <p className="text-gray-400 text-sm">가이드 불러오는 중...</p>}
          {error && <p className="text-red-400 text-sm">{error}</p>}
          {content && (
            <div className="prose prose-invert prose-sm max-w-none
              prose-headings:text-gray-100 prose-headings:font-semibold
              prose-h1:text-xl prose-h1:border-b prose-h1:border-gray-700 prose-h1:pb-2
              prose-h2:text-base prose-h2:mt-6
              prose-p:text-gray-300 prose-p:leading-relaxed
              prose-a:text-indigo-400 prose-a:no-underline hover:prose-a:underline
              prose-strong:text-gray-100
              prose-code:text-indigo-300 prose-code:bg-gray-800 prose-code:px-1.5 prose-code:py-0.5 prose-code:rounded prose-code:text-xs prose-code:before:content-none prose-code:after:content-none
              prose-pre:bg-transparent prose-pre:p-0 prose-pre:my-3
              prose-table:text-sm prose-table:w-full
              prose-th:text-gray-300 prose-th:bg-gray-800 prose-th:px-3 prose-th:py-2 prose-th:text-left
              prose-td:text-gray-400 prose-td:px-3 prose-td:py-1.5 prose-td:border-b prose-td:border-gray-800
              prose-blockquote:border-indigo-500 prose-blockquote:text-gray-400 prose-blockquote:bg-gray-800/50 prose-blockquote:px-4 prose-blockquote:py-1 prose-blockquote:rounded-r
              prose-li:text-gray-300 prose-li:marker:text-gray-500
              [&_.hljs]:rounded-lg [&_.hljs]:text-xs [&_.hljs]:border [&_.hljs]:border-gray-700 [&_.hljs]:p-4 [&_.hljs]:overflow-auto">
              <ReactMarkdown
                remarkPlugins={[remarkGfm]}
                rehypePlugins={[rehypeHighlight]}
              >{content}</ReactMarkdown>
            </div>
          )}
        </div>
        {/* Footer */}
        <div className="flex justify-end px-8 py-4 border-t border-white/5 bg-white/[0.01]">
          <button type="button" onClick={onClose}
            className="px-6 py-2 text-sm font-bold bg-white text-gray-950 rounded-xl hover:bg-gray-200 transition-all duration-300">
            확인했습니다
          </button>
        </div>
      </div>
    </div>
  );
}

function PluginSettingsModal({ plugin, onClose, onAuthChange, currentEmoji, onEmojiChange }: {
  plugin: Plugin;
  onClose: () => void;
  onAuthChange: (pluginId: string, configured: boolean) => void;
  currentEmoji: string;
  onEmojiChange: (pluginId: string, emoji: string) => void;
}) {
  const [emojiInput, setEmojiInput] = useState(currentEmoji);
  const [editingEmoji, setEditingEmoji] = useState(false);
  const [fields, setFields] = useState<Record<string, string>>({});
  const [isSaving, setIsSaving] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const [authStatus, setAuthStatus] = useState<boolean | null>(null);
  const [showAuthForm, setShowAuthForm] = useState(false);
  const [oauthStep, setOauthStep] = useState<'idle' | 'waiting' | 'done'>('idle');
  // Cache TTL state (Obsidian 제외)
  const showCacheSettings = plugin.id !== 'com.doxus.obsidian';
  const [cacheEnabled, setCacheEnabled] = useState(false);
  const [cacheTtl, setCacheTtl] = useState(30);
  const [isSavingTtl, setIsSavingTtl] = useState(false);

  useEffect(() => {
    invoke<{ configured: boolean }>('plugin_get_auth_status', { pluginId: plugin.id })
      .then((res) => {
        setAuthStatus(res.configured);
        setShowAuthForm(!res.configured); // 미인증이면 폼 바로 표시
      })
      .catch(() => { setAuthStatus(false); setShowAuthForm(true); });
  }, [plugin.id]);

  useEffect(() => {
    if (!showCacheSettings) return;
    invoke<{ cache_ttl_minutes: number | null }>('plugin_get_cache_ttl', { pluginId: plugin.id })
      .then((res) => {
        if (res.cache_ttl_minutes != null) {
          setCacheEnabled(true);
          setCacheTtl(res.cache_ttl_minutes);
        } else {
          setCacheEnabled(false);
        }
      })
      .catch(() => {});
  }, [plugin.id, showCacheSettings]);

  const handleSaveTtl = async (enabled: boolean, minutes: number) => {
    setIsSavingTtl(true);
    try {
      await invoke('plugin_set_cache_ttl', {
        pluginId: plugin.id,
        ttlMinutes: enabled ? minutes : null,
      });
    } catch (e) {
      setResult(`✗ TTL 저장 실패: ${String(e)}`);
    } finally {
      setIsSavingTtl(false);
    }
  };

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
    <div className="fixed inset-0 bg-gray-950/60 backdrop-blur-md flex items-center justify-center z-50 animate-in fade-in duration-300">
      <div className="glass-card border-white/10 rounded-3xl p-8 w-full max-w-md flex flex-col gap-6 shadow-[0_0_50px_-12px_rgba(99,102,241,0.25)] animate-in zoom-in-95 duration-300">
        {/* Header */}
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-4">
            {editingEmoji ? (
              <input
                autoFocus
                type="text"
                className="w-12 h-12 rounded-2xl bg-white/5 border border-indigo-500 text-center text-2xl focus:outline-none shadow-inner"
                onBlur={() => setEditingEmoji(false)}
                onKeyDown={(e) => {
                  if (e.key === 'Escape') setEditingEmoji(false);
                }}
                onChange={(e) => {
                  const arr = [...e.target.value]; const emoji = arr[arr.length - 1] ?? '';
                  if (emoji) {
                    setEmojiInput(emoji);
                    onEmojiChange(plugin.id, emoji);
                    setEditingEmoji(false);
                  }
                }}
              />
            ) : (
              <button
                type="button"
                onClick={() => setEditingEmoji(true)}
                className="w-12 h-12 rounded-2xl bg-white/5 border border-white/10 hover:border-indigo-500 flex items-center justify-center text-2xl transition-all duration-300 hover:scale-110 active:scale-95 shadow-inner"
                title="클릭해서 이모지 변경"
              >
                {emojiInput}
              </button>
            )}
            <div className="flex flex-col">
              <h2 className="text-xl font-bold text-white tracking-tight leading-tight">{plugin.name} 설정</h2>
              <p className="text-[10px] text-gray-500 font-mono mt-0.5 tracking-wider">{plugin.id}</p>
            </div>
          </div>
          <button type="button" onClick={onClose} className="w-8 h-8 flex items-center justify-center rounded-full hover:bg-white/5 text-gray-500 hover:text-white transition-colors">✕</button>
        </div>

        {/* Auth status badge */}
        {authStatus !== null && (
          <div className={`flex items-center justify-between text-xs px-3 py-2 rounded-lg border ${
            authStatus
              ? 'bg-emerald-950 border-emerald-800 text-emerald-400'
              : 'bg-gray-800 border-gray-700 text-gray-400'
          }`}>
            <div className="flex items-center gap-2">
              <span>{authStatus ? '●' : '○'}</span>
              <span>{authStatus ? '인증 완료' : '인증 미설정'}</span>
            </div>
            {authStatus && authType !== 'none' && (
              <button
                type="button"
                onClick={() => setShowAuthForm((v) => !v)}
                className="text-xs text-emerald-600 hover:text-emerald-400 underline transition-colors"
              >
                {showAuthForm ? '취소' : '인증 변경'}
              </button>
            )}
          </div>
        )}

        {/* Auth UI by type */}
        {authType === 'none' && (
          <p className="text-sm text-gray-400 text-center py-4">이 플러그인은 별도 인증이 필요하지 않습니다.</p>
        )}

        {authType === 'api_token' && showAuthForm && (
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

        {authType === 'oauth' && showAuthForm && (
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

        {/* Cache TTL 설정 (Obsidian 제외) */}
        {showCacheSettings && (
          <div className="flex flex-col gap-3 border-t border-gray-800 pt-4">
            <div className="flex items-center justify-between">
              <div>
                <p className="text-xs font-medium text-gray-300">문서 캐시</p>
                <p className="text-xs text-gray-500 mt-0.5">가져온 문서 내용을 일정 시간 캐시합니다</p>
              </div>
              <button
                type="button"
                onClick={() => {
                  const next = !cacheEnabled;
                  setCacheEnabled(next);
                  handleSaveTtl(next, cacheTtl);
                }}
                className={`relative inline-flex h-5 w-9 items-center rounded-full transition-colors ${
                  cacheEnabled ? 'bg-indigo-600' : 'bg-gray-700'
                }`}
              >
                <span className={`inline-block h-3.5 w-3.5 transform rounded-full bg-white transition-transform ${
                  cacheEnabled ? 'translate-x-4' : 'translate-x-1'
                }`} />
              </button>
            </div>
            {cacheEnabled && (
              <div className="flex flex-col gap-2">
                <div className="flex items-center justify-between">
                  <label className="text-xs text-gray-500">캐시 유지 시간</label>
                  <span className="text-xs font-medium text-indigo-400">{cacheTtl}분</span>
                </div>
                <input
                  type="range"
                  min={10}
                  max={120}
                  step={10}
                  value={cacheTtl}
                  onChange={(e) => setCacheTtl(Number(e.target.value))}
                  onMouseUp={(e) => handleSaveTtl(true, Number((e.target as HTMLInputElement).value))}
                  onTouchEnd={(e) => handleSaveTtl(true, Number((e.target as HTMLInputElement).value))}
                  className="w-full accent-indigo-500 cursor-pointer"
                />
                <div className="flex justify-between text-xs text-gray-600">
                  <span>10분</span>
                  <span>120분</span>
                </div>
                {isSavingTtl && <p className="text-xs text-gray-500">저장 중...</p>}
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
        <div className="flex gap-4 justify-end pt-4 border-t border-white/5">
          <button type="button" onClick={onClose}
            className="px-4 py-2 text-sm font-semibold text-gray-500 hover:text-gray-200 transition-colors">
            닫기
          </button>
          {authType === 'api_token' && showAuthForm && (
            <button
              onClick={handleSave}
              disabled={isSaving || schema.filter(f => f.required).some(f => !(fields[f.key] ?? '').trim())}
              className="px-6 py-2.5 text-sm font-bold bg-white text-gray-950 rounded-xl hover:bg-gray-200 disabled:opacity-30 transition-all duration-300 shadow-xl active:scale-95"
            >
              {isSaving ? '저장 중...' : '저장 완료'}
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
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [filter, setFilter] = useState<FilterKey>('all');
  const [pendingIds, setPendingIds] = useState<Set<string>>(new Set());
  const [settingsPlugin, setSettingsPlugin] = useState<Plugin | null>(null);
  const [guidePlugin, setGuidePlugin] = useState<Plugin | null>(null);
  const { authStates, fetchAuthStatus, setConfigured, getEmoji, setEmoji } = usePluginStore();

  useEffect(() => {
    let installedIds = new Set<string>();

    invoke<{ plugins?: Array<{ id: string }> } | Array<{ id: string }>>('market_list_installed')
      .then((res) => {
        const arr = Array.isArray(res) ? res : (res as { plugins?: Array<{ id: string }> })?.plugins ?? [];
        installedIds = new Set(arr.map((p) => p.id));
      })
      .catch(() => { /* installed state stays empty */ })
      .finally(() => {
        invoke<RegistryEntry[]>('market_fetch_registry')
          .then((entries) => {
            const list = entries.map((e) => registryEntryToPlugin(e, installedIds));
            setPlugins(list);
            list.filter((p) => p.installed).forEach((p) => fetchAuthStatus(p.id));
          })
          .catch((e) => setError(String(e)))
          .finally(() => setIsLoading(false));
      });
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
    <div className="flex flex-col h-full bg-transparent p-6 gap-8 max-w-5xl mx-auto animate-in fade-in duration-700">
      {settingsPlugin && (
        <PluginSettingsModal
          plugin={settingsPlugin}
          onClose={() => setSettingsPlugin(null)}
          onAuthChange={(pluginId, configured) => setConfigured(pluginId, configured)}
          currentEmoji={getEmoji(settingsPlugin.id)}
          onEmojiChange={setEmoji}
        />
      )}
      {guidePlugin && (
        <PluginGuideModal
          plugin={guidePlugin}
          onClose={() => setGuidePlugin(null)}
        />
      )}

      {/* 헤더 */}
      <div className="flex flex-col gap-1">
        <h1 className="text-4xl font-extrabold text-white tracking-tight bg-clip-text text-transparent bg-gradient-to-br from-white to-gray-500">
          확장 마켓플레이스
        </h1>
        <p className="text-gray-500 text-sm font-medium">플러그인을 통해 doxus의 지식 통합 능력을 극대화하세요.</p>
      </div>

      {/* 검색 + 필터 */}
      <div className="flex gap-4 flex-wrap items-center">
        <div className="relative flex-1 min-w-[300px] group">
          <input
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="필요한 플러그인을 검색하세요..."
            className="w-full pl-11 pr-4 py-3 bg-white/5 border border-white/10 rounded-2xl text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-indigo-500/50 focus:bg-white/10 transition-all duration-300 text-sm shadow-inner"
          />
          <div className="absolute left-4 top-1/2 -translate-y-1/2 text-gray-500 group-focus-within:text-indigo-400 transition-colors">
            <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/></svg>
          </div>
        </div>
        <div className="flex gap-1 p-1.5 glass-card border-white/10 rounded-2xl shadow-inner">
          {FILTERS.map((f) => (
            <button
              key={f.key}
              onClick={() => setFilter(f.key)}
              className={`px-4 py-1.5 rounded-xl text-xs font-bold tracking-tight transition-all duration-300 ${
                filter === f.key
                  ? 'bg-white text-gray-950 shadow-lg'
                  : 'text-gray-500 hover:text-gray-200'
              }`}
            >
              {f.label}
            </button>
          ))}
        </div>
      </div>

      {/* 오류 배너 */}
      {error && (
        <div className="px-3 py-2 rounded-lg bg-red-950 border border-red-800 text-red-400 text-sm">
          레지스트리 로드 실패: {error}
        </div>
      )}

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
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-1 gap-4">
            {filtered.map((plugin) => {
              const badge = TRUST_BADGE[plugin.trust];
              const isPending = pendingIds.has(plugin.id);
              const isAuthenticated = authStates[plugin.id]?.configured ?? false;

              return (
                <div
                  key={plugin.id}
                  className="glass-card border-white/5 rounded-3xl p-6 flex items-start gap-6 hover:border-white/20 transition-all duration-300 group shadow-lg"
                >
                  {/* 아이콘 */}
                  <div className="w-14 h-14 rounded-2xl bg-white/5 border border-white/10 flex items-center justify-center shrink-0 group-hover:scale-110 group-hover:bg-indigo-500/10 group-hover:border-indigo-500/30 transition-all duration-500 shadow-inner">
                    <span className="text-3xl">
                      {getEmoji(plugin.id)}
                    </span>
                  </div>

                  {/* 정보 */}
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 flex-wrap mb-1">
                      <h3 className="text-lg font-bold text-white tracking-tight">{plugin.name}</h3>
                      <span className="text-gray-600 text-[10px] font-mono bg-white/5 px-1.5 py-0.5 rounded">v{plugin.version}</span>
                    </div>
                    
                    <div className="flex items-center gap-1.5 flex-wrap mt-1 mb-3">
                      <span className={`text-[9px] px-2 py-0.5 rounded uppercase font-bold tracking-tighter shadow-sm ${badge.className}`}>
                        {badge.label}
                      </span>
                      {plugin.builtin && (
                        <span className="text-[9px] px-2 py-0.5 rounded uppercase font-bold tracking-tighter bg-gray-800 text-gray-500 border border-gray-700">
                          Built-in
                        </span>
                      )}
                      {plugin.installed && (
                        <span className={`text-[9px] px-2 py-0.5 rounded uppercase font-bold tracking-tighter border transition-all duration-300 ${
                          isAuthenticated 
                            ? 'bg-emerald-500/10 text-emerald-400 border-emerald-500/20' 
                            : 'bg-yellow-500/10 text-yellow-500 border-yellow-500/20'
                        }`}>
                          {isAuthenticated ? '● Authenticated' : '○ Pending Auth'}
                        </span>
                      )}
                    </div>
                    
                    <p className="text-sm text-gray-400 line-clamp-2 leading-relaxed h-10">{plugin.description}</p>
                    <p className="text-[10px] text-gray-600 mt-2 font-mono truncate opacity-0 group-hover:opacity-100 transition-opacity">{plugin.id}</p>
                  </div>

                  {/* 액션 */}
                  <div className="shrink-0 flex flex-col gap-2 items-end pt-1">
                    {plugin.guide_url && (
                      <button
                        onClick={() => setGuidePlugin(plugin)}
                        className="w-full px-4 py-2 rounded-xl text-xs font-bold border border-white/10 text-gray-400 hover:text-emerald-400 hover:border-emerald-500/40 hover:bg-emerald-500/5 transition-all duration-300"
                      >
                        Guide
                      </button>
                    )}
                    {plugin.installed && (
                      <button
                        onClick={() => setSettingsPlugin(plugin)}
                        className="w-full px-4 py-2 rounded-xl text-xs font-bold border border-white/10 text-gray-400 hover:text-indigo-400 hover:border-indigo-500/40 hover:bg-indigo-500/5 transition-all duration-300"
                      >
                        Settings
                      </button>
                    )}
                    {plugin.builtin ? (
                      <span className="px-4 py-2 text-[10px] font-bold text-gray-600 uppercase tracking-widest">Included</span>
                    ) : plugin.installed ? (
                      <button
                        onClick={() => handleToggle(plugin)}
                        disabled={isPending}
                        className="w-full px-4 py-2 rounded-xl text-xs font-bold border border-red-900/30 text-red-500/70 hover:text-red-400 hover:bg-red-500/10 transition-all duration-300 disabled:opacity-30"
                      >
                        {isPending ? '...' : 'Uninstall'}
                      </button>
                    ) : (
                      <button
                        onClick={() => handleToggle(plugin)}
                        disabled={isPending}
                        className="w-full px-6 py-2 rounded-xl text-xs font-bold bg-white text-gray-950 hover:bg-gray-200 shadow-lg shadow-white/5 transition-all duration-300 disabled:opacity-30 active:scale-95"
                      >
                        {isPending ? '...' : 'Install'}
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
// Force Vite refresh: Fri Apr 17 20:18:33 KST 2026
