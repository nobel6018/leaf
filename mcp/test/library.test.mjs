import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { fitJpeg, jpegSize } from "../dist/image.js";
import {
  applyEdits,
  findDocument,
  insideDir,
  listDocuments,
  normalizeTag,
  normalizeTags,
  orderOutputs,
  tagMatches,
} from "../dist/library.js";
import { compact, findSpans, matchesWithInitials, snippet } from "../dist/search.js";
import { parseRange } from "../dist/server.js";
import { buildLibrary, fixtureManifest, IDS, PAGE_IDS, solidJpeg } from "./library-fixture.mjs";

describe("edits 적용 (crates/leaf-core/tests/edits.rs와 같은 경우)", () => {
  test("맥에서 고친 이름과 꼭짓점이 우선한다", () => {
    const original = fixtureManifest("manifest-v1.json");
    const quad = { topLeft: { x: 0.1, y: 0.1 }, topRight: { x: 0.9, y: 0.1 }, bottomRight: { x: 0.9, y: 0.9 }, bottomLeft: { x: 0.1, y: 0.9 } };
    const merged = applyEdits(original, { name: "고친 이름", quads: { [original.pages[1].id]: quad } });
    assert.equal(merged.name, "고친 이름");
    assert.deepEqual(merged.pages[1].quad, quad);
    assert.equal(original.name, "2026 예시박람회_예시로보틱스", "원본 manifest는 바뀌지 않는다");
  });

  test("순서에 없는 새 페이지는 뒤에 붙는다", () => {
    const original = fixtureManifest("manifest-v1.json");
    const [first, second] = original.pages.map((p) => p.id);
    original.pages.push({ ...original.pages[0], id: "99999999-0000-4000-8000-000000000000" });
    const merged = applyEdits(original, { pageOrder: [second, first] });
    assert.deepEqual(
      merged.pages.map((p) => p.id),
      [second, first, "99999999-0000-4000-8000-000000000000"],
    );
  });

  test("지운 페이지는 목록에서 빠진다", () => {
    const original = fixtureManifest("manifest-v1.json");
    assert.equal(applyEdits(original, { deletedPages: [original.pages[0].id] }).pages.length, 1);
  });

  test("종류, 묶음, 펼침 방식을 덮고 null은 무시한다", () => {
    const original = fixtureManifest("manifest-v1.json");
    const merged = applyEdits(original, { kind: "book", session: "다른 묶음", layouts: { [original.pages[0].id]: "trifold" } });
    assert.equal(merged.kind, "book");
    assert.equal(merged.session, "다른 묶음");
    assert.equal(merged.pages[0].layout, "trifold");
    assert.equal(applyEdits(original, { name: null, session: null }).session, "2026 예시박람회");
  });
});

describe("출력 페이지 순서", () => {
  test("원본 순서를 따르고 한 원본 안에서는 왼쪽부터", () => {
    const m = fixtureManifest("manifest-v1-trifold.json");
    const p = m.pages[0].id;
    const outputs = [{ panel: 2 }, { panel: 0 }, { panel: 1 }].map((part) => ({ id: `${p}-P${part.panel + 1}`, sourcePage: p, part }));
    assert.deepEqual(
      orderOutputs(m, { outputs }).map((r) => [r.number, r.output.id]),
      [
        [1, `${p}-P1`],
        [2, `${p}-P2`],
        [3, `${p}-P3`],
      ],
    );
  });
});

describe("태그 (crates/leaf-core/src/tags.rs와 같은 규칙)", () => {
  test("하위 태그도 상위 필터에 든다", () => {
    const tags = ["업무/영업/한빛"];
    assert.ok(tagMatches(tags, "업무"));
    assert.ok(tagMatches(tags, "업무/영업"));
    assert.ok(!tagMatches(tags, "업무/영"));
  });

  test("필터도 정리하고 대소문자는 무시한다", () => {
    assert.ok(tagMatches(["Work/A"], " #work / a "));
  });

  test("공백과 빈 단계와 샵을 정리한다", () => {
    assert.equal(normalizeTag("  #업무 / 영업//한빛  로보틱스/ "), "업무/영업/한빛 로보틱스");
    assert.equal(normalizeTag(" / "), null);
  });

  test("같은 태그는 처음 것만 남기고 10개까지", () => {
    assert.deepEqual(normalizeTags(["회의", "Work/A", "work/a", "", "개인"]), ["회의", "Work/A", "개인"]);
    assert.equal(normalizeTags(Array.from({ length: 15 }, (_, i) => `t${i}`)).length, 10);
  });
});

