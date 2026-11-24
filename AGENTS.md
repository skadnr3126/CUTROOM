# 컷룸 개발과 편집 지침 유지보수

이 저장소를 Codex에서 연 상태로 **영상 자체를 편집**해 달라는 요청을 받으면 `integrations/codex-workspace/AGENTS.md`의 편집 지침을 적용한다.

이 저장소에서 MCP 명령, 자막·미디어 처리, 프로젝트 패키지 또는 홈의 편집 절차를 바꾸면 같은 변경에서 `integrations/cutroom-mcp/WORKFLOW.md`를 갱신한다. 명령 이름·입력·제약은 실제 서버와 앱 구현을 기준으로 확인한다.

새 Codex 작업 폴더에 배포하는 파일은 `integrations/codex-workspace/AGENTS.md`다. 이 파일은 설치판의 `workspace_guide`를 읽는 진입점이다. 편집 절차는 WORKFLOW.md에서 관리하고, 연결이나 초기 설정 방식이 달라질 때 진입점도 갱신한다.

Windows 설치 빌드에서는 두 파일을 MCP 런타임과 함께 묶고, 패키징된 서버에서 지침 조회와 도구 목록을 검사한다. GitHub 릴리스의 AGENTS.md는 해당 릴리스 태그에 들어 있는 작업 폴더용 원본을 사용한다.
