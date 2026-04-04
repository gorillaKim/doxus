import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface SystemStatus {
  app: { version: string; status: string };
  database: { path: string; exists: boolean; status: string };
  mcp: { status: string; note: string };
  cli: { status: string; path: string };
  agent: { status: string; note: string };
}

type StatusLevel = 'ok' | 'warn' | 'error' | 'unknown';

function statusLevel(status: string): StatusLevel {
  if (status === 'running' || status === 'connected' || status === 'installed') return 'ok';
  if (status === 'not found' || status === 'not installed') return 'error';
  if (status === 'not started') return 'warn';
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

  const icon = {
    ok: '●',
    warn: '◐',
    error: '○',
    unknown: '?',
  }[level];

  return (
    <span className={`inline-flex items-center gap-1.5 text-xs px-2.5 py-1 rounded-full font-medium border ${cls}`}>
      <span>{icon}</span>
      {status}
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
  const [dbTestResult, setDbTestResult] = useState<string | null>(null);
  const [dbTestLoading, setDbTestLoading] = useState(false);
  const [mcpTestResult, setMcpTestResult] = useState<string | null>(null);
  const [mcpTestLoading, setMcpTestLoading] = useState(false);

  useEffect(() => {
    invoke<SystemStatus>('get_system_status')
      .then(setSysStatus)
      .catch(() => {
        // 폴백: 앱은 실행 중이므로 기본값 표시
        setSysStatus({
          app: { version: '0.1.0', status: 'running' },
          database: { path: '~/.doxus/db/doxus.db', exists: false, status: 'unknown' },
          mcp: { status: 'unknown', note: 'MCP 서버는 별도 프로세스로 실행됩니다' },
          cli: { status: 'unknown', path: '' },
          agent: { status: 'not started', note: 'Agent sidecar는 Phase 3에서 구현됩니다' },
        });
      })
      .finally(() => setIsLoading(false));
  }, []);

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
    // MCP 서버 포트 확인 (실제 구현 전 stub)
    setTimeout(() => {
      setMcpTestResult('✗ MCP 서버가 실행되지 않음 (Phase 1에서 구현 예정)');
      setMcpTestLoading(false);
    }, 800);
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
          </div>
        ) : null}
      </section>

      {/* 앱 정보 */}
      <section className="flex flex-col gap-3">
        <h2 className="text-xs font-semibold text-gray-500 uppercase tracking-wider">앱 정보</h2>
        <div className="bg-gray-900 border border-gray-800 rounded-xl p-5 flex flex-col gap-2">
          <InfoRow label="앱 이름" value="doxus" />
          <InfoRow label="버전" value={sysStatus?.app.version ?? '—'} />
          <InfoRow label="플랫폼" value="macOS (Tauri v2)" />
          <InfoRow label="DB 경로" value={sysStatus?.database.path ?? '—'} mono />
          <InfoRow label="Phase" value="Phase 2b — WASM MVP 진행 중" />
        </div>
      </section>

      {/* 개발 도구 */}
      <section className="flex flex-col gap-3">
        <h2 className="text-xs font-semibold text-gray-500 uppercase tracking-wider">개발 도구</h2>
        <div className="bg-gray-900 border border-gray-800 rounded-xl p-5 flex flex-col gap-3">
          <p className="text-sm text-gray-400">로컬 개발 환경에서 사용 가능한 도구</p>
          <div className="flex flex-wrap gap-2">
            <DevButton
              label="DB 재인덱싱"
              onClick={async () => {
                try {
                  await invoke('trigger_reindex');
                } catch {
                  // not yet implemented
                }
              }}
            />
            <DevButton
              label="검색 엔진 상태"
              onClick={async () => {
                try {
                  await invoke('search_engine_status');
                } catch {
                  // not yet implemented
                }
              }}
            />
            <DevButton
              label="플러그인 로그"
              onClick={async () => {
                try {
                  await invoke('get_plugin_logs');
                } catch {
                  // not yet implemented
                }
              }}
            />
          </div>
          <p className="text-xs text-gray-600">
            * 대부분의 개발 도구는 Phase 6 (관측성/디버깅)에서 구현됩니다
          </p>
        </div>
      </section>
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

function DevButton({ label, onClick }: { label: string; onClick: () => void }) {
  const [result, setResult] = useState<string | null>(null);
  const handleClick = async () => {
    setResult('실행 중...');
    try {
      await onClick();
      setResult('완료');
    } catch {
      setResult('미구현');
    }
    setTimeout(() => setResult(null), 2000);
  };
  return (
    <button
      onClick={handleClick}
      className="px-3 py-1.5 text-xs bg-gray-800 hover:bg-gray-700 text-gray-300 rounded-lg border border-gray-700 transition-colors"
    >
      {result ?? label}
    </button>
  );
}
