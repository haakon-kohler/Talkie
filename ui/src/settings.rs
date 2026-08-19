//! The settings window.
//!
//! M0 proves the round trip: read the host's settings, change them, write them
//! back, and see them persist across a restart. M3 turns this into the real
//! form — a shortcut recorder, a file picker, a microphone list.
//!
//! Labels are synced from `COPY.md` at the repo root, which is the source of
//! truth for every user-visible string.

use leptos::prelude::*;
use leptos::task::spawn_local;
use talkie_shared::{commands, SetSettingsArgs, Settings};

use crate::ipc;
use crate::model::ModelSection;

#[component]
pub fn SettingsPage() -> impl IntoView {
    let settings = RwSignal::new(None::<Settings>);
    let status = RwSignal::new(String::new());

    Effect::new(move |_| {
        spawn_local(async move {
            match ipc::fetch::<Settings>(commands::GET_SETTINGS).await {
                Ok(loaded) => settings.set(Some(loaded)),
                Err(e) => status.set(format!("Could not read settings: {e}")),
            }
        });
    });

    let save = move |_| {
        let Some(current) = settings.get_untracked() else {
            return;
        };
        spawn_local(async move {
            let args = SetSettingsArgs { settings: current };
            match ipc::call_void(commands::SET_SETTINGS, &args).await {
                Ok(()) => status.set("Saved.".to_string()),
                Err(e) => status.set(format!("Could not save: {e}")),
            }
        });
    };

    view! {
        <div class="page settings-page">
            <h1>"Settings"</h1>

            <Show
                when=move || settings.get().is_some()
                fallback=|| view! { <p class="muted">"Loading…"</p> }
            >
                <label class="field">
                    <span class="field-label">"Notes file"</span>
                    <input
                        type="text"
                        prop:value=move || settings.get().map(|s| s.note_path).unwrap_or_default()
                        on:input=move |ev| {
                            let value = event_target_value(&ev);
                            settings.update(|s| { if let Some(s) = s { s.note_path = value; } });
                        }
                    />
                </label>

                <label class="field">
                    <span class="field-label">"Shortcut"</span>
                    <input
                        type="text"
                        prop:value=move || settings.get().map(|s| s.shortcut).unwrap_or_default()
                        on:input=move |ev| {
                            let value = event_target_value(&ev);
                            settings.update(|s| { if let Some(s) = s { s.shortcut = value; } });
                        }
                    />
                </label>

                <label class="toggle">
                    <input
                        type="checkbox"
                        prop:checked=move || settings.get().map(|s| s.push_to_talk).unwrap_or(false)
                        on:change=move |ev| {
                            let value = event_target_checked(&ev);
                            settings.update(|s| { if let Some(s) = s { s.push_to_talk = value; } });
                        }
                    />
                    <span>"Toggle Push-to-Talk (Push Twice Instead Of Tap-and-Hold)"</span>
                </label>

                <label class="toggle">
                    <input
                        type="checkbox"
                        prop:checked=move || settings.get().map(|s| s.play_sounds).unwrap_or(true)
                        on:change=move |ev| {
                            let value = event_target_checked(&ev);
                            settings.update(|s| { if let Some(s) = s { s.play_sounds = value; } });
                        }
                    />
                    <span>"Play Sound When Recording Starts/Stops"</span>
                </label>

                <label class="toggle">
                    <input
                        type="checkbox"
                        prop:checked=move || settings.get().map(|s| s.launch_at_login).unwrap_or(false)
                        on:change=move |ev| {
                            let value = event_target_checked(&ev);
                            settings.update(|s| { if let Some(s) = s { s.launch_at_login = value; } });
                        }
                    />
                    <span>"Start at Login"</span>
                </label>

                <div class="actions">
                    <span class="status">{move || status.get()}</span>
                    <button class="primary" on:click=save>"Save"</button>
                </div>

                <section class="section">
                    // COPY: settings.model.label
                    <h2>"Speech model"</h2>
                    // Onboarding is not the only route to the model: it can be
                    // skipped, and the directory can go missing later. This is
                    // the place to get it back.
                    <ModelSection on_ready=|| {} />
                </section>
            </Show>
        </div>
    }
}
