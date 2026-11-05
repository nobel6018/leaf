"""LaMa(Apache-2.0) ONNX를 받아 해시를 확인한다. leaf-vision 인페인팅 모델.

Carve/LaMa-ONNX의 lama_fp32.onnx(big-lama를 torch.onnx.export로 옮긴 것)를 커밋 고정으로 받는다.
입력: image [1, 3, 512, 512] float32 RGB 0~1, mask [1, 1, 512, 512] float32 (1 = 지울 곳)
출력: output [1, 3, 512, 512] float32 RGB 0~255

@see docs/vision.md
"""

import argparse
import hashlib
import shutil
import sys
import urllib.request
from pathlib import Path

REVISION = "c3c0c9e468934d62e79c329e35d82dd09ff8c444"
URL = f"https://huggingface.co/Carve/LaMa-ONNX/resolve/{REVISION}/lama_fp32.onnx"
SIZE = 208044816


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=Path, required=True, help="저장할 .onnx 경로")
    args = parser.parse_args()
    args.out.parent.mkdir(parents=True, exist_ok=True)
    part = args.out.with_suffix(".onnx.part")
    with urllib.request.urlopen(URL) as response, part.open("wb") as f:
        shutil.copyfileobj(response, f, 1 << 20)
    if part.stat().st_size != SIZE:
        sys.exit(f"크기가 다릅니다: {part.stat().st_size} != {SIZE}")
    part.rename(args.out)

    import onnxruntime as ort

    session = ort.InferenceSession(str(args.out), providers=["CPUExecutionProvider"])
    for i in session.get_inputs():
        print("입력", i.name, i.shape)
    for o in session.get_outputs():
        print("출력", o.name, o.shape)
    print(f"{args.out}  {SIZE} bytes  sha256 {sha256(args.out)}")


if __name__ == "__main__":
    main()
