// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 wulflo

use std::collections::HashMap;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

/// Limpia Markdown y devuelve texto plano.
pub fn clean_markdown(input: &str, include_code: bool) -> String {
    let parser = Parser::new(input);
    let mut out = String::new();
    let mut in_code_block = false;
    for event in parser {
        match event {
            Event::Start(Tag::CodeBlock(_)) => {
                in_code_block = true;
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
            }
            Event::Start(pulldown_cmark::Tag::Paragraph) | Event::Start(pulldown_cmark::Tag::Heading { .. }) => {
                if !out.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                }
            }
            Event::End(pulldown_cmark::TagEnd::Paragraph) | Event::End(pulldown_cmark::TagEnd::Heading(_)) => {
                out.push('\n');
            }
            Event::Text(t) | Event::Code(t) => {
                if in_code_block && !include_code {
                    continue;
                }
                let t = t.trim();
                if t.is_empty() {
                    continue;
                }
                if !out.is_empty() && !out.ends_with(' ') && !out.ends_with('\n') {
                    out.push(' ');
                }
                out.push_str(t);
                out.push(' ');
            }
            Event::SoftBreak | Event::HardBreak => {
                out.push('\n');
            }
            _ => {}
        }
    }
    // Las exportaciones de Anki añaden líneas `clave:: valor` (created-at,
    // card-s, due-date…). No son contenido de la nota y no deben aparecer ni
    // contar para el ranking de palabras.
    out.lines()
        .filter(|line| !is_property_line(line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Detecta líneas de metadatos tipo Anki: `clave:: valor`, donde la clave no
/// tiene espacios y solo usa letras, números, guiones o guiones bajos.
pub fn is_property_line(line: &str) -> bool {
    let trimmed = line.trim();
    let Some((key, _value)) = trimmed.split_once("::") else {
        return false;
    };
    let key = key.trim();
    if key.is_empty() || key.len() > 48 || key.contains(char::is_whitespace) {
        return false;
    }
    key.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn clean_markdown_default(input: &str) -> String {
    clean_markdown(input, false)
}

/// Normaliza una palabra: minúsculas y sin acentos (NFD, quita marcas combinantes).
pub fn normalize_word(word: &str) -> String {
    word.chars()
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .nfd()
        .filter(|c| !is_combining_mark(*c))
        .collect()
}

fn is_combining_mark(c: char) -> bool {
    // Combining Diacritical Marks and related blocks.
    matches!(c as u32, 0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
}

/// Tokeniza texto en palabras Unicode.
pub fn tokenize(text: &str) -> Vec<String> {
    text.unicode_words().map(|w| w.to_string()).collect()
}

/// Stopwords por defecto (español + inglés), ya normalizadas.
pub fn default_stopwords() -> Vec<String> {
    let raw = [
        // English
        "the", "a", "an", "and", "or", "but", "if", "then", "else", "when", "at", "by", "for",
        "with", "about", "against", "between", "into", "through", "during", "before", "after",
        "above", "below", "to", "from", "up", "down", "in", "out", "on", "off", "over", "under",
        "again", "further", "once", "here", "there", "where", "why", "how", "all", "any", "both",
        "each", "few", "more", "most", "other", "some", "such", "no", "nor", "not", "only", "own",
        "same", "so", "than", "too", "very", "can", "will", "just", "don", "should", "now", "is",
        "are", "was", "were", "be", "been", "being", "have", "has", "had", "having", "do", "does",
        "did", "doing", "of", "it", "its", "this", "that", "these", "those", "i", "me", "my",
        "myself", "we", "our", "ours", "you", "your", "yours", "he", "him", "his", "she", "her",
        "hers", "they", "them", "their", "theirs", "what", "which", "who", "whom",
        // Español
        "el", "la", "los", "las", "un", "una", "unos", "unas", "y", "o", "pero", "si", "sí",
        "no", "que", "qué", "como", "cómo", "cuando", "cuándo", "donde", "dónde", "por", "para",
        "con", "sin", "sobre", "entre", "hasta", "desde", "hacia", "según", "durante", "mediante",
        "ante", "bajo", "tras", "en", "a", "de", "del", "al", "es", "son", "era", "fue", "ser",
        "estar", "está", "están", "estoy", "estamos", "tengo", "tiene", "tienen", "hay", "ha",
        "han", "he", "haber", "hacer", "hace", "yo", "tú", "tu", "él", "ella", "ello", "nosotros",
        "vosotros", "ellos", "ellas", "mi", "mis", "su", "sus", "nuestro", "nuestra", "este",
        "esta", "estos", "estas", "ese", "esa", "esos", "esas", "aquel", "aquella", "aquellos",
        "aquellas", "más", "mas", "muy", "ya", "también", "tampoco", "solo", "sólo", "así",
        "asi", "todo", "toda", "todos", "todas", "otro", "otra", "otros", "otras", "mismo",
        "misma", "mismos", "mismas", "cada", "algun", "algún", "alguna", "algunos", "algunas",
        "ningun", "ningún", "ninguna", "ningunos", "ningunas", "poco", "poca", "pocos", "pocas",
        "mucho", "mucha", "muchos", "muchas", "bien", "mal", "mejor", "peor", "tan", "tanto",
        "tanta", "tantos", "tantas",
    ];
    raw.iter().map(|w| normalize_word(w)).collect()
}

pub fn is_stopword(word_normalized: &str, stopwords: &[String]) -> bool {
    stopwords.iter().any(|s| s == word_normalized)
}

/// Indica si el token es una URL o número suelto o sintaxis markdown residual.
pub fn is_ignored_token(raw: &str) -> bool {
    let t = raw.trim();
    if t.is_empty() {
        return true;
    }
    // URL
    if t.starts_with("http://") || t.starts_with("https://") || t.starts_with("www.") {
        return true;
    }
    // número suelto
    if t.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ',' || c == '-') && t.chars().any(|c| c.is_ascii_digit()) {
        // si no tiene letras, ignorar
        if !t.chars().any(|c| c.is_alphabetic()) {
            return true;
        }
    }
    false
}

/// Cuenta palabras en un texto ya limpio/plano, devolviendo (normalizado, display, count).
pub fn count_words(text: &str, stopwords: &[String], min_len: usize) -> Vec<(String, String, u32)> {
    let mut map: HashMap<String, (String, u32)> = HashMap::new();
    for raw in tokenize(text) {
        if is_ignored_token(&raw) {
            continue;
        }
        let norm = normalize_word(&raw);
        if norm.len() < min_len {
            continue;
        }
        if norm.chars().count() < min_len {
            continue;
        }
        if is_stopword(&norm, stopwords) {
            continue;
        }
        let entry = map.entry(norm.clone()).or_insert_with(|| (raw.clone(), 0));
        entry.1 += 1;
        // keep first display form
    }
    let mut v: Vec<(String, String, u32)> = map.into_iter().map(|(n, (d, c))| (n, d, c)).collect();
    v.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    v
}

#[cfg(test)]
mod clean {
    use super::*;
    #[test]
    fn removes_headings_and_bold() {
        let md = "# Título\n\nEsto es **negrita** y `code`.\n";
        let out = clean_markdown_default(md);
        assert!(out.contains("Título"));
        assert!(out.contains("negrita"));
        assert!(!out.contains("#"));
        assert!(!out.contains("**"));
    }
    #[test]
    fn omits_code_blocks_by_default() {
        let md = "Texto\n\n```\nsecret code\n```\nFin\n";
        let out = clean_markdown_default(md);
        assert!(!out.contains("secret code"));
        assert!(out.contains("Texto"));
        assert!(out.contains("Fin"));
    }
    #[test]
    fn includes_code_blocks_when_asked() {
        let md = "Texto\n\n```\nsecret code\n```\n";
        let out = clean_markdown(md, true);
        assert!(out.contains("secret code"));
    }
    #[test]
    fn links_text_only() {
        let md = "Ver [aquí](https://example.com) ahora";
        let out = clean_markdown_default(md);
        assert!(out.contains("aquí"));
        assert!(!out.contains("https://example.com"));
    }
    #[test]
    fn drops_anki_property_lines() {
        let md = "\
Pregunta real de la nota

created-at:: 2026-10-26T16:27:01.863Z
card-s:: 4.72
card-d:: 6.04
card-reps:: 2
card-lapses:: 0
last-reviewed:: 2026-10-29T01:12:25.590Z
interval-days:: 5
ease-factor:: 2.4499999999999997
due-date:: 2026-11-03T01:12:25.590Z
card-state:: 2

Otra línea válida
";
        let out = clean_markdown_default(md);
        assert!(out.contains("Pregunta real"));
        assert!(out.contains("Otra línea válida"));
        for key in ["created-at", "card-s", "card-reps", "due-date", "card-state"] {
            assert!(!out.contains(key), "no debe quedar {key}: {out}");
        }
    }
    #[test]
    fn property_detection_requires_key_without_spaces() {
        assert!(is_property_line("card-s:: 4.72"));
        assert!(is_property_line("due-date:: 2026-11-03"));
        assert!(!is_property_line("La respuesta es:: esta"));
        assert!(!is_property_line("texto normal sin metadatos"));
    }
}

#[cfg(test)]
mod tokens {
    use super::*;
    #[test]
    fn tokenizes_unicode() {
        let toks = tokenize("Hola, mundo! café");
        assert!(toks.iter().any(|t| t == "Hola"));
        assert!(toks.iter().any(|t| t == "café"));
    }
    #[test]
    fn normalizes_accents() {
        assert_eq!(normalize_word("Café"), "cafe");
        assert_eq!(normalize_word("NIÑO"), "nino");
    }
}

#[cfg(test)]
mod stopwords {
    use super::*;
    #[test]
    fn filters_spanish_and_english() {
        let sw = default_stopwords();
        assert!(is_stopword("el", &sw));
        assert!(is_stopword("the", &sw));
        assert!(!is_stopword("rust", &sw));
    }
    #[test]
    fn count_words_filters_stopwords_and_short() {
        let sw = default_stopwords();
        let text = "rust rust el the a bc palabra palabra";
        let c = count_words(text, &sw, 3);
        // rust 2, palabra 2; bc ignored (<3), stopwords ignored
        let rust = c.iter().find(|x| x.0 == "rust").unwrap();
        assert_eq!(rust.2, 2);
        let pal = c.iter().find(|x| x.0 == "palabra").unwrap();
        assert_eq!(pal.2, 2);
        assert!(c.iter().all(|x| x.0 != "el"));
    }
    #[test]
    fn ignores_numbers_and_urls() {
        let sw = default_stopwords();
        let text = "ver https://example.com 42 rust";
        let c = count_words(text, &sw, 3);
        assert!(c.iter().any(|x| x.0 == "rust"));
        assert!(c.iter().all(|x| x.0 != "42"));
    }
}
