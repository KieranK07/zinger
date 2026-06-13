import { invoke } from "@tauri-apps/api/core";
import type {
  AdapterInfo,
  FbStatus,
  Listing,
  RunReport,
  SavedSearch,
  SearchInput,
} from "./types";

export const api = {
  listSearches: () => invoke<SavedSearch[]>("list_searches"),
  createSearch: (input: SearchInput) => invoke<number>("create_search", { input }),
  updateSearch: (search: SavedSearch) => invoke<void>("update_search", { search }),
  deleteSearch: (id: number) => invoke<void>("delete_search", { id }),
  runSearch: (searchId: number) => invoke<RunReport>("run_search", { searchId }),
  listListings: (searchId: number, includeHidden = false) =>
    invoke<Listing[]>("list_listings", { searchId, includeHidden }),
  setListingAction: (listingId: number, field: "seen" | "hidden" | "saved", value: boolean) =>
    invoke<void>("set_listing_action", { listingId, field, value }),
  getSettings: () => invoke<Record<string, string>>("get_settings"),
  setSetting: (key: string, value: string) => invoke<void>("set_setting", { key, value }),
  adapterHealth: () => invoke<AdapterInfo[]>("adapter_health"),
  setEbayCredentials: (clientId: string, clientSecret: string) =>
    invoke<void>("set_ebay_credentials", { clientId, clientSecret }),
  ebayCredentialsStatus: () => invoke<boolean>("ebay_credentials_status"),
  testEbayConnection: () => invoke<string>("test_ebay_connection"),
  fbStatus: () => invoke<FbStatus>("fb_status"),
  fbAcceptTos: () => invoke<void>("fb_accept_tos"),
  fbSetEnabled: (enabled: boolean) => invoke<void>("fb_set_enabled", { enabled }),
  fbSetMaxPages: (maxPages: number) => invoke<void>("fb_set_max_pages", { maxPages }),
  fbInstallChromium: () => invoke<void>("fb_install_chromium"),
  fbLogin: () => invoke<string>("fb_login"),
  fbRefreshChromium: () => invoke<boolean>("fb_refresh_chromium"),
};
