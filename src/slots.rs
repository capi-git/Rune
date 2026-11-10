//! A meeting with no calendar event is told apart by when it happens: the
//! day of the week and the time it starts. (A stand-up is at the same time
//! on the same days.) The person says one time where its notes are kept;
//! the next time the app proposes that page, and after that it puts the
//! notes there by itself.

use serde::{Deserialize, Serialize};

/// Meetings that start this many minutes apart, or less, are at "the same time".
pub const NEAR: i64 = 20;
/// How many times the person must choose a page before the app uses it by itself.
const SURE: u32 = 2;
const MOST: usize = 60;

/// One meeting that comes again: its time, its days, and its page.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Slot {
    /// The minute of the day it starts at (it follows the last few starts).
    pub minute: i64,
    /// The days of the week it was seen on (bit 0 Monday … bit 6 Sunday).
    pub days: u8,
    /// The page its notes are kept in.
    pub home: String,
    /// How many times the person chose that page for it, or took the proposal.
    pub count: u32,
    /// The notes page made for it last (the person can give that page a name).
    #[serde(default)]
    pub page: String,
    /// When it was seen last.
    #[serde(default)]
    pub last: i64,
}

/// What the app does with the notes of a meeting that starts now.
#[derive(Clone, Debug, PartialEq)]
pub enum Found {
    /// Nothing is known about a meeting at this time.
    Nothing,
    /// The notes go in this page, with no question (the index of the slot).
    Keep(usize),
    /// These pages are proposed (the indexes of the slots, the likeliest first).
    Propose(Vec<usize>),
}

fn near(slot: &Slot, minute: i64) -> bool {
    (slot.minute - minute).abs() <= NEAR
}

/// `day`: 0 Monday … 6 Sunday. `minute`: the minute of the day.
pub fn find(slots: &[Slot], day: u32, minute: i64) -> Found {
    let bit = 1u8 << (day % 7);
    let mut at_time: Vec<usize> = (0..slots.len()).filter(|&i| !slots[i].home.is_empty() && near(&slots[i], minute)).collect();
    if at_time.is_empty() {
        return Found::Nothing;
    }
    let of_day: Vec<usize> = at_time.iter().copied().filter(|&i| slots[i].days & bit != 0).collect();
    // The meeting of this day and this time, chosen often enough.
    if let [i] = of_day[..] {
        if slots[i].count >= SURE {
            return Found::Keep(i);
        }
    }
    // The only meeting at this time, and it is on more days than one (a
    // meeting of every day): the same on a day it was not seen on yet.
    if let [i] = at_time[..] {
        if slots[i].count >= SURE && slots[i].days.count_ones() >= 2 {
            return Found::Keep(i);
        }
    }
    // A proposal: the meetings of this day first, then the newest.
    at_time.sort_by_key(|&i| (slots[i].days & bit == 0, std::cmp::Reverse(slots[i].last)));
    at_time.truncate(2);
    Found::Propose(at_time)
}

/// The person chose `home` for the notes of a meeting at this time (`page`).
pub fn learn(slots: &mut Vec<Slot>, day: u32, minute: i64, home: &str, page: &str, now: i64) {
    let bit = 1u8 << (day % 7);
    let same = slots.iter().position(|s| near(s, minute) && s.home == home);
    // On this day, the other meetings at this time are not this one.
    for (i, s) in slots.iter_mut().enumerate() {
        if Some(i) != same && near(s, minute) {
            s.days &= !bit;
        }
    }
    match same {
        Some(i) => {
            let s = &mut slots[i];
            s.days |= bit;
            s.count += 1;
            s.minute = (s.minute * 3 + minute) / 4;
            s.page = page.to_string();
            s.last = now;
        }
        None => slots.push(Slot { minute, days: bit, home: home.to_string(), count: 1, page: page.to_string(), last: now }),
    }
    slots.retain(|s| s.days != 0);
    if slots.len() > MOST {
        slots.sort_by_key(|s| std::cmp::Reverse(s.last));
        slots.truncate(MOST);
    }
}

/// The app put the notes of a meeting in the page of slot `i` by itself.
pub fn seen(slots: &mut [Slot], i: usize, day: u32, minute: i64, page: &str, now: i64) {
    if let Some(s) = slots.get_mut(i) {
        s.days |= 1u8 << (day % 7);
        s.minute = (s.minute * 3 + minute) / 4;
        s.page = page.to_string();
        s.last = now;
    }
}

