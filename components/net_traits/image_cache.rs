/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use FetchResponseMsg;
use image::base::{Image, ImageMetadata};
use NetworkError;
use servo_url::ServoUrl;
use std::collections::HashMap;
use std::mem;
use std::sync::{Arc, RwLock};

// Represents all the currently pending loads/decodings. For
// performance reasons, loads are indexed by a dedicated load key.
struct AllPendingLoads {
    // The loads, indexed by a load key. Used during most operations,
    // for performance reasons.
    loads: RwLock<HashMap<LoadKey, PendingLoad>>,

    // Get a load key from its url. Used ony when starting and
    // finishing a load or when adding a new listener.
    url_to_load_key: RwLock<HashMap<ServoUrl, LoadKey>>,

    // A counter used to generate instances of LoadKey
    keygen: LoadKeyGenerator,
}

impl AllPendingLoads {
    fn new() -> AllPendingLoads {
        AllPendingLoads {
            loads: RwLock::new(HashMap::new()),
            url_to_load_key: RwLock::new(HashMap::new()),
            keygen: LoadKeyGenerator::new(),
        }
    }

    /// `true` if there is no currently pending load, `false` otherwise.
    fn is_empty(&self) -> bool {
        let loads = self.loads.read().unwrap();
        assert!(loads.is_empty() == self.url_to_load_key.read().unwrap().is_empty());
        loads.is_empty()
    }

    /*XXX
    /// Get aPendingLoad from its LoadKey.
    fn get_by_key_mut(&mut self, key: &LoadKey) -> Option<&mut PendingLoad> {
        self.loads.read().unwrap().get_mut(key)
    }*/

    /// Remove a PendingLoad given its LoadKey.
    fn remove(&mut self, key: &LoadKey) -> Option<PendingLoad> {
        self.loads.write().unwrap().remove(key).
            and_then(|pending_load| {
                self.url_to_load_key.write().unwrap().remove(&pending_load.url).unwrap();
                Some(pending_load)
            })
    }
/*
    fn get_cached<'a>(&'a mut self, url: ServoUrl, can_request: CanRequestImages)
                      -> CacheResult<'a> {
        match self.url_to_load_key.read().unwrap().entry(url.clone()) {
            Occupied(url_entry) => {
                let load_key = url_entry.get();
                CacheResult::Hit(*load_key, self.loads.get_mut(load_key).unwrap())
            }
            Vacant(url_entry) => {
                if can_request == CanRequestImages::No {
                    return CacheResult::Miss(None);
                }

                let load_key = self.keygen.next();
                url_entry.insert(load_key);

                let pending_load = PendingLoad::new(url);
                match self.loads.entry(load_key) {
                    Occupied(_) => unreachable!(),
                    Vacant(load_entry) => {
                        let mut_load = load_entry.insert(pending_load);
                        CacheResult::Miss(Some((load_key, mut_load)))
                    }
                }
            }
        }
    }*/
}

/// Whether a consumer is in a position to request images or not. This can
/// occur when animations are being processed by the layout thread while the
/// script thread is executing in parallel.
#[derive(Copy, Clone, PartialEq, Deserialize, Serialize)]
pub enum CanRequestImages {
    No,
    Yes,
}

/// Represents an image that has completed loading.
/// Images that fail to load (due to network or decode
/// failure) are still stored here, so that they aren't
/// fetched again.
struct CompletedLoad {
    image_response: ImageResponse,
    id: PendingImageId,
}

impl CompletedLoad {
    fn new(image_response: ImageResponse, id: PendingImageId) -> CompletedLoad {
        CompletedLoad {
            image_response: image_response,
            id: id,
        }
    }
}

enum ImageBytes {
    InProgress(Vec<u8>),
    Complete(Arc<Vec<u8>>),
}

impl ImageBytes {
    fn extend_from_slice(&mut self, data: &[u8]) {
        match *self {
            ImageBytes::InProgress(ref mut bytes) => bytes.extend_from_slice(data),
            ImageBytes::Complete(_) => panic!("attempted modification of complete image bytes"),
        }
    }

