export interface Settings {
  game_path: string | null;
  game_source: string | null;
  steam_appid: string | null;
  nexus_api_key: string | null;
  is_premium: boolean;
  nexus_user: string | null;
  active_profile: string;
  linux_launch: string | null;
  setup_complete: boolean;
  temp_email: string | null;
  temp_username: string | null;
  temp_password: string | null;
}

export interface DetectedGame {
  path: string;
  source: string;
  steam_appid: string | null;
  version: string | null;
}

export interface BepInExStatus {
  installed: boolean;
  version: string | null;
  enabled: boolean;
  needs_proton_setup: boolean;
  proton_launch_option: string | null;
}

export interface ValidateResult {
  valid: boolean;
  user_id: number | null;
  name: string | null;
  is_premium: boolean;
  email: string | null;
}

export interface InstalledMod {
  key: string;
  source: string;
  mod_id: number;
  file_id: number;
  name: string;
  version: string;
  author: string | null;
  picture_url: string | null;
  page_url: string | null;
  enabled: boolean;
}

export interface GbMod {
  mod_id: number;
  name: string;
  author: string | null;
  image_url: string | null;
  page_url: string | null;
  version: string | null;
  summary: string | null;
  likes: number;
  ts_modified: number;
  has_files: boolean;
}

export interface GbFile {
  file_id: number;
  filename: string;
  download_url: string;
  size: number;
  version: string | null;
  description: string | null;
  ts_added: number;
  av_clean: boolean;
}

/** Nexus's `updated.json` only returns these lightweight stubs, not full mod info. */
export interface UpdatedModRef {
  mod_id: number;
  latest_file_update: number;
  latest_mod_activity: number;
}

export interface UpdateInfo {
  key: string;
  mod_id: number;
  current_file_id: number;
  current_version: string;
  latest_file_id: number | null;
  latest_version: string | null;
  update_available: boolean;
}

/** Shape of a mod object from the Nexus browse/details endpoints. */
export interface NexusMod {
  mod_id: number;
  name: string;
  summary?: string;
  version?: string;
  author?: string;
  picture_url?: string;
  endorsement_count?: number;
  updated_timestamp?: number;
  available?: boolean;
}

export interface TempAccount {
  email: string;
  username: string;
  password: string;
}

export interface VerificationResult {
  found: boolean;
  link: string | null;
  subject: string | null;
  code: string | null;
}

export interface CodeModRef {
  source: string;
  mod_id: number;
  file_id: number;
  name: string;
}

export interface ImportCodeStart {
  profile: string;
  mods: CodeModRef[];
}
