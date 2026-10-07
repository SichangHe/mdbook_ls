(function() {
    const wsProtocol = location.protocol === "https:" ? "wss" : "ws";
    const wsAddress =
        `${wsProtocol}://${location.host}/__mdbook_incremental_preview_live_patch${location.pathname}`;
    const web_socket = new WebSocket(wsAddress);
    // NOTE: We assume that the content is in <main> as per `index.hbs`.
    const contentElement = document.querySelector("main");
    const searchSocket = new WebSocket(`${wsProtocol}://${location.host}/__mdbook_source_search`);
    const markers = () => [...document.querySelectorAll("main .mdbook-source-marker")];
    function revealSource() {
        const match = location.hash.match(/^#mdbook-source-line=(\d+)$/);
        if (!match) return;
        const line = Number(match[1]);
        const candidates = markers().filter(marker => marker.dataset.sourceLine !== "")
            .sort((a, b) => Number(a.dataset.sourceLine) - Number(b.dataset.sourceLine));
        let marker = candidates[0];
        for (const candidate of candidates) {
            if (Number(candidate.dataset.sourceLine) > line) break;
            marker = candidate;
        }
        const target = marker?.nextElementSibling;
        target?.scrollIntoView({block: "center"});
    }
    searchSocket.onmessage = event => {
        const {url} = JSON.parse(event.data);
        if (url) {
            if (location.pathname + location.hash === url) revealSource();
            else location.assign(url);
        }
    };
    window.addEventListener("hashchange", revealSource);
    revealSource();
    contentElement?.addEventListener("click", event => {
        if (!(event.ctrlKey || event.metaKey || event.altKey) || searchSocket.readyState !== WebSocket.OPEN) return;
        let marker;
        for (const candidate of markers()) {
            if (candidate.compareDocumentPosition(event.target) & Node.DOCUMENT_POSITION_FOLLOWING) marker = candidate;
            else break;
        }
        if (!marker || marker.dataset.sourceLine === "") return;
        event.preventDefault();
        event.stopPropagation();
        searchSocket.send(JSON.stringify({path: marker.dataset.sourcePath, line: Number(marker.dataset.sourceLine)}));
    }, true);
    web_socket.onmessage = (event) => {
        if (event.data === "__RELOAD") {
            location.reload();
            return;
        }
        contentElement.innerHTML = event.data;
        revealSource();
        document.dispatchEvent(new Event("load"));
        if (window.hljs && window.hljs.initHighlighting) {
            // Re-highlight with highlight.js.
            window.hljs.initHighlighting.called = false;
            window.hljs.initHighlighting();
        }
    };
})();
