/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */
/*
 * The origin of this IDL file is:
 * https://w3c.github.io/media-source/#idl-def-mediasource
 */

[Constructor, Exposed=Window]
interface MediaSource : EventTarget {
    readonly attribute SourceBufferList sourceBuffers;
    readonly attribute SourceBufferList activeSourceBuffers;
    readonly attribute ReadyState readyState;
    [SetterThrows]
    attribute unrestricted double duration;
    attribute EventHandler onsourceopen;
    attribute EventHandler onsourceended;
    attribute EventHandler onsourceclose;
    [Throws]
    SourceBuffer addSourceBuffer(DOMString type);
    [Throws]
    void removeSourceBuffer(SourceBuffer sourceBuffer);
    [Throws]
    void endOfStream(optional EndOfStreamError error);
    [Throws]
    void setLiveSeekableRange(double start, double end);
    [Throws]
    void clearLiveSeekableRange();
    static boolean isTypeSupported(DOMString type);
};

enum ReadyState {
    "closed",
    "open",
    "ended"
};

enum EndOfStreamError {
    "network",
    "decode"
};
