// 문서 태그 모아 보기. `/`로 계층을 만든 태그(분야/냉각, 책/경제)를 폴더 트리처럼 보여 준다.
// 정리 규칙은 Rust(crates/leaf-core/src/tags.rs)와 같다
// @see docs/organize.md

export interface TagNode {
  /** 전체 경로(분야/냉각) */
  path: string;
  /** 마지막 단계 이름(냉각) */
  name: string;
  /** 이 태그나 하위 태그가 붙은 문서 수 */
  count: number;
  children: TagNode[];
}

/** 태그 없는 문서만 보는 필터 값 */
export const UNTAGGED = "\u0000untagged";

/** 문서 하나에 붙일 수 있는 태그 수(leaf-core MAX_TAGS) */
export const MAX_TAGS = 10;

/** 태그 하나를 정리한다. 앞뒤 공백, `/` 앞뒤 공백, 빈 단계, `#` 머리를 없앤다 */
export function normalizeTag(raw: string): string | null {
  const parts = raw
    .trim()
    .replace(/^#+/, "")
    .split("/")
    .map((p) => p.split(/\s+/).filter(Boolean).join(" "))
    .filter(Boolean);
  if (!parts.length) return null;
  return [...parts.join("/")].slice(0, 60).join("").replace(/\/+$/, "");
}

/** 태그 목록을 정리한다. 순서는 지키고 같은 태그(대소문자 무시)는 처음 것만 남긴다 */
export function normalizeTags(raw: string[]): string[] {
  const out: string[] = [];
  for (const t of raw) {
    const tag = normalizeTag(t);
    if (tag && !out.some((o) => o.toLowerCase() === tag.toLowerCase())) out.push(tag);
  }
  return out.slice(0, MAX_TAGS);
}

/** 이 문서가 필터(태그 경로)에 드는지. 하위 태그가 붙은 문서도 든다(leaf-core tags::matches) */
export function matchesTag(tags: string[] | undefined, filter: string | null): boolean {
  if (!filter) return true;
  if (filter === UNTAGGED) return !tags?.length;
  return (tags ?? []).some((t) => t === filter || t.startsWith(filter + "/"));
}

/** 문서들의 태그로 트리를 만든다. 형제는 문서 수가 많은 순, 같으면 이름순 */
export function buildTagTree(docs: { tags?: string[] }[]): TagNode[] {
  const root: TagNode = { path: "", name: "", count: 0, children: [] };
  for (const d of docs) {
    // 한 문서가 같은 상위 태그를 여러 번 세지 않도록 경로를 모아 한 번씩 센다
    const paths = new Set<string>();
    for (const tag of d.tags ?? []) {
      const parts = tag.split("/");
      parts.forEach((_, i) => paths.add(parts.slice(0, i + 1).join("/")));
    }
    for (const path of paths) {
      let node = root;
      for (const name of path.split("/")) {
        const childPath = node.path ? `${node.path}/${name}` : name;
        let child = node.children.find((c) => c.path === childPath);
        if (!child) {
          child = { path: childPath, name, count: 0, children: [] };
          node.children.push(child);
        }
        node = child;
      }
      node.count += 1;
    }
  }
  const sort = (nodes: TagNode[]) => {
    nodes.sort((a, b) => b.count - a.count || a.name.localeCompare(b.name, "ko"));
    nodes.forEach((n) => sort(n.children));
  };
  sort(root.children);
  return liftUniversal(root.children, docs.length);
}

/** 모든 문서에 붙은 상위 태그는 걸러 내는 힘이 없어 건너뛰고 하위 태그를 바로 보인다.
 * 경로(path)는 그대로라 누르면 원래 태그로 거른다 */
function liftUniversal(nodes: TagNode[], total: number): TagNode[] {
  if (total < 2) return nodes;
  const out: TagNode[] = [];
  for (const n of nodes) {
    if (n.count === total && n.children.length > 0) out.push(...liftUniversal(n.children, total));
    else out.push(n);
  }
  out.sort((a, b) => b.count - a.count || a.name.localeCompare(b.name, "ko"));
  return out;
}

/** 쓰인 태그와 그 상위 태그 경로 전부. 이름순. 질문 범위 고르기에 쓴다 */
export function allTagPaths(docs: { tags?: string[] }[]): string[] {
  const paths = new Set<string>();
  for (const d of docs)
    for (const tag of d.tags ?? []) {
      const parts = tag.split("/");
      parts.forEach((_, i) => paths.add(parts.slice(0, i + 1).join("/")));
    }
  return [...paths].sort((a, b) => a.localeCompare(b, "ko"));
}

/** 쓰인 태그와 문서 수. 자동완성 후보로 쓴다. 많이 쓴 순 */
export function tagCounts(docs: { tags?: string[] }[]): [string, number][] {
  const counts = new Map<string, number>();
  for (const d of docs) for (const t of d.tags ?? []) counts.set(t, (counts.get(t) ?? 0) + 1);
  return [...counts].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0], "ko"));
}

const INITIALS = "ㄱㄲㄴㄷㄸㄹㅁㅂㅃㅅㅆㅇㅈㅉㅊㅋㅌㅍㅎ";

function initialOf(c: string): string | null {
  const code = c.charCodeAt(0);
  return code >= 0xac00 && code <= 0xd7a3 ? INITIALS[Math.floor((code - 0xac00) / 588)] : null;
}

/** 초성을 섞은 검색어로 부분 문자열을 찾는다(ㄴㄱ, 냉ㄱ). 대소문자와 띄어쓰기는 무시한다 */
export function matchesQuery(text: string, query: string): boolean {
  const hay = [...text.toLowerCase().replace(/\s+/g, "")];
  const needle = [...query.toLowerCase().replace(/\s+/g, "")];
  if (!needle.length) return true;
  for (let i = 0; i + needle.length <= hay.length; i++) {
    if (needle.every((n, j) => hay[i + j] === n || (INITIALS.includes(n) && initialOf(hay[i + j]) === n))) return true;
  }
  return false;
}

/** 태그 입력 자동완성 후보. 이미 붙은 태그는 빼고 많이 쓴 순으로 */
export function suggestTags(all: [string, number][], query: string, current: string[], limit = 8): string[] {
  return all
    .map(([t]) => t)
    .filter((t) => !current.includes(t) && matchesQuery(t, query))
    .slice(0, limit);
}
