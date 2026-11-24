use super::{Inverse, Project, model::Kind};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Style {
    pub id: Uuid,
    pub name: String,
    pub text: String,
    pub reference_ids: Vec<Uuid>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StyleInput {
    pub id: Option<Uuid>,
    pub name: String,
    pub text: String,
    pub reference_ids: Vec<Uuid>,
}

pub(super) fn default_selection() -> Option<Uuid> {
    Some(Uuid::nil())
}

pub(super) fn initial_styles() -> Vec<Style> {
    vec![Style {
        id: Uuid::nil(),
        name: "기본 스타일".into(),
        text: String::new(),
        reference_ids: vec![],
    }]
}

impl Project {
    pub(super) fn select_style(&mut self, id: Option<Uuid>) -> Result<(), String> {
        if id.is_some_and(|id| !self.guideline_styles.iter().any(|s| s.id == id)) {
            return Err("선택한 제작 스타일을 찾을 수 없습니다.".into());
        }
        self.selected_style_id = id;
        Ok(())
    }

    pub(super) fn save_style(
        &mut self,
        expected: u32,
        input: StyleInput,
    ) -> Result<Option<Inverse>, String> {
        if self.guidelines_revision != expected {
            return Err("제작 지침이 변경되었습니다. 최신 내용을 확인해 주세요.".into());
        }
        let name = input.name.trim().to_owned();
        if name.is_empty() {
            return Err("스타일 이름을 입력해 주세요.".into());
        }
        if self
            .guideline_styles
            .iter()
            .any(|s| s.name == name && Some(s.id) != input.id)
        {
            return Err("같은 이름의 스타일이 있습니다.".into());
        }
        let mut reference_ids = Vec::new();
        for id in input.reference_ids {
            let asset = self.asset(id)?;
            if asset.removed_from_bin || asset.kind == Kind::Audio {
                return Err("보관함의 사진과 영상만 참고 자료로 저장할 수 있습니다.".into());
            }
            if !reference_ids.contains(&id) {
                reference_ids.push(id);
            }
        }
        let id = input.id.unwrap_or_else(Uuid::new_v4);
        let style = Style {
            id,
            name,
            text: input.text.trim().into(),
            reference_ids,
        };
        let before = self.guideline_styles.clone();
        let previous_selection = self.selected_style_id;
        if input.id.is_some() {
            let saved = self
                .guideline_styles
                .iter_mut()
                .find(|s| s.id == id)
                .ok_or("수정할 스타일을 찾을 수 없습니다.")?;
            if *saved == style {
                return Ok(None);
            }
            *saved = style;
        } else {
            self.guideline_styles.push(style);
            self.selected_style_id = Some(id);
        }
        self.guidelines_revision += 1;
        Ok(Some(Inverse::Guidelines {
            before,
            after: self.guideline_styles.clone(),
            previous_selection,
        }))
    }

    // Requests saved by the single-guideline editor address the original default style.
    pub(super) fn set_production_guidelines(
        &mut self,
        expected: u32,
        text: String,
    ) -> Result<Option<Inverse>, String> {
        let style = self
            .guideline_styles
            .iter()
            .find(|s| s.id.is_nil())
            .ok_or("기본 스타일을 찾을 수 없습니다.")?;
        self.save_style(
            expected,
            StyleInput {
                id: Some(style.id),
                name: style.name.clone(),
                text,
                reference_ids: style.reference_ids.clone(),
            },
        )
    }

    pub(super) fn detach_style_reference(&mut self, id: Uuid) {
        let mut changed = false;
        for style in &mut self.guideline_styles {
            if style.reference_ids.contains(&id) {
                style.reference_ids.retain(|asset| *asset != id);
                changed = true;
            }
        }
        if changed {
            self.guidelines_revision += 1;
        }
    }
}
