pub(crate) fn decode_html_entities(text: &str) -> String {
    let mut output = String::new();
    let mut index = 0usize;
    while index < text.len() {
        let Some(relative_ampersand) = text[index..].find('&') else {
            output.push_str(&text[index..]);
            break;
        };
        let ampersand = index + relative_ampersand;
        output.push_str(&text[index..ampersand]);

        let Some(relative_semicolon) = text[ampersand + 1..].find(';') else {
            output.push('&');
            index = ampersand + 1;
            continue;
        };
        let semicolon = ampersand + 1 + relative_semicolon;
        let entity = &text[ampersand + 1..semicolon];
        if entity.len() > 32 {
            output.push('&');
            index = ampersand + 1;
            continue;
        }
        if let Some(decoded) = decode_html_entity(entity) {
            output.push(decoded);
            index = semicolon + 1;
            continue;
        }

        output.push('&');
        index = ampersand + 1;
    }
    output
}

fn decode_html_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "apos" => Some('\''),
        "copy" => Some('\u{00a9}'),
        "gt" => Some('>'),
        "hellip" => Some('\u{2026}'),
        "ldquo" => Some('\u{201c}'),
        "lsquo" => Some('\u{2018}'),
        "lt" => Some('<'),
        "mdash" => Some('\u{2014}'),
        "ndash" => Some('\u{2013}'),
        "nbsp" => Some(' '),
        "quot" => Some('"'),
        "rdquo" => Some('\u{201d}'),
        "reg" => Some('\u{00ae}'),
        "rsquo" => Some('\u{2019}'),
        "trade" => Some('\u{2122}'),
        _ => decode_numeric_entity(entity),
    }
}

fn decode_numeric_entity(entity: &str) -> Option<char> {
    let numeric = entity.strip_prefix('#')?;
    let codepoint = if let Some(hex) = numeric
        .strip_prefix('x')
        .or_else(|| numeric.strip_prefix('X'))
    {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        numeric.parse::<u32>().ok()?
    };
    char::from_u32(codepoint)
}

#[cfg(test)]
mod tests {
    use super::decode_html_entities;

    #[test]
    fn decodes_named_decimal_and_hex_entities() {
        assert_eq!(
            decode_html_entities("Africa&#x27;s world&rsquo;s &amp; fast &#169;"),
            "Africa's world\u{2019}s & fast \u{00a9}"
        );
    }

    #[test]
    fn leaves_unknown_entities_intact() {
        assert_eq!(decode_html_entities("A &unknown; B"), "A &unknown; B");
    }
}
