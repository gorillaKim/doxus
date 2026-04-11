import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface AppSettings {
  embedding_model: 'onnx' | 'ollama';
  language: 'ko' | 'en';
  theme: 'light' | 'dark' | 'system';
}

interface SystemStatus {
  app: { version: string; status: string };
  database: { path: string; exists: boolean; status: string };
  mcp: { status: string; note: string };
  cli: { status: string; path: string };
  agent: { status: string; note: string };
}

type StatusLevel = 'ok' | 'warn' | 'error' | 'unknown';

function statusLevel(status: string): StatusLevel {
  if (status === 'ok' || status === 'running' || status === 'connected' || status === 'installed') return 'ok';
  if (status === 'warn' || status === 'not started') return 'warn';
  if (status === 'error' || status === 'not found' || status === 'not installed') return 'error';
  return 'unknown';
}

function StatusBadge({ status }: { status: string }) {
  const level = statusLevel(status);
  const cls = {
    ok: 'bg-emerald-950 text-emerald-400 border-emerald-800',
    warn: 'bg-yellow-950 text-yellow-400 border-yellow-800',
    error: 'bg-red-950 text-red-400 border-red-800',
    unknown: 'bg-gray-800 text-gray-400 border-gray-700',
  }[level];

  const icon = { ok: '●', warn: '◐', error: '○', unknown: '?' }[level];
  const label = { ok: '정상', warn: '경고', error: '오류', unknown: '미확인' }[level];

  return (
    <span className={`inline-flex items-center gap-1.5 text-xs px-2.5 py-1 rounded-full font-medium border ${cls}`}>
      <span>{icon}</span>
      {label}
    </span>
  );
}

interface StatusCardProps {
  title: string;
  status: string;
  detail?: string;
  note?: string;
  onTest?: () => void;
  testLabel?: string;
  testLoading?: boolean;
  testResult?: string | null;
}

function StatusCard({
  title, status, detail, note, onTest, testLabel, testLoading, testResult,
}: StatusCardProps) {
  return (
    <div className="bg-gray-900 border border-gray-800 rounded-xl p-5 flex flex-col gap-3">
      <div className="flex items-center justify-between gap-3">
        <h3 className="text-white font-semibold">{title}</h3>
        <StatusBadge status={status} />
      </div>
      {detail && (
        <p className="text-xs text-gray-500 font-mono break-all">{detail}</p>
      )}
      {note && (
        <p className="text-xs text-gray-600">{note}</p>
      )}
      {onTest && (
        <div className="flex items-center gap-3 pt-1">
          <button
            onClick={onTest}
            disabled={testLoading}
            className="px-3 py-1.5 text-xs bg-gray-800 hover:bg-gray-700 text-gray-300 rounded-lg border border-gray-700 transition-colors disabled:opacity-50"
          >
            {testLoading ? '테스트 중...' : (testLabel ?? '연결 테스트')}
          </button>
          {testResult && (
            <span className="text-xs text-gray-400">{testResult}</span>
          )}
        </div>
      )}
    </div>
  );
}

