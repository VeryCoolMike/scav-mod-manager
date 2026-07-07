import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLink, Heart, RefreshCw, Trash2 } from "lucide-react";
import { api, errMessage } from "../lib/api";
import type { InstalledMod, UpdateInfo } from "../lib/types";
import { Button, Spinner, useAsync, useToast } from "../lib/ui";
import { useApp } from "../App";

export default function ModsView() {
  const { modsVersion, bumpMods, settings } = useApp();
  const toast = useToast();
  const { data, loading, error, reload } = useAsync<InstalledMod[]>(
    () => api.listInstalled(),
    [modsVersion]
  );
  const [updates, setUpdates] = useState<Record<string, UpdateInfo>>({});
  const [checking, setChecking] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);

  async function toggle(m: InstalledMod) {
    setBusy(m.key);
    try {
      await api.setModEnabled(m.key, !m.enabled);
      await reload();
    } catch (e) {
      toast.push("error", errMessage(e));
    } finally {
      setBusy(null);
    }
  }

  async function remove(m: InstalledMod) {
    setBusy(m.key);
    try {
      await api.uninstallMod(m.key);
      await reload();
      bumpMods();
    } catch (e) {
      toast.push("error", errMessage(e));
    } finally {
      setBusy(null);
    }
  }

  async function checkUpdates() {
    setChecking(true);
    try {
      const list = await api.checkUpdates();
      const map: Record<string, UpdateInfo> = {};
      let n = 0;
      for (const u of list) {
        map[u.key] = u;
        if (u.update_available) n++;
      }
      setUpdates(map);
      toast.push(n ? "info" : "success", n ? `${n} update(s) available` : "Everything is up to date");
    } catch (e) {
      toast.push("error", errMessage(e));
    } finally {
      setChecking(false);
    }
  }

  async function endorse(m: InstalledMod) {
    try {
      await api.nexusEndorse(m.mod_id, true, m.version);
      toast.push("success", `Endorsed ${m.name}`);
    } catch (e) {
      toast.push("error", errMessage(e));
    }
  }

  const mods = data ?? [];

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-xl font-semibold">Installed mods</h2>
          <p className="text-xs text-neutral-500">
            Profile "{settings.active_profile}" · {mods.length} mod(s) ·{" "}
            {mods.filter((m) => m.enabled).length} enabled
          </p>
        </div>
        <Button onClick={checkUpdates} disabled={checking || mods.length === 0}>
          {checking ? <Spinner /> : <RefreshCw className="h-4 w-4" />} Check for updates
        </Button>
      </div>

      {loading && (
        <div className="flex items-center gap-2 text-neutral-400">
          <Spinner /> Loading…
        </div>
      )}
      {error && <div className="text-sm text-red-400">{error}</div>}

      {!loading && mods.length === 0 && (
        <div className="rounded-lg border border-dashed border-neutral-700 p-8 text-center text-sm text-neutral-400">
          No mods installed yet. Head to the <b>Online</b> tab, or open a mod on Nexus and click{" "}
          <b>“Mod Manager Download”</b> — it will install here automatically.
        </div>
      )}

      <div className="flex flex-col gap-2">
        {mods.map((m) => {
          const u = updates[m.key];
          return (
            <div
              key={m.key}
              className="flex items-center gap-4 rounded-lg border border-neutral-800 bg-neutral-900/40 p-3"
            >
              <img
                src={m.picture_url ?? ""}
                onError={(e) => ((e.target as HTMLImageElement).style.visibility = "hidden")}
                className="h-12 w-12 flex-shrink-0 rounded object-cover"
                alt=""
              />
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className="truncate font-medium">{m.name}</span>
                  {u?.update_available && (
                    <span className="rounded bg-amber-600/30 px-1.5 py-0.5 text-[10px] text-amber-300">
                      update → {u.latest_version}
                    </span>
                  )}
                </div>
                <div className="truncate text-xs text-neutral-500">
                  v{m.version}
                  {m.author ? ` · ${m.author}` : ""}
                </div>
              </div>

              {m.page_url && (
                <button
                  onClick={() => openUrl(m.page_url!)}
                  className="flex items-center gap-1 text-xs text-neutral-400 hover:text-green-400"
                  title={m.source === "gamebanana" ? "View on GameBanana" : "View on Nexus"}
                >
                  {m.source === "gamebanana" ? "GB" : "Nexus"}
                  <ExternalLink className="h-3 w-3" />
                </button>
              )}
              {m.source === "nexus" && (
                <button
                  onClick={() => endorse(m)}
                  className="flex items-center gap-1 text-xs text-neutral-400 hover:text-amber-400"
                >
                  <Heart className="h-3 w-3" /> Endorse
                </button>
              )}

              <label className="flex cursor-pointer items-center gap-2 text-xs">
                <input
                  type="checkbox"
                  checked={m.enabled}
                  disabled={busy === m.key}
                  onChange={() => toggle(m)}
                  className="h-4 w-4 accent-green-500"
                />
                {m.enabled ? "Enabled" : "Disabled"}
              </label>

              <Button variant="danger" disabled={busy === m.key} onClick={() => remove(m)}>
                {busy === m.key ? <Spinner /> : <Trash2 className="h-4 w-4" />}
              </Button>
            </div>
          );
        })}
      </div>
    </div>
  );
}
