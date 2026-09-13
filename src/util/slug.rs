/// Lower-case, non-alphanumerics collapsed to `-`, trimmed. Identical to mcbackup's
/// `slugify`, so a server's mcplug id and its mcbackup source name always agree.
pub fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "unnamed".into()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn matches_mcbackup() {
        assert_eq!(super::slugify("Labubu SMP"), "labubu-smp");
        assert_eq!(super::slugify("Labubu SMP (old world)"), "labubu-smp-old-world");
        assert_eq!(super::slugify("__MCSM_GLOBAL_INSTANCE__"), "mcsm-global-instance");
        assert_eq!(super::slugify("!!!"), "unnamed");
    }
}
