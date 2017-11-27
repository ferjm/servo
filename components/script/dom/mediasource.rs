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
use gecko_media::{GeckoMedia, GeckoMediaSource};
use gecko_media::{GeckoMediaSourceImpl, GeckoMediaTimeInterval};
use mime::Mime;
use std::cell::{Cell, Ref};
use std::f64;
use std::ptr;
use std::rc::Rc;

// Arbitrary limit set by GeckoMedia.
static MAX_SOURCE_BUFFERS: usize = 12;

#[derive(JSTraceable, MallocSizeOf)]
#[allow(unrooted_must_root)]
struct MediaSourceAttributes {
    owner: MutNullableDom<MediaSource>,
    source_buffers: DomRefCell<Vec<Dom<SourceBuffer>>>,
    /// https://w3c.github.io/media-source/#dom-mediasource-activesourcebuffers
    source_buffers_list: MutNullableDom<SourceBufferList>,
    /// https://w3c.github.io/media-source/#dom-mediasource-activesourcebuffers
    active_source_buffers_list: MutNullableDom<SourceBufferList>,
    /// https://w3c.github.io/media-source/#dom-readystate
    ready_state: Cell<ReadyState>,
    /// https://w3c.github.io/media-source/#dom-mediasource-duration
    duration: Cell<f64>,
    /// https://w3c.github.io/media-source/#live-seekable-range
    live_seekable_range: MutNullableDom<TimeRanges>,
}

impl MediaSourceAttributes {
    pub fn new() -> Self {
        Self {
            owner: Default::default(),
            source_buffers: Default::default(),
            source_buffers_list: Default::default(),
            active_source_buffers_list: Default::default(),
            ready_state: Cell::new(ReadyState::Closed),
            duration: Cell::new(f64::NAN),
            live_seekable_range: Default::default(),
        }
    }

    pub fn set_owner(&self, owner: &MediaSource) {
        self.owner.set(Some(owner));
    }
}

impl GeckoMediaSourceImpl for MediaSourceAttributes {
    fn get_ready_state(&self) -> i32 {
        self.ready_state.get().into()
    }

    fn set_ready_state(&self, ready_state: i32) {
        match self.owner.get() {
            Some(owner) => owner.set_ready_state(ready_state.into()),
            None => warn!("Could not set ready state. MediaSource gone."),
        };
    }

    fn get_duration(&self) -> f64 {
        self.duration.get()
    }

    fn has_live_seekable_range(&self) -> bool {
        self.live_seekable_range.get().is_some()
    }

    fn get_live_seekable_range(&self) -> GeckoMediaTimeInterval {
        let mut start = 0.;
        let mut end = 0.;
        if let Some(time_ranges) = self.live_seekable_range.get() {
            let ranges = time_ranges.ranges();
            if !ranges.is_empty() {
                let range = ranges.get(0).ok_or(Some(start..end)).unwrap();
                start = *range.start;
                end = *range.end;
            }
        }
        GeckoMediaTimeInterval {
            mStart: start,
            mEnd: end,
        }
    }

    fn get_source_buffers(&self) -> *mut usize {
        match self.owner.get() {
            Some(owner) => {
                let id = Box::new(
                    self.source_buffers_list
                        .or_init(|| SourceBufferList::new(&*owner, ListMode::All))
                        .id(),
                );
                Box::into_raw(id)
            },
            None => ptr::null_mut(),
        }
    }

    fn get_active_source_buffers(&self) -> *mut usize {
        match self.owner.get() {
            Some(ref owner) => {
                let id = Box::new(
                    self.source_buffers_list
                        .or_init(|| SourceBufferList::new(&*owner, ListMode::Active))
                        .id(),
                );
                Box::into_raw(id)
            },
            None => ptr::null_mut(),
        }
    }
}

/// A `MediaSource` DOM instance.
///
/// https://w3c.github.io/media-source/#idl-def-mediasource
#[dom_struct]
pub struct MediaSource {
    eventtarget: EventTarget,
    #[ignore_malloc_size_of = "Rc"]
    attributes: Rc<MediaSourceAttributes>,
    #[ignore_malloc_size_of = "Defined in GeckoMedia"]
    gecko_media: GeckoMediaSource,
}

impl MediaSource {
    fn new(window: &Window) -> DomRoot<Self> {
        reflect_dom_object(
            Box::new(Self::new_inherited()),
            window,
            MediaSourceBinding::Wrap,
        )
    }

