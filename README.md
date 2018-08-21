A websocket client library which supports both desktop and webassembly. It's only been tested on Linux and wasm32-unknown-emscripten, but should work on other desktop platforms and other webassembly targets.

It uses a polling API for receiving messages, so it's probably most suitable for games, which would poll every frame. It may not be suitable for applications which need to respond to messages within milliseconds of receiving them, since the polling would add a slight delay.

This supports both text and binary data. On desktop, it uses `websocket`. On webassembly, it uses JavaScript through `stdweb`.
