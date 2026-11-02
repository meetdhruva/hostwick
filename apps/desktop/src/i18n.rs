//! Translations: every user-visible string of the desktop app comes from a
//! Fluent file in `apps/desktop/locales/<code>.ftl`, looked up with `t!`.
//!
//! ```ignore
//! t!("tab-close")                                  // plain message
//! t!("files-selected", count = 3)                  // plurals: `{ $count -> [one] … *[other] … }`
//! t!("host-delete-confirm", name = &profile.name)  // named placeholders: `{ $name }`
//! ```
//!
//! English (`en.ftl`) is the source; a message missing from another language
//! falls back to English. The tests at the bottom keep every language
//! complete: same keys, same placeholders, no empty strings, and every `t!`
//! key in the source exists.
//!
//! The current language is process-wide (prompts are composed on network
//! threads too). Switching it re-renders the windows; text already baked into
//! an open dialog changes when the dialog is opened again.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use gpui_kit::SharedString;
use unic_langid::LanguageIdentifier;

/// A UI language: its code (as stored in `Settings.language`), its name in
/// its own script (for the picker) and its messages.
pub struct Language {
    pub code: &'static str,
    pub native_name: &'static str,
    source: &'static str,
}

/// English first: it is the source and the fallback.
pub const LANGUAGES: &[Language] = &[
    Language {
        code: "en",
        native_name: "English",
        source: include_str!("../locales/en.ftl"),
    },
    Language {
        code: "de",
        native_name: "Deutsch",
        source: include_str!("../locales/de.ftl"),
    },
    Language {
        code: "es",
        native_name: "Español",
        source: include_str!("../locales/es.ftl"),
    },
    Language {
        code: "fr",
        native_name: "Français",
        source: include_str!("../locales/fr.ftl"),
    },
    Language {
        code: "pt-BR",
        native_name: "Português (Brasil)",
        source: include_str!("../locales/pt-BR.ftl"),
    },
    Language {
        code: "hi",
        native_name: "हिन्दी",
        source: include_str!("../locales/hi.ftl"),
    },
    Language {
        code: "zh-CN",
        native_name: "简体中文",
        source: include_str!("../locales/zh-CN.ftl"),
    },
    Language {
        code: "ja",
        native_name: "日本語",
        source: include_str!("../locales/ja.ftl"),
    },
];

/// Index into [`LANGUAGES`] of the language in use.
static CURRENT: AtomicUsize = AtomicUsize::new(0);

fn bundles() -> &'static [FluentBundle<FluentResource>] {
    static BUNDLES: OnceLock<Vec<FluentBundle<FluentResource>>> = OnceLock::new();
    BUNDLES.get_or_init(|| LANGUAGES.iter().map(build_bundle).collect())
}

fn build_bundle(lang: &Language) -> FluentBundle<FluentResource> {
    let id: LanguageIdentifier = lang.code.parse().unwrap_or_default();
    let mut bundle = FluentBundle::new_concurrent(vec![id]);
    // No Unicode isolation marks around placeholders: GPUI would draw them.
    bundle.set_use_isolating(false);
    let resource = match FluentResource::try_new(lang.source.to_owned()) {
        Ok(r) => r,
        Err((r, errors)) => {
            tracing::warn!(lang = lang.code, ?errors, "translation file has errors");
            r
        }
    };
    if let Err(errors) = bundle.add_resource(resource) {
        tracing::warn!(lang = lang.code, ?errors, "duplicate translation keys");
    }
    bundle
}

/// The language code to use for a `Settings.language` value: the setting
/// itself when it names a known language, else the OS language (`""`).
pub fn resolve(setting: &str) -> &'static str {
    let wanted = if setting.trim().is_empty() {
        sys_locale::get_locale().unwrap_or_default()
    } else {
        setting.trim().to_owned()
    };
    match_locale(&wanted).unwrap_or("en")
}

