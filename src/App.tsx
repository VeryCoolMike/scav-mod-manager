import { createContext, useCallback, useContext, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { check as checkForUpdate, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import {
  Blocks,
  Download,
  Globe,
  FolderKanban,
  Play,
  Settings as SettingsIcon,
  Star,
  type LucideIcon,
} from "lucide-react";
import { api, errMessage } from "./lib/api";
import type { Settings } from "./lib/types";
import { Button, Spinner, ToastProvider, useToast } from "./lib/ui";
import Setup from "./routes/Setup";
import ModsView from "./routes/Installed";
import Online from "./routes/Online";
import Profiles from "./routes/Profiles";
import SettingsView from "./routes/Settings";

type Tab = "mods" | "online" | "profiles" | "settings";

interface AppCtx {
  settings: Settings;
  reloadSettings: () => Promise<Settings>;
  profiles: string[];
  reloadProfiles: () => Promise<void>;
  modsVersion: number;
  bumpMods: () => void;
  go: (tab: Tab) => void;
}

const Ctx = createContext<AppCtx>(null as unknown as AppCtx);
export function useApp() {
  return useContext(Ctx);
}

function Shell() {
  const toast = useToast();
  const [settings, setSettings] = useState<Settings | null>(null);
  const [profiles, setProfiles] = useState<string[]>([]);
  const [tab, setTab] = useState<Tab>("mods");
  const [modsVersion, setModsVersion] = useState(0);
  const [booting, setBooting] = useState(true);
  const [busyLaunch, setBusyLaunch] = useState(false);
  const [available, setAvailable] = useState<Update | null>(null);
  const [updating, setUpdating] = useState(false);

  const reloadSettings = useCallback(async () => {
    const s = await api.getSettings();
    setSettings(s);
    return s;
  }, []);

  const reloadProfiles = useCallback(async () => {
    setProfiles(await api.listProfiles());
  }, []);

  const bumpMods = useCallback(() => setModsVersion((v) => v + 1), []);

  useEffect(() => {
    (async () => {
      try {
        await reloadSettings();
        await reloadProfiles();
      } catch (e) {
        toast.push("error", errMessage(e));
      } finally {
        setBooting(false);
      }
    })();
  }, [reloadSettings, reloadProfiles, toast]);

  // Listen for nxm:// deep-link install events fired by the backend.
  useEffect(() => {
    const unlisten = Promise.all([
      listen<string>("nxm://start", () => toast.push("info", "Download started from Nexus…")),
      listen<any>("nxm://done", (e) => {
        toast.push("success", `Installed "${e.payload?.name ?? "mod"}"`);
        bumpMods();
      }),
      listen<string>("nxm://error", (e) => toast.push("error", `Install failed: ${e.payload}`)),
    ]);
    return () => {
      unlisten.then((fns) => fns.forEach((f) => f()));
    };
  }, [toast, bumpMods]);

  // Check for an app update once on startup. Silent on failure (e.g. offline)
  // — this shouldn't nag the user, just quietly skip if the check fails.
  useEffect(() => {
    checkForUpdate()
      .then((u) => {
        if (u) setAvailable(u);
      })
      .catch(() => {});
  }, []);

  async function installUpdate() {
    if (!available) return;
    setUpdating(true);
    try {
      await available.downloadAndInstall();
      await relaunch();
    } catch (e) {
      toast.push("error", errMessage(e));
      setUpdating(false);
    }
  }

  if (booting || !settings) {
    return (
      <div className="flex h-full items-center justify-center gap-3 text-neutral-400">
        <Spinner /> Loading…
      </div>
    );
  }

  const needsSetup = !settings.game_path || !settings.setup_complete;

  const ctx: AppCtx = {
    settings,
    reloadSettings,
    profiles,
    reloadProfiles,
    modsVersion,
    bumpMods,
    go: setTab,
  };

  async function launch(modded: boolean) {
    setBusyLaunch(true);
    try {
      await api.launchGame(modded);
      toast.push("success", modded ? "Launching (modded)…" : "Launching vanilla…");
    } catch (e) {
      toast.push("error", errMessage(e));
    } finally {
      setBusyLaunch(false);
    }
  }

  if (needsSetup) {
    return (
      <Ctx.Provider value={ctx}>
        <Setup />
      </Ctx.Provider>
    );
  }

  const tabs: { id: Tab; label: string; icon: LucideIcon }[] = [
    { id: "mods", label: "Installed", icon: Blocks },
    { id: "online", label: "Online", icon: Globe },
    { id: "profiles", label: "Profiles", icon: FolderKanban },
    { id: "settings", label: "Settings", icon: SettingsIcon },
  ];

  return (
    <Ctx.Provider value={ctx}>
      <div className="flex h-full">
        {/* Sidebar */}
        <aside className="flex w-56 flex-col border-r border-neutral-800 bg-neutral-950/60">
          <div className="px-4 py-4">
            <div className="text-lg font-bold text-green-400">Scav Mods</div>
            <div className="text-xs text-neutral-500">Casualties: Unknown</div>
          </div>
          <nav className="flex flex-1 flex-col gap-1 px-2">
            {tabs.map((t) => (
              <button
                key={t.id}
                onClick={() => setTab(t.id)}
                className={`flex items-center gap-3 rounded-md px-3 py-2 text-left text-sm transition ${
                  tab === t.id
                    ? "bg-green-600/20 text-green-300"
                    : "text-neutral-300 hover:bg-neutral-800"
                }`}
              >
                <t.icon className="h-4 w-4" />
                {t.label}
              </button>
            ))}
          </nav>
          <div className="border-t border-neutral-800 p-3 text-xs text-neutral-500">
            <div className="mb-1 truncate">
              Profile: <span className="text-neutral-300">{settings.active_profile}</span>
            </div>
            <div className="truncate">
              {settings.nexus_user ? (
                <>
                  Nexus: <span className="text-neutral-300">{settings.nexus_user}</span>
                  {settings.is_premium && (
                    <Star className="ml-1 inline-block h-3 w-3 fill-amber-400 text-amber-400" />
                  )}
                </>
              ) : (
                <span className="text-neutral-500">Nexus: not signed in</span>
              )}
            </div>
          </div>
        </aside>

        {/* Main */}
        <main className="flex flex-1 flex-col overflow-hidden">
          {available && (
            <div className="flex items-center justify-between gap-3 border-b border-green-800 bg-green-950/30 px-6 py-2 text-sm">
              <span>
                A new version (<span className="text-green-300">{available.version}</span>) is
                available.
              </span>
              <div className="flex gap-2">
                <Button variant="primary" disabled={updating} onClick={installUpdate}>
                  {updating ? <Spinner /> : <Download className="h-4 w-4" />} Update & Restart
                </Button>
                <Button variant="ghost" disabled={updating} onClick={() => setAvailable(null)}>
                  Later
                </Button>
              </div>
            </div>
          )}
          <header className="flex items-center justify-between border-b border-neutral-800 px-6 py-3">
            <div className="text-sm text-neutral-400 capitalize">{tab}</div>
            <div className="flex gap-2">
              <Button variant="ghost" disabled={busyLaunch} onClick={() => launch(false)}>
                Launch Vanilla
              </Button>
              <Button variant="primary" disabled={busyLaunch} onClick={() => launch(true)}>
                {busyLaunch ? <Spinner /> : <Play className="h-4 w-4" />} Launch Modded
              </Button>
            </div>
          </header>
          <div className="flex-1 overflow-y-auto p-6">
            {tab === "mods" && <ModsView />}
            {tab === "online" && <Online />}
            {tab === "profiles" && <Profiles />}
            {tab === "settings" && <SettingsView />}
          </div>
        </main>
      </div>
    </Ctx.Provider>
  );
}

export default function App() {
  return (
    <ToastProvider>
      <Shell />
    </ToastProvider>
  );
}
