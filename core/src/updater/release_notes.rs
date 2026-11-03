//! Release notes arrive as Markdown and have to be made readable without
//! rendering anything: they are text written by whoever publishes, i.e. from
//! outside, so nothing in them becomes markup (the Java `ReleaseNotes`).

use std::sync::LazyLock;

use regex::Regex;

static MARKERS: LazyLock<[(Regex, &str); 5]> = LazyLock::new(|| {
    [
        (Regex::new(r"\*\*(.+?)\*\*").unwrap(), "$1"), // **bold**
        (Regex::new(r"__(.+?)__").unwrap(), "$1"),     // __bold__
        (Regex::new(r"\*(.+?)\*").unwrap(), "$1"),     // *italics*
        (Regex::new(r"`(.+?)`").unwrap(), "$1"),       // `code`
        // [text](link) -> text: the link cannot be clicked here and only gets
        // in the way of reading.
        (Regex::new(r"\[(.+?)]\((.+?)\)").unwrap(), "$1"),
    ]
});

/// The notes as plain text: headings in capitals, bullets as "  • ", no
/// markers left. Empty when there are none: nothing is invented.
pub fn to_plain_text(markdown: Option<&str>) -> String {
    let Some(markdown) = markdown.filter(|m| !m.trim().is_empty()) else {
        return String::new();
    };

    let mut out: Vec<String> = Vec::new();
    for raw in markdown.trim().lines() {
        let line = raw.trim();

        // Horizontal rules: they add nothing without formatting.
        if is_rule(line) {
            out.push(String::new());
            continue;
        }

        if line.starts_with('#') {
            let title = line.trim_start_matches('#').trim_start();
            if !title.trim().is_empty() {
                // A blank line before the heading, except at the very start.
                if out.last().is_some_and(|last| !last.trim().is_empty()) {
                    out.push(String::new());
                }
                out.push(clean(title).to_uppercase());
            }
            continue;
        }

        if line.starts_with("- ") || line.starts_with("* ") || line.starts_with("+ ") {
            out.push(format!("  • {}", clean(line[2..].trim())));
            continue;
        }

        out.push(clean(line));
    }

    let first = out.iter().position(|l| !l.trim().is_empty()).unwrap_or(out.len());
    let last = out.iter().rposition(|l| !l.trim().is_empty()).map_or(first, |i| i + 1);
    out[first..last].join("\n")
}

fn is_rule(line: &str) -> bool {
    line.len() >= 3 && ['-', '*', '_'].iter().any(|&c| line.chars().all(|x| x == c))
}

fn clean(text: &str) -> String {
    let mut text = text.to_string();
    for (pattern, replacement) in MARKERS.iter() {
        text = pattern.replace_all(&text, *replacement).into_owned();
    }
    text.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(markdown: &str) -> String {
        to_plain_text(Some(markdown))
    }

    #[test]
    fn markdown_markers_go_away_the_text_stays() {
        assert_eq!(plain("**Actualizaciones automáticas.** Ya *funciona*."), "Actualizaciones automáticas. Ya funciona.");
        assert_eq!(plain("Se llama `settings.json`"), "Se llama settings.json");
        assert_eq!(plain("__negrita con guion bajo__"), "negrita con guion bajo");
    }

    #[test]
    fn headings_read_as_headings_and_bullets_as_bullets() {
        let text = plain("## Novedades\n\n- Primera cosa\n- Segunda cosa\n");

        assert!(text.starts_with("NOVEDADES"), "{text}");
        assert!(text.contains("  • Primera cosa"), "{text}");
        assert!(text.contains("  • Segunda cosa"), "{text}");
        assert!(!text.contains("##"), "not one marker may be left");
        assert!(!text.contains("- Primera"), "the dash is replaced by the bullet");
    }

    #[test]
    fn a_link_leaves_its_text_behind() {
        assert_eq!(plain("Mira la [documentación](https://ejemplo.com/doc)"), "Mira la documentación");
    }

    #[test]
    fn horizontal_rules_leave_no_rubbish_behind() {
        let text = plain("Antes\n\n---\n\nDespués");

        assert!(!text.contains("---"), "{text}");
        assert!(text.contains("Antes") && text.contains("Después"));
    }

    #[test]
    fn with_no_notes_nothing_is_invented() {
        assert_eq!(to_plain_text(None), "");
        assert_eq!(plain(""), "");
        assert_eq!(plain("   \n\n  "), "");
    }

    #[test]
    fn no_stray_blank_lines_at_either_end() {
        assert_eq!(plain("\n\n---\n\nContenido\n\n---\n\n"), "Contenido");
    }

    #[test]
    fn real_published_notes_come_out_readable() {
        let real = "## Novedades\n\n\
            **Actualizaciones automáticas.** Soundify comprueba si hay una versión nueva.\n\n\
            - La comprobación es **una vez al día como mucho**.\n\
            - La consulta **no envía ningún dato tuyo**.\n\n\
            ---\n\n\
            *Esta versión se publica firmada.*\n";

        let text = plain(real);

        assert!(!text.contains("**") && !text.contains("##") && !text.contains("---"), "{text}");
        assert!(text.contains("NOVEDADES"));
        assert!(text.contains("  • La comprobación es una vez al día como mucho."));
        assert!(text.contains("Esta versión se publica firmada."));
    }
}
