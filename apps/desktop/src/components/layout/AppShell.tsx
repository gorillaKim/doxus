import { ReactNode } from "react";
import { NavLink } from "react-router-dom";
import { ChatDrawer } from "./ChatDrawer";
import { useChatStore } from "../../stores/useChatStore";

const NAV_ITEMS = [
  { to: "/", label: "Dashboard" },
  { to: "/search", label: "Search" },
  { to: "/projects", label: "Projects" },
  { to: "/workspace", label: "Workspace" },
  { to: "/market", label: "Market" },
];

export function AppShell({ children }: { children: ReactNode }) {
  const { isOpen, toggle } = useChatStore();

  return (
    <div className="flex h-screen bg-gray-950 text-gray-100">
      {/* Sidebar */}
      <nav className="w-48 flex-shrink-0 border-r border-gray-800 flex flex-col p-4 gap-2">
        <span className="text-lg font-bold mb-4 text-indigo-400">doxus</span>
        {NAV_ITEMS.map(({ to, label }) => (
          <NavLink
            key={to}
            to={to}
            className={({ isActive }) =>
              `px-3 py-2 rounded text-sm ${
                isActive
                  ? "bg-indigo-600 text-white"
                  : "text-gray-400 hover:text-white hover:bg-gray-800"
              }`
            }
          >
            {label}
          </NavLink>
        ))}
        <div className="mt-auto">
          <button
            onClick={toggle}
            className="w-full px-3 py-2 rounded text-sm text-gray-400 hover:text-white hover:bg-gray-800"
          >
            {isOpen ? "Close Chat" : "Chat"}
          </button>
        </div>
      </nav>

      {/* Main content */}
      <main className="flex-1 overflow-auto p-6">{children}</main>

      {/* Chat drawer overlay */}
      {isOpen && <ChatDrawer />}
    </div>
  );
}
