/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! The `MediaSource` DOM implementation.

use dom::bindings::codegen::Bindings::MediaSourceBinding;
use dom::bindings::codegen::Bindings::MediaSourceBinding::EndOfStreamError;
use dom::bindings::codegen::Bindings::MediaSourceBinding::ReadyState;
use dom::bindings::codegen::Bindings::MediaSourceBinding::MediaSourceMethods;
use dom::bindings::codegen::Bindings::SourceBufferBinding::SourceBufferMethods;
use dom::bindings::cell::DomRefCell;
use dom::bindings::error::{Error, ErrorResult, Fallible};
use dom::bindings::inheritance::Castable;
use dom::bindings::num::Finite;
use dom::bindings::reflector::{DomObject, reflect_dom_object};
use dom::bindings::root::{Dom, DomRoot, MutNullableDom};
use dom::bindings::str::DOMString;
use dom::eventtarget::EventTarget;
use dom::sourcebuffer::SourceBuffer;
use dom::sourcebufferlist::{ListMode, SourceBufferList};
use dom::timeranges::TimeRanges;
use dom::window::Window;
use dom_struct::dom_struct;
use mime::Mime;
use std::cell::{Cell, Ref};
use std::f64;

/// A `MediaSource` DOM instance.
///
/// https://w3c.github.io/media-source/#idl-def-mediasource
#[dom_struct]
pub struct MediaSource {
    eventtarget: EventTarget,
    source_buffers: DomRefCell<Vec<Dom<SourceBuffer>>>,
    source_buffers_list: MutNullableDom<SourceBufferList>,
    active_source_buffers_list: MutNullableDom<SourceBufferList>,
    ready_state: Cell<ReadyState>,
    duration: Cell<f64>,
    /// https://w3c.github.io/media-source/#live-seekable-range
    live_seekable_range: MutNullableDom<TimeRanges>,
}

impl MediaSource {
    fn new(window: &Window) -> DomRoot<Self> {
        reflect_dom_object(
            Box::new(Self::new_inherited()),
            window,
            MediaSourceBinding::Wrap,
        )
    }

    fn new_inherited() -> Self {
        Self {
            eventtarget: EventTarget::new_inherited(),
            source_buffers: Default::default(),
            source_buffers_list: Default::default(),
            active_source_buffers_list: Default::default(),
            ready_state: Cell::new(ReadyState::Closed),
            duration: Cell::new(f64::NAN),
            live_seekable_range: Default::default(),
        }
    }

    pub fn source_buffers<'a>(&'a self) -> Ref<'a, [Dom<SourceBuffer>]> {
        Ref::map(self.source_buffers.borrow(), |buffers| &**buffers)
    }

    /// https://w3c.github.io/media-source/#dom-mediasource-istypesupported
    fn parse_mime_type(input: &str) -> Option<Mime> {
        // Steps 1-2.
        let _mime = match input.parse::<Mime>() {
            Ok(mime) => mime,
            Err(_) => return None,
        };

        // Steps 3-5.
        // FIXME(nox): Implement the checks.

        // Step 6.
        // FIXME(nox): Should be Ok(mime).
        None
    }

    pub fn set_ready_state(&self, ready_state: ReadyState) {
        self.ready_state.set(ready_state);
    }
}

impl MediaSource {
    pub fn Constructor(window: &Window) -> Fallible<DomRoot<Self>> {
        Ok(Self::new(window))
    }

    /// https://w3c.github.io/media-source/#dom-mediasource-istypesupported
    pub fn IsTypeSupported(_window: &Window, type_: DOMString) -> bool {
        Self::parse_mime_type(&type_).is_some()
    }
}

impl MediaSourceMethods for MediaSource {
    /// https://w3c.github.io/media-source/#dom-mediasource-sourcebuffers
    fn SourceBuffers(&self) -> DomRoot<SourceBufferList> {
        self.source_buffers_list.or_init(|| {
            SourceBufferList::new(self, ListMode::All)
        })
    }

    /// https://w3c.github.io/media-source/#dom-mediasource-sourcebuffers
    fn ActiveSourceBuffers(&self) -> DomRoot<SourceBufferList> {
        self.active_source_buffers_list.or_init(|| {
            SourceBufferList::new(self, ListMode::Active)
        })
    }

