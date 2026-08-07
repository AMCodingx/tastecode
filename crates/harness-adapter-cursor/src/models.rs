use harness_protocol::Model;

/// Parse the account-specific table emitted by `cursor-agent models`.
pub fn parse_cursor_models(output: &str) -> Vec<Model> {
    let output = strip_csi(output);
    let mut models = Vec::new();
    let mut reading_models = false;
    for raw_line in output.split(['\n', '\r']) {
        let line = raw_line.trim();
        if line == "Available models" {
            reading_models = true;
            continue;
        }
        if !reading_models || line.is_empty() {
            continue;
        }
        if line.starts_with("Tip:") {
            break;
        }

        let (details, labels) = status_suffix(line);
        let (id, display_name) = details
            .split_once(" - ")
            .map_or((details, details), |(id, name)| (id.trim(), name.trim()));
        if id.is_empty()
            || id.chars().any(char::is_whitespace)
            || matches!(id.to_ascii_lowercase().as_str(), "auto" | "automatic")
        {
            continue;
        }
        models.push(Model {
            id: id.into(),
            display_name: if display_name.is_empty() {
                id.into()
            } else {
                display_name.into()
            },
            description: None,
            is_default: labels.contains(&"default"),
            reasoning_efforts: Vec::new(),
            default_reasoning_effort: None,
            service_tiers: Vec::new(),
            default_service_tier: None,
        });
    }
    models
}

fn status_suffix(line: &str) -> (&str, Vec<&str>) {
    let Some(details) = line.strip_suffix(')') else {
        return (line, Vec::new());
    };
    let Some((details, status)) = details.rsplit_once(" (") else {
        return (line, Vec::new());
    };
    let labels = status.split(',').map(str::trim).collect::<Vec<_>>();
    if labels
        .iter()
        .all(|label| matches!(*label, "current" | "default"))
    {
        (details, labels)
    } else {
        (line, Vec::new())
    }
}

fn strip_csi(output: &str) -> String {
    let mut cleaned = String::with_capacity(output.len());
    let mut characters = output.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\u{1b}' && characters.peek() == Some(&'[') {
            let _ = characters.next();
            for character in characters.by_ref() {
                if ('@'..='~').contains(&character) {
                    break;
                }
            }
        } else {
            cleaned.push(character);
        }
    }
    cleaned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_concrete_models_without_the_automatic_route() {
        let models = parse_cursor_models(concat!(
            "\u{1b}[1mAvailable models\u{1b}[0m\r\n",
            "\r\n",
            "auto - Automatic\r\n",
            "composer-2.5 - Composer 2.5 Fast (current, default)\r\n",
            "claude-4.6-sonnet - Claude 4.6 Sonnet\r\n",
            "bad row - Still accepted (unknown)\r\n",
            "\r\n",
            "Tip: use --model <id> to switch.\r\n"
        ));
        assert_eq!(
            models
                .iter()
                .map(|model| (
                    model.id.as_str(),
                    model.display_name.as_str(),
                    model.is_default
                ))
                .collect::<Vec<_>>(),
            [
                ("composer-2.5", "Composer 2.5 Fast", true),
                ("claude-4.6-sonnet", "Claude 4.6 Sonnet", false),
            ]
        );
    }

    #[test]
    fn accepts_the_current_no_models_response() {
        assert!(parse_cursor_models("No models available for this account.\n").is_empty());
    }
}
