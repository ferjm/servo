/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! The `SourceBuffer` DOM implementation.

use dom::bindings::codegen::Bindings::MediaSourceBinding::EndOfStreamError;
use dom::bindings::codegen::Bindings::MediaSourceBinding::MediaSourceMethods;
use dom::bindings::codegen::Bindings::MediaSourceBinding::ReadyState;
use dom::bindings::codegen::Bindings::SourceBufferBinding;
use dom::bindings::codegen::Bindings::SourceBufferBinding::AppendMode;
use dom::bindings::codegen::Bindings::SourceBufferBinding::SourceBufferMethods;
use dom::bindings::error::{Error, ErrorResult, Fallible};
use dom::bindings::inheritance::Castable;
use dom::bindings::num::Finite;
use dom::bindings::reflector::{DomObject, reflect_dom_object};
use dom::bindings::root::{DomRoot, MutNullableDom};
use dom::eventtarget::EventTarget;
use dom::mediasource::MediaSource;
use dom::window::Window;
use dom_struct::dom_struct;
use gecko_media::{GeckoMedia, GeckoMediaSourceBuffer, GeckoMediaSourceBufferImpl};
use js::jsapi::{JSContext, JSObject, Rooted};
use js::typedarray::{ArrayBuffer, ArrayBufferView};
use mime::{Mime, SubLevel, TopLevel};
use std::cell::Cell;
use std::f64;
use std::os::raw::c_void;
use std::ptr;
use std::rc::Rc;

#[derive(JSTraceable, MallocSizeOf)]
#[allow(unrooted_must_root)]
pub struct SourceBufferAttributes {
    #[ignore_malloc_size_of = "Rc"]
    owner: MutNullableDom<SourceBuffer>,
    /// https://w3c.github.io/media-source/#dom-sourcebuffer-appendwindowstart
    append_window_start: Cell<Finite<f64>>,
    /// https://w3c.github.io/media-source/#dom-sourcebuffer-appendwindowend
    append_window_end: Cell<f64>,
    /// https://w3c.github.io/media-source/#dom-sourcebuffer-timestampoffset
    timestamp_offset: Cell<Finite<f64>>,
    /// https://w3c.github.io/media-source/#dom-sourcebuffer-mode
    append_mode: Cell<AppendMode>,
    /// https://w3c.github.io/media-source/#sourcebuffer-generate-timestamps-flag
    timestamp_mode: TimestampMode,
    /// https://w3c.github.io/media-source/#sourcebuffer-append-state
    append_state: Cell<AppendState>,
    /// https://w3c.github.io/media-source/#sourcebuffer-group-start-timestamp
    group_start_timestamp: Cell<Option<Finite<f64>>>,
    /// https://w3c.github.io/media-source/#sourcebuffer-group-end-timestamp
    group_end_timestamp: Cell<Finite<f64>>,
    /// https://w3c.github.io/media-source/#dom-sourcebuffer-updating
    updating: Cell<bool>,
    active: Cell<bool>,
}

impl SourceBufferAttributes {
    fn new(timestamp_mode: TimestampMode) -> Self {
        SourceBufferAttributes {
            owner: Default::default(),
            // FIXME(nox): This assumes that the presentation start time is 0.
            append_window_start: Default::default(),
            append_window_end: Cell::new(f64::INFINITY),
            timestamp_offset: Default::default(),
            append_mode: Cell::new(timestamp_mode.into()),
            timestamp_mode,
            append_state: Cell::new(AppendState::WaitingForSegment),
            group_start_timestamp: Default::default(),
            group_end_timestamp: Default::default(),
            updating: Default::default(),
            active: Cell::new(false),
        }
    }

    fn set_owner(&self, owner: &SourceBuffer) {
        self.owner.set(Some(owner));
    }
}

impl GeckoMediaSourceBufferImpl for SourceBufferAttributes {
    fn owner(&self) -> *mut c_void {
        match self.owner.get() {
            None => ptr::null_mut(),
            Some(ref mut owner) => owner as *mut _ as *mut c_void,
        }
    }