    fn mark_complete(&mut self) -> Arc<Vec<u8>> {
        let bytes = {
            let own_bytes = match *self {
                ImageBytes::InProgress(ref mut bytes) => bytes,
                ImageBytes::Complete(_) => panic!("attempted modification of complete image bytes"),
            };
            mem::replace(own_bytes, vec![])
        };
        let bytes = Arc::new(bytes);
        *self = ImageBytes::Complete(bytes.clone());
        bytes
    }

    fn as_slice(&self) -> &[u8] {
        match *self {
            ImageBytes::InProgress(ref bytes) => &bytes,
            ImageBytes::Complete(ref bytes) => &*bytes,
        }
    }
}

/// Indicating either entire image or just metadata availability
#[derive(Clone, Deserialize, Serialize, HeapSizeOf)]
pub enum ImageOrMetadataAvailable {
    ImageAvailable(Arc<Image>),
    MetadataAvailable(ImageMetadata),
}

/// The returned image.
#[derive(Clone, Deserialize, Serialize, HeapSizeOf)]
pub enum ImageResponse {
    /// The requested image was loaded.
    Loaded(Arc<Image>),
    /// The request image metadata was loaded.
    MetadataLoaded(ImageMetadata),
    /// The requested image failed to load, so a placeholder was loaded instead.
    PlaceholderLoaded(Arc<Image>),
    /// Neither the requested image nor the placeholder could be loaded.
    None,
}

/// The current state of an image in the cache.
#[derive(PartialEq, Copy, Clone, Deserialize, Serialize)]
pub enum ImageState {
    Pending(PendingImageId),
    LoadError,
    NotRequested(PendingImageId),
}

// A key used to communicate during loading.
type LoadKey = PendingImageId;

struct LoadKeyGenerator {
    counter: RwLock<u64>
}

impl LoadKeyGenerator {
    fn new() -> LoadKeyGenerator {
        LoadKeyGenerator {
            counter: RwLock::new(0)
        }
    }
    fn next(&mut self) -> PendingImageId {
        let mut counter = self.counter.write().unwrap();
        *counter += 1;
        PendingImageId(*counter)
    }
}

/// The unique id for an image that has previously been requested.
#[derive(Copy, Clone, PartialEq, Eq, Deserialize, Serialize, HeapSizeOf, Hash, Debug)]
pub struct PendingImageId(pub u64);

/// Represents an image that is either being loaded
/// by the resource thread, or decoded by a worker thread.
struct PendingLoad {
    // The bytes loaded so far. Reset to an empty vector once loading
    // is complete and the buffer has been transmitted to the decoder.
    bytes: ImageBytes,

    // Image metadata, if available.
    metadata: Option<ImageMetadata>,

    // Once loading is complete, the result of the operation.
    result: Option<Result<(), NetworkError>>,
 //XXX   listeners: Vec<ImageResponder>,

    // The url being loaded. Do not forget that this may be several Mb
    // if we are loading a data: url.
    url: ServoUrl,
}

impl PendingLoad {
    fn new(url: ServoUrl) -> PendingLoad {
        PendingLoad {
            bytes: ImageBytes::InProgress(vec!()),
            metadata: None,
            result: None,
//XXX            listeners: vec!(),
            url: url,
        }
    }

    /*XXX
    fn add_listener(&mut self, listener: ImageResponder) {
        self.listeners.push(listener);
    }*/
}

#[derive(Copy, Clone, PartialEq, Hash, Eq, Deserialize, Serialize)]
pub enum UsePlaceholder {
    No,
    Yes,
}

/// Implementation of the image cache
pub struct ImageCache {
    ///XXX TEST> REMOVE ME!
    remove_me: RwLock<i32>,
    // Images that have finished loading (successful or not).
    completed_loads: RwLock<HashMap<ServoUrl, CompletedLoad>>,
    // Images that are loading over network, or decoding.
    pending_loads: AllPendingLoads,
}

