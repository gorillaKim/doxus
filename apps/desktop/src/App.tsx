import { useEffect, useRef, useState } from "react";
import { BrowserRouter, Route, Routes } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { AppShell } from "./components/layout/AppShell";
import { SearchPage } from "./pages/SearchPage";
import { ProjectsPage } from "./pages/ProjectsPage";
import WorkspacePage from "./pages/WorkspacePage";
import MarketPage from "./pages/MarketPage";
import DashboardPage from "./pages/DashboardPage";
import SettingsPage from "./pages/SettingsPage";

export default function App() {
  const [cacheToast, setCacheToast] = useState<string | null>(null);
  const toastTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    const unlisten = listen<{ count: number }>("cache:cleanup", (event) => {
      const { count } = event.payload;
      if (toastTimer.current) clearTimeout(toastTimer.current);
      setCacheToast(`캐시 정리 완료 — 만료된 항목 ${count}개 제거됨`);
      toastTimer.current = setTimeout(() => setCacheToast(null), 4000);
    });
    return () => { unlisten.then(fn => fn()); };
  }, []);

  return (
    <BrowserRouter>
      <AppShell>
        <Routes>
          <Route path="/" element={<DashboardPage />} />
          <Route path="/search" element={<SearchPage />} />
          <Route path="/projects" element={<ProjectsPage />} />
          <Route path="/workspace" element={<WorkspacePage />} />
          <Route path="/market" element={<MarketPage />} />
          <Route path="/settings" element={<SettingsPage />} />
        </Routes>
      </AppShell>

      {cacheToast && (
        <div className="fixed bottom-6 right-6 z-50 px-4 py-3 bg-gray-900 border border-gray-700 rounded-xl shadow-xl text-sm text-gray-200 max-w-xs">
          🗑️ {cacheToast}
        </div>
      )}
    </BrowserRouter>
  );
}