    fn get_append_window_start(&self) -> f64 {
        *self.append_window_start.get()
    }

    fn set_append_window_start(&self, value: f64) {
        self.append_window_start.set(Finite::wrap(value));
    }

    fn get_append_window_end(&self) -> f64 {
        self.append_window_end.get()
    }

    fn set_append_window_end(&self, value: f64) {
        self.append_window_end.set(value);
    }

    fn get_timestamp_offset(&self) -> f64 {
        *self.timestamp_offset.get()
    }

    fn set_timestamp_offset(&self, value: f64) {
        self.timestamp_offset.set(Finite::wrap(value));
    }

    fn get_append_mode(&self) -> i32 {
        self.append_mode.get().into()
    }

    fn set_append_mode(&self, value: i32) {
        self.append_mode.set(value.into());
    }

    #[allow(unsafe_code)]
    fn get_group_start_timestamp(&self, timestamp: *mut f64) {
        if timestamp == ptr::null_mut() {
            return;
        }
        if let Some(timestamp_) = self.group_start_timestamp.get() {
            unsafe { *timestamp = *timestamp_ }
        }
    }

    fn set_group_start_timestamp(&self, value: f64) {
        self.group_start_timestamp.set(Some(Finite::wrap(value)));
    }

    fn have_group_start_timestamp(&self) -> bool {
        self.group_start_timestamp.get().is_some()
    }

    fn reset_group_start_timestamp(&self) {
        self.group_start_timestamp.set(None);
    }

    fn restart_group_start_timestamp(&self) {
        self.group_start_timestamp.set(Some(
            self.group_end_timestamp.get(),
        ));
    }

    fn get_group_end_timestamp(&self) -> f64 {
        *self.group_end_timestamp.get()
    }

    fn set_group_end_timestamp(&self, value: f64) {
        self.group_end_timestamp.set(Finite::wrap(value));
    }

    fn get_append_state(&self) -> i32 {
        self.append_state.get().into()
    }

    fn set_append_state(&self, value: i32) {
        self.append_state.set(value.into());
    }

    fn get_updating(&self) -> bool {
        self.updating.get()
    }

    fn set_updating(&self, updating: bool) {
        if let Some(owner) = self.owner.get() {
            let window = DomRoot::downcast::<Window>(owner.global()).unwrap();
            let event = if updating {
                atom!("updatestart")
            } else {
                atom!("updateend")
            };
            window.dom_manipulation_task_source().queue_simple_event(
                owner.upcast(),
                event,
                &window,
            );
            self.updating.set(updating);
            return;
        }
        unreachable!();

    }

    fn get_active(&self) -> bool {
        self.active.get()
    }

    fn set_active(&self, active: bool) {
        if let Some(owner) = self.owner.get() {
            if let Some(media_source) = owner.parent_media_source.get() {
                let window = DomRoot::downcast::<Window>(owner.global()).unwrap();
                let event = if active {
                    atom!("addsourcebuffer")
                } else {
                    atom!("removesourcebuffer")
                };
                window.dom_manipulation_task_source().queue_simple_event(
                    media_source
                        .ActiveSourceBuffers()
                        .upcast(),
                    event,
                    &window,
                );
                self.active.set(active);
                return;
            }
        }
        unreachable!();
    }

    fn on_data_appended(&self, result: u32) {
        if let Some(owner) = self.owner.get() {
            if result == 0 {
                owner.on_data_appended_success();
            } else {
                owner.on_data_appended_error(result);
            }
            return;
        }
        unreachable!();
    }

    fn on_range_removed(&self) {
        if let Some(owner) = self.owner.get() {
            owner.on_range_removed();
            return;
        }
        unreachable!();
    }
}

