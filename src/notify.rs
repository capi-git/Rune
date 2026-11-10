//! Mac notifications: calendar reminders and "AI reply ready".
//!
//! Reminders for the next two weeks are handed to macOS ahead of time, so they
//! arrive even when the app is closed; the plan is redone whenever events
//! change. Clicking a reminder opens the event. Notifications need the app
//! bundle (Rune.app); a bare `cargo run` skips them.

use crate::model::Event;
use crate::when;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AnyThread, MainThreadMarker};
use objc2_foundation::{NSBundle, NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent, UNNotification,
    UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse, UNNotificationSettings,
    UNNotificationSound, UNTimeIntervalNotificationTrigger, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};
use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::sync::{Arc, Mutex};

const EVENT_PREFIX: &str = "event:";
const PLAN_DAYS: i64 = 14;
const MAX_PLANNED: usize = 60;

/// Reminder choices: (minutes before, label). None: no reminder.
pub const REMINDERS: [(Option<i64>, &str); 8] = [
    (None, "None"),
    (Some(0), "At start"),
    (Some(5), "5 min before"),
    (Some(10), "10 min before"),
    (Some(15), "15 min before"),
    (Some(30), "30 min before"),
    (Some(60), "1 hour before"),
    (Some(24 * 60), "1 day before"),
];

/// "10" → Some(10), "none" → None. The value stored for the default reminder.
pub fn parse_reminder(s: &str) -> Option<i64> {
    s.trim().parse::<i64>().ok().filter(|m| *m >= 0)
}

/// Reminder for new events (Settings), 10 minutes unless changed.
pub fn default_reminder() -> Option<i64> {
    match crate::store::setting("default_reminder") {
        Some(v) => parse_reminder(&v),
        None => Some(10),
    }
}

thread_local! {
    static DELEGATE: RefCell<Option<Retained<Delegate>>> = const { RefCell::new(None) };
    static ASKED: Cell<bool> = const { Cell::new(false) };
    static DIRTY: Cell<bool> = const { Cell::new(true) };
}

/// Notification clicks waiting to be handled (identifiers).
static CLICKED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Whether this copy of the app can post notifications (it must be the bundle).
pub fn available() -> bool {
    NSBundle::mainBundle().bundleIdentifier().is_some()
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "Notion2NotificationDelegate"]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        /// Show banners even while the app is in front.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion.call((UNNotificationPresentationOptions::Banner
                | UNNotificationPresentationOptions::List
                | UNNotificationPresentationOptions::Sound,));
        }

        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion: &block2::DynBlock<dyn Fn()>,
        ) {
            let id = response.notification().request().identifier().to_string();
            if let Ok(mut c) = CLICKED.lock() {
                c.push(id);
            }
            completion.call(());
        }
    }
);

/// Set up once, on the main thread, when the first window opens.
pub fn install() {
    if MainThreadMarker::new().is_none() || !available() || DELEGATE.with(|d| d.borrow().is_some()) {
        return;
    }
    let delegate: Retained<Delegate> = unsafe { msg_send![Delegate::alloc(), init] };
    UNUserNotificationCenter::currentNotificationCenter().setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    DELEGATE.with(|d| *d.borrow_mut() = Some(delegate));
}

/// The id of an event whose reminder was clicked, or "ai" for an AI reply.
pub fn take_clicked() -> Option<String> {
    let id = CLICKED.lock().ok()?.pop()?;
    if let Some(rest) = id.strip_prefix(EVENT_PREFIX) {
        return Some(rest.split(':').next().unwrap_or(rest).to_string());
    }
    Some(id)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Permission {
    NotAsked,
    Allowed,
    Denied,
    /// Not the app bundle (a development run).
    Unavailable,
}

/// Ask macOS whether notifications are allowed.
pub async fn permission() -> Permission {
    if !available() {
        return Permission::Unavailable;
    }
    let (tx, rx) = futures_channel::oneshot::channel();
    let tx = Arc::new(Mutex::new(Some(tx)));
    let handler = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
        let status = unsafe { settings.as_ref() }.authorizationStatus();
        let p = if status == UNAuthorizationStatus::NotDetermined {
            Permission::NotAsked
        } else if status == UNAuthorizationStatus::Denied {
            Permission::Denied
        } else {
            Permission::Allowed
        };
        if let Some(tx) = tx.lock().ok().and_then(|mut t| t.take()) {
            let _ = tx.send(p);
        }
    });
    UNUserNotificationCenter::currentNotificationCenter().getNotificationSettingsWithCompletionHandler(&handler);
    rx.await.unwrap_or(Permission::Denied)
}

/// Show macOS's "allow notifications?" prompt (only ever shown once).
pub async fn request_permission() -> Permission {
    if !available() {
        return Permission::Unavailable;
    }
    ASKED.with(|a| a.set(true));
    let (tx, rx) = futures_channel::oneshot::channel::<()>();
    let tx = Arc::new(Mutex::new(Some(tx)));
    let handler = RcBlock::new(move |_granted: Bool, _error: *mut NSError| {
        if let Some(tx) = tx.lock().ok().and_then(|mut t| t.take()) {
            let _ = tx.send(());
        }
    });
    UNUserNotificationCenter::currentNotificationCenter().requestAuthorizationWithOptions_completionHandler(
        UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
        &handler,
    );
    let _ = rx.await;
    permission().await
}

