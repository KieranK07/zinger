// Mirrors of the Rust models in src-tauri/src/models.rs. Keep in sync by hand;
// the surface is small enough that codegen isn't worth it yet.

export type Condition = "new" | "like_new" | "good" | "fair" | "parts";

export interface SavedSearch {
  id: number;
  name: string;
  keywords: string;
  category: string | null;
  location_text: string;
  lat: number | null;
  lng: number | null;
  radius_km: number;
  price_ceiling: number | null;
  enabled: boolean;
  notify_score_threshold: number | null;
  /** null = use the global poll_interval_minutes setting */
  poll_interval_minutes: number | null;
}

export interface SearchInput {
  name: string;
  keywords: string;
  category: string | null;
  location_text: string;
  lat: number | null;
  lng: number | null;
  radius_km: number;
  price_ceiling: number | null;
  poll_interval_minutes: number | null;
}

export interface Listing {
  id: number;
  search_id: number;
  source: string;
  source_id: string;
  source_url: string;
  title: string;
  description: string | null;
  price: number;
  currency: string;
  location_text: string | null;
  lat: number | null;
  lng: number | null;
  images: string[];
  posted_at: string | null;
  fetched_at: string;
  condition: Condition | null;
  category_guess: string | null;
  dedup_hash: string;
  seen: boolean;
  hidden: boolean;
  saved: boolean;
}

export type AdapterStatus =
  | { state: "ok" }
  | { state: "not_configured"; reason: string }
  | { state: "degraded"; reason: string }
  | { state: "disabled"; reason: string };

export interface AdapterInfo {
  id: string;
  display_name: string;
  status: AdapterStatus;
}

export interface FbStatus {
  enabled: boolean;
  tos_accepted: boolean;
  chromium_installed: boolean;
  logged_in: boolean;
  session_expired: boolean;
  disabled_until: string | null;
  max_pages: number;
}

export interface FbProgress {
  kind: "download" | "login" | "done" | "error";
  pct: number | null;
  message: string | null;
}

export interface AdapterRunReport {
  adapter_id: string;
  fetched: number;
  /** present when the adapter was skipped (not configured / backing off) */
  skipped: string | null;
  error: string | null;
}

export interface RunReport {
  search_id: number;
  adapters: AdapterRunReport[];
  inserted: number;
  duplicates_skipped: number;
  already_known: number;
}