describe("검색 규칙 (src-tauri/src/search.rs와 같은 규칙)", () => {
  test("공백과 대소문자를 무시한다", () => {
    assert.equal(compact(" Data 센터\n냉각 "), "data센터냉각");
  });

  test("초성과 글자를 섞어 찾는다", () => {
    assert.ok(matchesWithInitials(compact("2026 예시박람회_한빛로보틱스"), "ㅎㅂㄹ"));
    assert.ok(matchesWithInitials("한빛로보틱스", "한ㅂ"));
    assert.ok(!matchesWithInitials("한빛로보틱스", "ㄱㄴ"));
    assert.ok(!matchesWithInitials("한빛", ""));
  });

  test("원문에서 공백을 건너뛰며 자리를 찾고 조각을 만든다", () => {
    const text = "1장 데이터 센터의 냉각";
    const spans = findSpans(text, compact("데이터센터"));
    assert.deepEqual(spans, [[3, 9]]);
    assert.equal(snippet(text, spans), "1장 «데이터 센터»의 냉각");
  });
});

describe("보관함 읽기", () => {
  test("읽을 수 없는 폴더는 건너뛰고 최신순으로 놓는다", async () => {
    const ids = (await listDocuments(buildLibrary())).map((d) => d.manifest.id);
    // 두 팜플렛 예시는 만든 시각이 같다(2026-11-05), 그다음 메모(11-04), 책(2025)
    assert.deepEqual(ids.slice(0, 2).sort(), [IDS.trifold, IDS.pamphlet].sort());
    assert.deepEqual(ids.slice(2), [IDS.memo, IDS.book]);
  });

  test("처리 상태와 쪽 순서", async () => {
    const docs = await listDocuments(buildLibrary());
    const by = (id) => docs.find((d) => d.manifest.id === id);
    assert.equal(by(IDS.memo).state, "unprocessed");
    assert.equal(by(IDS.book).state, "stale", "지운 원본의 출력이 남아 있다");
    assert.deepEqual(
      by(IDS.book).pages.map((r) => r.output.id),
      [PAGE_IDS.B[0], PAGE_IDS.B[2]],
    );
    const pamphlet = by(IDS.pamphlet);
    assert.equal(pamphlet.state, "processed");
    assert.deepEqual(
      pamphlet.pages.map((r) => r.output.id),
      [`${PAGE_IDS.P2}-L`, `${PAGE_IDS.P2}-R`, PAGE_IDS.P1],
    );
  });

  test("id 앞부분으로 찾되 짧거나 겹치면 정하지 않는다", async () => {
    const docs = await listDocuments(buildLibrary());
    assert.equal(findDocument(docs, IDS.pamphlet.slice(0, 8)).manifest.id, IDS.pamphlet);
    assert.equal(findDocument(docs, IDS.pamphlet.toUpperCase()).manifest.id, IDS.pamphlet);
    assert.equal(findDocument(docs, "7d1c"), null);
    assert.equal(findDocument(docs, "../../etc"), null);
  });

  test("문서 폴더 밖 경로는 거부한다", () => {
    assert.equal(insideDir("/lib/doc", "pages/a.jpg"), "/lib/doc/pages/a.jpg");
    assert.equal(insideDir("/lib/doc", "../other/pages/a.jpg"), null);
    assert.equal(insideDir("/lib/doc", "/etc/passwd"), null);
    assert.equal(insideDir("/lib/doc", "pages/./a.jpg"), null);
  });
});

describe("쪽 범위", () => {
  test("여러 꼴을 읽는다", () => {
    assert.deepEqual(parseRange(undefined, 10), [1, 10]);
    assert.deepEqual(parseRange("3", 10), [3, 3]);
    assert.deepEqual(parseRange("3-5", 10), [3, 5]);
    assert.deepEqual(parseRange("3~", 10), [3, 10]);
    assert.deepEqual(parseRange("-4", 10), [1, 4]);
    assert.deepEqual(parseRange("8-30", 10), [8, 10]);
    assert.equal(parseRange("5-3", 10), null);
    assert.equal(parseRange("abc", 10), null);
    assert.equal(parseRange("-", 10), null);
  });
});

describe("페이지 그림 줄이기", () => {
  test("긴 변을 한도로 줄이고 비율을 지킨다", () => {
    const fitted = fitJpeg(solidJpeg(2000, 1000), 1568);
    assert.equal(fitted.resized, true);
    assert.deepEqual([fitted.width, fitted.height], [1568, 784]);
    assert.deepEqual(jpegSize(fitted.data), { width: 1568, height: 784 });
  });

  test("이미 작으면 다시 인코딩하지 않는다", () => {
    const small = solidJpeg(400, 300);
    const fitted = fitJpeg(small, 1568);
    assert.equal(fitted.resized, false);
    assert.equal(fitted.data, small);
  });
});
