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

/// A `SourceBufferList` DOM instance.
///
/// https://w3c.github.io/media-source/#idl-def-sourcebufferlist
#[dom_struct]
pub struct SourceBufferList {
    eventtarget: EventTarget,
    media_source: Dom<MediaSource>,
    list_mode: ListMode,
}

#[derive(MallocSizeOf, JSTraceable)]
pub enum ListMode {
    All,
    Active,
}

impl SourceBufferList {
    fn new_inherited(media_source: &MediaSource, list_mode: ListMode) -> Self {
        Self {
            eventtarget: EventTarget::new_inherited(),
            media_source: Dom::from_ref(media_source),
            list_mode,
        }
    }

    pub fn new(media_source: &MediaSource, list_mode: ListMode) -> DomRoot<Self> {
        reflect_dom_object(
            Box::new(Self::new_inherited(media_source, list_mode)),
            &*media_source.global(),
            SourceBufferListBinding::Wrap,
        )
    }
}

impl SourceBufferListMethods for SourceBufferList {
    /// https://w3c.github.io/media-source/#dom-sourcebufferlist-length
    fn Length(&self) -> u32 {
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

    event_handler!(
        addsourcebuffer,
        GetOnaddsourcebuffer,
        SetOnaddsourcebuffer
    );

    event_handler!(
        removesourcebuffer,
        GetOnremovesourcebuffer,
        SetOnremovesourcebuffer
    );

    /// https://w3c.github.io/media-source/#dfn-sourcebufferlist-getter
    fn IndexedGetter(&self, index: u32) -> Option<DomRoot<SourceBuffer>> {
        let buffers = self.media_source.source_buffers();
        if index as usize >= buffers.len() {
            return None;
        }
        match self.list_mode {
            ListMode::All => Some(DomRoot::from_ref(&*buffers[index as usize])),
            ListMode::Active => {
                // FIXME(nox): Inefficient af, should have a cache to the last
                // accessed active source buffer.
                buffers
                    .iter()
                    .filter(|buffer| buffer.is_active())
                    .nth(index as usize)
                    .map(|buffer| DomRoot::from_ref(&**buffer))
            }
        }
    }
}
