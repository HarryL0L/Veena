//! Follow the desktop's light or dark setting.
//!
//! GNOME, KDE and other desktops publish it through the XDG desktop portal
//! as `org.freedesktop.appearance color-scheme` (1 dark, 2 light, 0 no
//! preference) and announce changes, so the window can switch the moment
//! the desktop does. Without a portal, Veena stays dark.

use std::future::poll_fn;
use std::pin::Pin;

use mpris_server::zbus::export::futures_core::Stream;
use mpris_server::zbus::{Connection, Proxy};
use mpris_server::zbus::{OwnedVariant, Variant};

const NAMESPACE: &str = "org.freedesktop.appearance";
const KEY: &str = "color-scheme";

pub fn watch(window: slint::Weak<crate::AppWindow>) {
    let _ = slint::spawn_local(async move {
        if let Err(e) = follow(&window).await {
            veena_ffi::app_log(format!("no desktop colour setting: {e}"));
        }
    });
}

async fn follow(window: &slint::Weak<crate::AppWindow>) -> mpris_server::zbus::Result<()> {
    let conn = Connection::session().await?;
    let portal = Proxy::new(
        &conn,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Settings",
    )
    .await?;
    // Listen first, so a change during the first read isn't missed.
    let mut changes = portal.receive_signal("SettingChanged").await?;
    // ReadOne is the current call; older portals only have Read.
    let now: mpris_server::zbus::Result<OwnedVariant> =
        match portal.call("ReadOne", &(NAMESPACE, KEY)).await {
            Ok(v) => Ok(v),
            Err(_) => portal.call("Read", &(NAMESPACE, KEY)).await,
        };
    if let Ok(v) = now {
        apply(window, &v);
    }
    while let Some(message) = poll_fn(|cx| Pin::new(&mut changes).poll_next(cx)).await {
        if let Ok((namespace, key, value)) = message
            .body()
            .deserialize::<(String, String, OwnedVariant)>()
            && namespace == NAMESPACE
            && key == KEY
        {
            apply(window, &value);
        }
    }
    Ok(())
}

fn apply(window: &slint::Weak<crate::AppWindow>, value: &Variant) {
    let Some(scheme) = scheme(value) else { return };
    if let Some(w) = window.upgrade() {
        // No preference counts as dark, Veena's own look.
        w.set_system_dark(scheme != 2);
    }
}

/// The number inside the value (Read wraps it in one more variant).
fn scheme(value: &Variant) -> Option<u32> {
    match value {
        Variant::U32(n) => Some(*n),
        Variant::Variant(inner) => scheme(inner),
        _ => None,
    }
}