/// A `SourceBuffer` DOM instance.
///
/// https://w3c.github.io/media-source/#idl-def-sourcebuffer
#[dom_struct]
pub struct SourceBuffer {
    eventtarget: EventTarget,
    /// https://w3c.github.io/media-source/#parent-media-source
    parent_media_source: MutNullableDom<MediaSource>,
    /// https://w3c.github.io/media-source/#sourcebuffer-buffer-full-flag
    buffer_full: Cell<bool>,
    /// The MIME type provided when that `SourceBuffer` was created.
    #[ignore_malloc_size_of = "defined in mime"]
    mime: Mime,
    /// Whether we are currently running the range removal algorithm.
    in_range_removal: Cell<bool>,
    #[ignore_malloc_size_of = "Rc"]
    attributes: Rc<SourceBufferAttributes>,
    #[ignore_malloc_size_of = "Defined in GeckoMedia"]
    gecko_media: GeckoMediaSourceBuffer,
}

/// https://w3c.github.io/media-source/#sourcebuffer-append-state
#[derive(Clone, Copy, JSTraceable, MallocSizeOf, PartialEq)]
enum AppendState {
    WaitingForSegment,
    ParsingInitSegment,
    ParsingMediaSegment,
}

impl From<AppendState> for i32 {
    fn from(append_state: AppendState) -> Self {
        match append_state {
            AppendState::WaitingForSegment => 0,
            AppendState::ParsingInitSegment => 1,
            AppendState::ParsingMediaSegment => 2,
        }
    }
}

impl From<i32> for AppendState {
    fn from(value: i32) -> Self {
        match value {
            0 => AppendState::WaitingForSegment,
            1 => AppendState::ParsingInitSegment,
            2 => AppendState::ParsingMediaSegment,
            _ => unreachable!(),
        }
    }
}

/// https://w3c.github.io/media-source/#sourcebuffer-generate-timestamps-flag
#[derive(Clone, Copy, JSTraceable, MallocSizeOf, PartialEq)]
enum TimestampMode {
    /// Timestamps are extracted from source.
    FromSource,
    /// Timestamps are generated by the source buffer itself.
    Generated,
}

impl SourceBuffer {
    pub fn new(parent_media_source: &MediaSource, mime: Mime) -> DomRoot<Self> {
        reflect_dom_object(
            Box::new(Self::new_inherited(parent_media_source, mime)),
            &*parent_media_source.global(),
            SourceBufferBinding::Wrap,
        )
    }

    pub fn id(&self) -> usize {
        self.gecko_media.get_id()
    }

    pub fn is_active(&self) -> bool {
        self.attributes.active.get()
    }

    pub fn clear_parent_media_source(&self) {
        debug_assert!(self.parent_media_source.get().is_some());
        self.parent_media_source.set(None);
    }
}

impl SourceBufferMethods for SourceBuffer {
    /// https://w3c.github.io/media-source/#dom-sourcebuffer-mode
    fn Mode(&self) -> AppendMode {
        self.attributes.append_mode.get()
    }

    /// https://w3c.github.io/media-source/#dom-sourcebuffer-mode
    fn SetMode(&self, new_mode: AppendMode) -> ErrorResult {
        // Step 1.
        let parent_media_source = match self.parent_media_source.get() {
            Some(source) => source,
            None => return Err(Error::InvalidState),
        };

        // Step 2.
        if self.attributes.updating.get() {
            return Err(Error::InvalidState);
        }

        // Step 3.
        // The argument is already named new_mode.

        // Step 4.
        if self.attributes.timestamp_mode == TimestampMode::Generated && new_mode == AppendMode::Segments {
            return Err(Error::Type("New mode cannot be \"segments\".".to_owned()));
        }

        // Step 5.
        if parent_media_source.ReadyState() == ReadyState::Ended {
            // Step 5.1 and 5.2.
            parent_media_source.set_ready_state(ReadyState::Open);
        }

        // Step 6.
        if self.attributes.append_state.get() == AppendState::ParsingMediaSegment {
            return Err(Error::InvalidState);
        }

        // Step 7.
        if new_mode == AppendMode::Sequence {
            self.attributes.group_start_timestamp.set(Some(
                self.attributes
                    .group_end_timestamp
                    .get(),
            ));
        }

        // Step 8.
        self.attributes.append_mode.set(new_mode);

        Ok(())
    }

