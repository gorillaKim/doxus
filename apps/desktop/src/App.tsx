import { useEffect, useRef, useState } from "react";
import { BrowserRouter, Route, Routes } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { AppShell } from "./components/layout/AppShell";
import { SearchPage } from "./pages/SearchPage";
import { GraphPage } from "./pages/GraphPage";
import { ProjectsPage } from "./pages/ProjectsPage";
import MarketPage from "./pages/MarketPage";
import DashboardPage from "./pages/DashboardPage";
import SettingsPage from "./pages/SettingsPage";
import AgentPage from "./pages/AgentPage";
import GuidePage from "./pages/GuidePage";
import FreshnessPage from "./pages/FreshnessPage";
import SchedulerPage from "./pages/SchedulerPage";
import { ModelDownloadModal } from "./components/ModelDownloadModal";
import { setupMigrationListeners } from "./services/migrationListener";

export default function App() {
  const [cacheToast, setCacheToast] = useState<string | null>(null);
  const toastTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [indexToast, setIndexToast] = useState<{ name: string; message: string } | null>(null);
  const indexToastTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [migrationToast, setMigrationToast] = useState<string | null>(null);
  const migrationToastTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    const unlisten = listen<{ count: number }>("cache:cleanup", (event) => {
      const { count } = event.payload;
      if (toastTimer.current) clearTimeout(toastTimer.current);
      setCacheToast(`캐시 정리 완료 — 만료된 항목 ${count}개 제거됨`);
      toastTimer.current = setTimeout(() => setCacheToast(null), 4000);
    });
    return () => { unlisten.then(fn => fn()); };
  }, []);

  useEffect(() => {
    const setup = setupMigrationListeners(
      () => {
        if (migrationToastTimer.current) clearTimeout(migrationToastTimer.current);
        setMigrationToast("앱 업데이트 후 검색 성능 향상을 위한 데이터 재구성이 진행 중입니다...");
      },
      () => {
        if (migrationToastTimer.current) clearTimeout(migrationToastTimer.current);
        setMigrationToast("데이터 재구성이 완료되었습니다.");
        migrationToastTimer.current = setTimeout(() => setMigrationToast(null), 4000);
      },
    );
    return () => { setup.then((fn) => fn()); };
  }, []);

  useEffect(() => {
    const unlisten = listen<{ project_name: string; indexed: number }>("project-indexed", (event) => {
      const { project_name, indexed } = event.payload;
      const message = indexed === 0 ? "이미 최신 상태입니다" : `${indexed}개 문서 인덱싱 완료`;
      if (indexToastTimer.current) clearTimeout(indexToastTimer.current);
      setIndexToast({ name: project_name, message });
      indexToastTimer.current = setTimeout(() => setIndexToast(null), 5000);
    });
    return () => { unlisten.then(fn => fn()); };
  }, []);

  return (
    <BrowserRouter>
      <AppShell>
        <Routes>
          <Route path="/" element={<DashboardPage />} />
          <Route path="/graph" element={<GraphPage />} />
          <Route path="/search" element={<SearchPage />} />
          <Route path="/projects" element={<ProjectsPage />} />
          <Route path="/freshness" element={<FreshnessPage />} />
          <Route path="/scheduler" element={<SchedulerPage />} />
          <Route path="/market" element={<MarketPage />} />
          <Route path="/agent" element={<AgentPage />} />
          <Route path="/guide" element={<GuidePage />} />
          <Route path="/settings" element={<SettingsPage />} />
        </Routes>
      </AppShell>

      {cacheToast && (
        <div className="fixed bottom-6 right-6 z-50 px-4 py-3 bg-gray-900 border border-gray-700 rounded-xl shadow-xl text-sm text-gray-200 max-w-xs">
          🗑️ {cacheToast}
        </div>
      )}

      {indexToast && (
        <div className="fixed bottom-10 right-10 z-[100] animate-in slide-in-from-right duration-500">
          <div className="px-5 py-4 bg-gray-900/90 backdrop-blur-xl border border-white/10 rounded-2xl shadow-2xl flex flex-col gap-1 min-w-[280px]">
            <div className="flex items-center gap-2">
              <span className="w-2 h-2 rounded-full bg-indigo-500 animate-pulse" />
              <span className="text-xs font-black text-indigo-400 uppercase tracking-tighter">Indexing Complete</span>
            </div>
            <div className="flex flex-col text-sm">
              <span className="font-bold text-white uppercase">{indexToast.name}</span>
              <p className="text-xs text-gray-400 mt-1">{indexToast.message}</p>
            </div>
          </div>
        </div>
      )}

      {migrationToast && (
        <div className="fixed bottom-6 left-1/2 -translate-x-1/2 z-50 px-5 py-3 bg-indigo-900/90 backdrop-blur-xl border border-indigo-500/30 rounded-2xl shadow-2xl text-sm text-indigo-100 max-w-sm text-center">
          {migrationToast}
        </div>
      )}

      <ModelDownloadModal />
    </BrowserRouter>
  );
}
