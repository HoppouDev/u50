//! The two HTML escape flavors style50 uses: the stdlib `html` module
//! for diff content and markupsafe for template interpolation.

/// Python `html.escape(s, quote=True)`: escapes `&`, `<`, `>`, `"` and
/// `'` (in that order — `&` first, so already-escaped text is never
/// double-escaped).
/// Python `html.escape(s, quote=True)`: escapes `&`, `<`, `>`, `"` and
/// `'` (in that order — `&` first, so already-escaped text is never
/// double-escaped). Used for the diff content (`html_diff`'s `fmt`).
pub(crate) fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

/// `markupsafe.escape` — the template's Jinja autoescape for
/// `{{ file.name }}` / `{{ file.error }}`: same as [`html_escape`] except
/// quotes become `&#34;` / `&#39;` (style50 uses two escape flavors: the
/// stdlib `html` module for the diff, markupsafe for the template).
pub(crate) fn markup_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&#34;")
        .replace('\'', "&#39;")
}