    /// https://w3c.github.io/media-source/#dom-sourcebuffer-updating
    fn Updating(&self) -> bool {
        self.attributes.updating.get()
    }

    // TODO Buffered

    /// https://w3c.github.io/media-source/#dom-sourcebuffer-timestampoffset
    fn TimestampOffset(&self) -> Finite<f64> {
        self.attributes.timestamp_offset.get()
    }

    /// https://w3c.github.io/media-source/#dom-sourcebuffer-timestampoffset
    fn SetTimestampOffset(&self, new_timestamp_offset: Finite<f64>) -> ErrorResult {
        // Step 1.
        // The argument is already named new_timestamp_offset.

        // Step 2.
        let parent_media_source = match self.parent_media_source.get() {
            Some(source) => source,
            None => return Err(Error::InvalidState),
        };

        // Step 3.
        if self.attributes.updating.get() {
            return Err(Error::InvalidState);
        }

        // Step 4.
        if parent_media_source.ReadyState() == ReadyState::Ended {
            // Step 4.1. and 4.2.
            parent_media_source.set_ready_state(ReadyState::Open);
        }

        // Step 5.
        if self.attributes.append_state.get() == AppendState::ParsingMediaSegment {
            return Err(Error::InvalidState);
        }

        // Step 6.
        if self.attributes.append_mode.get() == AppendMode::Sequence {
            self.attributes.group_start_timestamp.set(Some(
                new_timestamp_offset,
            ));
        }

        // Step 7.
        self.attributes.timestamp_offset.set(new_timestamp_offset);

        Ok(())
    }

    // TODO AudioTracks.

    // TODO VideoTracks.

    // TODO TextTracks.

    /// https://w3c.github.io/media-source/#dom-sourcebuffer-appendwindowstart
    fn AppendWindowStart(&self) -> Finite<f64> {
        self.attributes.append_window_start.get()
    }

    /// https://w3c.github.io/media-source/#dom-sourcebuffer-appendwindowstart
    fn SetAppendWindowStart(&self, value: Finite<f64>) -> ErrorResult {
        // Step 1.
        if self.parent_media_source.get().is_none() {
            return Err(Error::InvalidState);
        }

        // Step 2.
        if self.attributes.updating.get() {
            return Err(Error::InvalidState);
        }

        // Step 3.
        if *value < 0. || *value >= self.attributes.append_window_end.get() {
            return Err(Error::Type("Value is out of range.".to_owned()));
        }

        // Step 4.
        self.attributes.append_window_start.set(value);

        Ok(())
    }

    /// https://w3c.github.io/media-source/#dom-sourcebuffer-appendwindowend
    fn AppendWindowEnd(&self) -> f64 {
        self.attributes.append_window_end.get()
    }

    /// https://w3c.github.io/media-source/#dom-sourcebuffer-appendwindowend
    fn SetAppendWindowEnd(&self, value: f64) -> ErrorResult {
        // Step 1.
        if self.parent_media_source.get().is_none() {
            return Err(Error::InvalidState);
        }

        // Step 2.
        if self.attributes.updating.get() {
            return Err(Error::InvalidState);
        }

        // Step 3.
        if value.is_nan() {
            return Err(Error::Type("Value is NaN.".to_owned()));
        }

        // Step 4.
        if value <= *self.attributes.append_window_start.get() {
            return Err(Error::Type("Value is out of range.".to_owned()));
        }

        // Step 5.
        self.attributes.append_window_end.set(value);

        Ok(())
    }

