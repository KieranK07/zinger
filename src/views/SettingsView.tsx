import { useEffect, useState } from "react";
import { api } from "../lib/api";
import type { AdapterInfo } from "../lib/types";

const EDITABLE: { key: string; label: string; hint: string }[] = [
  { key: "platform_fee_pct", label: "Platform fee %", hint: "Deducted from est. resale (eBay ≈ 13)" },
  { key: "shipping_flat_default", label: "Default shipping estimate ($)", hint: "Used when no category table entry exists" },
  { key: "poll_interval_minutes", label: "Poll interval (minutes)", hint: "Background polling starts in M2" },
  { key: "notify_score_threshold_default", label: "Notification score threshold", hint: "Notify when a new listing scores above this (M4)" },
  { key: "quiet_hours_start", label: "Quiet hours start", hint: "HH:MM, no notifications after this time" },
  { key: "quiet_hours_end", label: "Quiet hours end", hint: "HH:MM" },
];

export function SettingsView() {
  const [settings, setSettings] = useState<Record<string, string>>({});
  const [adapters, setAdapters] = useState<AdapterInfo[]>([]);
  const [savedKey, setSavedKey] = useState<string | null>(null);

  useEffect(() => {
    api.getSettings().then(setSettings);
    api.adapterHealth().then(setAdapters);
  }, []);

  const save = async (key: string, value: string) => {
    await api.setSetting(key, value);
    setSavedKey(key);
    setTimeout(() => setSavedKey(null), 1500);
  };

  return (
    <div className="flex max-w-2xl flex-col gap-6">
      <section className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-4">
        <h2 className="mb-3 text-sm font-semibold text-zinc-300">Valuation & polling</h2>
        <div className="flex flex-col gap-3">
          {EDITABLE.map(({ key, label, hint }) => (
            <div key={key} className="flex items-center gap-3">
              <div className="w-64">
                <div className="text-sm text-zinc-200">{label}</div>
                <div className="text-xs text-zinc-500">{hint}</div>
              </div>
              <input
                className="w-32 rounded border border-zinc-700 bg-zinc-900 px-3 py-1.5 text-sm"
                value={settings[key] ?? ""}
                onChange={(e) => setSettings({ ...settings, [key]: e.target.value })}
                onBlur={(e) => save(key, e.target.value)}
              />
              {savedKey === key && <span className="text-xs text-emerald-400">saved</span>}
            </div>
          ))}
        </div>
      </section>

      <section className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-4">
        <h2 className="mb-3 text-sm font-semibold text-zinc-300">Adapters</h2>
        <div className="flex flex-col gap-2">
          {adapters.map((a) => (
            <div key={a.id} className="flex items-center justify-between text-sm">
              <span className="text-zinc-200">{a.display_name}</span>
              <span
                className={
                  a.status.state === "ok"
                    ? "rounded bg-emerald-500/15 px-2 py-0.5 text-xs text-emerald-400"
                    : "rounded bg-amber-500/15 px-2 py-0.5 text-xs text-amber-400"
                }
              >
                {a.status.state}
                {"reason" in a.status && ` — ${a.status.reason}`}
              </span>
            </div>
          ))}
          <p className="mt-1 text-xs text-zinc-500">
            Craigslist and eBay adapters arrive in M2. Facebook Marketplace and OfferUp are
            post-MVP, off by default, and carry ToS risk — see the README.
          </p>
        </div>
      </section>

      <section className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-4">
        <h2 className="mb-1 text-sm font-semibold text-zinc-300">AI (optional, BYOK)</h2>
        <p className="text-xs text-zinc-500">
          Coming in M5. Nexus is fully functional without AI; keys will be stored in the OS
          keychain, never in the database.
        </p>
      </section>
    </div>
  );
}
