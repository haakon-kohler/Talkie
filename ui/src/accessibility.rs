//! The Accessibility grant, shared by first run and settings.
//!
//! Onboarding is not the only way to get here, and it is the way *nobody* who
//! already ran Talkie can get here: onboarding runs once. The permission can
//! also be revoked in System Settings, and macOS drops it by itself whenever the
//! binary changes — which for a dev build is every rebuild. So this lives in one
//! component that both surfaces mount, and it always reflects what macOS
//! actually says rather than what onboarding believes happened.
//!
//! When the permission is granted it renders nothing at all, which is what keeps
//! it out of the settings window in the ordinary case.
//!
//! Prose is synced from `COPY.md` at the repo root.

use std::time::Duration;

use leptos::leptos_dom::helpers::{set_interval_with_handle, IntervalHandle};
use leptos::prelude::*;
use leptos::task::spawn_local;
use talkie_shared::commands;

use crate::ipc;

/// How often to ask macOS whether the switch has been flipped yet.
const POLL: Duration = Duration::from_millis(700);

/// A short explanation and one button, or nothing when the grant is in place.
///
/// `on_ready` fires once the permission is there — onboarding uses it to advance
/// a step; settings passes a no-op and simply lets the section disappear.
#[component]
pub fn AccessibilitySection<F>(on_ready: F) -> impl IntoView
where
    // `Send + Sync` beyond what `ModelSection` asks for, because here the
    // callback is reached from inside the view: Leptos requires children to be
    // both.
    F: Fn() + Copy + Send + Sync + 'static,
{
    let granted = RwSignal::new(None::<bool>);
    let error = RwSignal::new(String::new());
    let poll = StoredValue::new(None::<IntervalHandle>);

    // Everything that happens the moment the permission appears, from either
    // route: stop asking, re-bind the shortcut the engine gave up on at
    // start-up, and tell whoever mounted us.
    let settle = move || {
        if let Some(handle) = poll.get_value() {
            handle.clear();
            poll.set_value(None);
        }
        granted.set(Some(true));
        spawn_local(async move {
            if let Err(e) = ipc::fire(commands::RETRY_SHORTCUT).await {
                error.set(e);
            }
        });
        on_ready();
    };

    Effect::new(move |_| {
        spawn_local(async move {
            match ipc::fetch::<bool>(commands::GET_ACCESSIBILITY).await {
                // Already granted at mount: nothing to re-bind, the engine will
                // have managed that at start-up.
                Ok(true) => {
                    granted.set(Some(true));
                    on_ready();
                }
                Ok(false) => granted.set(Some(false)),
                Err(e) => error.set(e),
            }
        });
    });

    // macOS grants Accessibility in System Settings, not in a dialog we can
    // await, so the only way to know it happened is to keep asking.
    let grant = move |_| {
        error.set(String::new());
        spawn_local(async move {
            if let Err(e) = ipc::fire(commands::OPEN_ACCESSIBILITY_SETTINGS).await {
                error.set(e);
                return;
            }
            if poll.get_value().is_some() {
                return;
            }
            let started = set_interval_with_handle(
                move || {
                    spawn_local(async move {
                        if let Ok(true) = ipc::fetch::<bool>(commands::GET_ACCESSIBILITY).await {
                            settle();
                        }
                    });
                },
                POLL,
            );
            match started {
                Ok(handle) => poll.set_value(Some(handle)),
                Err(e) => error.set(format!("could not watch for the grant: {e:?}")),
            }
        });
    };

    on_cleanup(move || {
        if let Some(handle) = poll.get_value() {
            handle.clear();
        }
    });

    view! {
        <Show when=move || granted.get() == Some(false)>
            <div class="accessibility-section">
                // COPY: onboarding.accessibility.body
                <p class="muted">
                    "This setting allows Talkie to use specific modifier keys as your shortcut
                    button (like the right Option key). We don't look at any information in
                    other apps."
                </p>
                <div class="actions">
                    <span class="status error">{move || error.get()}</span>
                    <button class="primary" on:click=grant>
                        // COPY: onboarding.accessibility.cta
                        "Allow Accessibility"
                    </button>
                </div>
            </div>
        </Show>
    }
}