    event_handler!(updatestart, GetOnupdatestart, SetOnupdatestart);
    event_handler!(update, GetOnupdate, SetOnupdate);
    event_handler!(updateend, GetOnupdateend, SetOnupdateend);
    event_handler!(error, GetOnerror, SetOnerror);
    event_handler!(abort, GetOnabort, SetOnabort);

    /// https://w3c.github.io/media-source/#dom-sourcebuffer-appendbuffer
    #[allow(unsafe_code)]
    unsafe fn AppendBuffer(&self, cx: *mut JSContext, data: *mut JSObject) -> ErrorResult {
        let mut root_1 = Rooted::new_unrooted();
        let mut root_2 = Rooted::new_unrooted();
        let mut buffer_source = BufferSource::new(cx, &mut root_1, &mut root_2, data)?;
        self.append_buffer(&mut buffer_source)
    }

    /// https://w3c.github.io/media-source/#dom-sourcebuffer-abort
    fn Abort(&self) -> ErrorResult {
        // Step 1.
        let parent_media_source = match self.parent_media_source.get() {
            Some(source) => source,
            None => return Err(Error::InvalidState),
        };

        // Step 2.
        if parent_media_source.ReadyState() != ReadyState::Open {
            return Err(Error::InvalidState);
        }

        // Step 3.
        if self.in_range_removal.get() {
            return Err(Error::InvalidState);
        }

        // Step 4.
        if self.attributes.updating.get() {
            // Step 4.1.
            self.gecko_media.abort_buffer_append();

            // Step 4.2. and 4.4.
            self.attributes.set_updating(false);

            // Step 4.3.
            let window = DomRoot::downcast::<Window>(self.global()).unwrap();
            window.dom_manipulation_task_source().queue_simple_event(
                self.upcast(),
                atom!("abort"),
                &window,
            );
        }

        // Step 5.
        self.gecko_media.reset_parser_state();

        // Step 6.
        // This assumes that presentation start time is always 0.
        self.attributes.append_window_start.set(Default::default());

        // Step 7.
        self.attributes.append_window_end.set(f64::INFINITY);

        Ok(())
    }

    /// https://w3c.github.io/media-source/#dom-sourcebuffer-remove
    fn Remove(&self, start: Finite<f64>, end: f64) -> ErrorResult {
        // Step 1.
        let parent_media_source = match self.parent_media_source.get() {
            Some(source) => source,
            None => return Err(Error::InvalidState),
        };

        // Step 2.
        if self.attributes.updating.get() {
            return Err(Error::InvalidState);
        }

        // Step 3.
        let duration = parent_media_source.Duration();
        if duration.is_nan() {
            return Err(Error::Type(
                "Parent media source's duration is NaN.".to_owned(),
            ));
        }

        // Step 4.
        if *start < 0. || *start > duration {
            return Err(Error::Type("Start value is out of range.".to_owned()));
        }

        // Step 5.
        if end <= *start {
            return Err(Error::Type("End value is out of range.".to_owned()));
        }
        if end.is_nan() {
            return Err(Error::Type("End value is NaN.".to_owned()));
        }

        // Step 6.
        if parent_media_source.ReadyState() == ReadyState::Ended {
            // Step 6.1. and 6.2.
            parent_media_source.set_ready_state(ReadyState::Open);
        }

        // Step 7.
        self.remove_range(start, end);

        Ok(())
    }
}

impl SourceBuffer {
    #[allow(unrooted_must_root)]
    fn new_inherited(parent_media_source: &MediaSource, mime: Mime) -> Self {
        let timestamp_mode = Self::timestamp_mode(&mime);
        let generate_timestamps = timestamp_mode == TimestampMode::Generated;
        let attributes = Rc::new(SourceBufferAttributes::new(timestamp_mode));
        let weak_attributes = Rc::downgrade(&(&attributes));
        let this = Self {
            eventtarget: EventTarget::new_inherited(),
            parent_media_source: MutNullableDom::new(Some(parent_media_source)),
            buffer_full: Default::default(),
            mime: mime.clone(),
            in_range_removal: Default::default(),
            attributes: attributes.clone(),
            gecko_media: GeckoMedia::create_source_buffer(
                weak_attributes,
                parent_media_source.id(),
                &mime.to_string(),
                generate_timestamps,
            ).unwrap(),
        };
        attributes.set_owner(&this);
        this
    }

