//! Shared reasoning about a configured frontend launch command.
//!
//! A frontend can only claim it *strictly* applied a launch flag when Brain is
//! the one appending that flag to a plain invocation of the frontend's own
//! executable. A wrapper (a shell one-liner, a script, an alias-like command
//! that already passes the same flag) may rewrite, drop, or duplicate what
//! Brain appends, so the honest report for one of those is advisory.

#[cfg(test)]
mod tests;

/// Whether `command` is a direct invocation of `executable` that leaves every
/// flag in `owned_flags` for Brain to supply.
///
/// `--` is treated as owned whenever it appears in `owned_flags`: a command
/// that already ends option parsing cannot receive Brain's own arguments.
pub(super) fn is_direct_invocation(command: &str, executable: &str, owned_flags: &[&str]) -> bool {
    let Some(arguments) = parse(command) else {
        return false;
    };
    let Some(program) = arguments.first() else {
        return false;
    };
    if std::path::Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        != Some(executable)
    {
        return false;
    }
    !arguments.iter().skip(1).any(|argument| {
        owned_flags.iter().any(|flag| {
            argument == flag || (*flag != "--" && argument.starts_with(&format!("{flag}=")))
        })
    })
}

/// Split a command the way a shell would, refusing anything a shell would treat
/// as more than one plain word list.
///
/// Returns `None` for any expansion, redirection, substitution, control
/// character, or unterminated quote: Brain cannot reason about what those
/// produce, so the caller must fall back to the cautious answer.
pub(super) fn parse(command: &str) -> Option<Vec<String>> {
    Some(
        parse_words(command)?
            .into_iter()
            .map(|word| word.value)
            .collect(),
    )
}

pub(super) struct Word {
    pub(super) value: String,
    pub(super) span: std::ops::Range<usize>,
}

pub(super) fn parse_words(command: &str) -> Option<Vec<Word>> {
    #[derive(Clone, Copy)]
    enum Quote {
        None,
        Single,
        Double,
    }

    let mut arguments = Vec::new();
    let mut argument = String::new();
    let mut quote = Quote::None;
    let mut escaped = false;
    let mut started = false;
    let offset = command.len() - command.trim_start().len();
    let mut start = 0;
    for (index, character) in command.trim().char_indices() {
        let index = index + offset;
        if !started {
            start = index;
        }
        if character.is_control() && character != '\t' {
            return None;
        }
        if escaped {
            if matches!(quote, Quote::Double) && !matches!(character, '\\' | '"' | '$' | '`') {
                argument.push('\\');
            }
            argument.push(character);
            escaped = false;
            started = true;
            continue;
        }
        match quote {
            Quote::Single => {
                if character == '\'' {
                    quote = Quote::None;
                } else {
                    argument.push(character);
                }
                started = true;
            }
            Quote::Double => match character {
                '"' => quote = Quote::None,
                '\\' => escaped = true,
                '$' | '`' => return None,
                _ => {
                    argument.push(character);
                    started = true;
                }
            },
            Quote::None => match character {
                '\'' => {
                    quote = Quote::Single;
                    started = true;
                }
                '"' => {
                    quote = Quote::Double;
                    started = true;
                }
                '\\' => {
                    escaped = true;
                    started = true;
                }
                ' ' | '\t' => {
                    if started {
                        arguments.push(Word {
                            value: std::mem::take(&mut argument),
                            span: start..index,
                        });
                        started = false;
                    }
                }
                ';' | '|' | '&' | '<' | '>' | '(' | ')' | '#' | '$' | '`' | '*' | '?' | '['
                | ']' | '{' | '}' | '~' => return None,
                _ => {
                    argument.push(character);
                    started = true;
                }
            },
        }
    }
    if escaped || !matches!(quote, Quote::None) {
        return None;
    }
    if started {
        arguments.push(Word {
            value: argument,
            span: start..command.trim_end().len(),
        });
    }
    Some(arguments)
}

/// A literal command prefix whose executable will receive appended options.
pub(super) fn accepts_appended_options(command: &str) -> bool {
    let Some(words) = parse(command) else {
        return false;
    };
    let mut words = words.iter().skip_while(|word| assignment(word));
    let Some(mut executable) = words.next() else {
        return false;
    };
    if std::path::Path::new(executable)
        .file_name()
        .is_some_and(|name| name == "env")
    {
        let Some(program) = words.find(|word| !assignment(word)) else {
            return false;
        };
        executable = program;
    }
    !executable.starts_with('-')
        && !std::path::Path::new(executable)
            .file_name()
            .is_some_and(|name| {
                ["sh", "bash", "zsh", "dash", "fish", "csh", "ksh"]
                    .iter()
                    .any(|shell| name == *shell)
            })
        && !words.any(|word| word == "--")
}

fn assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    })
}