export default function SettingsPage() {
  const [sysStatus, setSysStatus] = useState<SystemStatus | null>(null);
  const [isLoading, setIsLoading] = useState(true);

  // App settings state
  const [appSettings, setAppSettings] = useState<AppSettings>({
    embedding_model: 'onnx',
    language: 'ko',
    theme: 'system',
  });
  const [settingsSaving, setSettingsSaving] = useState(false);
  const [settingsResult, setSettingsResult] = useState<string | null>(null);
  const [dbTestResult, setDbTestResult] = useState<string | null>(null);
  const [dbTestLoading, setDbTestLoading] = useState(false);
  const [mcpTestResult, setMcpTestResult] = useState<string | null>(null);
  const [mcpTestLoading, setMcpTestLoading] = useState(false);
  const [claudeStatus, setClaudeStatus] = useState<'ok' | 'warn' | 'unknown'>('unknown');
  const [claudeTestResult, setClaudeTestResult] = useState<string | null>(null);
  const [claudeTestLoading, setClaudeTestLoading] = useState(false);
  const [geminiStatus, setGeminiStatus] = useState<'ok' | 'warn' | 'unknown'>('unknown');
  const [geminiTestResult, setGeminiTestResult] = useState<string | null>(null);
  const [geminiTestLoading, setGeminiTestLoading] = useState(false);

  useEffect(() => {
    invoke<AppSettings>('load_settings')
      .then(setAppSettings)
      .catch(() => { /* use defaults */ });

    invoke<SystemStatus>('get_system_status')
      .then(setSysStatus)
      .catch(() => {
        setSysStatus({
          app: { version: '0.1.0', status: 'running' },
          database: { path: '~/.doxus/db/doxus.db', exists: false, status: 'unknown' },
          mcp: { status: 'unknown', note: 'MCP 서버는 별도 프로세스로 실행됩니다' },
          cli: { status: 'unknown', path: '' },
          agent: { status: 'not started', note: 'Agent sidecar는 Phase 3에서 구현됩니다' },
        });
      })
      .finally(() => setIsLoading(false));

    // Auto-detect Claude / Gemini on mount
    invoke<{ status: string; message: string }>('check_claude_status')
      .then((res) => setClaudeStatus(res.status as 'ok' | 'warn' | 'unknown'))
      .catch(() => setClaudeStatus('warn'));

    invoke<{ status: string; message: string }>('check_gemini_status')
      .then((res) => setGeminiStatus(res.status as 'ok' | 'warn' | 'unknown'))
      .catch(() => setGeminiStatus('warn'));
  }, []);

  const handleSaveSettings = async () => {
    setSettingsSaving(true);
    setSettingsResult(null);
    try {
      await invoke('save_settings', { settings: appSettings });
      setSettingsResult('✓ 저장됨');
    } catch (e) {
      setSettingsResult(`✗ ${String(e)}`);
    } finally {
      setSettingsSaving(false);
    }
  };

  const handleRefresh = async () => {
    setIsLoading(true);
    try {
      const s = await invoke<SystemStatus>('get_system_status');
      setSysStatus(s);
    } catch {
      // ignore
    } finally {
      setIsLoading(false);
    }
  };

  const handleDbTest = async () => {
    setDbTestLoading(true);
    setDbTestResult(null);
    try {
      await invoke('get_workspaces');
      setDbTestResult('✓ 연결 성공');
    } catch (e) {
      setDbTestResult(`✗ ${String(e)}`);
    } finally {
      setDbTestLoading(false);
    }
  };

  const handleMcpTest = async () => {
    setMcpTestLoading(true);
    setMcpTestResult(null);
    try {
      const res = await invoke<SystemStatus>('get_system_status');
      setMcpTestResult(res.mcp.status === 'running' ? '✓ 연결됨' : '✗ 실행되지 않음');
    } catch (e) {
      setMcpTestResult(`✗ ${String(e)}`);
    } finally {
      setMcpTestLoading(false);
    }
  };

  const handleClaudeTest = async () => {
    setClaudeTestLoading(true);
    setClaudeTestResult(null);
    try {
      const res = await invoke<{ status: string; message: string }>('check_claude_status');
      setClaudeStatus(res.status as 'ok' | 'warn' | 'unknown');
      setClaudeTestResult(res.message);
    } catch (e) {
      setClaudeTestResult(`✗ ${String(e)}`);
    } finally {
      setClaudeTestLoading(false);
    }
  };

  const handleGeminiTest = async () => {
    setGeminiTestLoading(true);
    setGeminiTestResult(null);
    try {
      const res = await invoke<{ status: string; message: string }>('check_gemini_status');
      setGeminiStatus(res.status as 'ok' | 'warn' | 'unknown');
      setGeminiTestResult(res.message);
    } catch (e) {
      setGeminiTestResult(`✗ ${String(e)}`);
    } finally {
      setGeminiTestLoading(false);
    }
  };

  return (
    <div className="flex flex-col gap-8 max-w-2xl">
      {/* 헤더 */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold text-white tracking-tight">설정</h1>
          <p className="text-sm text-gray-400 mt-0.5">앱 상태 확인 및 구성 요소 테스트</p>
        </div>
        <button
          onClick={handleRefresh}
          disabled={isLoading}
          className="px-3 py-1.5 text-sm border border-gray-700 text-gray-400 rounded-lg hover:bg-gray-800 hover:text-gray-200 disabled:opacity-50 transition-colors"
        >
          {isLoading ? '새로고침 중...' : '새로고침'}
        </button>
      </div>

      {/* 시스템 상태 */}
      <section className="flex flex-col gap-3">
        <h2 className="text-xs font-semibold text-gray-500 uppercase tracking-wider">시스템 상태</h2>
        {isLoading ? (
          <div className="flex items-center justify-center h-32">
            <p className="text-gray-500 text-sm">상태 확인 중...</p>
          </div>
        ) : sysStatus ? (
          <div className="flex flex-col gap-3">
            <StatusCard
              title="앱 (doxus desktop)"
              status={sysStatus.app.status}
              detail={`버전 ${sysStatus.app.version}`}
            />
            <StatusCard
              title="데이터베이스 (SQLite)"
              status={sysStatus.database.status}
              detail={sysStatus.database.path}
              onTest={handleDbTest}
              testLabel="DB 연결 테스트"
              testLoading={dbTestLoading}
              testResult={dbTestResult}
            />
            <StatusCard
              title="MCP 서버 (doxus-mcp)"
              status={sysStatus.mcp.status}
              note={sysStatus.mcp.note}
              onTest={handleMcpTest}
              testLabel="MCP 연결 테스트"
              testLoading={mcpTestLoading}
              testResult={mcpTestResult}
            />
            <StatusCard
              title="CLI (doxus-cli)"
              status={sysStatus.cli.status}
              detail={sysStatus.cli.path || undefined}
              note={sysStatus.cli.status === 'not installed'
                ? 'cargo install doxus-cli 로 설치하세요'
                : undefined}
            />
            <StatusCard
              title="에이전트 사이드카"
              status={sysStatus.agent.status}
              note={sysStatus.agent.note}
            />
            <StatusCard
              title="Claude (AI 에이전트)"
              status={claudeStatus}
              note="Claude Code CLI 또는 ANTHROPIC_API_KEY 필요"
              onTest={handleClaudeTest}
              testLabel="연결 테스트"
              testLoading={claudeTestLoading}
              testResult={claudeTestResult}
            />
            <StatusCard
              title="Gemini (AI 에이전트)"
              status={geminiStatus}
              note="Gemini CLI 또는 GEMINI_API_KEY 필요"
              onTest={handleGeminiTest}
              testLabel="연결 테스트"
              testLoading={geminiTestLoading}
              testResult={geminiTestResult}
            />
          </div>
        ) : null}
      </section>

      {/* 앱 설정 */}
      <section className="flex flex-col gap-3">
        <h2 className="text-xs font-semibold text-gray-500 uppercase tracking-wider">앱 설정</h2>
        <div className="bg-gray-900 border border-gray-800 rounded-xl p-5 flex flex-col gap-4">
          <div className="flex items-center justify-between gap-4">
            <label className="text-sm text-gray-400 shrink-0">임베딩 모델</label>
            <select
              value={appSettings.embedding_model}
              onChange={(e) => setAppSettings({ ...appSettings, embedding_model: e.target.value as AppSettings['embedding_model'] })}
              className="bg-gray-800 border border-gray-700 text-gray-300 text-sm rounded-lg px-3 py-1.5 focus:outline-none focus:border-gray-500"
            >
              <option value="onnx">ONNX (내장, 기본값)</option>
              <option value="ollama">Ollama (외부 서버)</option>
            </select>
          </div>
          <div className="flex items-center justify-between gap-4">
            <label className="text-sm text-gray-400 shrink-0">언어</label>
            <select
              value={appSettings.language}
              onChange={(e) => setAppSettings({ ...appSettings, language: e.target.value as AppSettings['language'] })}
              className="bg-gray-800 border border-gray-700 text-gray-300 text-sm rounded-lg px-3 py-1.5 focus:outline-none focus:border-gray-500"
            >
              <option value="ko">한국어</option>
              <option value="en">English</option>
            </select>
          </div>
          <div className="flex items-center justify-between gap-4">
            <label className="text-sm text-gray-400 shrink-0">테마</label>
            <select
              value={appSettings.theme}
              onChange={(e) => setAppSettings({ ...appSettings, theme: e.target.value as AppSettings['theme'] })}
              className="bg-gray-800 border border-gray-700 text-gray-300 text-sm rounded-lg px-3 py-1.5 focus:outline-none focus:border-gray-500"
            >
              <option value="system">시스템</option>
              <option value="light">라이트</option>
              <option value="dark">다크</option>
            </select>
          </div>
          <div className="flex items-center gap-3 pt-1">
            <button
              onClick={handleSaveSettings}
              disabled={settingsSaving}
              className="px-4 py-1.5 text-sm bg-blue-600 hover:bg-blue-500 text-white rounded-lg transition-colors disabled:opacity-50"
            >
              {settingsSaving ? '저장 중...' : '설정 저장'}
            </button>
            {settingsResult && (
              <span className="text-xs text-gray-400">{settingsResult}</span>
            )}
          </div>
        </div>
      </section>

      {/* 앱 정보 */}
      <section className="flex flex-col gap-3">
        <h2 className="text-xs font-semibold text-gray-500 uppercase tracking-wider">앱 정보</h2>
        <div className="bg-gray-900 border border-gray-800 rounded-xl p-5 flex flex-col gap-2">
          <InfoRow label="앱 이름" value="doxus" />
          <InfoRow label="버전" value={sysStatus?.app.version ?? '—'} />
          <InfoRow label="플랫폼" value="macOS (Tauri v2)" />
          <InfoRow label="DB 경로" value={sysStatus?.database.path ?? '—'} mono />
        </div>
      </section>

      {/* 개발 도구 */}
      <DevToolsSection />
    </div>
  );
}

function InfoRow({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="flex items-baseline justify-between gap-4">
      <span className="text-xs text-gray-500 shrink-0">{label}</span>
      <span className={`text-sm text-gray-300 truncate ${mono ? 'font-mono text-xs' : ''}`}>
        {value}
      </span>
    </div>
  );
}

function DevToolsSection() {
  const [devResult, setDevResult] = useState<string | null>(null);

  return (
    <section className="flex flex-col gap-3">
      <h2 className="text-xs font-semibold text-gray-500 uppercase tracking-wider">개발 도구</h2>
      <div className="bg-gray-900 border border-gray-800 rounded-xl p-5 flex flex-col gap-3">
        <p className="text-sm text-gray-400">로컬 개발 환경에서 사용 가능한 도구</p>
        <div className="flex flex-wrap gap-2">
          <button
            className="px-3 py-1.5 text-xs bg-gray-800 hover:bg-gray-700 text-gray-300 rounded-lg border border-gray-700 transition-colors"
            onClick={async () => {
              try {
                const res = await invoke<{ indexed: number; message: string }>('trigger_reindex');
                setDevResult(res.message);
              } catch (e) {
                setDevResult(`✗ ${String(e)}`);
              }
            }}
          >
            DB 재인덱싱
          </button>
          <button
            className="px-3 py-1.5 text-xs bg-gray-800 hover:bg-gray-700 text-gray-300 rounded-lg border border-gray-700 transition-colors"
            onClick={async () => {
              try {
                const res = await invoke<{ total_documents: number; total_projects: number }>('search_engine_status');
                setDevResult(`문서 ${res.total_documents}개, 프로젝트 ${res.total_projects}개`);
              } catch (e) {
                setDevResult(`✗ ${String(e)}`);
              }
            }}
          >
            검색 엔진 상태
          </button>
          <button
            className="px-3 py-1.5 text-xs bg-gray-800 hover:bg-gray-700 text-gray-300 rounded-lg border border-gray-700 transition-colors"
            onClick={async () => {
              try {
                const res = await invoke<{ logs: { level: string; message: string }[] }>('get_plugin_logs');
                setDevResult(`최근 로그 ${res.logs.length}건`);
              } catch (e) {
                setDevResult(`✗ ${String(e)}`);
              }
            }}
          >
            플러그인 로그
          </button>
        </div>
        {devResult && <p className="text-xs text-emerald-400 mt-1">{devResult}</p>}
        <p className="text-xs text-gray-600">
          * 대부분의 개발 도구는 Phase 6 (관측성/디버깅)에서 구현됩니다
        </p>
      </div>
    </section>
  );
}
