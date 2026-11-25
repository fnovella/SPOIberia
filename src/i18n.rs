use clap::ValueEnum;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    Es,
    En,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LangArg {
    /// Detecta automáticamente el idioma del sistema operativo / Auto-detect OS language
    Auto,
    /// Español
    Es,
    /// English
    En,
}

impl Lang {
    pub fn resolve(arg: LangArg) -> Self {
        match arg {
            LangArg::Auto => Self::detect_system(),
            LangArg::Es => Lang::Es,
            LangArg::En => Lang::En,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Lang::Es => "es",
            Lang::En => "en",
        }
    }

    #[inline]
    pub fn tr<'a>(self, es: &'a str, en: &'a str) -> &'a str {
        match self {
            Lang::Es => es,
            Lang::En => en,
        }
    }

    /// Parse a locale / language tag (e.g. `es_ES.UTF-8`, `en-US`, `"es-ES"`).
    /// Returns `None` for empty or neutral `C`/`POSIX` locales.
    pub fn from_locale_str(raw: &str) -> Option<Self> {
        let cleaned = raw
            .trim()
            .trim_matches(|c| c == '"' || c == '\'' || c == '(' || c == ')')
            .trim();
        if cleaned.is_empty() {
            return None;
        }
        let first_entry = cleaned.split(',').next().unwrap_or(cleaned).trim();
        let without_encoding = first_entry
            .split(['.', '@'])
            .next()
            .unwrap_or(first_entry)
            .trim();
        let lang_code = without_encoding
            .split(['-', '_'])
            .next()
            .unwrap_or(without_encoding)
            .trim()
            .to_ascii_lowercase();

        if lang_code.is_empty() || lang_code == "c" || lang_code == "posix" {
            None
        } else if lang_code == "es" {
            Some(Lang::Es)
        } else {
            Some(Lang::En)
        }
    }

    pub fn detect_system() -> Self {
        for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(val) = std::env::var(var) {
                if let Some(lang) = Self::from_locale_str(&val) {
                    return lang;
                }
            }
        }

        #[cfg(target_os = "windows")]
        {
            if let Some(lang) = detect_windows_locale() {
                return lang;
            }
        }

        #[cfg(target_os = "macos")]
        {
            if let Some(lang) = detect_macos_locale() {
                return lang;
            }
        }

        Lang::En
    }
}

#[cfg(target_os = "windows")]
fn detect_windows_locale() -> Option<Lang> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetUserDefaultLocaleName(lpLocaleName: *mut u16, cchLocaleName: i32) -> i32;
    }
    let mut buf = [0u16; 85];
    let len = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
    if len > 1 {
        let raw = String::from_utf16_lossy(&buf[..(len as usize).saturating_sub(1)]);
        return Lang::from_locale_str(&raw);
    }
    None
}

#[cfg(target_os = "macos")]
fn detect_macos_locale() -> Option<Lang> {
    let out = std::process::Command::new("defaults")
        .args(["read", "-g", "AppleLanguages"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    for line in stdout.lines() {
        let t = line.trim().trim_matches([',', '"', '\'', '(', ')']);
        if let Some(lang) = Lang::from_locale_str(t) {
            return Some(lang);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_locale_str() {
        assert_eq!(Lang::from_locale_str("es_ES.UTF-8"), Some(Lang::Es));
        assert_eq!(Lang::from_locale_str("es-419"), Some(Lang::Es));
        assert_eq!(Lang::from_locale_str("ES"), Some(Lang::Es));
        assert_eq!(Lang::from_locale_str("en_US.UTF-8"), Some(Lang::En));
        assert_eq!(Lang::from_locale_str("fr_FR.UTF-8"), Some(Lang::En));
        assert_eq!(Lang::from_locale_str("C.UTF-8"), None);
        assert_eq!(Lang::from_locale_str("POSIX"), None);
        assert_eq!(Lang::from_locale_str(""), None);
    }

    #[test]
    fn test_resolve_explicit() {
        assert_eq!(Lang::resolve(LangArg::Es), Lang::Es);
        assert_eq!(Lang::resolve(LangArg::En), Lang::En);
        assert_eq!(Lang::Es.tr("Hola", "Hello"), "Hola");
        assert_eq!(Lang::En.tr("Hola", "Hello"), "Hello");
    }
}
