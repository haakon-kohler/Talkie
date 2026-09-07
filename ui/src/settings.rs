//! The settings window.
//!
//! M0 proved the round trip: read the host's settings, change them, write them
//! back, and see them persist across a restart. M1.5 replaced the free-text
//! accelerator with a real recorder; M3 added the notes-file picker and the
//! microphone dropdown.
//!
//! Labels are synced from `COPY.md` at the repo root, which is the source of
//! truth for every user-visible string.

use leptos::prelude::*;
use leptos::task::spawn_local;
use talkie_shared::{commands, SetSettingsArgs, Settings};

use crate::accessibility::AccessibilitySection;
use crate::ipc;
use crate::model::ModelSection;
use crate::shortcut::ShortcutField;

#[component]
pub fn SettingsPage() -> impl IntoView {
    let settings = RwSignal::new(None::<Settings>);
    let status = RwSignal::new(String::new());
    let microphones = RwSignal::new(Vec::<String>::new());

    Effect::new(move |_| {
        spawn_local(async move {
            match ipc::fetch::<Settings>(commands::GET_SETTINGS).await {
                Ok(loaded) => settings.set(Some(loaded)),
                Err(e) => status.set(format!("Could not read settings: {e}")),
            }
        });
        spawn_local(async move {
            // No list is not an error worth a message: the dropdown still
            // offers the system default, which is also the setting's default.
            if let Ok(devices) = ipc::fetch::<Vec<String>>(commands::LIST_MICROPHONES).await {
                microphones.set(devices);
            }
        });
    });

    let pick_note_file = move |_| {
        spawn_local(async move {
            match ipc::fetch::<Option<String>>(commands::PICK_NOTE_PATH).await {
                // Into the field only — the Save button commits it, the same
                // as a typed path.
                Ok(Some(path)) => settings.update(|s| {
                    if let Some(s) = s {
                        s.note_path = path;
                    }
                }),
                Ok(None) => {}
                // COPY: settings.note_path.picker_error — placeholder
                Err(e) => status.set(format!("Could not open the file picker: {e}")),
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
                        <button class="secondary" on:click=pick_note_file>"Choose…"</button>
                    </div>
                </label>

                // Records and saves itself the moment the keys come up, so it
                // is deliberately outside the Save button's remit.
                <ShortcutField settings=settings />

                <label class="field">
                    // COPY: settings.microphone.label — placeholder
                    <span class="field-label">"Microphone"</span>
                    <select
                        // The empty value is the system default — the setting's
                        // own default, so the dropdown ships on it.
                        prop:value=move || {
                            settings.get().and_then(|s| s.microphone).unwrap_or_default()
                        }
                        on:change=move |ev| {
                            let value = event_target_value(&ev);
                            settings.update(|s| {
                                if let Some(s) = s {
                                    s.microphone = (!value.is_empty()).then_some(value);
                                }
                            });
                        }
                    >
                        // COPY: settings.microphone.default — placeholder
                        <option value="">"System Default"</option>
                        {move || {
                            // A saved microphone that is currently unplugged
                            // still gets its row: the dropdown has to show the
                            // truth of the setting, not silently jump to
                            // something else.
                            let mut names = microphones.get();
                            if let Some(current) = settings.get().and_then(|s| s.microphone) {
                                if !names.contains(&current) {
                                    names.push(current);
                                }
                            }
                            names
                                .into_iter()
                                .map(|name| {
                                    let value = name.clone();
                                    view! { <option value=value>{name}</option> }
                                })
                                .collect_view()
                        }}
                    </select>
                </label>

                // Renders nothing while the permission is in place. It has to
                // be here and not only in onboarding: onboarding runs once,
                // but macOS drops the grant whenever the binary changes.
                <AccessibilitySection on_ready=|| {} />

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
                    <span>"Turn Off Push-to-Talk (Toggle Record)"</span>
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