    /// https://w3c.github.io/media-source/#dom-readystate
    fn ReadyState(&self) -> ReadyState {
        self.ready_state.get()
    }

    /// https://w3c.github.io/media-source/#dom-mediasource-duration
    fn Duration(&self) -> f64 {
        // Step 1.
        if self.ready_state.get() == ReadyState::Closed {
            return f64::NAN;
        }
        // Step 2.
        self.duration.get()
    }

    /// https://w3c.github.io/media-source/#dom-mediasource-duration
    fn SetDuration(&self, value: f64) -> ErrorResult {
        // Step 1.
        if value < 0. {
            return Err(Error::Type("value should not be negative".to_owned()));
        }
        if value.is_nan() {
            return Err(Error::Type("value should not be NaN".to_owned()));
        }

        // Step 2.
        if self.ready_state.get() != ReadyState::Open {
            return Err(Error::InvalidState);
        }

        // Step 3.
        if self.source_buffers().iter().any(|buffer| buffer.is_active()) {
            return Err(Error::InvalidState);
        }

        // Step 4.
        self.change_duration(value)
    }

    event_handler!(sourceopen, GetOnsourceopen, SetOnsourceopen);
    event_handler!(sourceended, GetOnsourceended, SetOnsourceended);
    event_handler!(sourceclose, GetOnsourceclose, SetOnsourceclose);

    /// https://w3c.github.io/media-source/#dom-mediasource-addsourcebuffer
    fn AddSourceBuffer(&self, type_: DOMString) -> Fallible<DomRoot<SourceBuffer>> {
        // Step 1.
        if type_.is_empty() {
            return Err(Error::Type("source type is empty".to_owned()));
        }

        // Step 2.
        let mime = Self::parse_mime_type(&type_).ok_or(Error::NotSupported)?;
        if self.is_compatible_with_other_source_buffers(&mime) {
            return Err(Error::NotSupported);
        }

        // Step 3.
        // FIXME(nox): Implement quota checks.

        // Step 4.
        if self.ready_state.get() != ReadyState::Open {
            return Err(Error::InvalidState);
        }

        let window = DomRoot::downcast::<Window>(self.global()).unwrap();

        // Steps 5-7.
        let source_buffer = SourceBuffer::new(self, mime);

        // Step 8.
        self.source_buffers.borrow_mut().push(Dom::from_ref(&*source_buffer));
        // TODO(nox): If we do our own `Runnable`, we could avoid creating
        // the `sourceBuffers` object if the user doesn't access it.
        window.dom_manipulation_task_source().queue_simple_event(
            self.SourceBuffers().upcast(),
            atom!("addsourcebuffer"),
            &window,
        );

        // Step 9.
        Ok(source_buffer)
    }

    /// https://w3c.github.io/media-source/#dom-mediasource-removesourcebuffer
    fn RemoveSourceBuffer(&self, source_buffer: &SourceBuffer) -> ErrorResult {
        // Step 1.
        let position = self.source_buffers()
            .iter()
            .position(|b| &**b == source_buffer)
            .ok_or(Error::NotFound)?;

        let window = DomRoot::downcast::<Window>(self.global()).unwrap();
        let task_source = window.dom_manipulation_task_source();

        // Step 2.
        if source_buffer.Updating() {
            // Step 2.1-2.2.
            // FIXME(nox): Abort the buffer append algorithm if it is running
            // and set the source buffer's updating flag to false.

            // Step 2.3.
            task_source.queue_simple_event(
                source_buffer.upcast(),
                atom!("abort"),
                &window,
            );

            // Step 2.4.
            task_source.queue_simple_event(
                source_buffer.upcast(),
                atom!("updateend"),
                &window,
            );
        }

        // Steps 3-4.
        // FIXME(nox): Handle audio tracks created by this source buffer.

        // Steps 5-6.
        // FIXME(nox): Handle video tracks created by this source buffer.

        // Steps 7-8.
        // FIXME(nox): Handle text tracks created by this source buffer.

        // Step 9.
        if source_buffer.is_active() {
            // FIXME(nox): Set source buffer's active flag to false.
            // TODO(nox): If we do our own `Runnable`, we could avoid creating
            // the `activeSourceBuffers` object if the user doesn't access it.
            task_source.queue_simple_event(
                self.ActiveSourceBuffers().upcast(),
                atom!("removesourcebuffer"),
                &window,
            );
        }

        // Step 10.
        self.source_buffers.borrow_mut().remove(position);
        source_buffer.clear_parent_media_source();
        // TODO(nox): If we do our own `Runnable`, we could avoid creating
        // the `sourceBuffers` object if the user doesn't access it.
        task_source.queue_simple_event(
            self.SourceBuffers().upcast(),
            atom!("removesourcebuffer"),
            &window,
        );

        // Step 11.
        // FIXME(nox): Destroy resources of the source buffer.

        Ok(())
    }

