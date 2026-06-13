import { useState } from "react";
import "./index.css";
import { DealsFeed } from "./views/DealsFeed";
import { SearchManager } from "./views/SearchManager";
import { SettingsView } from "./views/SettingsView";

type View = "deals" | "searches" | "settings";

const NAV: { id: View; label: string }[] = [
  { id: "deals", label: "Deals" },
  { id: "searches", label: "Searches" },
  { id: "settings", label: "Settings" },
];

export default function App() {
  const [view, setView] = useState<View>("deals");

  return (
    <div className="flex h-screen">
      <nav className="flex w-44 shrink-0 flex-col border-r border-zinc-800 bg-zinc-950 p-3">
        <div className="mb-6 px-2 text-lg font-bold tracking-tight text-zinc-100">
          Zinger
        </div>
        {NAV.map(({ id, label }) => (
          <button
            key={id}
            onClick={() => setView(id)}
            className={`mb-1 rounded px-3 py-2 text-left text-sm font-medium transition-colors ${
              view === id
                ? "bg-zinc-800 text-zinc-100"
                : "text-zinc-400 hover:bg-zinc-900 hover:text-zinc-200"
            }`}
          >
            {label}
          </button>
        ))}
        <div className="mt-auto px-2 text-xs text-zinc-600">M1 · mock data</div>
      </nav>

      <main className="flex-1 overflow-y-auto p-6">
        {view === "deals" && <DealsFeed />}
        {view === "searches" && <SearchManager />}
        {view === "settings" && <SettingsView />}
      </main>
    </div>
  );
}
