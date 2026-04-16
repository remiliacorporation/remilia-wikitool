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
        "bull" => Some('\u{2022}'),
        "copy" => Some('\u{00a9}'),
        "deg" => Some('\u{00b0}'),
        "divide" => Some('\u{00f7}'),
        "euro" => Some('\u{20ac}'),
        "gt" => Some('>'),
        "hellip" => Some('\u{2026}'),
        "iexcl" => Some('\u{00a1}'),
        "iquest" => Some('\u{00bf}'),
        "laquo" => Some('\u{00ab}'),
        "ldquo" => Some('\u{201c}'),
        "lsquo" => Some('\u{2018}'),
        "lt" => Some('<'),
        "mdash" => Some('\u{2014}'),
        "micro" => Some('\u{00b5}'),
        "middot" => Some('\u{00b7}'),
        "nbsp" => Some(' '),
        "ndash" => Some('\u{2013}'),
        "para" => Some('\u{00b6}'),
        "plusmn" => Some('\u{00b1}'),
        "pound" => Some('\u{00a3}'),
        "quot" => Some('"'),
        "raquo" => Some('\u{00bb}'),
        "rdquo" => Some('\u{201d}'),
        "reg" => Some('\u{00ae}'),
        "rsquo" => Some('\u{2019}'),
        "sect" => Some('\u{00a7}'),
        "sup1" => Some('\u{00b9}'),
        "sup2" => Some('\u{00b2}'),
        "sup3" => Some('\u{00b3}'),
        "times" => Some('\u{00d7}'),
        "trade" => Some('\u{2122}'),
        "yen" => Some('\u{00a5}'),
        // Common accented Latin letters
        "Aacute" => Some('\u{00c1}'),
        "aacute" => Some('\u{00e1}'),
        "Acirc" => Some('\u{00c2}'),
        "acirc" => Some('\u{00e2}'),
        "AElig" => Some('\u{00c6}'),
        "aelig" => Some('\u{00e6}'),
        "Agrave" => Some('\u{00c0}'),
        "agrave" => Some('\u{00e0}'),
        "Aring" => Some('\u{00c5}'),
        "aring" => Some('\u{00e5}'),
        "Atilde" => Some('\u{00c3}'),
        "atilde" => Some('\u{00e3}'),
        "Auml" => Some('\u{00c4}'),
        "auml" => Some('\u{00e4}'),
        "Ccedil" => Some('\u{00c7}'),
        "ccedil" => Some('\u{00e7}'),
        "Eacute" => Some('\u{00c9}'),
        "eacute" => Some('\u{00e9}'),
        "Ecirc" => Some('\u{00ca}'),
        "ecirc" => Some('\u{00ea}'),
        "Egrave" => Some('\u{00c8}'),
        "egrave" => Some('\u{00e8}'),
        "Euml" => Some('\u{00cb}'),
        "euml" => Some('\u{00eb}'),
        "Iacute" => Some('\u{00cd}'),
        "iacute" => Some('\u{00ed}'),
        "Icirc" => Some('\u{00ce}'),
        "icirc" => Some('\u{00ee}'),
        "Igrave" => Some('\u{00cc}'),
        "igrave" => Some('\u{00ec}'),
        "Iuml" => Some('\u{00cf}'),
        "iuml" => Some('\u{00ef}'),
        "Ntilde" => Some('\u{00d1}'),
        "ntilde" => Some('\u{00f1}'),
        "Oacute" => Some('\u{00d3}'),
        "oacute" => Some('\u{00f3}'),
        "Ocirc" => Some('\u{00d4}'),
        "ocirc" => Some('\u{00f4}'),
        "Ograve" => Some('\u{00d2}'),
        "ograve" => Some('\u{00f2}'),
        "Oslash" => Some('\u{00d8}'),
        "oslash" => Some('\u{00f8}'),
        "Otilde" => Some('\u{00d5}'),
        "otilde" => Some('\u{00f5}'),
        "Ouml" => Some('\u{00d6}'),
        "ouml" => Some('\u{00f6}'),
        "szlig" => Some('\u{00df}'),
        "Uacute" => Some('\u{00da}'),
        "uacute" => Some('\u{00fa}'),
        "Ucirc" => Some('\u{00db}'),
        "ucirc" => Some('\u{00fb}'),
        "Ugrave" => Some('\u{00d9}'),
        "ugrave" => Some('\u{00f9}'),
        "Uuml" => Some('\u{00dc}'),
        "uuml" => Some('\u{00fc}'),
        "Yacute" => Some('\u{00dd}'),
        "yacute" => Some('\u{00fd}'),
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

    #[test]
    fn decodes_common_symbol_entities() {
        assert_eq!(
            decode_html_entities("50 &plusmn; 2 &middot; 3 &times; 4 &divide; 2 &deg; &micro;"),
            "50 \u{00b1} 2 \u{00b7} 3 \u{00d7} 4 \u{00f7} 2 \u{00b0} \u{00b5}"
        );
    }

    #[test]
    fn decodes_common_accented_letters() {
        assert_eq!(
            decode_html_entities("caf&eacute; na&iuml;ve &uuml;ber"),
            "caf\u{00e9} na\u{00ef}ve \u{00fc}ber"
        );
    }

    #[test]
    fn decodes_quotation_and_bullet_entities() {
        assert_eq!(
            decode_html_entities("&laquo;note&raquo; &bull; item"),
            "\u{00ab}note\u{00bb} \u{2022} item"
        );
    }
}
