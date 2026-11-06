"""UVDoc(MIT) 가중치를 ONNX로 내보낸다. scanshelf-vision 곡면 보정 모델.

입력: image [1, 3, 712, 488] float32, RGB 0~1
출력: grid [1, 2, 45, 31] float32, 정규화 좌표(-1~1, x 먼저). grid_sample(align_corners=True)용 역방향 좌표

사용법은 docs/vision.md, 모델 목록은 crates/scanshelf-vision/src/models.rs.
@see docs/vision.md
"""

import argparse
import hashlib
import subprocess
import sys
from pathlib import Path

UVDOC_REPO = "https://github.com/tanguymagne/UVDoc.git"
# 재현성을 위해 커밋과 가중치 해시를 고정한다
UVDOC_COMMIT = "4c9b82b537057aff2526e6dd118a847cdd072e82"
WEIGHTS_SHA256 = "7e90861b8a516eb4bc51f84bd889cb77275743d2d1d3ca8091951ec9f2b7da23"
IMG_W, IMG_H = 488, 712


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def fetch_repo(work: Path) -> Path:
    repo = work / "UVDoc"
    if not repo.exists():
        subprocess.run(["git", "clone", UVDOC_REPO, str(repo)], check=True)
    subprocess.run(["git", "-C", str(repo), "checkout", "--quiet", UVDOC_COMMIT], check=True)
    weights = repo / "model" / "best_model.pkl"
    if sha256(weights) != WEIGHTS_SHA256:
        sys.exit(f"가중치 해시가 다릅니다: {weights}")
    return repo


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--work", type=Path, required=True, help="UVDoc을 받을 작업 폴더(저장소 밖)")
    parser.add_argument("--out", type=Path, required=True, help="내보낼 .onnx 경로")
    args = parser.parse_args()

    repo = fetch_repo(args.work)
    sys.path.insert(0, str(repo))
    import torch
    from model import UVDocnet

    model = UVDocnet(num_filter=32, kernel_size=5)
    ckpt = torch.load(repo / "model" / "best_model.pkl", map_location="cpu", weights_only=False)
    model.load_state_dict(ckpt["model_state"])
    model.eval()

    class Grid2D(torch.nn.Module):
        """3D 좌표 출력은 학습용이라 버리고 2D 격자만 내보낸다"""

        def __init__(self, inner):
            super().__init__()
            self.inner = inner

        def forward(self, x):
            grid, _ = self.inner(x)
            return grid

    wrapped = Grid2D(model).eval()
    dummy = torch.rand(1, 3, IMG_H, IMG_W)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    torch.onnx.export(
        wrapped,
        (dummy,),
        str(args.out),
        input_names=["image"],
        output_names=["grid"],
        opset_version=17,
        dynamo=False,
    )

    # PyTorch 결과와 ONNX Runtime 결과가 같은지 확인한다
    import numpy as np
    import onnxruntime as ort

    with torch.no_grad():
        expected = wrapped(dummy).numpy()
    session = ort.InferenceSession(str(args.out), providers=["CPUExecutionProvider"])
    got = session.run(None, {"image": dummy.numpy()})[0]
    diff = float(np.abs(expected - got).max())
    print(f"출력 {got.shape}, 최대 차이 {diff:.2e}")
    if diff > 1e-3:
        sys.exit("ONNX 출력이 PyTorch와 다릅니다")
    print(f"{args.out}  {args.out.stat().st_size} bytes  sha256 {sha256(args.out)}")


if __name__ == "__main__":
    main()