/// The closest supported language for a BCP 47 / POSIX locale name
/// (`de-AT`, `pt_PT.UTF-8`, `zh-Hans-CN`), if any.
pub fn match_locale(locale: &str) -> Option<&'static str> {
    let locale = locale
        .split(['.', '@'])
        .next()
        .unwrap_or_default()
        .replace('_', "-")
        .to_ascii_lowercase();
    if let Some(l) = LANGUAGES
        .iter()
        .find(|l| l.code.to_ascii_lowercase() == locale)
    {
        return Some(l.code);
    }
    let primary = locale.split('-').next().unwrap_or_default();
    match primary {
        // Every Portuguese and Chinese variant gets the one we have.
        "pt" => Some("pt-BR"),
        "zh" => Some("zh-CN"),
        _ => LANGUAGES.iter().find(|l| l.code == primary).map(|l| l.code),
    }
}

/// Switch the UI language (a `Settings.language` value). Returns the code
/// now in use.
pub fn set_language(setting: &str) -> &'static str {
    let code = resolve(setting);
    let ix = LANGUAGES.iter().position(|l| l.code == code).unwrap_or(0);
    CURRENT.store(ix, Ordering::Relaxed);
    // The widget library's own few strings (text-field menus, "Show
    // password"); it has pt-BR and zh-CN and falls back to English.
    gpui_kit::component::set_locale(code);
    code
}

/// The code of the language in use.
pub fn current() -> &'static str {
    LANGUAGES
        .get(CURRENT.load(Ordering::Relaxed))
        .map_or("en", |l| l.code)
}

/// A placeholder value for [`tr_args`]; numbers select plural forms.
#[derive(Debug, Clone)]
pub enum Arg {
    Str(String),
    Num(f64),
}

impl From<&str> for Arg {
    fn from(s: &str) -> Self {
        Arg::Str(s.to_owned())
    }
}
impl From<&&str> for Arg {
    fn from(s: &&str) -> Self {
        Arg::Str((*s).to_owned())
    }
}
impl From<String> for Arg {
    fn from(s: String) -> Self {
        Arg::Str(s)
    }
}
impl From<&String> for Arg {
    fn from(s: &String) -> Self {
        Arg::Str(s.clone())
    }
}
impl From<SharedString> for Arg {
    fn from(s: SharedString) -> Self {
        Arg::Str(s.to_string())
    }
}
impl From<&SharedString> for Arg {
    fn from(s: &SharedString) -> Self {
        Arg::Str(s.to_string())
    }
}
macro_rules! num_arg {
    ($($t:ty),*) => {$(
        impl From<$t> for Arg {
            #[allow(clippy::cast_precision_loss, clippy::cast_lossless)]
            fn from(n: $t) -> Self {
                Arg::Num(n as f64)
            }
        }
    )*};
}
num_arg!(u8, u16, u32, u64, usize, i32, i64, f32, f64);

thread_local! {
    /// Formatted argument-less messages for the current language.
    static CACHE: RefCell<(usize, HashMap<&'static str, SharedString>)> =
        RefCell::new((usize::MAX, HashMap::new()));
}

fn format(ix: usize, key: &str, args: Option<&FluentArgs<'_>>) -> Option<String> {
    let bundle = bundles().get(ix)?;
    let pattern = bundle.get_message(key)?.value()?;
    let mut errors = Vec::new();
    let text = bundle.format_pattern(pattern, args, &mut errors);
    if !errors.is_empty() {
        tracing::debug!(key, ?errors, "translation formatting problems");
    }
    Some(text.into_owned())
}

fn lookup(key: &str, args: Option<&FluentArgs<'_>>) -> String {
    let ix = CURRENT.load(Ordering::Relaxed);
    format(ix, key, args)
        .or_else(|| format(0, key, args))
        .unwrap_or_else(|| {
            tracing::warn!(key, "missing translation key");
            key.to_owned()
        })
}

/// The message `key` in the current language. Use [`t!`](crate::t).
pub fn tr(key: &'static str) -> SharedString {
    let ix = CURRENT.load(Ordering::Relaxed);
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.0 != ix {
            *c = (ix, HashMap::new());
        }
        c.1.entry(key)
            .or_insert_with(|| SharedString::from(lookup(key, None)))
            .clone()
    })
}

/// The message `key` with placeholders filled in. Use [`t!`](crate::t).
pub fn tr_args(key: &'static str, args: &[(&'static str, Arg)]) -> SharedString {
    let mut fa = FluentArgs::new();
    for (name, value) in args {
        match value {
            Arg::Str(s) => fa.set(*name, FluentValue::from(s.clone())),
            Arg::Num(n) => fa.set(*name, FluentValue::from(*n)),
        }
    }
    SharedString::from(lookup(key, Some(&fa)))
}

