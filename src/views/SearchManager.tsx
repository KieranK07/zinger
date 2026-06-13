import { useEffect, useState } from "react";
import { api } from "../lib/api";
import type { SavedSearch, SearchInput } from "../lib/types";

const EMPTY: SearchInput = {
  name: "",
  keywords: "",
  category: null,
  location_text: "",
  lat: null,
  lng: null,
  radius_km: 40,
  price_ceiling: null,
  poll_interval_minutes: null,
};

export function SearchManager() {
  const [searches, setSearches] = useState<SavedSearch[]>([]);
  const [form, setForm] = useState<SearchInput>(EMPTY);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = () => api.listSearches().then(setSearches).catch((e) => setError(String(e)));
  useEffect(() => {
    refresh();
  }, []);

  const submit = async () => {
    setError(null);
    if (!form.name.trim() || !form.keywords.trim()) {
      setError("Name and keywords are required.");
      return;
    }
    try {
      if (editingId == null) {
        await api.createSearch(form);
      } else {
        const existing = searches.find((s) => s.id === editingId);
        if (existing) {
          await api.updateSearch({ ...existing, ...form });
        }
      }
      setForm(EMPTY);
      setEditingId(null);
      refresh();
    } catch (e) {
      setError(String(e));
    }
  };

  const startEdit = (s: SavedSearch) => {
    setEditingId(s.id);
    setForm({
      name: s.name,
      keywords: s.keywords,
      category: s.category,
      location_text: s.location_text,
      lat: s.lat,
      lng: s.lng,
      radius_km: s.radius_km,
      price_ceiling: s.price_ceiling,
      poll_interval_minutes: s.poll_interval_minutes,
    });
  };

  const remove = async (id: number) => {
    await api.deleteSearch(id);
    if (editingId === id) {
      setEditingId(null);
      setForm(EMPTY);
    }
    refresh();
  };

  const field = "w-full rounded border border-zinc-700 bg-zinc-900 px-3 py-1.5 text-sm";
  const label = "mb-1 block text-xs font-medium text-zinc-400";

  return (
    <div className="flex flex-col gap-6">
      <div className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-4">
        <h2 className="mb-3 text-sm font-semibold text-zinc-300">
          {editingId == null ? "New search" : "Edit search"}
        </h2>
        <div className="grid grid-cols-2 gap-3">
          <div>
            <label className={label}>Name</label>
            <input
              className={field}
              value={form.name}
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              placeholder="Cordless drills under $150"
            />
          </div>
          <div>
            <label className={label}>Keywords</label>
            <input
              className={field}
              value={form.keywords}
              onChange={(e) => setForm({ ...form, keywords: e.target.value })}
              placeholder="dewalt drill"
            />
          </div>
          <div>
            <label className={label}>Location</label>
            <input
              className={field}
              value={form.location_text}
              onChange={(e) => setForm({ ...form, location_text: e.target.value })}
              placeholder="Seattle, WA"
            />
          </div>
          <div>
            <label className={label}>Radius (km)</label>
            <input
              type="number"
              className={field}
              value={form.radius_km}
              onChange={(e) => setForm({ ...form, radius_km: Number(e.target.value) || 40 })}
            />
          </div>
          <div>
            <label className={label}>Price ceiling ($, optional)</label>
            <input
              type="number"
              className={field}
              value={form.price_ceiling ?? ""}
              onChange={(e) =>
                setForm({
                  ...form,
                  price_ceiling: e.target.value === "" ? null : Number(e.target.value),
                })
              }
            />
          </div>
          <div>
            <label className={label}>Category (optional)</label>
            <input
              className={field}
              value={form.category ?? ""}
              onChange={(e) =>
                setForm({ ...form, category: e.target.value === "" ? null : e.target.value })
              }
              placeholder="tools"
            />
          </div>
          <div>
            <label className={label}>Poll interval (min, optional)</label>
            <input
              type="number"
              className={field}
              value={form.poll_interval_minutes ?? ""}
              placeholder="global default"
              onChange={(e) =>
                setForm({
                  ...form,
                  poll_interval_minutes:
                    e.target.value === "" ? null : Math.max(1, Number(e.target.value)),
                })
              }
            />
          </div>
        </div>
        {error && <div className="mt-2 text-sm text-red-400">{error}</div>}
        <div className="mt-3 flex gap-2">
          <button
            onClick={submit}
            className="rounded bg-emerald-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-emerald-500"
          >
            {editingId == null ? "Create" : "Save changes"}
          </button>
          {editingId != null && (
            <button
              onClick={() => {
                setEditingId(null);
                setForm(EMPTY);
              }}
              className="rounded bg-zinc-800 px-4 py-1.5 text-sm text-zinc-300 hover:bg-zinc-700"
            >
              Cancel
            </button>
          )}
        </div>
      </div>

      <div className="flex flex-col gap-2">
        {searches.map((s) => (
          <div
            key={s.id}
            className="flex items-center justify-between rounded-lg border border-zinc-800 bg-zinc-900 px-4 py-3"
          >
            <div>
              <div className="text-sm font-medium text-zinc-100">{s.name}</div>
              <div className="text-xs text-zinc-500">
                "{s.keywords}" · {s.location_text || "anywhere"} · {s.radius_km} km
                {s.price_ceiling != null && ` · ≤ $${s.price_ceiling}`}
              </div>
            </div>
            <div className="flex gap-2">
              <button
                onClick={() => startEdit(s)}
                className="rounded bg-zinc-800 px-3 py-1 text-xs text-zinc-300 hover:bg-zinc-700"
              >
                Edit
              </button>
              <button
                onClick={() => remove(s.id)}
                className="rounded bg-zinc-800 px-3 py-1 text-xs text-red-400 hover:bg-red-950"
              >
                Delete
              </button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
