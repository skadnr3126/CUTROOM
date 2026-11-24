# Codex에서 컷룸 자막 편집하기

컷룸을 열고 Codex에서 “보스 플레이 영상과 자막 구간의 마지막 인사말을 바꾸고 그 위치를 보여줘”처럼 요청합니다. 컷룸의 **자막 편집**에서도 같은 자막의 문구와 표시 시간을 조정할 수 있습니다. 내보낼 때는 “이 구간을 새 MP4로 내보내 줘”라고 요청합니다.

## Windows 설치판 연결

컷룸 설치 프로그램이 서버와 전용 런타임을 설치하고 Codex 사용자 설정에 등록합니다. Codex를 먼저 설치하거나 Node.js와 Codex CLI를 별도로 설치할 필요는 없습니다. 이미 열린 Codex 대화에서 도구가 보이지 않으면 새 대화를 열거나 Codex를 다시 실행하세요.

설치판은 `CODEX_HOME`이 지정된 경우 그 폴더의 `config.toml`을 사용합니다. 기본 위치는 `%USERPROFILE%\.codex\config.toml`입니다. 기존 개발용 컷룸 등록은 설치 경로로 전환하고 사용자가 지정한 내보내기 제한 시간과 활성화 설정은 유지합니다. 새로 등록할 때의 제한 시간은 1,800초입니다.

## 개발용 소스에서 연결하기

앱 저장소 루트에서 한 번 실행합니다.

```powershell
npm.cmd ci --prefix integrations/cutroom-mcp
codex mcp add cutroom -- (Get-Command node.exe).Source (Resolve-Path integrations/cutroom-mcp/server.mjs).Path
```

긴 영상도 내보내려면 Codex 설정의 `[mcp_servers.cutroom]`에 `tool_timeout_sec = 1800`을 지정합니다. 컷룸 제어 기능을 포함한 앱을 실행하고, 새 Codex 대화에서 `cutroom` 도구를 사용할 수 있습니다.

## 스타일을 담은 자막 파일

`import_ass`에 구간 ID, 읽어 둔 리비전, UTF-8 ASS 파일의 전체 경로를 전달합니다. 컷룸이 해당 구간에 파일 내용을 보관합니다. 컷룸의 **ASS 자막 편집**에서 문구와 시간을 고치거나 편집한 ASS 파일을 따로 저장할 수 있습니다. MP4 출력에는 기존 `export_scene`을 사용합니다.

## 외부에서 생성한 소스 등록

1. 생성을 시작할 때 `timeline`을 호출해 대상 `projectId`, `mediaDirectory`, 보관함의 `folders`를 확인합니다.
2. 완성 영상·이미지·오디오는 `mediaDirectory`의 원하는 하위 폴더에 새 이름으로 저장합니다. 컷룸이 파일을 자동 등록합니다. 이미 다른 곳에서 만든 결과는 원래 경로를 그대로 넘길 수 있습니다.
3. `register_output`에 `projectId`, 완성 파일마다 고유한 `jobId`, 절대 경로 `path`를 전달합니다. 표시할 `title`과 보관함의 `folderId`는 선택 사항입니다. 재시도할 때는 같은 `jobId`를 사용합니다. 반환된 `assetId`와 `path`가 등록 결과입니다. 생성 중 사용자가 프로젝트를 전환해도 처음 지정한 프로젝트에 들어갑니다.

## 편집 흐름

“마지막 구간 이름을 ‘마무리’로 바꿔 줘”, “평딜 구간을 전투 분석 바로 앞으로 옮겨 줘”처럼 요청할 수 있습니다. `rename_scene`은 `timeline`에서 읽은 구간 ID와 리비전, 새 이름을 받습니다. `move_scene`은 이동할 구간 ID와 기준 구간 ID(`targetId`)를 받아 `after: false`이면 바로 앞에, `true`이면 바로 뒤에 배치합니다. 두 작업 모두 컷룸에서 실행 취소할 수 있습니다.

1. `timeline`으로 구간을 찾고 `scene`으로 저장된 자막과 리비전을 읽습니다.
2. `captions`에 전체 자막 목록과 읽었던 리비전을 보냅니다. 시간은 구간 시작 기준 밀리초입니다. 다른 곳에서 먼저 저장했다면 구간을 다시 읽고 변경할 자막만 반영합니다.
3. `seek`로 문구가 보이는 시점으로 이동합니다. 컷룸에서 수동으로 고친 뒤에도 `scene`으로 다시 읽을 수 있습니다.
4. `export_scene`에 리비전과 새 MP4 경로를 보내면 해당 구간을 내보냅니다. 수정 가능한 자막 원본은 컷룸 프로젝트에 저장됩니다.

이미 자막을 입힌 영상은 같은 길이의 무자막 소스를 준비한 뒤 `import_video`와 `replace_video`로 교체할 수 있습니다. 교체는 영상 하나로 구성된 구간을 지원합니다.
