/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! The `TimeRanges` DOM implementation.

use dom::bindings::codegen::Bindings::TimeRangesBinding;
use dom::bindings::codegen::Bindings::TimeRangesBinding::TimeRangesMethods;
use dom::bindings::error::{Error, Fallible};
use dom::bindings::num::Finite;
use dom::bindings::reflector::{Reflector, reflect_dom_object};
use dom::bindings::root::DomRoot;
use dom::window::Window;
use dom_struct::dom_struct;
use std::ops::Range;

/// A `TimeRanges` DOM instance.
///
/// https://w3c.github.io/media-source/#idl-def-TimeRanges
#[dom_struct]
pub struct TimeRanges {
    reflector: Reflector,
    #[ignore_heap_size_of = "FIXME(nox): https://github.com/servo/heapsize/pull/89"]
    ranges: Vec<Range<Finite<f64>>>,
}

impl TimeRanges {
    fn new_inherited(ranges: Vec<Range<Finite<f64>>>) -> Self {
        Self { reflector: Reflector::new(), ranges }
    }

    pub fn new(window: &Window, ranges: Vec<Range<Finite<f64>>>) -> DomRoot<Self> {
        reflect_dom_object(
            box Self::new_inherited(ranges),
            window,
            TimeRangesBinding::Wrap,
        )
    }

    pub fn ranges(&self) -> &[Range<Finite<f64>>] {
        &self.ranges
    }
}

impl TimeRangesMethods for TimeRanges {
    /// https://html.spec.whatwg.org/multipage/#dom-timeranges-length-2
    fn Length(&self) -> u32 {
        self.ranges.len() as u32
    }

    /// https://html.spec.whatwg.org/multipage/#dom-timeranges-start-2
    fn Start(&self, index: u32) -> Fallible<Finite<f64>> {
        Ok(self.ranges.get(index as usize).ok_or(Error::IndexSize)?.start)
    }

    /// https://html.spec.whatwg.org/multipage/#dom-timeranges-end-2
    fn End(&self, index: u32) -> Fallible<Finite<f64>> {
        Ok(self.ranges.get(index as usize).ok_or(Error::IndexSize)?.end)
    }
}
