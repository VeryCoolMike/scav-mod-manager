import { invoke } from "@tauri-apps/api/core";
import type {
  BepInExStatus,
  DetectedGame,
  GbFile,
  GbMod,
  ImportCodeStart,
  InstalledMod,
  NexusMod,
  Settings,
  TempAccount,
  UpdatedModRef,
  UpdateInfo,
  ValidateResult,
  VerificationResult,
} from "./types";

export const api = {
  // settings & game
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  detectGames: () => invoke<DetectedGame[]>("detect_games"),
  validateGamePath: (dir: string) => invoke<string>("validate_game_path", { dir }),
  setGamePath: (dir: string, source?: string, steamAppid?: string | null) =>
    invoke<Settings>("set_game_path", { dir, source, steamAppid }),

  // bepinex
  bepinexStatus: () => invoke<BepInExStatus>("bepinex_status"),
  bepinexInstall: () => invoke<BepInExStatus>("bepinex_install"),
  bepinexUninstall: () => invoke<BepInExStatus>("bepinex_uninstall"),

  // nexus
  nexusSsoLogin: () => invoke<ValidateResult>("nexus_sso_login"),
  nexusLogout: () => invoke<Settings>("nexus_logout"),
  nexusValidate: (key: string) => invoke<ValidateResult>("nexus_validate", { key }),
  nexusBrowse: (list: string) => invoke<NexusMod[]>("nexus_browse", { list }),
  nexusUpdated: (period: string) => invoke<UpdatedModRef[]>("nexus_updated", { period }),
  nexusModDetails: (modId: number) => invoke<NexusMod>("nexus_mod_details", { modId }),
  nexusModFiles: (modId: number) => invoke<any>("nexus_mod_files", { modId }),
  nexusEndorse: (modId: number, endorse: boolean, version: string) =>
    invoke<any>("nexus_endorse", { modId, endorse, version }),
  nexusTracked: () => invoke<any>("nexus_tracked"),

  // gamebanana (account-free source)
  gbBrowse: (sort: string, page: number) => invoke<GbMod[]>("gb_browse", { sort, page }),
  gbSearch: (query: string, page: number) => invoke<GbMod[]>("gb_search", { query, page }),
  gbModFiles: (modId: number) => invoke<GbFile[]>("gb_mod_files", { modId }),
  gbInstall: (modId: number, fileId: number) =>
    invoke<InstalledMod>("gb_install", { modId, fileId }),

  // install (nexus, optional)
  installNxm: (link: string) => invoke<InstalledMod>("install_nxm", { link }),
  installModFile: (modId: number, fileId: number) =>
    invoke<InstalledMod>("install_mod_file", { modId, fileId }),

  // installed mods
  listInstalled: () => invoke<InstalledMod[]>("list_installed"),
  setModEnabled: (key: string, enabled: boolean) =>
    invoke<void>("set_mod_enabled", { key, enabled }),
  uninstallMod: (key: string) => invoke<void>("uninstall_mod", { key }),
  syncMods: () => invoke<number>("sync_mods"),

  // temporary account — fully automated
  autoCreateAccount: () => invoke<TempAccount>("auto_create_account"),
  autoPollVerification: (email: string, timeoutSecs: number) =>
    invoke<VerificationResult>("auto_poll_verification", { email, timeoutSecs }),
  autoFullRegister: () => invoke<ValidateResult>("auto_full_register"),
  nexusAutoDownload: (modId: number, fileId: number) =>
    invoke<InstalledMod>("nexus_auto_download", { modId, fileId }),
  listProfiles: () => invoke<string[]>("list_profiles"),
  createProfile: (name: string) => invoke<string>("create_profile", { name }),
  deleteProfile: (name: string) => invoke<string[]>("delete_profile", { name }),
  cloneProfile: (from: string, to: string) => invoke<string>("clone_profile", { from, to }),
  switchProfile: (name: string) => invoke<Settings>("switch_profile", { name }),
  exportProfileBundle: (name: string, dest: string) =>
    invoke<void>("export_profile_bundle", { name, dest }),
  importProfileBundle: (zipPath: string, newName?: string) =>
    invoke<string>("import_profile_bundle", { zipPath, newName }),
  exportProfileCode: (name: string) => invoke<string>("export_profile_code", { name }),
  importProfileCode: (code: string, newName?: string) =>
    invoke<ImportCodeStart>("import_profile_code", { code, newName }),

  // updates & launch
  checkUpdates: () => invoke<UpdateInfo[]>("check_updates"),
  launchGame: (modded: boolean) => invoke<void>("launch_game", { modded }),
};

/** Extract a readable message from a rejected invoke. */
export function errMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
