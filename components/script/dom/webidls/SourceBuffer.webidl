/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */
/*
 * The origin of this IDL file is:
 * https://w3c.github.io/media-source/#idl-def-sourcebuffer
 */

[Exposed=Window]
interface SourceBuffer : EventTarget {
    [SetterThrows] attribute AppendMode mode;
    readonly attribute boolean updating;
    // readonly attribute TimeRanges buffered;
    [SetterThrows] attribute double timestampOffset;
    // readonly attribute AudioTrackList audioTracks;
    // readonly attribute VideoTrackList videoTracks;
    // readonly attribute TextTrackList textTracks;
    [SetterThrows] attribute double appendWindowStart;
    [SetterThrows] attribute unrestricted double appendWindowEnd;
    attribute EventHandler onupdatestart;
    attribute EventHandler onupdate;
    attribute EventHandler onupdateend;
    attribute EventHandler onerror;
    attribute EventHandler onabort;
    [Throws] void appendBuffer(object data);
    [Throws] void abort();
    [Throws] void remove(double start, unrestricted double end);
};

enum AppendMode {
    "segments",
    "sequence"
};
