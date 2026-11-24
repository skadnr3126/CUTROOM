pub(super) fn filename(name: &str, number: usize) -> String {
    let clean = name.replace(|c: char| c.is_control() || r#"<>:"/\|?*"#.contains(c), "_");
    let mut clean = clean.trim_end_matches(['.', ' ']).to_owned();
    if clean.is_empty() {
        clean.push('_');
    }
    let device = clean
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_ascii_uppercase();
    if matches!(
        device.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        device
            .strip_prefix(prefix)
            .is_some_and(|n| n.chars().count() == 1 && "123456789¹²³".contains(n))
    }) {
        clean.insert(0, '_');
    }
    let (stem, extension) = clean
        .rfind('.')
        .filter(|i| *i > 0)
        .map_or((clean.as_str(), ""), |i| clean.split_at(i));
    let suffix = if number == 1 {
        String::new()
    } else {
        format!(" ({number})")
    };
    fn truncate(s: &str, mut units: usize) -> &str {
        for (i, c) in s.char_indices() {
            if c.len_utf16() > units {
                return &s[..i];
            }
            units -= c.len_utf16();
        }
        s
    }
    let extension = truncate(extension, 254 - suffix.len());
    let stem = truncate(stem, 255 - extension.encode_utf16().count() - suffix.len());
    format!("{stem}{suffix}{extension}")
}
