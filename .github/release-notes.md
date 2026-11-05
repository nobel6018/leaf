iPhone 거치대로 문서, 책, 팜플렛을 찍고 Mac에서 보정, OCR, 정리하는 개인용 스캐너입니다.

## 설치
- Mac (Apple Silicon): `.dmg`
- iPhone: AltStore에 소스 `https://github.com/nobel6018/leaf/releases/latest/download/apps.json`을 추가해 설치합니다([설치 방법](https://github.com/nobel6018/leaf/blob/main/docs/release.md))

코드 서명을 하지 않은 빌드입니다. Mac은 dmg에서 `leaf.app`을 응용 프로그램 폴더로 옮긴 뒤 터미널에서 아래 명령을 한 번 실행합니다. 브라우저로 받은 파일에 붙는 격리 표시를 지워, "손상되었기 때문에 열 수 없습니다" 경고 없이 열립니다.

```sh
xattr -cr /Applications/leaf.app
```

Mac 앱은 켤 때 새 버전을 알리고, "업데이트하고 다시 켜기"를 누르면 앱 안에서 설치합니다. 이때는 xattr 명령이 필요 없습니다. 처리, 받기, AI 정리 같은 작업이 진행 중이면 설치하지 않습니다([업데이트](https://github.com/nobel6018/leaf/blob/main/docs/update.md)).

## 처음 쓸 때
- iPhone 연결: Mac 앱 설정에서 "iPhone 연결"을 눌러 나온 6자리 코드를 iPhone에 넣습니다. 같은 Wi-Fi면 바로, 떨어져 있으면 iCloud Drive로 보냅니다
- 보정 모델: Mac 앱 설정의 "보정 모델"에서 받습니다(약 230MB). 없어도 스캔과 OCR은 됩니다
