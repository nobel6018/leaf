// 검색 규칙. Mac 앱 src-tauri/src/search.rs와 같다(공백 무시, 소문자, 부분 문자열, 이름과 묶음과 태그는 초성)
// @see ../../docs/search.md

/** 공백을 빼고 소문자로. search.rs compact */
export function compact(s: string): string {
  return s.replace(/\s+/gu, "").toLowerCase();
}

const INITIALS = [..."ㄱㄲㄴㄷㄸㄹㅁㅂㅃㅅㅆㅇㅈㅉㅊㅋㅌㅍㅎ"];
const INITIAL_SET = new Set(INITIALS);

function initialOf(c: string): string | null {
  const code = c.codePointAt(0) ?? 0;
  return code >= 0xac00 && code <= 0xd7a3 ? INITIALS[Math.floor((code - 0xac00) / 588)] : null;
}

function charMatches(hay: string, needle: string): boolean {
  return hay === needle || (INITIAL_SET.has(needle) && initialOf(hay) === needle);
}

/** 초성을 섞은 검색어로 부분 문자열을 찾는다(ㅎㅂㄹ, 한ㅂ). 둘 다 compact를 거친 값. search.rs matches_with_initials */
export function matchesWithInitials(hay: string, needle: string): boolean {
  if (!needle) return false;
  if (hay.includes(needle)) return true;
  const n = [...needle];
  if (!n.some((c) => INITIAL_SET.has(c))) return false;
  const h = [...hay];
  for (let i = 0; i + n.length <= h.length; i++) {
    if (n.every((c, j) => charMatches(h[i + j], c))) return true;
  }
  return false;
}

/** 원문에서 공백을 무시하고 검색어가 나오는 자리(글자 위치, 끝은 포함 안 함). search.rs find_spans */
export function findSpans(text: string, needle: string): Array<[number, number]> {
  const n = [...needle];
  if (!n.length) return [];
  const flat: Array<[number, string]> = [];
  [...text].forEach((c, i) => {
    if (/\s/u.test(c)) return;
    for (const l of c.toLowerCase()) flat.push([i, l]);
  });
  const spans: Array<[number, number]> = [];
  let i = 0;
  while (i + n.length <= flat.length) {
    if (n.every((c, j) => flat[i + j][1] === c)) {
      spans.push([flat[i][0], flat[i + n.length - 1][0] + 1]);
      i += n.length;
    } else i++;
  }
  return spans;
}

const CONTEXT = 36;

/** 첫 자리 앞뒤 36글자 조각. 맞은 곳은 «»로 감싼다. 줄바꿈은 공백으로 */
export function snippet(text: string, spans: Array<[number, number]>): string {
  const chars = [...text].map((c) => (/\s/u.test(c) ? " " : c));
  if (!spans.length) return chars.slice(0, CONTEXT * 2).join("");
  const from = Math.max(0, spans[0][0] - CONTEXT);
  const to = Math.min(chars.length, spans[0][0] + CONTEXT * 2);
  let out = from > 0 ? "…" : "";
  let cursor = from;
  for (const [s, e] of spans.filter(([s]) => s >= from && s < to)) {
    out += chars.slice(cursor, s).join("");
    out += `«${chars.slice(s, Math.min(e, to)).join("")}»`;
    cursor = Math.min(e, to);
  }
  out += chars.slice(cursor, to).join("");
  if (to < chars.length) out += "…";
  return out.replace(/ {2,}/g, " ");
}
