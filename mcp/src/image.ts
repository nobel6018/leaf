import jpeg from "jpeg-js";

// 처리된 페이지(pages/<출력 id>.jpg)를 Claude가 보기 좋은 크기로 줄인다.
// 순수 JS(jpeg-js)라 네이티브 모듈 없이 어디서나 돈다. 긴 변이 이미 작으면 다시 인코딩하지 않는다

/** JPEG 머리(SOF)에서 크기만 읽는다. 못 읽으면 null */
export function jpegSize(bytes: Uint8Array): { width: number; height: number } | null {
  if (bytes[0] !== 0xff || bytes[1] !== 0xd8) return null;
  let i = 2;
  while (i + 9 < bytes.length) {
    if (bytes[i] !== 0xff) {
      i++;
      continue;
    }
    const marker = bytes[i + 1];
    // SOF0~SOF15 중 DHT(C4), JPG(C8), DAC(CC)를 뺀 것
    if (marker >= 0xc0 && marker <= 0xcf && marker !== 0xc4 && marker !== 0xc8 && marker !== 0xcc) {
      return { height: (bytes[i + 5] << 8) | bytes[i + 6], width: (bytes[i + 7] << 8) | bytes[i + 8] };
    }
    if (marker === 0xd8 || marker === 0x01 || (marker >= 0xd0 && marker <= 0xd7)) {
      i += 2;
      continue;
    }
    i += 2 + ((bytes[i + 2] << 8) | bytes[i + 3]);
  }
  return null;
}

/** 상자 평균으로 줄인다. 글자가 많은 스캔이라 최근접보다 덜 깨진다 */
function downscale(src: Uint8Array, w: number, h: number, nw: number, nh: number): Uint8Array {
  const out = new Uint8Array(nw * nh * 4);
  const sx = w / nw;
  const sy = h / nh;
  for (let y = 0; y < nh; y++) {
    const y0 = Math.floor(y * sy);
    const y1 = Math.max(y0 + 1, Math.min(h, Math.floor((y + 1) * sy)));
    for (let x = 0; x < nw; x++) {
      const x0 = Math.floor(x * sx);
      const x1 = Math.max(x0 + 1, Math.min(w, Math.floor((x + 1) * sx)));
      let r = 0;
      let g = 0;
      let b = 0;
      for (let yy = y0; yy < y1; yy++) {
        let p = (yy * w + x0) * 4;
        for (let xx = x0; xx < x1; xx++, p += 4) {
          r += src[p];
          g += src[p + 1];
          b += src[p + 2];
        }
      }
      const n = (y1 - y0) * (x1 - x0);
      const o = (y * nw + x) * 4;
      out[o] = r / n;
      out[o + 1] = g / n;
      out[o + 2] = b / n;
      out[o + 3] = 255;
    }
  }
  return out;
}

export interface Fitted {
  data: Buffer;
  width: number;
  height: number;
  /** 원본 크기 */
  original: { width: number; height: number };
  resized: boolean;
}

/** 긴 변이 maxEdge를 넘으면 줄여서 JPEG로 다시 인코딩한다 */
export function fitJpeg(bytes: Buffer, maxEdge: number, quality = 85): Fitted {
  const size = jpegSize(bytes);
  if (size && Math.max(size.width, size.height) <= maxEdge) {
    return { data: bytes, ...size, original: size, resized: false };
  }
  const img = jpeg.decode(bytes, { useTArray: true, maxResolutionInMP: 200, maxMemoryUsageInMB: 1024 });
  const original = { width: img.width, height: img.height };
  const scale = Math.min(1, maxEdge / Math.max(img.width, img.height));
  if (scale === 1) return { data: bytes, ...original, original, resized: false };
  const width = Math.max(1, Math.round(img.width * scale));
  const height = Math.max(1, Math.round(img.height * scale));
  const data = jpeg.encode({ data: downscale(img.data, img.width, img.height, width, height), width, height }, quality).data;
  return { data, width, height, original, resized: true };
}
