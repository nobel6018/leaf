# iPhone에서 Mac으로 보내기

2026-11-05 결정: 같은 Wi-Fi면 직접 보내고, 떨어져 있으면 iCloud 폴더로 보냅니다. 구현은 1단계 후반입니다.

## Wi-Fi 직접 전송 (기본)

- Mac 앱이 Bonjour로 `_leaf._tcp` 서비스를 엽니다. iPhone은 Network.framework로 찾습니다
- 처음 한 번 짝짓기를 합니다. Mac 화면의 6자리 코드를 iPhone에 넣어 공유 비밀 값을 만들고, 이후 요청은 이 값으로 인증합니다. 같은 Wi-Fi의 다른 기기가 보관함에 파일을 넣지 못하게 합니다
- 문서 단위로 보냅니다. 원본 사진을 먼저 보내고 manifest를 마지막에 보냅니다. Mac은 manifest가 도착해야 문서로 인식합니다
- 끊기면 받은 파일은 건너뛰고 이어서 보냅니다
- 짝짓기 비밀 값은 iPhone 키체인과 Mac 키체인에만 둡니다

## iCloud 폴더 (보조)

sori와 같이 무료 Apple ID 서명이라 CloudKit은 못 쓰고, iCloud Drive 폴더를 씁니다. 큰 책은 업로드와 다운로드를 기다려야 하므로 보조 경로입니다.
