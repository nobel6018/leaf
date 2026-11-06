# 보정 모델

곡면 보정(UVDoc)과 인페인팅(LaMa) 모델은 설치 파일에 넣지 않습니다. Mac 앱 설정의 "보정 모델"에서 받습니다.
모델이 없어도 스캔, 원근 보정, OCR, 내보내기는 됩니다. 곡면 보정은 건너뛰고, 손가락과 필기 자리는 종이 색으로 채웁니다.

| 파일 | 용도 | 크기 | 라이선스 | 출처 |
|---|---|---|---|---|
| `uvdoc.onnx` | 곡면 보정 | 약 30MB | MIT | tanguymagne/UVDoc, `scripts/models/export_uvdoc.py`로 변환 |
| `lama_fp32.onnx` | 손가락, 필기 지우기 | 약 198MB | Apache-2.0 | Carve/LaMa-ONNX, `scripts/models/fetch_lama.py` |

크기와 SHA-256은 `crates/scanshelf-vision/src/models.rs` `MODELS`에 있습니다.

## 받기

- 주소: `https://github.com/nobel6018/scanshelf/releases/download/models-v1/<파일>`. 앱 Release(`v*`)와 따로 둔 사전 공개(prerelease) Release라 `releases/latest`(업데이트 확인, AltStore)에 영향이 없습니다
- `src-tauri/src/download.rs`가 16MB 조각을 연결 4개로 받고, 끊기면 남은 조각만 받습니다
- 다 받으면 SHA-256을 확인합니다. 다르면 지우고 오류를 보여 줍니다(`src-tauri/src/models.rs`)
- 받는 동안은 진행 중인 작업으로 등록돼 업데이트가 앱을 다시 켜지 않습니다(docs/update.md)
- 저장 위치: 앱 데이터 폴더 `models/`. CoreML 컴파일 결과는 `models/coreml-cache/`

## 모델을 바꿀 때

새 태그(`models-v2`)로 올리고 `MODELS`의 주소, 크기, 해시를 함께 고칩니다. 옛 태그는 지우지 않습니다(이전 버전 앱이 계속 받습니다).