    /// https://w3c.github.io/media-source/#dom-mediasource-endofstream
    fn EndOfStream(&self, error: Option<EndOfStreamError>) -> ErrorResult {
        // Step 1.
        if self.ready_state.get() != ReadyState::Open {
            return Err(Error::InvalidState);
        }

        // Step 2.
        if self.source_buffers.borrow().iter().any(|buffer| buffer.Updating()) {
            return Err(Error::InvalidState);
        }

        // Step 3.
        self.end_of_stream(error);

        Ok(())
    }

    /// https://w3c.github.io/media-source/#dom-mediasource-endofstream
    fn SetLiveSeekableRange(
        &self,
        start: Finite<f64>,
        end: Finite<f64>,
    ) -> ErrorResult {
        // Step 1.
        if self.ready_state.get() != ReadyState::Open {
            return Err(Error::InvalidState);
        }

        // Step 2.
        if *start < 0. {
            return Err(Error::Type("start should not be negative".to_owned()));
        }
        if *start > *end {
            return Err(Error::Type("start should not be greater than end".to_owned()));
        }

        // Step 3.
        self.live_seekable_range.set(Some(&TimeRanges::new(
            self.global().as_window(),
            vec![start..end]
        )));

        Ok(())
    }


    /// https://w3c.github.io/media-source/#dom-mediasource-endofstream
    fn ClearLiveSeekableRange(&self) -> ErrorResult {
        // Step 1.
        if self.ready_state.get() != ReadyState::Open {
            return Err(Error::InvalidState);
        }

        // Step 2.
        if let Some(time_ranges) = self.live_seekable_range.get() {
            if !time_ranges.ranges().is_empty() {
                self.live_seekable_range.set(Some(&TimeRanges::new(
                    self.global().as_window(),
                    vec![],
                )));
            }
        }

        Ok(())
    }
}

impl MediaSource {
    /// https://w3c.github.io/media-source/#duration-change-algorithm
    fn change_duration(&self, new_duration: f64) -> ErrorResult {
        // Step 1.
        if self.duration.get() == new_duration {
            return Ok(());
        }

        // Step 2.
        if self.is_less_than_highest_presentation_time(new_duration) {
            return Err(Error::InvalidState);
        }

        // Step 3.
        let highest_end_time = self.highest_end_time();

        // Step 4.
        let new_duration = new_duration.max(highest_end_time);

        // Step 5.
        self.duration.set(new_duration);

        // Step 6.
        // FIXME(nox): Update media duration and run the `HTMLMediaElement`
        // duration change algorithm.

        Ok(())
    }

    /// https://w3c.github.io/media-source/#end-of-stream-algorithm
    fn end_of_stream(&self, _error: Option<EndOfStreamError>) {
        // Step 1.
        self.ready_state.set(ReadyState::Closed);

        let window = DomRoot::downcast::<Window>(self.global()).unwrap();

        // Step 2.
        window.dom_manipulation_task_source().queue_simple_event(
            self.upcast(),
            atom!("sourceended"),
            &window,
        );

        // Step 3.
        // FIXME(nox): Do the thing.
    }

    fn is_compatible_with_other_source_buffers(&self, _mime: &Mime) -> bool {
        // FIXME(nox): Implement the checks.
        false
    }

    fn is_less_than_highest_presentation_time(&self, _value: f64) -> bool {
        // FIXME(nox): Implement correctly.
        false
    }

    fn highest_end_time(&self) -> f64 {
        // FIXME(nox): Implement correctly.
        unimplemented!();
    }
}