impl ImageCache {
    /// Return a completed image if it exists, or None if there is no complete load
    /// or the complete load is not fully decoded or is unavailable.
    fn get_completed_image_if_available(&self,
                                        url: &ServoUrl,
                                        placeholder: UsePlaceholder)
                                        -> Option<Result<ImageOrMetadataAvailable, ImageState>> {
        self.completed_loads.read().unwrap().get(url).map(|completed_load| {
            match (&completed_load.image_response, placeholder) {
                (&ImageResponse::Loaded(ref image), _) |
                (&ImageResponse::PlaceholderLoaded(ref image), UsePlaceholder::Yes) => {
                    Ok(ImageOrMetadataAvailable::ImageAvailable(image.clone()))
                }
                (&ImageResponse::PlaceholderLoaded(_), UsePlaceholder::No) |
                (&ImageResponse::None, _) |
                (&ImageResponse::MetadataLoaded(_), _) => {
                    Err(ImageState::LoadError)
                }
            }
        })
    }

    /// Public API

    /// Create a new image cache.
    pub fn new() -> Self {
        ImageCache {
            remove_me: RwLock::new(0),
            completed_loads: RwLock::new(HashMap::new()),
            pending_loads: AllPendingLoads::new(),
        }
    }

    ///XXX Test method. REMOVE ME!
    pub fn inc(&self) {
        let mut remove_me = self.remove_me.write().unwrap();
        *remove_me += 1;
        println!("INC {:?}", *remove_me);
    }

    /// Return any available metadata or image for the given URL, or an indication that
    /// the image is not yet available if it is in progress, or else reserve a slot in
    /// the cache for the URL if the consumer can request images.
    pub fn find_image_or_metadata(&self,
                                  url: ServoUrl,
                                  use_placeholder: UsePlaceholder,
                                  can_request: CanRequestImages) {
                                  //-> Result<ImageOrMetadataAvailable, ImageState> {
     /*   if let Some(result) = self.get_completed_image_if_available(&url, placeholder) {
            debug!("{} is available", url);
            return result;
        }

        let decoded = {
            let result = self.pending_loads.get_cached(url.clone(), can_request);
            match result {
                CacheResult::Hit(key, pl) => match (&pl.result, &pl.metadata) {
                    (&Some(Ok(_)), _) => {
                        debug!("sync decoding {} ({:?})", url, key);
                        decode_bytes_sync(key, &pl.bytes.as_slice())
                    }
                    (&None, &Some(ref meta)) => {
                        debug!("metadata available for {} ({:?})", url, key);
                        return Ok(ImageOrMetadataAvailable::MetadataAvailable(meta.clone()))
                    }
                    (&Some(Err(_)), _) | (&None, &None) => {
                        debug!("{} ({:?}) is still pending", url, key);
                        return Err(ImageState::Pending(key));
                    }
                },
                CacheResult::Miss(Some((key, _pl))) => {
                    debug!("should be requesting {} ({:?})", url, key);
                    return Err(ImageState::NotRequested(key));
                }
                CacheResult::Miss(None) => {
                    debug!("couldn't find an entry for {}", url);
                    return Err(ImageState::LoadError);
                }
            }
        };

        // In the case where a decode is ongoing (or waiting in a queue) but we have the
        // full response available, we decode the bytes synchronously and ignore the
        // async decode when it finishes later.
        // TODO: make this behaviour configurable according to the caller's needs.
        self.handle_decoder(decoded);
        match self.get_completed_image_if_available(&url, placeholder) {
            Some(result) => result,
            None => Err(ImageState::LoadError),
        }*/

    }

    /// Inform the image cache about a response for a pending request.
    pub fn notify_pending_response(&self, id: PendingImageId, data: FetchResponseMsg) {
        //XXX
    }
}


