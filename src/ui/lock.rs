//! The screens of a locked page (see lock.rs): the box that gives a page
//! its password, and what a locked page shows in place of its lines.

use super::icons::Icon;
use crate::lock::{LockError, MIN_PASSWORD};
use crate::state::AppState;
use dioxus::prelude::*;

/// Lock the page in front with a password: typed twice, with what it means.
#[component]
pub fn LockPageModal() -> Element {
    let state = use_context::<AppState>();
    let page = use_signal(|| state.current_page());
    let mut first = use_signal(String::new);
    let mut again = use_signal(String::new);
    let mut working = use_signal(|| false);
    let Some(page_id) = page() else { return rsx! {} };
    let title = state.page_title(&page_id);
    let long_enough = first.read().chars().count() >= MIN_PASSWORD;
    let same = *first.read() == *again.read();
    let ready = long_enough && same && !working();
    let problem = if !again.read().is_empty() && !same {
        Some("The two passwords are not the same.")
    } else if !first.read().is_empty() && !long_enough {
        Some("Use at least 6 characters.")
    } else {
        None
    };
    let mut go = {
        let page_id = page_id.clone();
        move || {
            if !ready {
                return;
            }
            working.set(true);
            let (page_id, password) = (page_id.clone(), first.peek().clone());
            spawn(async move {
                state.lock_with(page_id, password).await;
                state.set_modal(None);
            });
        }
    };
    let mut go2 = go.clone();
    rsx! {
        div { class: "modal-backdrop soft", onclick: move |_| if !working() { state.set_modal(None) },
            div { class: "modal lock-modal", role: "dialog", "aria-label": "Lock page", onclick: move |e| e.stop_propagation(),
                div { class: "modal-head",
                    div { class: "modal-head-text",
                        h2 { "Lock “{title}”" }
                        div { class: "modal-sub", "Its lines are kept scrambled. Only this password opens them." }
                    }
                }
                div { class: "lock-fields",
                    input {
                        class: "text-input",
                        id: "lock-password",
                        r#type: "password",
                        placeholder: "Password",
                        autocomplete: "new-password",
                        value: "{first}",
                        onmounted: move |e: MountedEvent| async move {
                            let _ = e.set_focus(true).await;
                        },
                        oninput: move |e: FormEvent| first.set(e.value()),
                        onkeydown: move |e: KeyboardEvent| {
                            if e.key() == Key::Escape && !working() {
                                state.set_modal(None);
                            } else if e.key() == Key::Enter {
                                e.prevent_default();
                                document::eval("document.getElementById('lock-again')?.focus()");
                            }
                        },
                    }
                    input {
                        class: "text-input",
                        id: "lock-again",
                        r#type: "password",
                        placeholder: "The same password again",
                        autocomplete: "new-password",
                        value: "{again}",
                        oninput: move |e: FormEvent| again.set(e.value()),
                        onkeydown: move |e: KeyboardEvent| {
                            if e.key() == Key::Escape && !working() {
                                state.set_modal(None);
                            } else if e.key() == Key::Enter {
                                e.prevent_default();
                                go2();
                            }
                        },
                    }
                    if let Some(problem) = problem {
                        div { class: "lock-problem", "{problem}" }
                    }
                    p { class: "lock-warning",
                        "If you forget the password, nobody can open the page again, not even Rune."
                    }
                    // What the lock does not reach, said as it is: only the
                    // page history of this notes file is emptied, and only
                    // the page's own lines are sealed.
                    p { class: "lock-warning",
                        "Its earlier versions are removed here. "
                        "Backups and synced copies from before the lock keep the old text until they are replaced. "
                        "Pages inside it, and the pages of rows of its tables, are not locked with it."
                    }
                }
                div { class: "modal-foot lock-foot",
                    button { class: "secondary-btn", disabled: working(), onclick: move |_| state.set_modal(None), "Cancel" }
                    button { class: "primary-btn", id: "lock-go", disabled: !ready, onclick: move |_| go(),
                        if working() { "Locking…" } else { "Lock page" }
                    }
                }
            }
        }
    }
}

/// What a locked page shows in place of its lines: a field for its
/// password. The page opens for as long as a tab shows it.
#[component]
pub fn LockedPage(page_id: String) -> Element {
    let state = use_context::<AppState>();
    let mut password = use_signal(String::new);
    let mut working = use_signal(|| false);
    let mut wrong = use_signal(|| None::<&'static str>);
    let try_open = {
        let page_id = page_id.clone();
        move || {
            if working() || password.peek().is_empty() {
                return;
            }
            working.set(true);
            let (page_id, typed) = (page_id.clone(), password.peek().clone());
            spawn(async move {
                let result = state.unlock_with(page_id, typed).await;
                working.set(false);
                match result {
                    Ok(()) => password.set(String::new()),
                    Err(LockError::Wrong) => {
                        wrong.set(Some("That password does not open the page."));
                        document::eval("const f=document.getElementById('unlock-password');if(f){f.select();f.classList.remove('shake');void f.offsetWidth;f.classList.add('shake');}");
                    }
                    Err(LockError::Unreadable) => wrong.set(Some("This page was locked by a newer Rune, or it is damaged.")),
                }
            });
        }
    };
    let mut try_key = try_open.clone();
    let mut try_click = try_open;
    rsx! {
        div { class: "locked-page",
            span { class: "locked-icon", Icon { name: "lock", size: 22 } }
            div { class: "locked-title", "This page is locked" }
            div { class: "locked-row",
                input {
                    class: "text-input",
                    id: "unlock-password",
                    r#type: "password",
                    placeholder: "Password",
                    autocomplete: "current-password",
                    value: "{password}",
                    "aria-label": "Password of the page",
                    oninput: move |e: FormEvent| {
                        password.set(e.value());
                        wrong.set(None);
                    },
                    onkeydown: move |e: KeyboardEvent| {
                        if e.key() == Key::Enter {
                            e.prevent_default();
                            try_key();
                        }
                    },
                }
                button { class: "secondary-btn", id: "unlock-go", disabled: working() || password.read().is_empty(), onclick: move |_| try_click(),
                    if working() { "Opening…" } else { "Open" }
                }
            }
            if let Some(why) = wrong() {
                div { class: "lock-problem", "{why}" }
            }
        }
    }
}
