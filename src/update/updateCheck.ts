import { getVersion } from "@tauri-apps/api/app";

// latest.json을 못 읽었을 때(새 버전을 올리는 중, 네트워크) GitHub API로 최신 Release를 다시 확인한다.
// Release 다운로드 주소는 CORS 헤더가 없어 웹뷰에서 못 읽으므로, CORS를 허용하는 GitHub API를 쓴다(로그인 없이 IP당 시간당 60회).
// @see docs/update.md

const LATEST_RELEASE = "https://api.github.com/repos/nobel6018/leaf/releases/latest";

export interface UpdateInfo {
  current: string;
  latest: string;
  /** Release 페이지 */
  url: string;
}

/**
 * 이 플랫폼 파일이 붙은 더 새 버전이 있으면 알려 준다. 없거나 확인하지 못하면 null.
 * Release는 플랫폼마다 빌드가 끝나는 대로 파일이 붙으므로, 이 플랫폼 파일이 아직 없으면 알리지 않는다
 */
export async function checkForUpdate(asset: RegExp): Promise<UpdateInfo | null> {
  const r = await checkForUpdateDetailed(asset);
  return typeof r === "object" ? r : null;
}

/** 새 버전이 있으면 그 정보, 없으면 "none", GitHub에 묻지 못했으면 "error" */
export async function checkForUpdateDetailed(asset: RegExp): Promise<UpdateInfo | "none" | "error"> {
  try {
    const [current, res] = await Promise.all([getVersion(), fetch(LATEST_RELEASE, { headers: { Accept: "application/vnd.github+json" } })]);
    if (!res.ok) return "error";
    const release: { tag_name: string; html_url: string; assets: { name: string }[] } = await res.json();
    const latest = release.tag_name.replace(/^v/, "");
    if (!isNewer(latest, current)) return "none";
    if (!release.assets.some((a) => asset.test(a.name))) return "none";
    return { current, latest, url: release.html_url };
  } catch (e) {
    console.warn(`업데이트 확인 실패: ${e}`);
    return "error";
  }
}

/** 점으로 나눈 숫자를 앞에서부터 비교한다(0.10.0 > 0.9.1) */
export function isNewer(latest: string, current: string): boolean {
  const a = latest.split(".").map((n) => parseInt(n, 10) || 0);
  const b = current.split(".").map((n) => parseInt(n, 10) || 0);
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const diff = (a[i] ?? 0) - (b[i] ?? 0);
    if (diff !== 0) return diff > 0;
  }
  return false;
}