/// The person took the notes of a meeting at this time out of every page:
/// on this day nothing is known about it any more.
pub fn forget(slots: &mut Vec<Slot>, day: u32, minute: i64) {
    let bit = 1u8 << (day % 7);
    for s in slots.iter_mut().filter(|s| near(s, minute)) {
        s.days &= !bit;
    }
    slots.retain(|s| s.days != 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    const MON: u32 = 0;
    const TUE: u32 = 1;
    const WED: u32 = 2;
    const NINE_30: i64 = 9 * 60 + 30;

    #[test]
    fn a_stand_up_of_every_day_is_learned_in_two_days() {
        let mut slots = Vec::new();
        assert_eq!(find(&slots, MON, NINE_30), Found::Nothing);
        // Monday: the person chooses the project.
        learn(&mut slots, MON, NINE_30, "apollo", "p1", 1);
        // Tuesday, three minutes late: a proposal, not yet by itself.
        assert_eq!(find(&slots, TUE, NINE_30 + 3), Found::Propose(vec![0]));
        learn(&mut slots, TUE, NINE_30 + 3, "apollo", "p2", 2);
        assert_eq!((slots.len(), slots[0].count, slots[0].days), (1, 2, 0b11));
        // Wednesday (a day it was not seen on) and the next Monday: by itself.
        assert_eq!(find(&slots, WED, NINE_30 - 4), Found::Keep(0));
        assert_eq!(find(&slots, MON, NINE_30 + 10), Found::Keep(0));
        // Another time of the day is another meeting.
        assert_eq!(find(&slots, WED, 14 * 60), Found::Nothing);
        assert_eq!(find(&slots, WED, NINE_30 + NEAR + 5), Found::Nothing);
    }

    #[test]
    fn a_meeting_of_one_day_a_week_needs_that_day_two_times() {
        let mut slots = Vec::new();
        learn(&mut slots, MON, 600, "apollo", "p1", 1);
        assert_eq!(find(&slots, MON, 600), Found::Propose(vec![0]));
        learn(&mut slots, MON, 602, "apollo", "p2", 2);
        assert_eq!(find(&slots, MON, 598), Found::Keep(0));
        // On another day at that time: only a proposal (it is on one day).
        assert_eq!(find(&slots, TUE, 600), Found::Propose(vec![0]));
    }

    #[test]
    fn two_projects_at_one_time_on_different_days_stay_apart() {
        let mut slots = Vec::new();
        learn(&mut slots, MON, NINE_30, "apollo", "p1", 1);
        learn(&mut slots, MON, NINE_30, "apollo", "p2", 2);
        // Tuesday at that time is the other project: the person says so.
        learn(&mut slots, TUE, NINE_30, "atlas", "p3", 3);
        learn(&mut slots, TUE, NINE_30, "atlas", "p4", 4);
        let home = |f: Found| match f {
            Found::Keep(i) => slots[i].home.clone(),
            other => format!("{other:?}"),
        };
        assert_eq!(home(find(&slots, MON, NINE_30)), "apollo");
        assert_eq!(home(find(&slots, TUE, NINE_30)), "atlas");
        // A day that neither was seen on: both are proposed, the newest first.
        assert_eq!(find(&slots, WED, NINE_30), Found::Propose(vec![1, 0]));
    }

    #[test]
    fn a_change_of_mind_is_learned_and_no_page_forgets() {
        let mut slots = Vec::new();
        learn(&mut slots, MON, 600, "apollo", "p1", 1);
        learn(&mut slots, MON, 600, "apollo", "p2", 2);
        assert_eq!(find(&slots, MON, 600), Found::Keep(0));
        // The person moves the notes of that Monday to another project.
        learn(&mut slots, MON, 600, "atlas", "p3", 3);
        assert_eq!(slots.len(), 1, "the old one had only this day: it is gone");
        assert_eq!((slots[0].home.as_str(), slots[0].count), ("atlas", 1));
        assert_eq!(find(&slots, MON, 600), Found::Propose(vec![0]));
        forget(&mut slots, MON, 600);
        assert_eq!(find(&slots, MON, 600), Found::Nothing);
    }

    #[test]
    fn what_the_app_did_by_itself_adds_the_day_but_not_the_count() {
        let mut slots = Vec::new();
        learn(&mut slots, MON, 600, "apollo", "p1", 1);
        learn(&mut slots, TUE, 600, "apollo", "p2", 2);
        seen(&mut slots, 0, WED, 606, "p3", 3);
        assert_eq!((slots[0].count, slots[0].days, slots[0].page.as_str(), slots[0].last), (2, 0b111, "p3", 3));
    }
}
