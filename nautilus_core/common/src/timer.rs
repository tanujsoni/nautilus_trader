// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

use std::cmp::Ordering;
use std::ffi::c_char;
use std::fmt;
use std::rc::Rc;

use nautilus_core::correctness;
use nautilus_core::string::{cstr_to_string, string_to_cstr};
use nautilus_core::time::{TimedeltaNanos, UnixNanos};
use nautilus_core::uuid::UUID4;
use pyo3::ffi;

#[repr(C)]
#[derive(Clone, Debug)]
#[allow(clippy::redundant_allocation)] // C ABI compatibility
/// Represents a time event occurring at the event timestamp.
pub struct TimeEvent {
    /// The event name.
    pub name: Box<Rc<String>>,
    /// The event ID.
    pub event_id: UUID4,
    /// The message category
    pub ts_event: UnixNanos,
    /// The UNIX timestamp (nanoseconds) when the object was initialized.
    pub ts_init: UnixNanos,
}

impl TimeEvent {
    #[must_use]
    pub fn new(name: String, event_id: UUID4, ts_event: UnixNanos, ts_init: UnixNanos) -> Self {
        correctness::valid_string(&name, "`TimeEvent` name");

        TimeEvent {
            name: Box::new(Rc::new(name)),
            event_id,
            ts_event,
            ts_init,
        }
    }
}

impl fmt::Display for TimeEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "TimeEvent(name={}, event_id={}, ts_event={}, ts_init={})",
            self.name, self.event_id, self.ts_event, self.ts_init
        )
    }
}

impl PartialEq for TimeEvent {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.ts_event == other.ts_event
    }
}

#[repr(C)]
#[derive(Clone, Debug)]
/// Represents a time event and its associated handler.
pub struct TimeEventHandler {
    /// The event.
    pub event: TimeEvent,
    /// The event ID.
    pub callback_ptr: *mut ffi::PyObject,
}

impl PartialOrd for TimeEventHandler {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for TimeEventHandler {
    fn eq(&self, other: &Self) -> bool {
        self.event.ts_event == other.event.ts_event
    }
}

impl Eq for TimeEventHandler {}

impl Ord for TimeEventHandler {
    fn cmp(&self, other: &Self) -> Ordering {
        self.event.ts_event.cmp(&other.event.ts_event)
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
/// # Safety
/// - Assumes `name` is borrowed from a valid Python UTF-8 `str`.
#[no_mangle]
pub unsafe extern "C" fn time_event_new(
    name: *const c_char,
    event_id: UUID4,
    ts_event: u64,
    ts_init: u64,
) -> TimeEvent {
    TimeEvent::new(cstr_to_string(name), event_id, ts_event, ts_init)
}

#[no_mangle]
pub extern "C" fn time_event_clone(event: &TimeEvent) -> TimeEvent {
    event.clone()
}

#[no_mangle]
pub extern "C" fn time_event_drop(event: TimeEvent) {
    drop(event); // Memory freed here
}

#[no_mangle]
pub extern "C" fn time_event_name_to_cstr(event: &TimeEvent) -> *const c_char {
    string_to_cstr(&event.name)
}

/// Returns a [`TimeEvent`] as a C string pointer.
#[no_mangle]
pub extern "C" fn time_event_to_cstr(event: &TimeEvent) -> *const c_char {
    string_to_cstr(&event.to_string())
}

pub trait Timer {
    fn new(
        name: String,
        interval_ns: TimedeltaNanos,
        start_time_ns: UnixNanos,
        stop_time_ns: Option<UnixNanos>,
    ) -> Self;
    fn pop_event(&self, event_id: UUID4, ts_init: UnixNanos) -> TimeEvent;
    fn iterate_next_time(&mut self, ts_now: UnixNanos);
    fn cancel(&mut self);
}

#[no_mangle]
pub extern "C" fn dummy(v: TimeEventHandler) -> TimeEventHandler {
    v
}

#[derive(Clone)]
pub struct TestTimer {
    pub name: String,
    pub interval_ns: u64,
    pub start_time_ns: UnixNanos,
    pub stop_time_ns: Option<UnixNanos>,
    pub next_time_ns: UnixNanos,
    pub is_expired: bool,
}

impl TestTimer {
    #[must_use]
    pub fn new(
        name: String,
        interval_ns: u64,
        start_time_ns: UnixNanos,
        stop_time_ns: Option<UnixNanos>,
    ) -> Self {
        correctness::valid_string(&name, "`TestTimer` name");

        TestTimer {
            name,
            interval_ns,
            start_time_ns,
            stop_time_ns,
            next_time_ns: start_time_ns + interval_ns,
            is_expired: false,
        }
    }

    pub fn pop_event(&self, event_id: UUID4, ts_init: UnixNanos) -> TimeEvent {
        TimeEvent {
            name: Box::new(Rc::new(self.name.clone())),
            event_id,
            ts_event: self.next_time_ns,
            ts_init,
        }
    }

    /// Advance the test timer forward to the given time, generating a sequence
    /// of events. A [TimeEvent] is appended for each time a next event is
    /// <= the given `to_time_ns`.
    pub fn advance(&mut self, to_time_ns: UnixNanos) -> impl Iterator<Item = TimeEvent> + '_ {
        let advances =
            to_time_ns.saturating_sub(self.next_time_ns - self.interval_ns) / self.interval_ns;
        self.take(advances as usize).map(|(event, _)| event)
    }

    /// Cancels the timer (the timer will not generate an event).
    pub fn cancel(&mut self) {
        self.is_expired = true;
    }
}

impl Iterator for TestTimer {
    type Item = (TimeEvent, UnixNanos);