    /// https://w3c.github.io/media-source/byte-stream-format-registry.html
    fn timestamp_mode(mime: &Mime) -> TimestampMode {
        match *mime {
            Mime(TopLevel::Audio, SubLevel::Mpeg, _) => TimestampMode::Generated,
            Mime(TopLevel::Audio, SubLevel::Ext(ref ext), _) if ext.eq_ignore_ascii_case("aac") => {
                TimestampMode::Generated
            },
            _ => TimestampMode::FromSource,
        }
    }

    /// https://w3c.github.io/media-source/#dom-sourcebuffer-appendbuffer
    #[allow(unsafe_code)]
    fn append_buffer(&self, buffer_source: &mut BufferSource) -> ErrorResult {
        // Step 1.
        self.prepare_append(buffer_source)?;

        // Step 3 and 4.
        self.attributes.set_updating(true);

        // Step 2 and 5.
        self.buffer_append(buffer_source);

        Ok(())
    }

    /// https://w3c.github.io/media-source/#sourcebuffer-prepare-append
    #[allow(unsafe_code)]
    fn prepare_append(&self, buffer_source: &mut BufferSource) -> ErrorResult {
        // Step 1.
        let parent_media_source = match self.parent_media_source.get() {
            Some(source) => source,
            None => return Err(Error::InvalidState),
        };

        // Step 2.
        if self.attributes.updating.get() {
            return Err(Error::InvalidState);
        }

        // Step 3.
        // FIXME(nox): Check HTMLMediaElement.error.

        // Step 4.
        if parent_media_source.ReadyState() == ReadyState::Ended {
            // Step 4.1. and 4.2.
            parent_media_source.set_ready_state(ReadyState::Open);
        }

        // Step 5.
        self.evict_coded_frames(
            unsafe { buffer_source.as_slice().len() },
        )?;

        // Step 6.
        if self.buffer_full.get() {
            return Err(Error::QuotaExceeded);
        }

        Ok(())
    }

    /// https://w3c.github.io/media-source/#sourcebuffer-buffer-append
    #[allow(unsafe_code)]
    fn buffer_append(&self, buffer_source: &mut BufferSource) {
        // Step 1.
        unsafe {
            self.gecko_media.append_data(
                buffer_source.as_slice().as_ptr(),
                buffer_source.as_slice().len(),
            );
        }
        // Step 2 is run in on_data_appended_error.
        // Steps 3 to 5 are run in on_data_appended_success.
    }

    /// Steps 3 to 5 of https://w3c.github.io/media-source/#sourcebuffer-buffer-append
    pub fn on_data_appended_success(&self) {
        if !self.attributes.get_updating() {
            // The buffer append or range removal algorithm has been interrupted
            // by abort().
            return;
        }
        // Step 3 and 5.
        self.attributes.set_updating(false);

        // Step 4.
        let window = DomRoot::downcast::<Window>(self.global()).unwrap();
        window.dom_manipulation_task_source().queue_simple_event(
            self.upcast(),
            atom!("update"),
            &window,
        );
    }

    /// https://w3c.github.io/media-source/#sourcebuffer-append-error
    pub fn on_data_appended_error(&self, _: u32) {
        // Step 1 is run in gecko-media SourceBuffer::ApendDataErrored.

        // Steps 2 and 4.
        self.attributes.set_updating(false);

        // Step 3.
        let window = DomRoot::downcast::<Window>(self.global()).unwrap();
        window.dom_manipulation_task_source().queue_simple_event(
            self.upcast(),
            atom!("error"),
            &window,
        );

        // Step 5.
        if let Some(media_source) = self.parent_media_source.get() {
            let _ = media_source.end_of_stream(Some(EndOfStreamError::Decode));
        }
    }

