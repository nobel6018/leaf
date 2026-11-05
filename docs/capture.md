# 자동 촬영 설계 (iPhone)

거치대에 iPhone을 고정하고 종이만 바꿔 놓으면 한 장씩 찍히는 흐름이 목표입니다. 코드는 1단계에서 `ios/Leaf/Capture/`에 들어갑니다.

## 상태

```
대기 ─(문서 감지 + 화면 안정 + 손 없음)→ 촬영 ─→ 변화 대기 ─(내용이 바뀜)→ 대기
```

| 판단 | 방법 |
|---|---|
| 문서 감지 | Vision `VNDetectDocumentSegmentationRequest`. 미리보기에 테두리 표시 |
| 화면 안정 | 축소한 프레임끼리의 평균 차이가 기준 아래로 일정 시간(시작값 0.6초) 유지 |
| 손 없음 | `VNDetectHumanHandPoseRequest`. 손이 보이면 안정돼도 찍지 않는다. 종이를 내려놓는 손을 찍는 실수를 막는다 |
| 같은 장 중복 방지 | 직전 촬영본과 `VNGenerateImageFeaturePrintRequest` 거리를 비교. 가까우면 같은 장으로 보고 건너뛴다 |

기준값은 실제 팜플렛과 책으로 맞춥니다. 프레임 분석은 1초에 10번 정도로 줄여 발열을 낮춥니다.

## 셔터

- 자동 촬영을 끈 수동 모드도 둡니다
- 볼륨 버튼과 블루투스 카메라 리모컨: `AVCaptureEventInteraction`(iOS 17.2+). 시중 리모컨은 볼륨 키 신호를 보내므로 같이 처리됩니다
- 촬영은 `AVCapturePhotoOutput` 최대 해상도, HEIC로 저장합니다