    #[allow(unrooted_must_root)]
    fn new_inherited() -> Self {
        let attributes = Rc::new(MediaSourceAttributes::new());
        let weak_attributes = Rc::downgrade(&(&attributes));
        let this = Self {
            attributes: attributes.clone(),
            eventtarget: EventTarget::new_inherited(),
            gecko_media: GeckoMedia::create_media_source(weak_attributes).unwrap(),
        };
        attributes.set_owner(&this);
        this
    }

    pub fn id(&self) -> usize {
        self.gecko_media.get_id()
    }

    pub fn source_buffers<'a>(&'a self) -> Ref<'a, [Dom<SourceBuffer>]> {
        Ref::map(
            self.attributes.source_buffers.borrow(),
            |buffers| &**buffers,
        )
    }

    pub fn append_source_buffer(&self, source_buffer: &SourceBuffer, notify: bool) {
        self.attributes.source_buffers.borrow_mut().push(
            Dom::from_ref(
                &*source_buffer,
            ),
        );
        if !notify {
            return;
        }
        // TODO(nox): If we do our own `Runnable`, we could avoid creating
        // the `sourceBuffers` object if the user doesn't access it.
        let global = self.global();
        let window = global.as_window();
        window.dom_manipulation_task_source().queue_simple_event(
            self.SourceBuffers().upcast(),
            atom!("addsourcebuffer"),
            &window,
        );
    }

    pub fn clear_source_buffers(&self, list_mode: &ListMode) {
        let mut source_buffers = self.attributes.source_buffers.borrow_mut();
        match *list_mode {
            ListMode::All => source_buffers.clear(),
            ListMode::Active => {
                source_buffers.retain(|ref buffer| !buffer.is_active());
            },
        };
    }

    fn parse_mime_type(input: &str) -> Option<Mime> {
        let _mime = match input.parse::<Mime>() {
            Ok(mime) => mime,
            Err(_) => return None,
        };

        if let Ok(gecko_media) = GeckoMedia::get() {
            if gecko_media.is_type_supported(input) {
                return Some(_mime);
            }
        }

        None
    }

    pub fn set_ready_state(&self, ready_state: ReadyState) {
        // https://w3c.github.io/media-source/#mediasource-events
        let old_state = self.attributes.ready_state.get();
        self.attributes.ready_state.set(ready_state);

        let event =
            if ready_state == ReadyState::Open && (old_state == ReadyState::Closed || old_state == ReadyState::Ended) {
                if old_state == ReadyState::Ended {
                    // Notify reader that more data may come.
                    self.gecko_media.decoder_ended(false);
                }

                // readyState transitions from "closed" to "open" or from "ended" to "open".
                atom!("sourceopen")
            } else if ready_state == ReadyState::Ended && old_state == ReadyState::Open {
                // readyState transitions from "open" to "ended".
                atom!("sourceended")
            } else if ready_state == ReadyState::Closed &&
                       (old_state == ReadyState::Open || old_state == ReadyState::Ended)
            {
                // readyState transitions from "open" to "closed" or "ended" to "closed".
                atom!("sourceclose")
            } else {
                unreachable!("Invalid MediaSource readyState transition");
            };

        let window = DomRoot::downcast::<Window>(self.global()).unwrap();

        window.dom_manipulation_task_source().queue_simple_event(
            self.upcast(),
            event,
            &window,
        );
    }
}

impl MediaSource {
    pub fn Constructor(window: &Window) -> Fallible<DomRoot<Self>> {
        Ok(Self::new(window))
    }

    /// https://w3c.github.io/media-source/#dom-mediasource-istypesupported
    pub fn IsTypeSupported(_: &Window, type_: DOMString) -> bool {
        if let Ok(gecko_media) = GeckoMedia::get() {
            gecko_media.is_type_supported(&type_)
        } else {
            false
        }
    }
}

impl MediaSourceMethods for MediaSource {
    /// https://w3c.github.io/media-source/#dom-mediasource-sourcebuffers
    fn SourceBuffers(&self) -> DomRoot<SourceBufferList> {
        self.attributes.source_buffers_list.or_init(|| {
            SourceBufferList::new(self, ListMode::All)
        })
    }

    /// https://w3c.github.io/media-source/#dom-mediasource-sourcebuffers
    fn ActiveSourceBuffers(&self) -> DomRoot<SourceBufferList> {
        self.attributes.active_source_buffers_list.or_init(|| {
            SourceBufferList::new(self, ListMode::Active)
        })
    }

    /// https://w3c.github.io/media-source/#dom-readystate
    fn ReadyState(&self) -> ReadyState {
        self.attributes.ready_state.get()
    }

