//! The shortcut field: click it, press the keys, done.
//!
//! It is a field rather than a form. There is no Record button, no Save, no
//! Clear — the only interaction is pressing the shortcut you want, and Escape if
//! you change your mind. Everything shown comes from the host's key events, so
//! the field can display a *sided* modifier (right ⌘) that no web keyboard
//! event on its own could be trusted to describe.
//!
//! Prose is synced from `COPY.md` at the repo root.

use leptos::prelude::*;
use leptos::task::spawn_local;
use talkie_shared::{commands, events, SetSettingsArgs, Settings, ShortcutCapture};

use crate::ipc;

/// What the recorder has seen so far in this pass.
#[derive(Clone, Default)]
struct Pending {
    hotkey: String,
    /// A combination that includes a real key beats the modifier-only prefix
    /// that necessarily arrived before it: pressing ⌃⌥Space reports ⌃, then
    /// ⌃⌥, then ⌃⌥Space, and only the last of those is the shortcut.
    has_key: bool,
}

#[component]
pub fn ShortcutField(settings: RwSignal<Option<Settings>>) -> impl IntoView {
    let recording = RwSignal::new(false);
    let preview = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let pending = RwSignal::new(Pending::default());

    // Leaving recording mode is the same three lines from four places, so it is
    // worth a name: tell the host, then put the field back to rest.
    let finish = move || {
        recording.set(false);
        preview.set(String::new());
        pending.set(Pending::default());
        spawn_local(async move {
            let _ = ipc::fire(commands::STOP_SHORTCUT_RECORDING).await;
        });
    };

    let commit = move |hotkey: String| {
        let Some(mut current) = settings.get_untracked() else {
            return;
        };
        if current.shortcut == hotkey {
            finish();
            return;
        }
        current.shortcut = hotkey.clone();

        spawn_local(async move {
            let args = SetSettingsArgs {
                settings: current.clone(),
            };
            // The host validates and re-binds; an unbindable combination comes
            // back as an error and the old shortcut is left alone.
            match ipc::call_void(commands::SET_SETTINGS, &args).await {
                Ok(()) => settings.set(Some(current)),
                Err(e) => error.set(e),
            }
            finish();
        });
    };

    // One subscription for the window's lifetime; it ignores everything unless
    // the field is actually recording.
    ipc::listen::<ShortcutCapture, _>(events::SHORTCUT_CAPTURE, move |event| {
        if !recording.get_untracked() {
            return;
        }

        if event.is_escape && event.is_key_down {
            finish();
            return;
        }

        if !event.hotkey.is_empty() {
            preview.set(event.display.clone());
            let seen = pending.get_untracked();
            if event.has_key || !seen.has_key {
                pending.set(Pending {
                    hotkey: event.hotkey,
                    has_key: event.has_key,
                });
            }
            return;
        }

        // An empty combination on the way up means every key is off the
        // keyboard, which is the moment the shortcut is finished. Waiting for
        // *all* of them — rather than the first release — is what lets a
        // modifier-only shortcut like a held right ⌘ be recorded at all.
        if !event.is_key_down {
            let seen = pending.get_untracked();
            if seen.hotkey.is_empty() {
                finish();
            } else {
                commit(seen.hotkey);
            }
        }
    });

    let start = move |_| {
        if recording.get_untracked() {
            return;
        }
        error.set(String::new());
        preview.set(String::new());
        pending.set(Pending::default());
        spawn_local(async move {
            match ipc::fire(commands::START_SHORTCUT_RECORDING).await {
                Ok(()) => recording.set(true),
                // Almost always the missing Accessibility grant.
                Err(e) => error.set(e),
            }
        });
    };

    let label = move || {
        if recording.get() {
            let live = preview.get();
            return if live.is_empty() {
                // COPY: settings.shortcut.recording — placeholder
                "Press keys…".to_string()
            } else {
                live
            };
        }
        match settings.get().map(|s| s.shortcut) {
            Some(shortcut) if !shortcut.trim().is_empty() => {
                talkie_shared::format_shortcut(&shortcut)
            }
            // COPY: settings.shortcut.empty — placeholder
            _ => "None".to_string(),
        }
    };

    view! {
        <div class="field">
            // COPY: settings.shortcut.label
            <span class="field-label">"Shortcut"</span>
            <button
                type="button"
                class="shortcut-field"
                class:recording=move || recording.get()
                on:click=start
                // While recording, the keys are also going to the focused
                // window. Swallowing them here keeps a recorded ⌘W from
                // closing the settings window on the way past.
                on:keydown=move |ev| {
                    if recording.get() {
                        ev.prevent_default();
                    }
                }
            >
                {label}
            </button>
            <Show when=move || !error.get().is_empty()>
                <span class="status error">{move || error.get()}</span>
            </Show>
        </div>
    }
}
