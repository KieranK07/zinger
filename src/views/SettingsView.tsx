import { useEffect, useState } from "react";
import { api } from "../lib/api";
import type { AdapterInfo } from "../lib/types";
import { FacebookPanel } from "./FacebookPanel";

const EDITABLE: { key: string; label: string; hint: string }[] = [
  { key: "platform_fee_pct", label: "Platform fee %", hint: "Deducted from est. resale (eBay ≈ 13)" },
  { key: "shipping_flat_default", label: "Default shipping estimate ($)", hint: "Used when no category table entry exists" },
  { key: "poll_interval_minutes", label: "Poll interval (minutes)", hint: "Global default; searches can override it" },
  { key: "craigslist_site", label: "Craigslist site", hint: "e.g. seattle, sfbay. Empty = derive from search location. Restart to apply" },
  { key: "notify_score_threshold_default", label: "Notification score threshold", hint: "Notify when a new listing scores above this (M4)" },
  { key: "quiet_hours_start", label: "Quiet hours start", hint: "HH:MM, no notifications after this time" },
  { key: "quiet_hours_end", label: "Quiet hours end", hint: "HH:MM" },
];

export function SettingsView() {
  const [settings, setSettings] = useState<Record<string, string>>({});
  const [adapters, setAdapters] = useState<AdapterInfo[]>([]);
  const [savedKey, setSavedKey] = useState<string | null>(null);
  const [ebayConfigured, setEbayConfigured] = useState(false);
  const [ebayClientId, setEbayClientId] = useState("");
  const [ebayClientSecret, setEbayClientSecret] = useState("");
  const [ebayMessage, setEbayMessage] = useState<string | null>(null);
  const [ebayTesting, setEbayTesting] = useState(false);

  const refreshAdapters = () => {
    api.adapterHealth().then(setAdapters);
    api.ebayCredentialsStatus().then(setEbayConfigured);
  };

  useEffect(() => {
    api.getSettings().then(setSettings);
    refreshAdapters();
  }, []);

  const save = async (key: string, value: string) => {
    await api.setSetting(key, value);
    setSavedKey(key);
    setTimeout(() => setSavedKey(null), 1500);
  };

  const saveEbayKeys = async () => {
    setEbayMessage(null);
    try {
      await api.setEbayCredentials(ebayClientId, ebayClientSecret);
      setEbayClientId("");
      setEbayClientSecret("");
      setEbayMessage("Keys saved to your OS keychain.");
      refreshAdapters();
    } catch (e) {
      setEbayMessage(String(e));
    }
  };

  const testEbay = async () => {
    setEbayTesting(true);
    setEbayMessage(null);
    try {
      setEbayMessage(await api.testEbayConnection());
    } catch (e) {
      setEbayMessage(String(e));
    } finally {
      setEbayTesting(false);
    }
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
                    : a.status.state === "not_configured"
                      ? "rounded bg-zinc-700/40 px-2 py-0.5 text-xs text-zinc-400"
                      : "rounded bg-amber-500/15 px-2 py-0.5 text-xs text-amber-400"
                }
              >
                {a.status.state.replace("_", " ")}
                {"reason" in a.status && ` — ${a.status.reason}`}
              </span>
            </div>
          ))}
          <p className="mt-1 text-xs text-zinc-500">
            Craigslist parses public pages politely and may still violate its ToS — your call,
            see the README. Facebook Marketplace and OfferUp are not implemented.
          </p>
        </div>
      </section>

      <section className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-4">
        <h2 className="mb-1 text-sm font-semibold text-zinc-300">eBay API keys (BYOK)</h2>
        <p className="mb-3 text-xs text-zinc-500">
          Create an app at developer.ebay.com and paste the production App ID (client ID) and
          Cert ID (client secret). Stored in your OS keychain — never the database, never logs.
          {ebayConfigured && (
            <span className="ml-1 text-emerald-400">Keys are currently configured.</span>
          )}
        </p>
        <div className="flex flex-col gap-2">
          <input
            type="password"
            placeholder="App ID (client ID)"
            value={ebayClientId}
            onChange={(e) => setEbayClientId(e.target.value)}
            className="w-full rounded border border-zinc-700 bg-zinc-900 px-3 py-1.5 text-sm"
          />
          <input
            type="password"
            placeholder="Cert ID (client secret)"
            value={ebayClientSecret}
            onChange={(e) => setEbayClientSecret(e.target.value)}
            className="w-full rounded border border-zinc-700 bg-zinc-900 px-3 py-1.5 text-sm"
          />
          <div className="flex items-center gap-2">
            <button
              onClick={saveEbayKeys}
              disabled={!ebayClientId.trim() || !ebayClientSecret.trim()}
              className="rounded bg-emerald-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-emerald-500 disabled:opacity-50"
            >
              Save keys
            </button>
            <button
              onClick={testEbay}
              disabled={!ebayConfigured || ebayTesting}
              className="rounded bg-zinc-800 px-4 py-1.5 text-sm text-zinc-300 hover:bg-zinc-700 disabled:opacity-50"
            >
              {ebayTesting ? "Testing…" : "Test connection"}
            </button>
          </div>
          {ebayMessage && <div className="text-xs text-zinc-400">{ebayMessage}</div>}
        </div>
      </section>

      <FacebookPanel />

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