    /// https://w3c.github.io/media-source/#dom-mediasource-duration
    fn Duration(&self) -> f64 {
        // Step 1.
        if self.attributes.ready_state.get() == ReadyState::Closed {
            return f64::NAN;
        }
        // Step 2.
        self.attributes.duration.get()
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
        if self.attributes.ready_state.get() != ReadyState::Open {
            return Err(Error::InvalidState);
        }

        // Step 3.
        if self.source_buffers().iter().any(
            |buffer| buffer.is_active(),
        )
        {
            return Err(Error::InvalidState);
        }

        // Step 4.
        self.duration_change(value)
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

        // Step 3.
        if self.attributes.source_buffers.borrow().len() >= MAX_SOURCE_BUFFERS {
            return Err(Error::QuotaExceeded);
        }

        // Step 4.
        if self.attributes.ready_state.get() != ReadyState::Open {
            return Err(Error::InvalidState);
        }

        // Steps 5-7.
        let source_buffer = SourceBuffer::new(self, mime);

        // Step 8.
        self.append_source_buffer(
            &source_buffer,
            true, /* trigger addsourcebuffer event */
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
            task_source.queue_simple_event(source_buffer.upcast(), atom!("abort"), &window);

            // Step 2.4.
            task_source.queue_simple_event(source_buffer.upcast(), atom!("updateend"), &window);
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
        self.attributes.source_buffers.borrow_mut().remove(position);
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
        if self.attributes.ready_state.get() != ReadyState::Open {
            return Err(Error::InvalidState);
        }

        // Step 2.
        if self.attributes.source_buffers.borrow().iter().any(
            |buffer| {
                buffer.Updating()
            },
        )
        {
            return Err(Error::InvalidState);
        }

        // Step 3.
        self.end_of_stream(error)
    }

    /// https://w3c.github.io/media-source/#dom-mediasource-setliveseekablerange
    fn SetLiveSeekableRange(&self, start: Finite<f64>, end: Finite<f64>) -> ErrorResult {
        // Step 1.
        if self.attributes.ready_state.get() != ReadyState::Open {
            return Err(Error::InvalidState);
        }

        // Step 2.
        if *start < 0. {
            return Err(Error::Type("start should not be negative".to_owned()));
        }
        if *start > *end {
            return Err(Error::Type(
                "start should not be greater than end".to_owned(),
            ));
        }

        // Step 3.
        self.attributes.live_seekable_range.set(
            Some(&TimeRanges::new(
                self.global()
                    .as_window(),
                vec![start..end],
            )),
        );

        Ok(())
    }


    /// https://w3c.github.io/media-source/#dom-mediasource-endofstream
    fn ClearLiveSeekableRange(&self) -> ErrorResult {
        // Step 1.
        if self.attributes.ready_state.get() != ReadyState::Open {
            return Err(Error::InvalidState);
        }

        // Step 2.
        if let Some(time_ranges) = self.attributes.live_seekable_range.get() {
            if !time_ranges.ranges().is_empty() {
                self.attributes.live_seekable_range.set(
                    Some(&TimeRanges::new(
                        self.global()
                            .as_window(),
                        vec![],
                    )),
                );
            }
        }

        Ok(())
    }
}

impl MediaSource {
    /// https://w3c.github.io/media-source/#duration-change-algorithm
    fn duration_change(&self, new_duration: f64) -> ErrorResult {
        // Step 1.
        if self.attributes.duration.get() == new_duration {
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
        self.attributes.duration.set(new_duration);

        // Step 6.
        self.gecko_media.duration_change(new_duration);

        Ok(())
    }

    /// https://w3c.github.io/media-source/#end-of-stream-algorithm
    pub fn end_of_stream(&self, error: Option<EndOfStreamError>) -> ErrorResult {
        // Step 1 and step 2.
        self.set_ready_state(ReadyState::Ended);

        // Step 3.
        match error {
            Some(error) => {
                self.gecko_media.end_of_stream_error(error.into());
            },
            None => {
                let _ = self.duration_change(self.highest_end_time())?;
                // Notify reader that all data is now available.
                self.gecko_media.decoder_ended(true);
            },
        }

        Ok(())
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

impl From<ReadyState> for i32 {
    fn from(ready_state: ReadyState) -> Self {
        match ready_state {
            ReadyState::Closed => 0,
            ReadyState::Open => 1,
            ReadyState::Ended => 2,
        }
    }
}

impl From<i32> for ReadyState {
    fn from(ready_state: i32) -> Self {
        match ready_state {
            0 => ReadyState::Closed,
            1 => ReadyState::Open,
            2 => ReadyState::Ended,
            _ => unreachable!(),
        }
    }
}

impl From<EndOfStreamError> for i32 {
    fn from(error: EndOfStreamError) -> Self {
        match error {
            EndOfStreamError::Network => 0,
            EndOfStreamError::Decode => 1,
        }
    }
}
