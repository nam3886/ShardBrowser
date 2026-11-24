//! Sign-up flows per platform, on the automation layer (`web`).

pub mod breezeblue;
pub mod cf;
pub mod tts;

/// Each platform's label and the domains its mail comes from (Python's
/// `sender_has` and pick checks). A mailbox holding mail from one of them
/// already has an account there, or had one made elsewhere.
const MAIL_FROM: &[(&str, &str, &[&str])] = &[
    ("breezeblue", "Breeze Blue", &["breezeblue.ai"]),
    ("fish", "Fish Audio", &["fish.audio"]),
    ("noiz", "Noiz", &["noiz.ai"]),
    ("grok", "Grok", &["x.ai"]),
    ("qwen", "Qwen", &["qwenlm.ai"]),
];

/// The domains `platform`'s mail comes from (none for a platform not listed).
pub fn mail_domains(platform: &str) -> &'static [&'static str] {
    MAIL_FROM
        .iter()
        .find(|(k, _, _)| *k == platform)
        .map_or(&[], |(_, _, d)| d)
}

/// The platform's name for people ("Breeze Blue").
pub fn label(platform: &str) -> &str {
    MAIL_FROM
        .iter()
        .find(|(k, _, _)| *k == platform)
        .map_or(platform, |(_, l, _)| l)
}

/// The run error for a mailbox that already holds the platform's mail
/// (error code ACCOUNT_EXISTS).
pub fn has_mail_error(platform: &str) -> String {
    format!(
        "This email already has {} mail, likely registered elsewhere",
        label(platform)
    )
}