    fn next(&mut self) -> Option<Self::Item> {
        if self.is_expired {
            None
        } else {
            let item = (
                TimeEvent {
                    name: Box::new(Rc::new(self.name.clone())),
                    event_id: UUID4::new(),
                    ts_event: self.next_time_ns,
                    ts_init: self.next_time_ns,
                },
                self.next_time_ns,
            );

            // If current next event time has exceeded stop time, then expire timer
            if let Some(stop_time_ns) = self.stop_time_ns {
                if self.next_time_ns >= stop_time_ns {
                    self.is_expired = true;
                }
            }

            self.next_time_ns += self.interval_ns;

            Some(item)
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::{TestTimer, TimeEvent};

    #[test]
    fn test_pop_event() {
        let name = String::from("test_timer");
        let mut timer = TestTimer::new(name, 0, 1, None);

        assert!(timer.next().is_some());
        assert!(timer.next().is_some());
        timer.is_expired = true;
        assert!(timer.next().is_none());
    }

    #[test]
    fn test_advance_within_next_time_ns() {
        let name = String::from("test_timer");
        let mut timer = TestTimer::new(name, 5, 0, None);
        let _: Vec<TimeEvent> = timer.advance(1).collect();
        let _: Vec<TimeEvent> = timer.advance(2).collect();
        let _: Vec<TimeEvent> = timer.advance(3).collect();
        assert_eq!(timer.advance(4).count(), 0);
        assert_eq!(timer.next_time_ns, 5);
        assert!(!timer.is_expired)
    }

    #[test]
    fn test_advance_up_to_next_time_ns() {
        let name = String::from("test_timer");
        let mut timer = TestTimer::new(name, 1, 0, None);
        assert_eq!(timer.advance(1).count(), 1);
        assert!(!timer.is_expired);
    }

    #[test]
    fn test_advance_up_to_next_time_ns_with_stop_time() {
        let name = String::from("test_timer");
        let mut timer = TestTimer::new(name, 1, 0, Some(2));
        assert_eq!(timer.advance(2).count(), 2);
        assert!(timer.is_expired);
    }

    #[test]
    fn test_advance_beyond_next_time_ns() {
        let name = String::from("test_timer");
        let mut timer = TestTimer::new(name, 1, 0, Some(5));
        assert_eq!(timer.advance(5).count(), 5);
        assert!(timer.is_expired);
    }

    #[test]
    fn test_advance_beyond_stop_time() {
        let name = String::from("test_timer");
        let mut timer = TestTimer::new(name, 1, 0, Some(5));
        assert_eq!(timer.advance(10).count(), 5);
        assert!(timer.is_expired);
    }
}
