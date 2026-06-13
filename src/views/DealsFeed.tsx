import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "../lib/api";
import type { Listing, RunReport, SavedSearch } from "../lib/types";
import { ListingCard } from "../components/ListingCard";

export function DealsFeed() {
  const [searches, setSearches] = useState<SavedSearch[]>([]);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [listings, setListings] = useState<Listing[]>([]);
  const [running, setRunning] = useState(false);
  const [lastRun, setLastRun] = useState<RunReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .listSearches()
      .then((s) => {
        setSearches(s);
        if (s.length > 0) setSelectedId((cur) => cur ?? s[0].id);
      })
      .catch((e) => setError(String(e)));
  }, []);

  const refresh = useCallback(() => {
    if (selectedId == null) return;
    api
      .listListings(selectedId)
      .then(setListings)
      .catch((e) => setError(String(e)));
  }, [selectedId]);

  useEffect(refresh, [refresh]);

  // Background scheduler finished a cycle: refresh if it was our search.
  useEffect(() => {
    const unlisten = listen<RunReport>("poll-completed", (event) => {
      setLastRun(event.payload);
      if (event.payload.search_id === selectedId) refresh();
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [selectedId, refresh]);

  const runNow = async () => {
    if (selectedId == null) return;
    setRunning(true);
    setError(null);
    try {
      const report = await api.runSearch(selectedId);
      setLastRun(report);
      refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setRunning(false);
    }
  };

  const onAction = async (id: number, field: "hidden" | "saved", value: boolean) => {
    await api.setListingAction(id, field, value);
    refresh();
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center gap-3">
        <select
          value={selectedId ?? ""}
          onChange={(e) => setSelectedId(Number(e.target.value))}
          className="rounded border border-zinc-700 bg-zinc-900 px-3 py-1.5 text-sm"
        >
          {searches.map((s) => (
            <option key={s.id} value={s.id}>
              {s.name}
            </option>
          ))}
        </select>
        <button
          onClick={runNow}
          disabled={running || selectedId == null}
          className="rounded bg-emerald-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-emerald-500 disabled:opacity-50"
        >
          {running ? "Running…" : "Run now"}
        </button>
        {lastRun && (
          <span className="text-xs text-zinc-500">
            +{lastRun.inserted} new · {lastRun.already_known} known ·{" "}
            {lastRun.duplicates_skipped} dupes
            {lastRun.adapters
              .filter((a) => a.error)
              .map((a) => ` · ${a.adapter_id} failed`)
              .join("")}
          </span>
        )}
      </div>

      {error && (
        <div className="rounded border border-red-900 bg-red-950 px-3 py-2 text-sm text-red-300">
          {error}
        </div>
      )}

      {listings.length === 0 ? (
        <div className="py-16 text-center text-zinc-500">
          No listings yet. Hit "Run now" to poll sources.
        </div>
      ) : (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-4">
          {listings.map((l) => (
            <ListingCard key={l.id} listing={l} onAction={onAction} />
          ))}
        </div>
      )}
    </div>
  );
}
