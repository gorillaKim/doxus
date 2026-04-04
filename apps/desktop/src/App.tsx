import { BrowserRouter, Route, Routes } from "react-router-dom";
import { AppShell } from "./components/layout/AppShell";
import { SearchPage } from "./pages/SearchPage";
import { ProjectsPage } from "./pages/ProjectsPage";
import WorkspacePage from "./pages/WorkspacePage";
import MarketPage from "./pages/MarketPage";
import DashboardPage from "./pages/DashboardPage";

export default function App() {
  return (
    <BrowserRouter>
      <AppShell>
        <Routes>
          <Route path="/" element={<DashboardPage />} />
          <Route path="/search" element={<SearchPage />} />
          <Route path="/projects" element={<ProjectsPage />} />
          <Route path="/workspace" element={<WorkspacePage />} />
          <Route path="/market" element={<MarketPage />} />
        </Routes>
      </AppShell>
    </BrowserRouter>
  );
}
