/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! The `SourceBufferList` DOM implementation.

use dom::bindings::codegen::Bindings::SourceBufferListBinding;
use dom::bindings::codegen::Bindings::SourceBufferListBinding::SourceBufferListMethods;
use dom::bindings::reflector::{DomObject, reflect_dom_object};
use dom::bindings::root::{Dom, DomRoot};
use dom::eventtarget::EventTarget;
use dom::mediasource::MediaSource;
use dom::sourcebuffer::SourceBuffer;
use dom_struct::dom_struct;
use gecko_media::GeckoMedia;
use gecko_media::{GeckoMediaSourceBufferList, GeckoMediaSourceBufferListImpl};
use std::os::raw::c_void;
use std::ptr;
use std::rc::Rc;

#[derive(JSTraceable, MallocSizeOf)]
#[allow(unrooted_must_root)]
pub struct SourceBufferListInner {
    media_source: Dom<MediaSource>,
    list_mode: ListMode,
}

impl SourceBufferListInner {
    fn new(media_source: &MediaSource, list_mode: ListMode) -> Self {
        Self {
            media_source: Dom::from_ref(media_source),
            list_mode,
        }
    }
}

impl GeckoMediaSourceBufferListImpl for SourceBufferListInner {
    #[allow(unsafe_code)]
    fn indexed_getter(&self, index: u32, source_buffer: *mut usize) -> bool {
        if source_buffer == ptr::null_mut() {
            return false;
        }
        let buffers = self.media_source.source_buffers();
        if index as usize >= buffers.len() {
            return false;
        }
        let buffer = match self.list_mode {
            ListMode::All => Some(&*buffers[index as usize]),
            ListMode::Active => {
                buffers
                    .iter()
                    .filter(|buffer| buffer.is_active())
                    .nth(index as usize)
                    .map(|buffer| &**buffer)
            },
        };
        match buffer {
            Some(buffer) => {
                unsafe {
                    *source_buffer = buffer.id();
                }
                true
            },
            None => false,
        }
    }

    fn length(&self) -> u32 {
        let buffers = self.media_source.source_buffers();
        match self.list_mode {
            ListMode::All => buffers.len() as u32,
            ListMode::Active => {
                // FIXME(nox): Inefficient af, should cache the number of
                // active source buffers directly in the MediaSource instance.
                buffers.iter().filter(|buffer| buffer.is_active()).count() as u32
            },
        }
    }

    #[allow(unsafe_code)]
    fn append(&self, source_buffer: *mut c_void, notify: bool) {
        let source_buffer: &SourceBuffer = unsafe { &*(source_buffer as *mut SourceBuffer) };
        self.media_source.append_source_buffer(
            source_buffer,
            notify,
        );
    }

    fn clear(&self) {
        self.media_source.clear_source_buffers(&self.list_mode);
    }
}

/// A `SourceBufferList` DOM instance.
///
/// https://w3c.github.io/media-source/#idl-def-sourcebufferlist
#[dom_struct]
pub struct SourceBufferList {
    eventtarget: EventTarget,
    #[ignore_malloc_size_of = "Rc"]
    inner: Rc<SourceBufferListInner>,
    #[ignore_malloc_size_of = "Defined in GeckoMedia"]
    gecko_media: GeckoMediaSourceBufferList,
}

#[derive(MallocSizeOf, JSTraceable)]
pub enum ListMode {
    All,
    Active,
}

impl SourceBufferList {
    fn new_inherited(media_source: &MediaSource, list_mode: ListMode) -> Self {
        let inner = Rc::new(SourceBufferListInner::new(&media_source, list_mode));
        let inner_weak = Rc::downgrade(&(&inner));
        Self {
            eventtarget: EventTarget::new_inherited(),
            inner,
            gecko_media: GeckoMedia::create_source_buffer_list(inner_weak).unwrap(),
        }
    }

    pub fn new(media_source: &MediaSource, list_mode: ListMode) -> DomRoot<Self> {
        reflect_dom_object(
            Box::new(Self::new_inherited(media_source, list_mode)),
            &*media_source.global(),
            SourceBufferListBinding::Wrap,
        )
    }

    pub fn id(&self) -> usize {
        self.gecko_media.get_id()
    }
}

impl SourceBufferListMethods for SourceBufferList {
    /// https://w3c.github.io/media-source/#dom-sourcebufferlist-length
    fn Length(&self) -> u32 {
        self.inner.length()
    }

    event_handler!(addsourcebuffer, GetOnaddsourcebuffer, SetOnaddsourcebuffer);

    event_handler!(
        removesourcebuffer,
        GetOnremovesourcebuffer,
        SetOnremovesourcebuffer
    );

    /// https://w3c.github.io/media-source/#dfn-sourcebufferlist-getter
    fn IndexedGetter(&self, index: u32) -> Option<DomRoot<SourceBuffer>> {
        let buffers = self.inner.media_source.source_buffers();
        if index as usize >= buffers.len() {
            return None;
        }
        match self.inner.list_mode {
            ListMode::All => Some(DomRoot::from_ref(&*buffers[index as usize])),
            ListMode::Active => {
                // FIXME(nox): Inefficient af, should have a cache to the last
                // accessed active source buffer.
                buffers
                    .iter()
                    .filter(|buffer| buffer.is_active())
                    .nth(index as usize)
                    .map(|buffer| DomRoot::from_ref(&**buffer))
            },
        }
    }
}
