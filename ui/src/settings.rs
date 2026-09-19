//! The settings window.
//!
//! M0 proved the round trip: read the host's settings, change them, write them
//! back, and see them persist across a restart. M1.5 replaced the free-text
//! accelerator with a real recorder; M3 put a native file panel beside the
//! path field and a microphone list under it.
//!
//! Labels are synced from `COPY.md` at the repo root, which is the source of
//! truth for every user-visible string.

use leptos::prelude::*;
use leptos::task::spawn_local;
use talkie_shared::{commands, MicrophoneInfo, SetSettingsArgs, Settings};

use crate::accessibility::AccessibilitySection;
use crate::ipc;
use crate::model::ModelSection;
use crate::shortcut::ShortcutField;

#[component]
pub fn SettingsPage() -> impl IntoView {
    let settings = RwSignal::new(None::<Settings>);
    let status = RwSignal::new(String::new());
    let microphones = RwSignal::new(Vec::<MicrophoneInfo>::new());

    // The device list is a snapshot with nothing to invalidate it — cpal has
    // no device-change notification — so it is taken again every time the
    // list is about to be looked at.
    let refresh_microphones = move || {
        spawn_local(async move {
            match ipc::fetch::<Vec<MicrophoneInfo>>(commands::LIST_MICROPHONES).await {
                Ok(list) => microphones.set(list),
                Err(e) => web_sys::console::warn_1(
                    &format!("talkie: could not list microphones: {e}").into(),
                ),
            }
        });
    };

    Effect::new(move |_| {
        spawn_local(async move {
            match ipc::fetch::<Settings>(commands::GET_SETTINGS).await {
                Ok(loaded) => settings.set(Some(loaded)),
                Err(e) => status.set(format!("Could not read settings: {e}")),
            }
        });
        refresh_microphones();
    });

    // The panel answers with a path and nothing else; the field takes it and
    // Save validates it exactly as it would a typed one.
    let choose_note_path = move |_| {
        spawn_local(async move {
            match ipc::fetch::<Option<String>>(commands::PICK_NOTE_PATH).await {
                Ok(Some(path)) => {
                    settings.update(|s| {
                        if let Some(s) = s {
                            s.note_path = path;
                        }
                    });
                }
                Ok(None) => {}
                Err(e) => status.set(format!("Could not open the file panel: {e}")),
            }
        });
    };

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
                    // COPY: settings.note_path.label
                    <span class="field-label">"Notes file"</span>
                    <div class="field-row">
                        <input
                            type="text"
                            prop:value=move || settings.get().map(|s| s.note_path).unwrap_or_default()
                            on:input=move |ev| {
                                let value = event_target_value(&ev);
                                settings.update(|s| { if let Some(s) = s { s.note_path = value; } });
                            }
                        />
                        // COPY: settings.note_path.browse — placeholder
                        <button type="button" class="secondary" on:click=choose_note_path>"Choose…"</button>
                    </div>
                </label>

                // Records and saves itself the moment the keys come up, so it
                // is deliberately outside the Save button's remit.
                <ShortcutField settings=settings />

                // Renders nothing while the permission is in place. It has to
                // be here and not only in onboarding: onboarding runs once,
                // but macOS drops the grant whenever the binary changes.
                <AccessibilitySection on_ready=|| {} />

                <label class="field">
                    // COPY: settings.microphone.label — placeholder
                    <span class="field-label">"Microphone"</span>
                    <select
                        on:focus=move |_| refresh_microphones()
                        on:pointerenter=move |_| refresh_microphones()
                        on:change=move |ev| {
                            let value = event_target_value(&ev);
                            let chosen = (!value.is_empty()).then_some(value);
                            settings.update(|s| { if let Some(s) = s { s.microphone = chosen; } });
                        }
                    >
                        {move || {
                            let chosen = settings.get().and_then(|s| s.microphone);
                            let list = microphones.get();
                            let mut options = vec![view! {
                                // COPY: settings.microphone.default — placeholder
                                <option value="" selected=chosen.is_none()>"System default"</option>
                            }.into_any()];
                            for mic in &list {
                                let selected = chosen.as_deref() == Some(mic.name.as_str());
                                options.push(view! {
                                    <option value=mic.name.clone() selected=selected>{mic.name.clone()}</option>
                                }.into_any());
                            }
                            // A stored microphone that is not plugged in right
                            // now stays visible, marked, rather than silently
                            // reading as the default while the store says
                            // otherwise. The recorder falls back on its own.
                            if let Some(name) = chosen.filter(|name| !list.iter().any(|m| &m.name == name)) {
                                options.push(view! {
                                    // COPY: settings.microphone.missing — placeholder
                                    <option value=name.clone() selected=true>{format!("{name} (not connected)")}</option>
                                }.into_any());
                            }
                            options
                        }}
                    </select>
                </label>

                // Inverted on purpose: push-to-talk is the default, so the box
                // is the way *out* of it and ships unchecked. The stored field
                // is still `push_to_talk` — only the control reads backwards.
                <label class="toggle">
                    <input
                        type="checkbox"
                        prop:checked=move || settings.get().map(|s| !s.push_to_talk).unwrap_or(false)
                        on:change=move |ev| {
                            let value = event_target_checked(&ev);
                            settings.update(|s| { if let Some(s) = s { s.push_to_talk = !value; } });
                        }
                    />
                    // COPY: settings.push_to_talk.label
                    <span>"Toggle Record (Turn Off Push-to-Talk)"</span>
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