    /// https://w3c.github.io/media-source/#sourcebuffer-coded-frame-eviction
    fn evict_coded_frames(&self, buffer_len: usize) -> ErrorResult {
        // Step 1.
        // Gecko only cares about the length of the about to be appended data,
        // which is buffer_len.

        // Step 2.
        if !self.buffer_full.get() {
            return Ok(());
        }

        // Steps 3 and 4.
        let mut buffer_full = true;
        self.gecko_media.evict_coded_frames(
            buffer_len,
            &mut buffer_full,
        );
        self.buffer_full.set(buffer_full);

        Ok(())
    }

    /// https://w3c.github.io/media-source/#sourcebuffer-range-removal
    fn remove_range(&self, start: Finite<f64>, end: f64) {
        // Mark this SourceBuffer as being running the range removal algorithm,
        // so that the abort() method properly throws an exception.
        assert!(self.attributes.updating.get());
        self.in_range_removal.set(true);

        // Steps 1-2.
        // We assume that presentation start time is 0, thus we can just use
        // the arguments directly.

        // Step 3 and 4.
        self.attributes.set_updating(true);

        // Step 5 and 6.
        self.gecko_media.range_removal(*start, end);
    }

    /// https://w3c.github.io/media-source/#sourcebuffer-range-removal
    pub fn on_range_removed(&self) {
        // Step 7 and 9.
        self.attributes.set_updating(false);

        // Step 8.
        let window = DomRoot::downcast::<Window>(self.global()).unwrap();
        window.dom_manipulation_task_source().queue_simple_event(
            self.upcast(),
            atom!("update"),
            &window,
        );

        // FIXME(nox): I'm not too sure exactly if this should be done
        // at the very end of the range removal algorithm.
        self.in_range_removal.set(false);
    }
}

impl From<AppendMode> for i32 {
    fn from(append_mode: AppendMode) -> Self {
        match append_mode {
            AppendMode::Segments => 0,
            AppendMode::Sequence => 1,
        }
    }
}

impl From<i32> for AppendMode {
    fn from(value: i32) -> Self {
        match value {
            0 => AppendMode::Segments,
            1 => AppendMode::Sequence,
            _ => unreachable!(),
        }
    }
}

impl From<TimestampMode> for AppendMode {
    fn from(timestamp_mode: TimestampMode) -> Self {
        match timestamp_mode {
            TimestampMode::FromSource => AppendMode::Segments,
            TimestampMode::Generated => AppendMode::Sequence,
        }
    }
}

enum BufferSource<'root> {
    ArrayBuffer(ArrayBuffer<'root>),
    ArrayBufferView(ArrayBufferView<'root>),
}

impl<'root> BufferSource<'root> {
    #[allow(unsafe_code)]
    unsafe fn new(
        cx: *mut JSContext,
        root_1: &'root mut Rooted<*mut JSObject>,
        root_2: &'root mut Rooted<*mut JSObject>,
        data: *mut JSObject,
    ) -> Fallible<Self> {
        if let Ok(array) = ArrayBuffer::from(cx, root_1, data) {
            return Ok(BufferSource::ArrayBuffer(array));
        } else if let Ok(view) = ArrayBufferView::from(cx, root_2, data) {
            return Ok(BufferSource::ArrayBufferView(view));
        }
        Err(Error::Type(
            "Object should be an ArrayBuffer or ArrayBufferView."
                .to_owned(),
        ))
    }

    #[allow(unsafe_code)]
    unsafe fn as_slice(&mut self) -> &[u8] {
        match *self {
            BufferSource::ArrayBuffer(ref mut array) => array.as_slice(),
            BufferSource::ArrayBufferView(ref mut view) => view.as_slice(),
        }
    }
}