/// When an event's reminder should fire, if it has one.
pub fn fire_time(e: &Event) -> Option<i64> {
    let minutes = e.remind?;
    if e.all_day {
        // All-day events: 9 AM on the day (minus any days before).
        let day = when::add_days(when::date_of(e.start), -(minutes / (24 * 60)));
        Some(when::at(day, chrono::NaiveTime::from_hms_opt(9, 0, 0)?))
    } else {
        Some(e.start - minutes * when::MINUTE)
    }
}

fn reminder_text(e: &Event) -> String {
    // A meeting with an address to join by: a click on the reminder joins it.
    if !e.all_day && e.join_url().is_some() {
        return format!("{} · Click to join", reminder_when(e));
    }
    reminder_when(e)
}

fn reminder_when(e: &Event) -> String {
    if e.all_day {
        return format!("{} · all day", when::day_label(when::date_of(e.start)));
    }
    let range = format!("{} – {}", when::time_label(e.start), when::time_label(e.end));
    match e.remind.unwrap_or(0) {
        0 => format!("Now · {range}"),
        m if m < 60 => format!("In {m} min · {range}"),
        m if m < 24 * 60 => format!("In {} hr · {range}", m / 60),
        _ => format!("{} · {range}", when::day_label(when::date_of(e.start))),
    }
}

fn post(identifier: &str, title: &str, body: &str, after_secs: Option<f64>) {
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    content.setSound(Some(&UNNotificationSound::defaultSound()));
    let trigger = after_secs.map(|s| UNTimeIntervalNotificationTrigger::triggerWithTimeInterval_repeats(s.max(1.0), false));
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
        &NSString::from_str(identifier),
        &content,
        trigger.as_ref().map(|t| &***t),
    );
    UNUserNotificationCenter::currentNotificationCenter().addNotificationRequest_withCompletionHandler(&request, None);
}

/// Hand macOS the reminders for the coming two weeks (replacing the old plan).
pub async fn plan_reminders(events: Vec<Event>) {
    if !available() {
        return;
    }
    let now = crate::model::now_ms();
    let mut due: Vec<(i64, Event)> = events
        .into_iter()
        .filter_map(|e| fire_time(&e).map(|t| (t, e)))
        .filter(|(t, _)| *t > now && *t < now + PLAN_DAYS * 24 * when::HOUR)
        .collect();
    let mut state = permission().await;
    if state == Permission::NotAsked && !due.is_empty() && !ASKED.with(|a| a.get()) {
        state = request_permission().await;
    }
    let center = UNUserNotificationCenter::currentNotificationCenter();
    // Every pending request of ours is a reminder; start the plan afresh.
    center.removeAllPendingNotificationRequests();
    if state != Permission::Allowed {
        return;
    }
    due.sort_by_key(|(t, _)| *t);
    for (t, e) in due.into_iter().take(MAX_PLANNED) {
        let id = format!("{EVENT_PREFIX}{}:{}:{}", e.id, e.start, e.remind.unwrap_or(0));
        post(&id, e.display_title(), &reminder_text(&e), Some((t - now) as f64 / 1000.0));
    }
}

/// Post a notification now (when allowed), e.g. "AI reply ready".
pub async fn show_now(identifier: &str, title: &str, body: &str) {
    if permission().await == Permission::Allowed {
        post(identifier, title, body, None);
    }
}

/// Open System Settings at Notifications.
pub fn open_settings() {
    let _ = std::process::Command::new("open").arg("x-apple.systempreferences:com.apple.Notifications-Settings.extension").spawn();
}

/// Events changed: plan the reminders again soon.
pub fn mark_dirty() {
    DIRTY.with(|d| d.set(true));
}

pub fn take_dirty() -> bool {
    DIRTY.with(|d| d.replace(false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, NaiveTime};

    #[test]
    fn reminders_fire_before_the_start() {
        let d = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
        let start = when::at(d, NaiveTime::from_hms_opt(14, 0, 0).unwrap());
        let mut e = Event::new("s", "Sync", start, start + when::HOUR, false);
        assert_eq!(fire_time(&e), None);
        e.remind = Some(10);
        assert_eq!(fire_time(&e), Some(start - 10 * when::MINUTE));
        assert_eq!(reminder_text(&e), "In 10 min · 2 PM – 3 PM");
        let mut day = Event::new("s", "Offsite", when::midnight(d), when::midnight(when::next_day(d)), true);
        day.remind = Some(0);
        assert_eq!(fire_time(&day), Some(when::at(d, NaiveTime::from_hms_opt(9, 0, 0).unwrap())));
        assert_eq!(parse_reminder("none"), None);
        assert_eq!(parse_reminder("30"), Some(30));
    }
}
