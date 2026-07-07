import { useState, useEffect, useRef } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { listen } from "@tauri-apps/api/event";
import { api, errMessage } from "../lib/api";
import type { NexusMod, UpdatedModRef } from "../lib/types";
import { Button, Spinner, useAsync, useToast } from "../lib/ui";
import { useApp } from "../App";

interface LogEntry {
  step: string;
  detail: string;
  level: string;
  ts: string;
}

type Feed = "trending" | "latest_added" | "latest_updated";

// Nexus's trending/latest_added endpoints are fixed top-10 snapshots (no
// offset/page/count param affects them) — genuine scroll-driven paging only
// makes sense against "recently updated", which we pull as a much bigger
// (200+ mod) time-windowed list instead of the capped latest_updated.json.
const PAGE_SIZE = 12;

export default function Online() {
  const { settings, reloadSettings, bumpMods } = useApp();
  const toast = useToast();
  const [feed, setFeed] = useState<Feed>("trending");
  const [link, setLink] = useState("");
  const [busyLink, setBusyLink] = useState(false);
  const [installingId, setInstallingId] = useState<number | null>(null);
  const [loggingIn, setLoggingIn] = useState(false);
  const [autoCreating, setAutoCreating] = useState(false);
  const [autoStep, setAutoStep] = useState<"idle" | "registering" | "done">("idle");
  const [autoLogs, setAutoLogs] = useState<LogEntry[]>([]);
  const [visibleCount, setVisibleCount] = useState(PAGE_SIZE);
  const sentinelRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const unlisten = listen<LogEntry>("auto-register-log", (event) => {
      setAutoLogs((prev) => [...prev.slice(-19), event.payload]);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const loggedIn = !!settings.nexus_api_key;
  const isUpdatedFeed = feed === "latest_updated";

  // Trending / Newest: Nexus returns full mod objects for these already, but
  // as a fixed top-10 snapshot (verified: page/offset/count params do
  // nothing). Reveal them progressively anyway for a consistent scroll feel.
  const { data: simpleData, loading: simpleLoading, error: simpleError } = useAsync<NexusMod[]>(
    () => (loggedIn && !isUpdatedFeed ? api.nexusBrowse(feed) : Promise.resolve([])),
    [feed, loggedIn, isUpdatedFeed]
  );

  // Recently updated: the updated.json endpoint only returns lightweight
  // {mod_id, timestamps} stubs for ~200 mods, not full details. Fetching all
  // of them up front would burn through Nexus's rate limit, so full details
  // are fetched lazily in PAGE_SIZE batches as the user actually scrolls.
  const [updatedIndex, setUpdatedIndex] = useState<UpdatedModRef[]>([]);
  const [updatedMods, setUpdatedMods] = useState<NexusMod[]>([]);
  const [updatedIndexLoading, setUpdatedIndexLoading] = useState(false);
  const [updatedMoreLoading, setUpdatedMoreLoading] = useState(false);
  const [updatedError, setUpdatedError] = useState<string | null>(null);
  const loadingMoreRef = useRef(false);

  useEffect(() => {
    if (!loggedIn || !isUpdatedFeed) return;
    let cancelled = false;
    setUpdatedIndexLoading(true);
    setUpdatedError(null);
    setUpdatedIndex([]);
    setUpdatedMods([]);
    api
      .nexusUpdated("1m")
      .then((idx) => {
        if (!cancelled) setUpdatedIndex(idx);
      })
      .catch((e) => !cancelled && setUpdatedError(errMessage(e)))
      .finally(() => !cancelled && setUpdatedIndexLoading(false));
    return () => {
      cancelled = true;
    };
  }, [isUpdatedFeed, loggedIn]);

  async function loadMoreUpdated(currentlyLoaded: number) {
    if (loadingMoreRef.current) return;
    const next = updatedIndex.slice(currentlyLoaded, currentlyLoaded + PAGE_SIZE);
    if (next.length === 0) return;
    loadingMoreRef.current = true;
    setUpdatedMoreLoading(true);
    try {
      const results = await Promise.allSettled(next.map((r) => api.nexusModDetails(r.mod_id)));
      const ok = results
        .filter((r): r is PromiseFulfilledResult<NexusMod> => r.status === "fulfilled")
        .map((r) => r.value);
      setUpdatedMods((prev) => [...prev, ...ok]);
    } finally {
      loadingMoreRef.current = false;
      setUpdatedMoreLoading(false);
    }
  }

  // Kick off the first batch as soon as the index arrives.
  useEffect(() => {
    if (isUpdatedFeed && updatedIndex.length > 0 && updatedMods.length === 0) {
      loadMoreUpdated(0);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [updatedIndex]);

  const loading = isUpdatedFeed ? updatedIndexLoading : simpleLoading;
  const error = isUpdatedFeed ? updatedError : simpleError;

  // For trending/newest, "loading more" just reveals more of the already-
  // fetched 10 items; for recently-updated it fetches the next batch.
  useEffect(() => {
    setVisibleCount(PAGE_SIZE);
  }, [feed, simpleData]);

  const visible = isUpdatedFeed ? updatedMods : (simpleData ?? []).slice(0, visibleCount);
  const hasMore = isUpdatedFeed
    ? updatedMods.length < updatedIndex.length
    : visibleCount < (simpleData ?? []).length;

  useEffect(() => {
    const node = sentinelRef.current;
    if (!node) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (!entries[0]?.isIntersecting) return;
        if (isUpdatedFeed) {
          loadMoreUpdated(updatedMods.length);
        } else {
          setVisibleCount((n) => Math.min(n + PAGE_SIZE, (simpleData ?? []).length));
        }
      },
      { rootMargin: "400px" }
    );
    observer.observe(node);
    return () => observer.disconnect();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isUpdatedFeed, simpleData, updatedMods, updatedIndex]);

  async function autoCreate() {
    setAutoCreating(true);
    setAutoStep("registering");
    setAutoLogs([]);
    try {
      toast.push("info", "Creating account — this may take a minute…");
      const res = await api.autoFullRegister();
      await reloadSettings();
      setAutoStep("done");
      toast.push(res.valid ? "success" : "error", res.valid ? `Signed in as ${res.name}` : "Account creation failed");
    } catch (e) {
      toast.push("error", errMessage(e));
      setAutoStep("idle");
    } finally {
      setAutoCreating(false);
    }
  }

  async function login() {
    setLoggingIn(true);
    try {
      toast.push("info", "Opening Nexus in your browser — click Authorize.");
      const res = await api.nexusSsoLogin();
      await reloadSettings();
      toast.push(res.valid ? "success" : "error", res.valid ? `Signed in as ${res.name}` : "Login failed");
    } catch (e) {
      toast.push("error", errMessage(e));
    } finally {
      setLoggingIn(false);
    }
  }

  async function installFromLink() {
    const val = link.trim();
    if (!val) return;
    setBusyLink(true);
    try {
      if (val.startsWith("nxm://")) {
        const m = await api.installNxm(val);
        toast.push("success", `Installed "${m.name}"`);
        bumpMods();
        setLink("");
      } else {
        await openUrl(val.startsWith("http") ? val : `https://${val}`);
        toast.push("info", "Opened on Nexus — click ‘Mod Manager Download’ there.");
      }
    } catch (e) {
      toast.push("error", errMessage(e));
    } finally {
      setBusyLink(false);
    }
  }

  async function install(m: NexusMod) {
    setInstallingId(m.mod_id);
    try {
      toast.push("info", `Fetching download for "${m.name}"…`);
      const files = await api.nexusModFiles(m.mod_id);
      const arr: any[] = files?.files ?? [];
      const primary =
        arr.find((f: any) => f.is_primary) ??
        arr.find((f: any) => f.category_name === "MAIN") ??
        arr[arr.length - 1];
      if (!primary) throw "No downloadable file found";

      if (settings.is_premium) {
        // Direct API download (premium only)
        const installed = await api.installModFile(m.mod_id, primary.file_id);
        toast.push("success", `Installed "${installed.name}"`);
        bumpMods();
      } else {
        // Free account: open the mod page in a WebView and click whichever
        // download button Nexus offers for this file (Mod Manager or Slow).
        toast.push("info", "Opening download in Nexus…");
        const installed = await api.nexusAutoDownload(m.mod_id, primary.file_id);
        toast.push("success", `Installed "${installed.name}"`);
        bumpMods();
      }
    } catch (e) {
      toast.push("error", errMessage(e));
    } finally {
      setInstallingId(null);
    }
  }

  // ---- login gate -------------------------------------------------------
  if (!loggedIn) {
    return (
      <div className="mx-auto flex h-full max-w-md flex-col items-center justify-center gap-4 text-center">
        <div className="text-4xl">🔐</div>
        <h2 className="text-xl font-semibold">Connect your Nexus account</h2>
        <p className="text-sm text-neutral-400">
          Mods are hosted on Nexus, which needs a (free) account to download. Click below — a
          browser tab opens, you hit <b>Authorize</b> once, and you're in. No key copying.
        </p>
        <Button variant="primary" disabled={loggingIn} onClick={login}>
          {loggingIn ? <Spinner /> : "🔗"} Login with Nexus
        </Button>

        <div className="w-full border-t border-neutral-800 pt-4">
          {autoStep === "idle" ? (
            <>
              <p className="mb-2 text-xs text-neutral-500">No account? Let us create one for you.</p>
              <Button
                variant="ghost"
                disabled={autoCreating}
                onClick={autoCreate}
                className="w-full text-xs"
              >
                {autoCreating ? <Spinner /> : "✨"} Create temporary account automatically
              </Button>
            </>
          ) : autoStep === "registering" ? (
            <div className="w-full rounded-md border border-amber-800 bg-amber-950/20 text-xs">
              <div className="flex items-center gap-2 border-b border-amber-900/50 px-3 py-2 text-amber-300">
                <Spinner /> Creating Nexus account…
              </div>
              <div className="max-h-48 overflow-y-auto p-2 font-mono text-[11px] leading-relaxed">
                {autoLogs.length === 0 && (
                  <div className="text-neutral-500 italic">Starting…</div>
                )}
                {autoLogs.map((l, i) => (
                  <div
                    key={i}
                    className={l.level === "error" ? "text-red-400" : "text-neutral-400"}
                  >
                    <span className="text-neutral-600">[{l.step}]</span> {l.detail}
                  </div>
                ))}
              </div>
            </div>
          ) : autoStep === "done" ? (
            <div className="rounded-md border border-green-800 bg-green-950/30 p-3 text-xs">
              <div className="text-green-300">Account created and authorized! You can now browse mods.</div>
            </div>
          ) : null}
        </div>
      </div>
    );
  }

  const feeds: { id: Feed; label: string }[] = [
    { id: "trending", label: "Trending" },
    { id: "latest_added", label: "Newest" },
    { id: "latest_updated", label: "Recently updated" },
  ];

  return (
    <div className="flex flex-col gap-4">
      <div>
        <h2 className="text-xl font-semibold">Browse Nexus Mods</h2>
        <p className="text-xs text-neutral-500">
          Browse, install, and manage mods directly from Nexus Mods.
        </p>
      </div>

      {/* Install from link / URL (Nexus has no keyword search API) */}
      <div className="flex gap-2 rounded-lg border border-neutral-800 bg-neutral-900/40 p-3">
        <input
          value={link}
          onChange={(e) => setLink(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && installFromLink()}
          placeholder="Paste an nxm:// link or a Nexus mod URL…"
          className="flex-1 rounded-md border border-neutral-700 bg-neutral-900 px-3 py-1.5 text-sm outline-none focus:border-green-600"
        />
        <Button variant="primary" disabled={busyLink || !link.trim()} onClick={installFromLink}>
          {busyLink ? <Spinner /> : "Install"}
        </Button>
      </div>

      <div className="flex gap-2">
        {feeds.map((f) => (
          <button
            key={f.id}
            onClick={() => setFeed(f.id)}
            className={`rounded-md px-3 py-1 text-sm ${
              feed === f.id ? "bg-green-600/20 text-green-300" : "text-neutral-400 hover:bg-neutral-800"
            }`}
          >
            {f.label}
          </button>
        ))}
      </div>

      {loading && (
        <div className="flex items-center gap-2 text-neutral-400">
          <Spinner /> Loading feed…
        </div>
      )}
      {error && (
        <div className="rounded-md border border-red-900 bg-red-950/30 p-3 text-sm text-red-300">
          {error}
        </div>
      )}

      <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
        {visible.map((m) => (
          <div
            key={m.mod_id}
            className="flex flex-col overflow-hidden rounded-lg border border-neutral-800 bg-neutral-900/40"
          >
            {m.picture_url && <img src={m.picture_url} alt="" className="h-32 w-full object-cover" />}
            <div className="flex flex-1 flex-col gap-1 p-3">
              <div className="font-medium">{m.name}</div>
              <div className="line-clamp-2 flex-1 text-xs text-neutral-500">{m.summary}</div>
              <div className="mt-1 flex items-center justify-between text-[11px] text-neutral-500">
                <span>{m.author}</span>
                <span>♥ {m.endorsement_count ?? 0}</span>
              </div>
              <div className="mt-2 flex gap-2">
                <Button
                  variant="primary"
                  className="flex-1"
                  disabled={installingId === m.mod_id}
                  onClick={() => install(m)}
                >
                  {installingId === m.mod_id ? <Spinner /> : "Install"}
                </Button>
                <Button
                  variant="ghost"
                  onClick={() => openUrl(`https://www.nexusmods.com/scavprototype/mods/${m.mod_id}`)}
                >
                  ↗
                </Button>
              </div>
            </div>
          </div>
        ))}
      </div>

      {!loading && visible.length > 0 && (
        <div ref={sentinelRef} className="flex items-center justify-center gap-2 py-4">
          {hasMore ? (
            <>
              <Spinner />
              {isUpdatedFeed && updatedMoreLoading && (
                <span className="text-xs text-neutral-500">Loading more…</span>
              )}
            </>
          ) : (
            <span className="text-xs text-neutral-600">
              {feed === "latest_updated"
                ? "That's everything updated in the last month."
                : "That's the whole list — Nexus only exposes the top 10 here."}
            </span>
          )}
        </div>
      )}
    </div>
  );
}
