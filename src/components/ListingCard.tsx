import { openUrl } from "@tauri-apps/plugin-opener";
import type { Listing } from "../lib/types";

const CONDITION_LABELS: Record<string, string> = {
  new: "New",
  like_new: "Like new",
  good: "Good",
  fair: "Fair",
  parts: "Parts",
};

function relativeAge(iso: string | null): string {
  if (!iso) return "";
  const ms = Date.now() - new Date(iso).getTime();
  const hours = Math.floor(ms / 3_600_000);
  if (hours < 1) return "just now";
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

interface Props {
  listing: Listing;
  onAction: (id: number, field: "hidden" | "saved", value: boolean) => void;
}

export function ListingCard({ listing, onAction }: Props) {
  return (
    <div className="flex flex-col overflow-hidden rounded-lg border border-zinc-800 bg-zinc-900 transition-colors hover:border-zinc-600">
      <div className="relative h-40 bg-zinc-800">
        {listing.images[0] ? (
          <img
            src={listing.images[0]}
            alt=""
            className="h-full w-full object-cover"
            loading="lazy"
          />
        ) : (
          <div className="flex h-full items-center justify-center text-sm text-zinc-600">
            no image
          </div>
        )}
        {/* Score badge: valuation lands in M3; show an honest placeholder. */}
        <span className="absolute right-2 top-2 rounded bg-zinc-950/80 px-2 py-0.5 text-xs font-semibold text-zinc-400">
          score —
        </span>
      </div>

      <div className="flex flex-1 flex-col gap-1 p-3">
        <div className="line-clamp-2 text-sm font-medium text-zinc-100">{listing.title}</div>
        <div className="flex items-baseline gap-2">
          <span className="text-lg font-bold text-emerald-400">
            ${listing.price.toFixed(0)}
          </span>
          {listing.condition && (
            <span className="rounded bg-zinc-800 px-1.5 py-0.5 text-xs text-zinc-400">
              {CONDITION_LABELS[listing.condition]}
            </span>
          )}
        </div>
        <div className="text-xs text-zinc-500">
          {listing.location_text ?? "unknown location"}
          {listing.posted_at && ` · ${relativeAge(listing.posted_at)}`}
          {` · ${listing.source}`}
        </div>

        <div className="mt-auto flex gap-2 pt-2">
          <button
            onClick={() => onAction(listing.id, "saved", !listing.saved)}
            className={`rounded px-2 py-1 text-xs ${
              listing.saved
                ? "bg-amber-500/20 text-amber-300"
                : "bg-zinc-800 text-zinc-400 hover:bg-zinc-700"
            }`}
          >
            {listing.saved ? "★ Saved" : "☆ Save"}
          </button>
          <button
            onClick={() => onAction(listing.id, "hidden", true)}
            className="rounded bg-zinc-800 px-2 py-1 text-xs text-zinc-400 hover:bg-zinc-700"
          >
            Hide
          </button>
          <button
            onClick={() => openUrl(listing.source_url)}
            className="ml-auto rounded bg-zinc-800 px-2 py-1 text-xs text-zinc-400 hover:bg-zinc-700"
          >
            Open ↗
          </button>
        </div>
      </div>
    </div>
  );
}