/// `t!("key")` or `t!("key", name = value, count = n)`: a translated
/// [`SharedString`]. Keys and placeholder names must exist in `en.ftl`
/// (checked by the tests).
#[macro_export]
macro_rules! t {
    ($key:literal) => {
        $crate::i18n::tr($key)
    };
    ($key:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::tr_args(
            $key,
            &[$((stringify!($name), $crate::i18n::Arg::from($value))),+],
        )
    };
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use fluent_syntax::ast;

    use super::*;

    /// Message id → (placeholder names, message text) of one .ftl file.
    fn messages(source: &str) -> BTreeMap<String, (BTreeSet<String>, String)> {
        let res = fluent_syntax::parser::parse(source).unwrap_or_else(|(_, errors)| {
            panic!("parse errors: {errors:?}");
        });
        let mut out = BTreeMap::new();
        for entry in res.body {
            if let ast::Entry::Message(m) = entry {
                assert!(
                    m.attributes.is_empty(),
                    "{}: attributes are not used",
                    m.id.name
                );
                let pattern = m
                    .value
                    .unwrap_or_else(|| panic!("{} has no value", m.id.name));
                let mut vars = BTreeSet::new();
                let mut text = String::new();
                collect(&pattern, &mut vars, &mut text);
                let dup = out.insert(m.id.name.to_owned(), (vars, text));
                assert!(dup.is_none(), "duplicate key {}", m.id.name);
            }
        }
        out
    }

    fn collect(p: &ast::Pattern<&str>, vars: &mut BTreeSet<String>, text: &mut String) {
        for el in &p.elements {
            match el {
                ast::PatternElement::TextElement { value } => text.push_str(value),
                ast::PatternElement::Placeable { expression } => expr(expression, vars, text),
            }
        }
    }

    fn expr(e: &ast::Expression<&str>, vars: &mut BTreeSet<String>, text: &mut String) {
        match e {
            ast::Expression::Inline(i) => inline(i, vars, text),
            ast::Expression::Select { selector, variants } => {
                inline(selector, vars, text);
                for v in variants {
                    collect(&v.value, vars, text);
                }
            }
        }
    }

    fn inline(i: &ast::InlineExpression<&str>, vars: &mut BTreeSet<String>, text: &mut String) {
        match i {
            ast::InlineExpression::VariableReference { id } => {
                vars.insert(id.name.to_owned());
            }
            ast::InlineExpression::StringLiteral { value } => text.push_str(value),
            ast::InlineExpression::Placeable { expression } => expr(expression, vars, text),
            ast::InlineExpression::FunctionReference { arguments, .. } => {
                for a in &arguments.positional {
                    inline(a, vars, text);
                }
            }
            ast::InlineExpression::MessageReference { id, .. } => {
                panic!("message references are not used ({})", id.name)
            }
            _ => {}
        }
    }

    #[test]
    fn every_language_is_complete_and_consistent() {
        let en = messages(LANGUAGES[0].source);
        assert!(en.len() > 100, "en.ftl looks empty");
        for lang in LANGUAGES {
            let m = messages(lang.source);
            let missing: Vec<_> = en.keys().filter(|k| !m.contains_key(*k)).collect();
            assert!(missing.is_empty(), "{}: missing {missing:?}", lang.code);
            let extra: Vec<_> = m.keys().filter(|k| !en.contains_key(*k)).collect();
            assert!(extra.is_empty(), "{}: unknown keys {extra:?}", lang.code);
            for (key, (vars, text)) in &m {
                let (en_vars, _) = &en[key];
                assert_eq!(
                    vars, en_vars,
                    "{}: placeholders of {key} differ from English",
                    lang.code
                );
                assert!(
                    !text.trim().is_empty() || !vars.is_empty(),
                    "{}: {key} is empty",
                    lang.code
                );
            }
        }
    }

    #[test]
    fn every_language_formats_every_message() {
        for (ix, lang) in LANGUAGES.iter().enumerate() {
            let bundle = &bundles()[ix];
            for (key, (vars, _)) in messages(lang.source) {
                let mut args = FluentArgs::new();
                for v in &vars {
                    args.set(v.clone(), FluentValue::from(2));
                }
                let msg = bundle
                    .get_message(&key)
                    .and_then(|m| m.value())
                    .unwrap_or_else(|| panic!("{}: {key} not loaded", lang.code));
                let mut errors = Vec::new();
                let out = bundle.format_pattern(msg, Some(&args), &mut errors);
                assert!(errors.is_empty(), "{}: {key}: {errors:?}", lang.code);
                assert!(!out.trim().is_empty(), "{}: {key} formats empty", lang.code);
            }
        }
    }

    /// Every `t!("key", name = …)` in the source names a key (and
    /// placeholders) that exist in English, and every English key is used.
    #[test]
    fn source_keys_exist() {
        let en = messages(LANGUAGES[0].source);
        let re = regex::Regex::new(r#"t!\(\s*"([a-z0-9-]+)"((?:\s*,\s*[a-z_]+\s*=)?)"#)
            .unwrap_or_else(|e| panic!("{e}"));
        let arg_re = regex::Regex::new(r"[,(]\s*([a-z_][a-z0-9_]*)\s*=[^=]")
            .unwrap_or_else(|e| panic!("{e}"));
        let mut used = BTreeSet::new();
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut stack = vec![src];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{e}")) {
                let path = entry.unwrap_or_else(|e| panic!("{e}")).path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") || path.ends_with("i18n.rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}"));
                for cap in re.captures_iter(&text) {
                    let key = &cap[1];
                    let (vars, _) = en
                        .get(key)
                        .unwrap_or_else(|| panic!("{}: unknown key {key}", path.display()));
                    // The placeholders passed: up to the macro's closing paren.
                    let start = cap.get(0).map_or(0, |m| m.start());
                    let call = macro_call(&text[start..]);
                    let passed: BTreeSet<String> = arg_re
                        .captures_iter(call)
                        .map(|c| c[1].to_owned())
                        .collect();
                    for v in vars {
                        assert!(
                            passed.contains(v),
                            "{}: t!(\"{key}\") does not pass ${v}",
                            path.display()
                        );
                    }
                    used.insert(key.to_owned());
                }
            }
        }
        let unused: Vec<_> = en.keys().filter(|k| !used.contains(*k)).collect();
        assert!(unused.is_empty(), "keys not used in the source: {unused:?}");
    }

    /// `t!( … )` up to its matching parenthesis.
    fn macro_call(text: &str) -> &str {
        let mut depth = 0;
        for (i, c) in text.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return &text[..=i];
                    }
                }
                _ => {}
            }
        }
        text
    }

    #[test]
    fn locales_map_to_supported_languages() {
        assert_eq!(match_locale("de-AT"), Some("de"));
        assert_eq!(match_locale("de_DE.UTF-8"), Some("de"));
        assert_eq!(match_locale("pt-PT"), Some("pt-BR"));
        assert_eq!(match_locale("pt-BR"), Some("pt-BR"));
        assert_eq!(match_locale("zh-Hans-CN"), Some("zh-CN"));
        assert_eq!(match_locale("ja-JP"), Some("ja"));
        assert_eq!(match_locale("hi-IN"), Some("hi"));
        assert_eq!(match_locale("fr"), Some("fr"));
        assert_eq!(match_locale("ko-KR"), None);
        assert_eq!(resolve("es"), "es");
        assert_eq!(resolve("xx"), "en");
    }

    #[test]
    fn plurals_and_placeholders() {
        // Plural rules come from each language (French: 0 and 1 are "one").
        let fr = LANGUAGES.iter().position(|l| l.code == "fr").unwrap_or(0);
        let mut args = FluentArgs::new();
        args.set("count", FluentValue::from(0));
        let zero_fr = format(fr, "files-selected", Some(&args)).unwrap_or_default();
        let zero_en = format(0, "files-selected", Some(&args)).unwrap_or_default();
        args.set("count", FluentValue::from(1));
        let one_en = format(0, "files-selected", Some(&args)).unwrap_or_default();
        assert_ne!(zero_en, one_en.replace('1', "0"));
        assert!(zero_fr.contains('0'));
        let s = tr_args("files-selected", &[("count", Arg::from(5usize))]);
        assert!(s.contains('5'));
        assert_eq!(tr("no-such-key-for-test"), "no-such-key-for-test");
    }
}
