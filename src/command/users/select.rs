//! Numbered option prompts shared by the portable-user commands.

/// One offered answer: the value it produces and the row a human reads.
pub(super) struct Choice {
    pub(super) value: String,
    pub(super) label: String,
}

impl Choice {
    pub(super) fn new(value: &str, label: &str) -> Self {
        Self {
            value: value.to_owned(),
            label: label.to_owned(),
        }
    }
}

/// Render one numbered option list, ready to print one row per line.
pub(super) fn numbered_rows(choices: &[Choice]) -> Vec<String> {
    choices
        .iter()
        .enumerate()
        .map(|(index, choice)| format!("{}) {}", index + 1, choice.label))
        .collect()
}

/// Interpret one answer as a row number, falling back to the literal value.
pub(super) fn interpret_row(choices: &[Choice], answer: &str) -> Option<String> {
    let answer = answer.trim();
    if answer.is_empty() {
        return None;
    }
    if let Ok(row) = answer.parse::<usize>() {
        return choices
            .get(row.checked_sub(1)?)
            .map(|choice| choice.value.clone());
    }
    Some(answer.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{Choice, interpret_row, numbered_rows};

    fn choices() -> Vec<Choice> {
        vec![
            Choice::new("pablo", "pablo (Pablo)"),
            Choice::new("wife", "wife (Wife)"),
        ]
    }

    #[test]
    fn options_are_numbered_from_one() {
        assert_eq!(
            numbered_rows(&choices()),
            ["1) pablo (Pablo)", "2) wife (Wife)"]
        );
        assert!(numbered_rows(&[]).is_empty());
    }

    #[test]
    fn an_answer_is_a_row_number_or_the_literal_value_itself() {
        let choices = choices();

        assert_eq!(interpret_row(&choices, " 2 "), Some("wife".to_owned()));
        assert_eq!(interpret_row(&choices, "me"), Some("me".to_owned()));
        assert_eq!(interpret_row(&choices, "0"), None);
        assert_eq!(interpret_row(&choices, "3"), None);
        assert_eq!(interpret_row(&choices, "  "), None);
    }
}
