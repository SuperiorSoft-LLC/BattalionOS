// SPDX-License-Identifier: MPL-2.0
//! BOS-NOTIFY-1: a Windows toast on behalf of an app (`os_notify` on the desktop). WinRT's toast (the approach of the first app's
//! `os_notice.rs`), called directly so a toast can carry a tag: a repeat with the same tag replaces the shown one instead of
//! stacking. Showing never takes the focus; a click runs `on_click` (the caller brings the app forward).

/// The PowerShell app id: a dev run has no registered app id (no installed shortcut), and Windows shows a toast under this one.
pub const POWERSHELL_APP_ID: &str = "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe";

#[cfg(windows)]
pub fn show(app_id: &str, group: &str, attribution: &str, title: &str, text: &str, tag: Option<&str>, on_click: impl Fn() + Send + 'static) -> Result<(), String> {
    use windows::core::{IInspectable, HSTRING};
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::Foundation::TypedEventHandler;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    let xml = format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text><text placement=\"attribution\">{}</text></binding></visual></toast>",
        esc(title),
        esc(text),
        esc(attribution)
    );
    let e = |e: windows::core::Error| e.to_string();
    let doc = XmlDocument::new().map_err(e)?;
    doc.LoadXml(&HSTRING::from(xml)).map_err(e)?;
    let toast = ToastNotification::CreateToastNotification(&doc).map_err(e)?;
    // Tag + group name the toast: a new one with the same pair replaces it (each app is its own group).
    toast.SetGroup(&HSTRING::from(group)).map_err(e)?;
    if let Some(t) = tag {
        toast.SetTag(&HSTRING::from(t)).map_err(e)?;
    }
    toast
        .Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(move |_, _| {
            on_click();
            Ok(())
        }))
        .map_err(e)?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id)).map_err(e)?.Show(&toast).map_err(e)
}

#[cfg(not(windows))]
pub fn show(_app_id: &str, _group: &str, _attribution: &str, _title: &str, _text: &str, _tag: Option<&str>, _on_click: impl Fn() + Send + 'static) -> Result<(), String> {
    Err("desktop notices are Windows only for now".into())
}

/// XML text: the five special characters escaped.
pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    #[test]
    fn text_cannot_break_out_of_the_toast_xml() {
        assert_eq!(super::esc("<a href='x'>&\"</a>"), "&lt;a href=&apos;x&apos;&gt;&amp;&quot;&lt;/a&gt;");
    }
}
