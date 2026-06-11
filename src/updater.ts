// 升级检测与安装（UX 参考 auto-desktop）：
// 1) 首选 tauri-plugin-updater：latest.json + 签名校验 + 下载安装 + 自动重启
// 2) 插件不可用（如本地构建未注入公钥）时回退后端 check_for_update（GitHub API），
//    引导用户打开下载页手动升级
import { invoke } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";

const RELEASES_URL = "https://github.com/sealmail/seal-mail/releases";

function errText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}

export type UpdateInfo = {
  currentVersion: string;
  latestVersion: string;
  available: boolean;
  installed?: boolean;
  /** true = 自动升级不可用，只能打开下载页手动升级 */
  manual?: boolean;
  autoError?: string;
  releaseUrl: string;
  downloadUrl?: string | null;
};

export type UpdateProgress = {
  phase: "downloading" | "installing";
  downloaded: number;
  total?: number;
};

export type UpdateBarState =
  | { indeterminate: true; percent: null }
  | { indeterminate: false; percent: number };

export function updateBarState(progress: UpdateProgress): UpdateBarState {
  if (progress.phase === "installing") {
    return { indeterminate: false, percent: 100 };
  }
  if (progress.total && progress.total > 0) {
    return {
      indeterminate: false,
      percent: Math.min(100, Math.round((progress.downloaded / progress.total) * 100)),
    };
  }
  return { indeterminate: true, percent: null };
}

const upToDate = (): UpdateInfo => ({
  currentVersion: __APP_VERSION__,
  latestVersion: __APP_VERSION__,
  available: false,
  releaseUrl: RELEASES_URL,
  downloadUrl: null,
});

export async function checkForUpdate(): Promise<UpdateInfo> {
  try {
    const update = await check();
    if (!update) return upToDate();
    return {
      currentVersion: __APP_VERSION__,
      latestVersion: update.version,
      available: true,
      releaseUrl: `${RELEASES_URL}/latest`,
      downloadUrl: null,
    };
  } catch (e) {
    const fallback = await invoke<UpdateInfo>("check_for_update");
    return fallback.available ? { ...fallback, manual: true, autoError: errText(e) } : fallback;
  }
}

export async function installUpdate(
  info: UpdateInfo | null,
  onProgress?: (progress: UpdateProgress) => void
): Promise<UpdateInfo> {
  if (info?.manual) return info;

  try {
    const update = await check();
    if (!update) return upToDate();
    let downloaded = 0;
    let total: number | undefined;
    await update.downloadAndInstall((event) => {
      if (event.event === "Started") {
        downloaded = 0;
        total = event.data.contentLength;
        onProgress?.({ phase: "downloading", downloaded, total });
        return;
      }
      if (event.event === "Progress") {
        downloaded += event.data.chunkLength;
        onProgress?.({ phase: "downloading", downloaded, total });
        return;
      }
      onProgress?.({ phase: "installing", downloaded, total });
    });
    await relaunch();
    return {
      currentVersion: __APP_VERSION__,
      latestVersion: update.version,
      available: true,
      installed: true,
      releaseUrl: `${RELEASES_URL}/latest`,
      downloadUrl: null,
    };
  } catch (e) {
    const fallback = await invoke<UpdateInfo>("check_for_update");
    return fallback.available ? { ...fallback, manual: true, autoError: errText(e) } : fallback;
  }
}
